use anyhow::{Result, anyhow, bail};
use serde::Deserialize;
use serde_json::{Value, json};
use std::cmp::{max, min};

use crate::config::IpfsFetchBackend;
use crate::ipc_contract::IpcRequest;
use crate::ipfs_helper::{IpfsHelperBridge, IpfsHelperConfig};
use crate::state::{AppRuntimeCapabilities, AppState, IpfsCapabilityRule, UserEvent};

const DEFAULT_MAX_BYTES: usize = 512 * 1024;
const MAX_SNIPPET_LINES_DEFAULT: usize = 200;
const IPFS_PROGRESS_EVENT: &str = "vibefiIpfsProgress";

#[derive(Debug, Deserialize)]
struct ManifestFileEntry {
    path: String,
    bytes: usize,
}

#[derive(Debug, Deserialize)]
struct BundleManifest {
    #[serde(default)]
    files: Vec<ManifestFileEntry>,
}

fn normalize_gateway(gateway: &str) -> String {
    gateway.trim_end_matches('/').to_string()
}

fn guess_mime_from_path(path: &str) -> Option<String> {
    let lower = path.to_ascii_lowercase();
    if lower.ends_with(".png") {
        return Some("image/png".to_string());
    }
    if lower.ends_with(".jpg") || lower.ends_with(".jpeg") {
        return Some("image/jpeg".to_string());
    }
    if lower.ends_with(".webp") {
        return Some("image/webp".to_string());
    }
    if lower.ends_with(".gif") {
        return Some("image/gif".to_string());
    }
    None
}

fn normalize_path(input: Option<&str>) -> Result<String> {
    let raw = input.unwrap_or_default().trim();
    let mut path = raw.trim_start_matches('/').to_string();
    while path.contains("//") {
        path = path.replace("//", "/");
    }
    if path.is_empty() {
        return Ok(String::new());
    }
    if path.split('/').any(|seg| seg == "." || seg == "..") {
        bail!("invalid path traversal segments");
    }
    Ok(path)
}

fn path_matches(pattern: &str, path: &str) -> bool {
    let p = pattern.trim_start_matches('/');
    let v = path.trim_start_matches('/');
    if p.is_empty() || p == "*" || p == "**" {
        return true;
    }
    if let Some(prefix_raw) = p.strip_suffix("/**") {
        let prefix = prefix_raw.trim_end_matches('/');
        if prefix.is_empty() {
            return true;
        }
        return v == prefix || v.starts_with(&format!("{prefix}/"));
    }
    if let Some(prefix_raw) = p.strip_suffix("/*") {
        let prefix = prefix_raw.trim_end_matches('/');
        if prefix.is_empty() {
            return !v.contains('/');
        }
        let suffix = match v.strip_prefix(&format!("{prefix}/")) {
            Some(suffix) => suffix,
            None => return false,
        };
        return !suffix.is_empty() && !suffix.contains('/');
    }
    v == p
}

fn cid_matches(rule: &IpfsCapabilityRule, cid: &str) -> bool {
    match rule.cid.as_deref() {
        None => true,
        Some("*") => true,
        Some(allowed) => allowed == cid,
    }
}

fn find_matching_rules<'a>(
    caps: &'a AppRuntimeCapabilities,
    cid: &str,
    path: &str,
    kind: Option<&str>,
) -> Vec<&'a IpfsCapabilityRule> {
    caps.ipfs_allow
        .iter()
        .filter(|rule| {
            if !cid_matches(rule, cid) {
                return false;
            }
            if !rule.paths.iter().any(|p| path_matches(p, path)) {
                return false;
            }
            match kind {
                None => true,
                Some(kind) => rule.as_kinds.iter().any(|k| k == kind),
            }
        })
        .collect()
}

fn resolve_max_bytes(
    matching: &[&IpfsCapabilityRule],
    requested_max_bytes: Option<usize>,
) -> usize {
    let policy_max = matching
        .iter()
        .filter_map(|rule| rule.max_bytes)
        .min()
        .unwrap_or(DEFAULT_MAX_BYTES);
    match requested_max_bytes {
        None => policy_max,
        Some(req) => min(req, policy_max),
    }
}

fn detect_bidi_or_invisible_controls(text: &str) -> bool {
    text.chars().any(|c| {
        matches!(
            c,
            '\u{200B}'
                | '\u{200C}'
                | '\u{200D}'
                | '\u{2060}'
                | '\u{202A}'
                | '\u{202B}'
                | '\u{202C}'
                | '\u{202D}'
                | '\u{202E}'
                | '\u{2066}'
                | '\u{2067}'
                | '\u{2068}'
                | '\u{2069}'
        )
    })
}

fn sanitize_text(bytes: Vec<u8>) -> Result<(String, bool)> {
    if bytes.contains(&0) {
        bail!("binary content is not allowed for text/snippet reads");
    }
    let decoded = String::from_utf8(bytes).map_err(|_| anyhow!("invalid UTF-8 payload"))?;
    let mut out = String::with_capacity(decoded.len());
    for ch in decoded.chars() {
        if ch == '\r' {
            out.push('\n');
            continue;
        }
        if ch.is_control() && ch != '\n' && ch != '\t' {
            out.push(' ');
            continue;
        }
        out.push(ch);
    }
    let has_bidi_controls = detect_bidi_or_invisible_controls(&out);
    Ok((out, has_bidi_controls))
}

fn as_u64_field(value: Option<&Value>, label: &str) -> Result<Option<u64>> {
    match value {
        None => Ok(None),
        Some(v) => v
            .as_u64()
            .map(Some)
            .ok_or_else(|| anyhow!("{label} must be a positive integer")),
    }
}

fn parse_array_params(req: &IpcRequest) -> Result<&Vec<Value>> {
    req.params
        .as_array()
        .ok_or_else(|| anyhow!("params must be an array"))
}

fn load_capabilities_for_webview(
    state: &AppState,
    webview_id: &str,
) -> Result<AppRuntimeCapabilities> {
    state
        .app_capabilities_for(webview_id)
        .ok_or_else(|| anyhow!("IPFS capability is not available for this webview"))
}

fn parse_cid_path(params: &[Value]) -> Result<(String, String)> {
    let cid = params
        .first()
        .and_then(|v| v.as_str())
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
        .ok_or_else(|| anyhow!("cid is required"))?;
    let path = normalize_path(params.get(1).and_then(|v| v.as_str()))?;
    Ok((cid, path))
}

fn emit_ipfs_progress(
    state: &AppState,
    webview_id: &str,
    ipc_id: u64,
    method: &str,
    phase: &str,
    percent: u8,
    message: impl Into<String>,
    cid: Option<&str>,
    path: Option<&str>,
) {
    let mut value = json!({
        "ipcId": ipc_id,
        "method": method,
        "phase": phase,
        "percent": percent,
        "message": message.into(),
    });
    if let Some(cid) = cid {
        value["cid"] = Value::String(cid.to_string());
    }
    if let Some(path) = path {
        value["path"] = Value::String(path.to_string());
    }
    let _ = state.proxy.send_event(UserEvent::ProviderEvent {
        webview_id: webview_id.to_string(),
        event: IPFS_PROGRESS_EVENT.to_string(),
        value,
    });
}

fn load_manifest_listing(
    state: &AppState,
    cid: &str,
    mut on_progress: impl FnMut(u8, &str),
) -> Result<BundleManifest> {
    let resolved = state
        .resolved
        .as_ref()
        .ok_or_else(|| anyhow!("resolved config unavailable"))?;
    on_progress(12, "Fetching manifest.json from IPFS...");
    let raw = match resolved.ipfs_fetch_backend {
        IpfsFetchBackend::LocalNode => {
            let gateway = normalize_gateway(&resolved.ipfs_gateway);
            let url = format!("{}/ipfs/{}/manifest.json", gateway, cid);
            let res = resolved.http_client.get(url).send()?;
            if !res.status().is_success() {
                let body = res.text().unwrap_or_default();
                bail!("failed to fetch manifest: {}", body);
            }
            res.bytes()?.to_vec()
        }
        IpfsFetchBackend::Helia => {
            let mut helper = IpfsHelperBridge::spawn(IpfsHelperConfig {
                gateways: resolved.ipfs_helia_gateways.clone(),
                routers: resolved.ipfs_helia_routers.clone(),
            })?;
            let url = format!("ipfs://{cid}/manifest.json");
            let result = helper.fetch(&url, Some(resolved.ipfs_helia_timeout_ms))?;
            if !(200..300).contains(&result.status) {
                bail!("failed to fetch manifest with status {}", result.status);
            }
            result.body
        }
    };
    on_progress(58, "Parsing manifest.json...");
    let manifest: BundleManifest = serde_json::from_slice(&raw)?;
    Ok(manifest)
}

fn fetch_ipfs_bytes(
    state: &AppState,
    cid: &str,
    path: &str,
    max_bytes: usize,
    mut on_progress: impl FnMut(u8, &str),
) -> Result<(Vec<u8>, Option<String>)> {
    let resolved = state
        .resolved
        .as_ref()
        .ok_or_else(|| anyhow!("resolved config unavailable"))?;
    on_progress(18, "Fetching file from IPFS...");
    match resolved.ipfs_fetch_backend {
        IpfsFetchBackend::LocalNode => {
            let gateway = normalize_gateway(&resolved.ipfs_gateway);
            let path_part = if path.is_empty() {
                String::new()
            } else {
                format!("/{}", path)
            };
            let url = format!("{}/ipfs/{}{}", gateway, cid, path_part);
            let res = resolved.http_client.get(url).send()?;
            if !res.status().is_success() {
                let body = res.text().unwrap_or_default();
                bail!("ipfs fetch failed: {}", body);
            }
            on_progress(52, "Downloading file bytes...");
            if let Some(len) = res.content_length() {
                if len > max_bytes as u64 {
                    bail!("payload exceeds maxBytes");
                }
            }
            let content_type = res
                .headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok())
                .map(|s| s.to_string());
            let bytes = res.bytes()?.to_vec();
            on_progress(82, "Validating payload constraints...");
            if bytes.len() > max_bytes {
                bail!("payload exceeds maxBytes");
            }
            Ok((bytes, content_type))
        }
        IpfsFetchBackend::Helia => {
            let mut helper = IpfsHelperBridge::spawn(IpfsHelperConfig {
                gateways: resolved.ipfs_helia_gateways.clone(),
                routers: resolved.ipfs_helia_routers.clone(),
            })?;
            let url = if path.is_empty() {
                format!("ipfs://{cid}")
            } else {
                format!("ipfs://{cid}/{path}")
            };
            let result = helper.fetch(&url, Some(resolved.ipfs_helia_timeout_ms))?;
            if !(200..300).contains(&result.status) {
                bail!("ipfs fetch failed with status {}", result.status);
            }
            on_progress(74, "Validating payload constraints...");
            if result.body.len() > max_bytes {
                bail!("payload exceeds maxBytes");
            }
            Ok((result.body, guess_mime_from_path(path)))
        }
    }
}

fn handle_head(
    state: &AppState,
    webview_id: &str,
    caps: &AppRuntimeCapabilities,
    req: &IpcRequest,
) -> Result<Option<Value>> {
    let params = parse_array_params(req)?;
    let (cid, path) = parse_cid_path(params)?;
    let emit = |phase: &str, percent: u8, message: &str| {
        emit_ipfs_progress(
            state,
            webview_id,
            req.id,
            req.method.as_str(),
            phase,
            percent,
            message,
            Some(cid.as_str()),
            Some(path.as_str()),
        );
    };
    emit("start", 2, "Starting metadata read...");

    let matching = find_matching_rules(caps, &cid, &path, None);
    if matching.is_empty() {
        bail!("ipfs capability denied");
    }
    let max_bytes = resolve_max_bytes(&matching, None);
    let (bytes, content_type) =
        fetch_ipfs_bytes(state, &cid, &path, max_bytes, |percent, message| {
            emit("fetch", percent, message)
        })?;
    emit("done", 100, "Metadata read complete.");

    Ok(Some(json!({
        "cid": cid,
        "path": path,
        "size": bytes.len(),
        "contentType": content_type
    })))
}

fn handle_list(
    state: &AppState,
    webview_id: &str,
    caps: &AppRuntimeCapabilities,
    req: &IpcRequest,
) -> Result<Option<Value>> {
    let params = parse_array_params(req)?;
    let cid = params
        .first()
        .and_then(|v| v.as_str())
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
        .ok_or_else(|| anyhow!("cid is required"))?;
    let base_path = normalize_path(params.get(1).and_then(|v| v.as_str()))?;
    let emit = |phase: &str, percent: u8, message: &str| {
        emit_ipfs_progress(
            state,
            webview_id,
            req.id,
            req.method.as_str(),
            phase,
            percent,
            message,
            Some(cid.as_str()),
            Some(base_path.as_str()),
        );
    };
    emit("start", 2, "Loading bundle manifest...");

    let matching = find_matching_rules(caps, &cid, &base_path, None);
    if matching.is_empty() {
        bail!("ipfs capability denied");
    }
    let manifest = load_manifest_listing(state, &cid, |percent, message| {
        emit("manifest", percent, message)
    })?;
    emit("filter", 80, "Filtering manifest files...");

    let files: Vec<Value> = manifest
        .files
        .into_iter()
        .filter(|f| {
            if base_path.is_empty() {
                true
            } else {
                f.path.starts_with(&base_path)
            }
        })
        .map(|f| json!({ "path": f.path, "bytes": f.bytes }))
        .collect();
    emit("done", 100, "Manifest list complete.");

    Ok(Some(json!({
        "cid": cid,
        "path": base_path,
        "files": files
    })))
}

fn handle_read(
    state: &AppState,
    webview_id: &str,
    caps: &AppRuntimeCapabilities,
    req: &IpcRequest,
) -> Result<Option<Value>> {
    let params = parse_array_params(req)?;
    let (cid, path) = parse_cid_path(params)?;
    let options = params
        .get(2)
        .and_then(|v| v.as_object())
        .ok_or_else(|| anyhow!("options object is required"))?;
    let as_kind = options
        .get("as")
        .and_then(|v| v.as_str())
        .map(|v| v.to_lowercase())
        .ok_or_else(|| anyhow!("options.as is required"))?;
    if !matches!(as_kind.as_str(), "json" | "text" | "snippet" | "image") {
        bail!("options.as must be one of json|text|snippet|image");
    }
    let emit = |phase: &str, percent: u8, message: &str| {
        emit_ipfs_progress(
            state,
            webview_id,
            req.id,
            req.method.as_str(),
            phase,
            percent,
            message,
            Some(cid.as_str()),
            Some(path.as_str()),
        );
    };
    emit("start", 2, "Starting file read...");

    let matching = find_matching_rules(caps, &cid, &path, Some(as_kind.as_str()));
    if matching.is_empty() {
        bail!("ipfs capability denied");
    }

    let requested_max = as_u64_field(options.get("maxBytes"), "maxBytes")?.map(|v| v as usize);
    let max_bytes = resolve_max_bytes(&matching, requested_max);
    let (bytes, content_type) =
        fetch_ipfs_bytes(state, &cid, &path, max_bytes, |percent, message| {
            emit("fetch", percent, message)
        })?;

    match as_kind.as_str() {
        "json" => {
            emit("decode", 90, "Decoding JSON payload...");
            let text = String::from_utf8(bytes)
                .map_err(|_| anyhow!("json payload must be valid UTF-8"))?;
            let value: Value =
                serde_json::from_str(&text).map_err(|_| anyhow!("invalid JSON payload"))?;
            emit("done", 100, "JSON read complete.");
            Ok(Some(json!({
                "kind": "json",
                "cid": cid,
                "path": path,
                "value": value
            })))
        }
        "text" => {
            emit("decode", 90, "Sanitizing text payload...");
            let (text, has_bidi_controls) = sanitize_text(bytes)?;
            emit("done", 100, "Text read complete.");
            Ok(Some(json!({
                "kind": "text",
                "cid": cid,
                "path": path,
                "text": text,
                "hasBidiControls": has_bidi_controls
            })))
        }
        "snippet" => {
            emit("decode", 90, "Preparing snippet window...");
            let (text, has_bidi_controls) = sanitize_text(bytes)?;
            let lines: Vec<&str> = text.split('\n').collect();
            let start_line = options
                .get("startLine")
                .and_then(|v| v.as_u64())
                .unwrap_or(1) as usize;
            let requested_end_line = options
                .get("endLine")
                .and_then(|v| v.as_u64())
                .map(|v| v as usize);
            let max_lines = options
                .get("maxLines")
                .and_then(|v| v.as_u64())
                .map(|v| v as usize)
                .unwrap_or(MAX_SNIPPET_LINES_DEFAULT);

            let start = max(1, start_line);
            let mut end = requested_end_line.unwrap_or(start + max_lines.saturating_sub(1));
            end = min(end, start + max_lines.saturating_sub(1));
            end = min(end, lines.len());

            let start_idx = start.saturating_sub(1);
            let end_idx = end;
            let snippet_lines = if start_idx >= lines.len() {
                Vec::new()
            } else {
                lines[start_idx..end_idx].to_vec()
            };
            let snippet = snippet_lines.join("\n");
            emit("done", 100, "Snippet read complete.");

            Ok(Some(json!({
                "kind": "snippet",
                "cid": cid,
                "path": path,
                "text": snippet,
                "lineStart": start,
                "lineEnd": end,
                "truncatedHead": start > 1,
                "truncatedTail": end < lines.len(),
                "hasBidiControls": has_bidi_controls
            })))
        }
        "image" => {
            emit("decode", 90, "Validating image payload...");
            let mime = content_type.unwrap_or_else(|| "application/octet-stream".to_string());
            if !mime.starts_with("image/") || mime.contains("svg") {
                bail!("image reads only support raster image payloads");
            }
            emit("done", 100, "Image read complete.");
            Ok(Some(json!({
                "kind": "image",
                "cid": cid,
                "path": path,
                "contentType": mime,
                "dataHex": hex::encode(bytes)
            })))
        }
        _ => Err(anyhow!("unsupported read kind")),
    }
}

pub(super) fn handle_ipfs_ipc(
    state: &AppState,
    webview_id: &str,
    req: &IpcRequest,
) -> Result<Option<Value>> {
    let caps = load_capabilities_for_webview(state, webview_id)?;
    let result = match req.method.as_str() {
        "vibefi_ipfsHead" => handle_head(state, webview_id, &caps, req),
        "vibefi_ipfsList" => handle_list(state, webview_id, &caps, req),
        "vibefi_ipfsRead" => handle_read(state, webview_id, &caps, req),
        _ => Err(anyhow!("unsupported IPFS method: {}", req.method)),
    };

    if let Err(err) = &result {
        emit_ipfs_progress(
            state,
            webview_id,
            req.id,
            req.method.as_str(),
            "error",
            100,
            format!("IPFS request failed: {err}"),
            None,
            None,
        );
    }

    result
}

#[cfg(test)]
mod tests {
    use super::path_matches;

    #[test]
    fn wildcard_patterns_require_path_segment_boundaries() {
        assert!(path_matches("src/**", "src"));
        assert!(path_matches("src/**", "src/index.ts"));
        assert!(path_matches("src/*", "src/index.ts"));
        assert!(!path_matches("src/*", "src/nested/index.ts"));
        assert!(!path_matches("src/**", "src-malicious/index.ts"));
        assert!(!path_matches("src/*", "src-malicious/index.ts"));
    }
}
