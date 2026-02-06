use alloy_primitives::{Address, B256, Bytes, Log, U256};
use alloy_sol_types::{sol, SolEvent};
use anyhow::{anyhow, Context, Result};
use reqwest::blocking::Client as HttpClient;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    str::FromStr,
};

use crate::bundle::{build_bundle, verify_manifest, BundleManifest};
use crate::state::AppState;

#[derive(Debug, Deserialize, Clone)]
pub struct DevnetConfig {
    pub chainId: u64,
    pub deployBlock: Option<u64>,
    pub dappRegistry: String,
    pub developerPrivateKey: Option<String>,
}

#[derive(Debug, Clone)]
pub struct DevnetContext {
    pub config: DevnetConfig,
    pub rpc_url: String,
    pub ipfs_api: String,
    pub ipfs_gateway: String,
    pub cache_dir: PathBuf,
    pub http: HttpClient,
}

#[derive(Debug, Clone, Serialize)]
pub struct DappInfo {
    pub dappId: String,
    pub versionId: String,
    pub name: String,
    pub version: String,
    pub description: String,
    pub status: String,
    pub rootCid: String,
}

pub fn load_devnet(path: &Path) -> Result<DevnetConfig> {
    let raw = fs::read_to_string(path).context("read devnet.json")?;
    let cfg: DevnetConfig = serde_json::from_str(&raw).context("parse devnet.json")?;
    Ok(cfg)
}

sol! {
    event DappPublished(uint256 indexed dappId, uint256 indexed versionId, bytes rootCid, address proposer);
    event DappUpgraded(
        uint256 indexed dappId,
        uint256 indexed fromVersionId,
        uint256 indexed toVersionId,
        bytes rootCid,
        address proposer
    );
    event DappMetadata(uint256 indexed dappId, uint256 indexed versionId, string name, string version, string description);
    event DappPaused(uint256 indexed dappId, uint256 indexed versionId, address pausedBy, string reason);
    event DappUnpaused(uint256 indexed dappId, uint256 indexed versionId, address unpausedBy, string reason);
    event DappDeprecated(uint256 indexed dappId, uint256 indexed versionId, address deprecatedBy, string reason);
}

#[derive(Debug, Deserialize)]
struct RpcLog {
    address: String,
    data: String,
    topics: Vec<String>,
    #[serde(default)]
    blockNumber: Option<String>,
    #[serde(default)]
    logIndex: Option<String>,
}

struct LogEntry {
    block_number: u64,
    log_index: u64,
    kind: String,
    log: Log,
}

pub fn list_dapps(devnet: &DevnetContext) -> Result<Vec<DappInfo>> {
    if devnet.config.dappRegistry.is_empty() {
        return Err(anyhow!("devnet.json missing dappRegistry"));
    }
    let address = devnet.config.dappRegistry.clone();
    let published = rpc_get_logs(devnet, &address, DappPublished::SIGNATURE_HASH)?;
    let upgraded = rpc_get_logs(devnet, &address, DappUpgraded::SIGNATURE_HASH)?;
    let metadata = rpc_get_logs(devnet, &address, DappMetadata::SIGNATURE_HASH)?;
    let paused = rpc_get_logs(devnet, &address, DappPaused::SIGNATURE_HASH)?;
    let unpaused = rpc_get_logs(devnet, &address, DappUnpaused::SIGNATURE_HASH)?;
    let deprecated = rpc_get_logs(devnet, &address, DappDeprecated::SIGNATURE_HASH)?;

    let mut all = Vec::new();
    all.extend(published);
    all.extend(upgraded);
    all.extend(metadata);
    all.extend(paused);
    all.extend(unpaused);
    all.extend(deprecated);
    all.sort_by(|a, b| {
        let block_diff = a.block_number.cmp(&b.block_number);
        if block_diff != std::cmp::Ordering::Equal {
            return block_diff;
        }
        a.log_index.cmp(&b.log_index)
    });

    #[derive(Debug)]
    struct Version {
        version_id: u64,
        root_cid: Option<String>,
        name: Option<String>,
        version: Option<String>,
        description: Option<String>,
        status: Option<String>,
    }
    #[derive(Debug)]
    struct Dapp {
        dapp_id: u64,
        latest_version_id: u64,
        versions: HashMap<u64, Version>,
    }

    let mut dapps: HashMap<u64, Dapp> = HashMap::new();

    macro_rules! get_or_create_version {
        ($dapps:expr, $dapp_id:expr, $version_id:expr) => {{
            let dapp = $dapps.entry($dapp_id).or_insert_with(|| Dapp {
                dapp_id: $dapp_id,
                latest_version_id: 0,
                versions: HashMap::new(),
            });
            dapp.versions.entry($version_id).or_insert_with(|| Version {
                version_id: $version_id,
                root_cid: None,
                name: None,
                version: None,
                description: None,
                status: None,
            })
        }};
    }

    for log in all {
        match log.kind.as_str() {
            "DappPublished" => {
                let decoded = DappPublished::decode_log(&log.log)?;
                let dapp_id = u256_to_u64(decoded.data.dappId)?;
                let version_id = u256_to_u64(decoded.data.versionId)?;
                let root = bytes_to_string(&decoded.data.rootCid);
                let v = get_or_create_version!(dapps, dapp_id, version_id);
                v.root_cid = Some(root);
                v.status = Some("Published".to_string());
                dapps.get_mut(&dapp_id).unwrap().latest_version_id = version_id;
            }
            "DappUpgraded" => {
                let decoded = DappUpgraded::decode_log(&log.log)?;
                let dapp_id = u256_to_u64(decoded.data.dappId)?;
                let version_id = u256_to_u64(decoded.data.toVersionId)?;
                let root = bytes_to_string(&decoded.data.rootCid);
                let v = get_or_create_version!(dapps, dapp_id, version_id);
                v.root_cid = Some(root);
                v.status = Some("Published".to_string());
                dapps.get_mut(&dapp_id).unwrap().latest_version_id = version_id;
            }
            "DappMetadata" => {
                let decoded = DappMetadata::decode_log(&log.log)?;
                let dapp_id = u256_to_u64(decoded.data.dappId)?;
                let version_id = u256_to_u64(decoded.data.versionId)?;
                let v = get_or_create_version!(dapps, dapp_id, version_id);
                v.name = Some(decoded.data.name.to_string());
                v.version = Some(decoded.data.version.to_string());
                v.description = Some(decoded.data.description.to_string());
            }
            "DappPaused" => {
                let decoded = DappPaused::decode_log(&log.log)?;
                let dapp_id = u256_to_u64(decoded.data.dappId)?;
                let version_id = u256_to_u64(decoded.data.versionId)?;
                let v = get_or_create_version!(dapps, dapp_id, version_id);
                v.status = Some("Paused".to_string());
            }
            "DappUnpaused" => {
                let decoded = DappUnpaused::decode_log(&log.log)?;
                let dapp_id = u256_to_u64(decoded.data.dappId)?;
                let version_id = u256_to_u64(decoded.data.versionId)?;
                let v = get_or_create_version!(dapps, dapp_id, version_id);
                v.status = Some("Published".to_string());
            }
            "DappDeprecated" => {
                let decoded = DappDeprecated::decode_log(&log.log)?;
                let dapp_id = u256_to_u64(decoded.data.dappId)?;
                let version_id = u256_to_u64(decoded.data.versionId)?;
                let v = get_or_create_version!(dapps, dapp_id, version_id);
                v.status = Some("Deprecated".to_string());
            }
            _ => {}
        }
    }

    let mut result = Vec::new();
    let mut keys: Vec<u64> = dapps.keys().cloned().collect();
    keys.sort_unstable();
    for key in keys {
        if let Some(dapp) = dapps.get(&key) {
            let latest = dapp.versions.get(&dapp.latest_version_id);
            result.push(DappInfo {
                dappId: dapp.dapp_id.to_string(),
                versionId: dapp.latest_version_id.to_string(),
                name: latest.and_then(|v| v.name.clone()).unwrap_or_default(),
                version: latest.and_then(|v| v.version.clone()).unwrap_or_default(),
                description: latest.and_then(|v| v.description.clone()).unwrap_or_default(),
                status: latest.and_then(|v| v.status.clone()).unwrap_or_else(|| "Unknown".to_string()),
                rootCid: latest.and_then(|v| v.root_cid.clone()).unwrap_or_default(),
            });
        }
    }
    Ok(result)
}

fn rpc_get_logs(devnet: &DevnetContext, address: &str, topic0: B256) -> Result<Vec<LogEntry>> {
    let topics = vec![format!("0x{}", hex::encode(topic0))];
    let from_block = devnet
        .config
        .deployBlock
        .map(|b| format!("0x{:x}", b))
        .unwrap_or_else(|| "0x0".to_string());
    let payload = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "eth_getLogs",
        "params": [{
            "address": address,
            "topics": topics,
            "fromBlock": from_block,
            "toBlock": "latest"
        }]
    });
    let res = devnet
        .http
        .post(&devnet.rpc_url)
        .json(&payload)
        .send()
        .context("rpc getLogs failed")?;
    let v: serde_json::Value = res.json().context("rpc getLogs decode failed")?;
    if let Some(err) = v.get("error") {
        return Err(anyhow!("rpc getLogs error: {}", err));
    }
    let logs_val = v.get("result").cloned().unwrap_or(serde_json::Value::Array(Vec::new()));
    let logs: Vec<RpcLog> = serde_json::from_value(logs_val)?;
    let mut out = Vec::new();
    for log in logs {
        let log_entry = rpc_log_to_entry(log)?;
        out.push(log_entry);
    }
    Ok(out)
}

fn rpc_log_to_entry(rpc_log: RpcLog) -> Result<LogEntry> {
    let address = Address::from_str(&rpc_log.address)?;
    let mut topics = Vec::new();
    for topic in rpc_log.topics {
        topics.push(hex_to_b256(&topic)?);
    }
    let data = hex_to_bytes(&rpc_log.data)?;
    let log = Log::new_unchecked(address, topics, data);
    let kind = event_kind(&log)?;
    Ok(LogEntry {
        block_number: parse_hex_u64_opt(rpc_log.blockNumber.as_deref()).unwrap_or(0),
        log_index: parse_hex_u64_opt(rpc_log.logIndex.as_deref()).unwrap_or(0),
        kind,
        log,
    })
}

fn event_kind(log: &Log) -> Result<String> {
    let topics = log.topics();
    if topics.is_empty() {
        return Err(anyhow!("log missing topics"));
    }
    let topic0 = topics[0];
    if topic0 == DappPublished::SIGNATURE_HASH {
        Ok("DappPublished".to_string())
    } else if topic0 == DappUpgraded::SIGNATURE_HASH {
        Ok("DappUpgraded".to_string())
    } else if topic0 == DappMetadata::SIGNATURE_HASH {
        Ok("DappMetadata".to_string())
    } else if topic0 == DappPaused::SIGNATURE_HASH {
        Ok("DappPaused".to_string())
    } else if topic0 == DappUnpaused::SIGNATURE_HASH {
        Ok("DappUnpaused".to_string())
    } else if topic0 == DappDeprecated::SIGNATURE_HASH {
        Ok("DappDeprecated".to_string())
    } else {
        Err(anyhow!("unknown event signature"))
    }
}

pub fn handle_launcher_ipc(webview: &wry::WebView, state: &AppState, req: &crate::state::IpcRequest) -> Result<serde_json::Value> {
    let devnet = state.devnet.as_ref().ok_or_else(|| anyhow!("Devnet not enabled"))?;
    match req.method.as_str() {
        "vibefi_listDapps" => {
            println!("launcher: fetching dapp list from logs");
            let dapps = list_dapps(devnet)?;
            Ok(serde_json::to_value(dapps)?)
        }
        "vibefi_launchDapp" => {
            let root_cid = req
                .params
                .get(0)
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow!("missing rootCid"))?;
            println!("launcher: fetch bundle {root_cid}");
            let bundle_dir = devnet.cache_dir.join(root_cid);
            ensure_bundle_cached(devnet, root_cid, &bundle_dir)?;
            println!("launcher: verify bundle manifest");
            verify_manifest(&bundle_dir)?;
            println!("launcher: verify CID via IPFS");
            let computed = compute_ipfs_cid(&bundle_dir, &devnet.ipfs_api)?;
            if computed != root_cid {
                return Err(anyhow!("CID mismatch: expected {root_cid} got {computed}"));
            }
            let dist_dir = bundle_dir.join(".vibefi").join("dist");
            if dist_dir.join("index.html").exists() {
                println!("launcher: using cached build");
            } else {
                println!("launcher: build bundle");
                build_bundle(&bundle_dir, &dist_dir)?;
            }
            {
                let mut current = state.current_bundle.lock().unwrap();
                *current = Some(dist_dir);
            }
            webview.evaluate_script("window.location = 'app://index.html';")?;
            Ok(serde_json::Value::Bool(true))
        }
        _ => Err(anyhow!("Unsupported launcher method: {}", req.method)),
    }
}

fn ensure_bundle_cached(devnet: &DevnetContext, root_cid: &str, bundle_dir: &Path) -> Result<()> {
    if bundle_dir.join("manifest.json").exists() {
        return Ok(());
    }
    println!("launcher: download bundle from IPFS gateway");
    fs::create_dir_all(bundle_dir).context("create cache dir")?;
    let (manifest, manifest_bytes) = fetch_dapp_manifest(devnet, root_cid)?;
    download_dapp_bundle(devnet, root_cid, bundle_dir, &manifest, &manifest_bytes)?;
    Ok(())
}

fn fetch_dapp_manifest(devnet: &DevnetContext, root_cid: &str) -> Result<(BundleManifest, Vec<u8>)> {
    let gateway = normalize_gateway(&devnet.ipfs_gateway);
    let url = format!("{}/ipfs/{}/manifest.json", gateway, root_cid);
    let res = devnet.http.get(url).send().context("fetch manifest")?;
    if !res.status().is_success() {
        let text = res.text().unwrap_or_default();
        return Err(anyhow!("fetch manifest failed: {}", text));
    }
    let raw_bytes = res.bytes().context("read manifest bytes")?.to_vec();
    let manifest: BundleManifest = serde_json::from_slice(&raw_bytes).context("parse manifest")?;
    if manifest.files.is_empty() {
        return Err(anyhow!("manifest.json missing files list"));
    }
    Ok((manifest, raw_bytes))
}

fn download_dapp_bundle(
    devnet: &DevnetContext,
    root_cid: &str,
    out_dir: &Path,
    manifest: &BundleManifest,
    manifest_bytes: &[u8],
) -> Result<()> {
    let gateway = normalize_gateway(&devnet.ipfs_gateway);
    fs::write(out_dir.join("manifest.json"), manifest_bytes)?;
    for entry in &manifest.files {
        let url = format!("{}/ipfs/{}/{}", gateway, root_cid, entry.path);
        let res = devnet.http.get(url).send().context("fetch bundle file")?;
        if !res.status().is_success() {
            let text = res.text().unwrap_or_default();
            return Err(anyhow!("bundle fetch failed: {}", text));
        }
        let bytes = res.bytes().context("read bundle file")?;
        let dest = out_dir.join(&entry.path);
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(dest, &bytes)?;
    }
    Ok(())
}

fn compute_ipfs_cid(out_dir: &Path, ipfs_api: &str) -> Result<String> {
    let files = crate::bundle::walk_files(out_dir)?;
    let mut form = reqwest::blocking::multipart::Form::new();
    for file in files {
        let rel = file.strip_prefix(out_dir)?.to_string_lossy().replace('\\', "/");
        let data = fs::read(&file)?;
        let part = reqwest::blocking::multipart::Part::bytes(data).file_name(rel);
        form = form.part("file", part);
    }
    let url = format!("{}/api/v0/add", ipfs_api.trim_end_matches('/'));
    let res = HttpClient::new()
        .post(url)
        .query(&[
            ("recursive", "true"),
            ("wrap-with-directory", "true"),
            ("cid-version", "1"),
            ("pin", "false"),
            ("only-hash", "true"),
        ])
        .multipart(form)
        .send()
        .context("ipfs add failed")?;
    let body = res.text().context("read ipfs response")?;
    let lines: Vec<&str> = body.lines().filter(|l| !l.trim().is_empty()).collect();
    if lines.is_empty() {
        return Err(anyhow!("IPFS add returned empty response"));
    }
    let last = lines[lines.len() - 1];
    let json: serde_json::Value = serde_json::from_str(last).context("parse ipfs response")?;
    if let Some(hash) = json.get("Hash").and_then(|v| v.as_str()) {
        return Ok(hash.to_string());
    }
    if let Some(hash) = json.get("Cid").and_then(|v| v.get("/")).and_then(|v| v.as_str()) {
        return Ok(hash.to_string());
    }
    Err(anyhow!("IPFS add response missing CID"))
}

fn normalize_gateway(gateway: &str) -> String {
    gateway.trim_end_matches('/').to_string()
}

fn bytes_to_string(bytes: &Bytes) -> String {
    let mut out = bytes.to_vec();
    while out.last() == Some(&0) {
        out.pop();
    }
    String::from_utf8_lossy(&out).to_string()
}

fn hex_to_b256(s: &str) -> Result<B256> {
    let bytes = hex_to_vec(s)?;
    if bytes.len() != 32 {
        return Err(anyhow!("invalid topic length"));
    }
    Ok(B256::from_slice(&bytes))
}

fn hex_to_bytes(s: &str) -> Result<Bytes> {
    Ok(Bytes::from(hex_to_vec(s)?))
}

fn hex_to_vec(s: &str) -> Result<Vec<u8>> {
    let s = s.strip_prefix("0x").unwrap_or(s);
    Ok(hex::decode(s)?)
}

fn parse_hex_u64_opt(s: Option<&str>) -> Option<u64> {
    s.and_then(|v| parse_hex_u64(v))
}

fn parse_hex_u64(s: &str) -> Option<u64> {
    let s = s.strip_prefix("0x").unwrap_or(s);
    u64::from_str_radix(s, 16).ok()
}

fn u256_to_u64(value: U256) -> Result<u64> {
    value.try_into().map_err(|_| anyhow!("u256 out of range"))
}
