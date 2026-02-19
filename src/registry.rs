use alloy_primitives::{Address, B256, Bytes, Log, U256};
use alloy_sol_types::{SolEvent, sol};
use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fs,
    io::ErrorKind,
    path::{Component, Path, PathBuf},
    str::FromStr,
};

use crate::bundle::{BundleManifest, build_bundle, verify_manifest};
use crate::config::{IpfsFetchBackend, ResolvedConfig};
use crate::ipfs_helper::{IpfsHelperBridge, IpfsHelperConfig};
use crate::state::{AppState, TabAction, UserEvent};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DappInfo {
    pub dapp_id: String,
    pub version_id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    pub status: String,
    pub root_cid: String,
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
#[serde(rename_all = "camelCase")]
struct RpcLog {
    address: String,
    data: String,
    topics: Vec<String>,
    #[serde(default)]
    block_number: Option<String>,
    #[serde(default)]
    log_index: Option<String>,
}

struct LogEntry {
    block_number: u64,
    log_index: u64,
    kind: String,
    log: Log,
}

#[derive(Debug, Clone)]
struct EffectiveIpfsConfig {
    fetch_backend: IpfsFetchBackend,
    gateway_endpoint: String,
    helia_gateways: Vec<String>,
    helia_routers: Vec<String>,
    helia_timeout_ms: u64,
}

const LAUNCH_PROGRESS_EVENT: &str = "vibefiLaunchProgress";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct LaunchProgress {
    stage: String,
    message: String,
    percent: u8,
    #[serde(skip_serializing_if = "Option::is_none")]
    completed_files: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    total_files: Option<usize>,
}

impl LaunchProgress {
    fn simple(stage: &str, message: impl Into<String>, percent: u8) -> Self {
        Self {
            stage: stage.to_string(),
            message: message.into(),
            percent: percent.min(100),
            completed_files: None,
            total_files: None,
        }
    }

    fn files(
        stage: &str,
        message: impl Into<String>,
        percent: u8,
        completed_files: usize,
        total_files: usize,
    ) -> Self {
        Self {
            stage: stage.to_string(),
            message: message.into(),
            percent: percent.min(100),
            completed_files: Some(completed_files),
            total_files: Some(total_files),
        }
    }
}

pub fn list_dapps(devnet: &ResolvedConfig) -> Result<Vec<DappInfo>> {
    if devnet.dapp_registry.is_empty() {
        return Err(anyhow!("config missing dappRegistry"));
    }
    let address = devnet.dapp_registry.clone();
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
                dapps
                    .get_mut(&dapp_id)
                    .expect("dapp entry missing after version creation")
                    .latest_version_id = version_id;
            }
            "DappUpgraded" => {
                let decoded = DappUpgraded::decode_log(&log.log)?;
                let dapp_id = u256_to_u64(decoded.data.dappId)?;
                let version_id = u256_to_u64(decoded.data.toVersionId)?;
                let root = bytes_to_string(&decoded.data.rootCid);
                let v = get_or_create_version!(dapps, dapp_id, version_id);
                v.root_cid = Some(root);
                v.status = Some("Published".to_string());
                dapps
                    .get_mut(&dapp_id)
                    .expect("dapp entry missing after version creation")
                    .latest_version_id = version_id;
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
                dapp_id: dapp.dapp_id.to_string(),
                version_id: dapp.latest_version_id.to_string(),
                name: latest.and_then(|v| v.name.clone()).unwrap_or_default(),
                version: latest.and_then(|v| v.version.clone()).unwrap_or_default(),
                description: latest
                    .and_then(|v| v.description.clone())
                    .unwrap_or_default(),
                status: latest
                    .and_then(|v| v.status.clone())
                    .unwrap_or_else(|| "Unknown".to_string()),
                root_cid: latest.and_then(|v| v.root_cid.clone()).unwrap_or_default(),
            });
        }
    }
    Ok(result)
}

pub fn resolve_published_root_cid_by_dapp_id(devnet: &ResolvedConfig, studio_dapp_id: u64) -> Result<String> {
    let dapps = list_dapps(devnet)?;
    let studio = dapps
        .into_iter()
        .find(|dapp| dapp.dapp_id == studio_dapp_id.to_string())
        .ok_or_else(|| anyhow!("studio dappId {} not found in DappRegistry", studio_dapp_id))?;
    if studio.status != "Published" {
        bail!(
            "studio dappId {} latest version is {}, expected Published",
            studio_dapp_id,
            studio.status
        );
    }
    if studio.root_cid.trim().is_empty() {
        bail!(
            "studio dappId {} latest published version has an empty rootCid",
            studio_dapp_id
        );
    }
    Ok(studio.root_cid)
}

fn rpc_get_logs(devnet: &ResolvedConfig, address: &str, topic0: B256) -> Result<Vec<LogEntry>> {
    let topics = vec![format!("0x{}", hex::encode(topic0))];
    let from_block = devnet
        .deploy_block
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
        .http_client
        .post(&devnet.rpc_url)
        .json(&payload)
        .send()
        .context("rpc getLogs failed")?;
    let v: serde_json::Value = res.json().context("rpc getLogs decode failed")?;
    if let Some(err) = v.get("error") {
        return Err(anyhow!("rpc getLogs error: {}", err));
    }
    let logs_val = v
        .get("result")
        .cloned()
        .unwrap_or(serde_json::Value::Array(Vec::new()));
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
        block_number: parse_hex_u64_opt(rpc_log.block_number.as_deref()).unwrap_or(0),
        log_index: parse_hex_u64_opt(rpc_log.log_index.as_deref()).unwrap_or(0),
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

pub fn handle_launcher_ipc(
    state: &AppState,
    webview_id: &str,
    req: &crate::ipc_contract::IpcRequest,
) -> Result<Option<serde_json::Value>> {
    match req.method.as_str() {
        "vibefi_listDapps" => {
            let state_clone = state.clone();
            let webview_id = webview_id.to_string();
            let ipc_id = req.id;
            std::thread::spawn(move || {
                let result = (|| -> Result<serde_json::Value> {
                    let devnet = state_clone
                        .resolved
                        .as_ref()
                        .ok_or_else(|| anyhow!("Network not configured"))?;
                    tracing::info!("launcher: fetching dapp list from logs");
                    let mut dapps = list_dapps(devnet)?;
                    if let Some(studio_dapp_id) = devnet.studio_dapp_id {
                        let studio_id = studio_dapp_id.to_string();
                        dapps.retain(|dapp| dapp.dapp_id != studio_id);
                    }
                    Ok(serde_json::to_value(dapps)?)
                })()
                .map_err(|e| e.to_string());
                let _ = state_clone.proxy.send_event(UserEvent::RpcResult {
                    webview_id,
                    ipc_id,
                    result,
                });
            });
            Ok(None)
        }
        "vibefi_launchDapp" => {
            let root_cid = req
                .params
                .get(0)
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow!("missing rootCid"))?
                .to_string();
            let name = req
                .params
                .get(1)
                .and_then(|v| v.as_str())
                .unwrap_or(&root_cid)
                .to_string();
            let state_clone = state.clone();
            let webview_id = webview_id.to_string();
            let ipc_id = req.id;
            std::thread::spawn(move || {
                let result = launch_dapp(&state_clone, &webview_id, &root_cid, &name)
                    .map(|_| serde_json::Value::Bool(true))
                    .map_err(|e| e.to_string());
                let _ = state_clone.proxy.send_event(UserEvent::RpcResult {
                    webview_id,
                    ipc_id,
                    result,
                });
            });
            Ok(None)
        }
        "vibefi_openSettings" => {
            let _ = state.proxy.send_event(UserEvent::OpenSettings);
            Ok(Some(serde_json::Value::Bool(true)))
        }
        _ => Err(anyhow!("Unsupported launcher method: {}", req.method)),
    }
}

fn launch_dapp(state: &AppState, webview_id: &str, root_cid: &str, name: &str) -> Result<()> {
    let dist_dir = prepare_dapp_dist(state, root_cid, Some(webview_id))?;
    let source_dir = dist_dir
        .parent()
        .and_then(|path| path.parent())
        .and_then(|path| path.canonicalize().ok());
    let _ = state
        .proxy
        .send_event(UserEvent::TabAction(TabAction::OpenApp {
            name: name.to_string(),
            dist_dir,
            source_dir,
        }));
    Ok(())
}

pub fn prepare_dapp_dist(
    state: &AppState,
    root_cid: &str,
    progress_webview_id: Option<&str>,
) -> Result<PathBuf> {
    let devnet = state
        .resolved
        .as_ref()
        .ok_or_else(|| anyhow!("Network not configured"))?;
    tracing::info!(root_cid, "prepare dapp: fetch bundle");
    let bundle_dir = devnet.cache_dir.join(root_cid);
    let ipfs = resolve_effective_ipfs_config(state, devnet);
    tracing::info!(backend = ipfs.fetch_backend.as_str(), "ipfs backend");

    emit_launch_progress_if(
        state,
        progress_webview_id,
        LaunchProgress::simple("prepare", "Preparing bundle retrieval...", 2),
    );

    {
        let mut emit = |progress: LaunchProgress| {
            emit_launch_progress_if(state, progress_webview_id, progress)
        };
        ensure_bundle_cached(devnet, &ipfs, root_cid, &bundle_dir, &mut emit)?;
    }

    tracing::info!("prepare dapp: verify bundle manifest");
    emit_launch_progress_if(
        state,
        progress_webview_id,
        LaunchProgress::simple("verify", "Verifying downloaded bundle...", 88),
    );
    verify_manifest(&bundle_dir)?;

    let dist_dir = bundle_dir.join(".vibefi").join("dist");
    if dist_dir.join("index.html").exists() {
        tracing::info!("prepare dapp: using cached build");
        emit_launch_progress_if(
            state,
            progress_webview_id,
            LaunchProgress::simple("build", "Using cached build artifacts.", 96),
        );
    } else {
        tracing::info!("prepare dapp: build bundle");
        emit_launch_progress_if(
            state,
            progress_webview_id,
            LaunchProgress::simple("build", "Building bundle...", 94),
        );
        build_bundle(&bundle_dir, &dist_dir)?;
    }
    emit_launch_progress_if(
        state,
        progress_webview_id,
        LaunchProgress::simple("done", "Launch complete.", 100),
    );
    Ok(dist_dir)
}

fn emit_launch_progress(state: &AppState, webview_id: &str, progress: LaunchProgress) {
    let value = serde_json::to_value(progress).unwrap_or(serde_json::Value::Null);
    let _ = state.proxy.send_event(UserEvent::ProviderEvent {
        webview_id: webview_id.to_string(),
        event: LAUNCH_PROGRESS_EVENT.to_string(),
        value,
    });
}

fn emit_launch_progress_if(state: &AppState, webview_id: Option<&str>, progress: LaunchProgress) {
    if let Some(webview_id) = webview_id {
        emit_launch_progress(state, webview_id, progress);
    }
}

fn ensure_bundle_cached(
    devnet: &ResolvedConfig,
    ipfs: &EffectiveIpfsConfig,
    root_cid: &str,
    bundle_dir: &Path,
    on_progress: &mut dyn FnMut(LaunchProgress),
) -> Result<()> {
    if bundle_dir.join("manifest.json").exists() {
        match verify_manifest(bundle_dir) {
            Ok(()) => {
                on_progress(LaunchProgress::simple(
                    "download",
                    "Using cached IPFS bundle files.",
                    82,
                ));
                return Ok(());
            }
            Err(err) => {
                tracing::warn!(
                    error = %err,
                    "launcher: cached bundle invalid, purging cache and re-downloading"
                );
                on_progress(LaunchProgress::simple(
                    "download",
                    "Cached bundle is incomplete. Re-downloading...",
                    8,
                ));
                match fs::remove_dir_all(bundle_dir) {
                    Ok(()) => {}
                    Err(remove_err) if remove_err.kind() == ErrorKind::NotFound => {}
                    Err(remove_err) => {
                        return Err(remove_err).context("remove invalid bundle cache");
                    }
                }
            }
        }
    }
    let result = match ipfs.fetch_backend {
        IpfsFetchBackend::LocalNode => {
            ensure_bundle_cached_local_node(devnet, ipfs, root_cid, bundle_dir, on_progress)
        }
        IpfsFetchBackend::Helia => {
            ensure_bundle_cached_helia(ipfs, root_cid, bundle_dir, on_progress)
        }
    };
    if let Err(err) = result {
        // Prevent interrupted downloads from becoming sticky cache failures.
        let _ = fs::remove_dir_all(bundle_dir);
        return Err(err);
    }
    Ok(())
}

fn ensure_bundle_cached_local_node(
    devnet: &ResolvedConfig,
    ipfs: &EffectiveIpfsConfig,
    root_cid: &str,
    bundle_dir: &Path,
    on_progress: &mut dyn FnMut(LaunchProgress),
) -> Result<()> {
    tracing::info!("launcher: download bundle from local IPFS node");
    on_progress(LaunchProgress::simple(
        "download",
        "Downloading bundle from local IPFS node...",
        4,
    ));
    fs::create_dir_all(bundle_dir).context("create cache dir")?;
    let (manifest, manifest_bytes) = fetch_dapp_manifest_local_node(devnet, ipfs, root_cid)?;
    download_dapp_bundle_local_node(
        devnet,
        ipfs,
        root_cid,
        bundle_dir,
        &manifest,
        &manifest_bytes,
        on_progress,
    )?;
    Ok(())
}

fn ensure_bundle_cached_helia(
    ipfs: &EffectiveIpfsConfig,
    root_cid: &str,
    bundle_dir: &Path,
    on_progress: &mut dyn FnMut(LaunchProgress),
) -> Result<()> {
    tracing::info!("launcher: download bundle via Helia verified fetch");
    on_progress(LaunchProgress::simple(
        "download",
        "Fetching manifest from IPFS...",
        6,
    ));
    fs::create_dir_all(bundle_dir).context("create cache dir")?;
    let mut helper = IpfsHelperBridge::spawn(IpfsHelperConfig {
        gateways: ipfs.helia_gateways.clone(),
        routers: ipfs.helia_routers.clone(),
    })?;
    let manifest_url = format!("ipfs://{root_cid}/manifest.json");
    let manifest_resp = helper.fetch(&manifest_url, Some(ipfs.helia_timeout_ms))?;
    if !(200..300).contains(&manifest_resp.status) {
        return Err(anyhow!(
            "fetch manifest failed with status {}",
            manifest_resp.status
        ));
    }
    let raw_bytes = manifest_resp.body;
    let manifest: BundleManifest = serde_json::from_slice(&raw_bytes).context("parse manifest")?;
    if manifest.files.is_empty() {
        return Err(anyhow!("manifest.json missing files list"));
    }

    let total_files = manifest.files.len();
    on_progress(LaunchProgress::files(
        "download",
        format!("Downloading bundle files (0/{total_files})..."),
        10,
        0,
        total_files,
    ));
    for (idx, entry) in manifest.files.iter().enumerate() {
        let file_url = format!("ipfs://{root_cid}/{}", entry.path);
        let response = helper.fetch(&file_url, Some(ipfs.helia_timeout_ms))?;
        if !(200..300).contains(&response.status) {
            return Err(anyhow!(
                "bundle fetch failed for {} with status {}",
                entry.path,
                response.status
            ));
        }
        let dest = sanitize_bundle_destination(bundle_dir, &entry.path)?;
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(dest, &response.body)?;
        let completed = idx + 1;
        on_progress(LaunchProgress::files(
            "download",
            format!("Downloaded {completed}/{total_files}: {}", entry.path),
            download_percent(completed, total_files),
            completed,
            total_files,
        ));
    }
    fs::write(bundle_dir.join("manifest.json"), &raw_bytes).context("write manifest.json")?;
    Ok(())
}

fn fetch_dapp_manifest_local_node(
    devnet: &ResolvedConfig,
    ipfs: &EffectiveIpfsConfig,
    root_cid: &str,
) -> Result<(BundleManifest, Vec<u8>)> {
    let gateway = normalize_gateway(&ipfs.gateway_endpoint);
    let url = format!("{}/ipfs/{}/manifest.json", gateway, root_cid);
    let res = devnet
        .http_client
        .get(url)
        .send()
        .context("fetch manifest")?;
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

fn download_dapp_bundle_local_node(
    devnet: &ResolvedConfig,
    ipfs: &EffectiveIpfsConfig,
    root_cid: &str,
    out_dir: &Path,
    manifest: &BundleManifest,
    manifest_bytes: &[u8],
    on_progress: &mut dyn FnMut(LaunchProgress),
) -> Result<()> {
    let gateway = normalize_gateway(&ipfs.gateway_endpoint);
    let total_files = manifest.files.len();
    on_progress(LaunchProgress::files(
        "download",
        format!("Downloading bundle files (0/{total_files})..."),
        10,
        0,
        total_files,
    ));
    for (idx, entry) in manifest.files.iter().enumerate() {
        let url = format!("{}/ipfs/{}/{}", gateway, root_cid, entry.path);
        let res = devnet
            .http_client
            .get(url)
            .send()
            .context("fetch bundle file")?;
        if !res.status().is_success() {
            let text = res.text().unwrap_or_default();
            return Err(anyhow!("bundle fetch failed: {}", text));
        }
        let bytes = res.bytes().context("read bundle file")?;
        let dest = sanitize_bundle_destination(out_dir, &entry.path)?;
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(dest, &bytes)?;
        let completed = idx + 1;
        on_progress(LaunchProgress::files(
            "download",
            format!("Downloaded {completed}/{total_files}: {}", entry.path),
            download_percent(completed, total_files),
            completed,
            total_files,
        ));
    }
    fs::write(out_dir.join("manifest.json"), manifest_bytes)?;
    Ok(())
}

fn download_percent(completed: usize, total: usize) -> u8 {
    if total == 0 {
        return 80;
    }
    let pct = 10 + ((completed * 72) / total);
    pct.min(82) as u8
}

fn resolve_effective_ipfs_config(state: &AppState, devnet: &ResolvedConfig) -> EffectiveIpfsConfig {
    let mut fetch_backend = devnet.ipfs_fetch_backend;
    let mut gateway_endpoint = devnet.ipfs_gateway.clone();
    if let Some(config_path) = state.resolved.as_ref().and_then(|r| r.config_path.as_ref()) {
        let settings = crate::settings::load_settings(config_path);
        if let Some(backend) = settings.ipfs.fetch_backend {
            fetch_backend = backend;
        }
        if let Some(endpoint) = settings.ipfs.gateway_endpoint {
            let trimmed = endpoint.trim();
            if !trimmed.is_empty() {
                gateway_endpoint = trimmed.to_string();
            }
        }
    }
    EffectiveIpfsConfig {
        fetch_backend,
        gateway_endpoint,
        helia_gateways: devnet.ipfs_helia_gateways.clone(),
        helia_routers: devnet.ipfs_helia_routers.clone(),
        helia_timeout_ms: devnet.ipfs_helia_timeout_ms,
    }
}

fn sanitize_bundle_destination(root: &Path, entry_path: &str) -> Result<PathBuf> {
    let rel = Path::new(entry_path);
    if rel.as_os_str().is_empty() || rel.is_absolute() {
        return Err(anyhow!("invalid bundle path {}", entry_path));
    }
    for component in rel.components() {
        match component {
            Component::Normal(_) => {}
            Component::CurDir
            | Component::ParentDir
            | Component::RootDir
            | Component::Prefix(_) => {
                return Err(anyhow!("invalid bundle path {}", entry_path));
            }
        }
    }
    Ok(root.join(rel))
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

#[cfg(test)]
mod tests {
    use super::{DappInfo, RpcLog};
    use serde_json::json;

    #[test]
    fn dapp_info_serializes_with_camel_case_keys() {
        let dapp = DappInfo {
            dapp_id: "1".to_string(),
            version_id: "2".to_string(),
            name: "Name".to_string(),
            version: "1.0.0".to_string(),
            description: "Desc".to_string(),
            status: "Published".to_string(),
            root_cid: "bafy...".to_string(),
        };
        let value = serde_json::to_value(dapp).expect("serialize DappInfo");
        assert_eq!(value.get("dappId"), Some(&json!("1")));
        assert_eq!(value.get("versionId"), Some(&json!("2")));
        assert_eq!(value.get("rootCid"), Some(&json!("bafy...")));
        assert!(value.get("dapp_id").is_none());
        assert!(value.get("version_id").is_none());
        assert!(value.get("root_cid").is_none());
    }

    #[test]
    fn rpc_log_deserializes_camel_case_and_defaults_missing_fields() {
        let value = json!({
            "address": "0x0000000000000000000000000000000000000000",
            "data": "0x",
            "topics": [],
            "blockNumber": "0x10",
            "logIndex": "0x1"
        });
        let parsed: RpcLog = serde_json::from_value(value).expect("deserialize RpcLog");
        assert_eq!(parsed.block_number.as_deref(), Some("0x10"));
        assert_eq!(parsed.log_index.as_deref(), Some("0x1"));

        let missing = json!({
            "address": "0x0000000000000000000000000000000000000000",
            "data": "0x",
            "topics": []
        });
        let parsed_missing: RpcLog =
            serde_json::from_value(missing).expect("deserialize RpcLog defaults");
        assert!(parsed_missing.block_number.is_none());
        assert!(parsed_missing.log_index.is_none());
    }
}
