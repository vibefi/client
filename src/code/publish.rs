use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

use super::settings::{UploadConfig, UploadProvider};
use super::validator;
use crate::bundle;

const BUNDLE_DIRS: &[&str] = &["src", "assets", "abis"];
const BUNDLE_FILES: &[&str] = &["vibefi.json", "index.html"];
const STATIC_HTML_ALLOWED_EXTENSIONS: &[&str] = &[".html", ".js", ".json"];
const IGNORED_TOP_LEVEL: &[&str] = &["node_modules", "dist", "coverage", ".vibefi"];

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PackageResult {
    pub root_cid: String,
    pub name: String,
    pub version: String,
    pub description: String,
    pub dapp_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BundleLayout {
    Constrained,
    StaticHtml,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProjectManifest {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    version: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    fork_of: Option<ForkOfManifest>,
    #[serde(default)]
    constraints: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ForkOfManifest {
    #[serde(default)]
    dapp_id: Option<String>,
    #[serde(default)]
    name: Option<String>,
}

fn detect_layout(project_root: &Path) -> Result<BundleLayout> {
    let has_src = project_root.join("src").is_dir();
    let has_assets = project_root.join("assets").is_dir();
    let has_abis = project_root.join("abis").is_dir();
    let has_vibefi = project_root.join("vibefi.json").is_file();
    let has_index = project_root.join("index.html").is_file();
    let has_pkg = project_root.join("package.json").is_file();

    if has_src && has_assets && has_abis && has_vibefi && has_index && has_pkg {
        return Ok(BundleLayout::Constrained);
    }
    if has_vibefi && has_index {
        return Ok(BundleLayout::StaticHtml);
    }
    bail!(
        "Unsupported dapp layout. Expected either constrained layout \
         (src/, assets/, abis/, vibefi.json, index.html, package.json) \
         or static-html layout (vibefi.json, index.html)."
    );
}

fn collect_constrained_files(project_root: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for dir_name in BUNDLE_DIRS {
        let dir = project_root.join(dir_name);
        if dir.is_dir() {
            files.extend(bundle::walk_files(&dir)?);
        }
    }
    for file_name in BUNDLE_FILES {
        let file = project_root.join(file_name);
        if file.is_file() {
            files.push(file);
        }
    }
    Ok(files)
}

fn collect_static_html_files(project_root: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    collect_static_recursive(project_root, project_root, 0, &mut files)?;
    Ok(files)
}

fn collect_static_recursive(
    root: &Path,
    current: &Path,
    depth: usize,
    files: &mut Vec<PathBuf>,
) -> Result<()> {
    for entry in fs::read_dir(current)? {
        let entry = entry?;
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if name_str.starts_with('.') {
            continue;
        }
        if depth == 0 && IGNORED_TOP_LEVEL.contains(&name_str.as_ref()) {
            continue;
        }
        let path = entry.path();
        let file_type = entry.file_type()?;
        if file_type.is_symlink() {
            continue;
        }
        if file_type.is_dir() {
            collect_static_recursive(root, &path, depth + 1, files)?;
            continue;
        }
        let rel = path.strip_prefix(root).unwrap_or(&path).to_string_lossy();
        if rel == "manifest.json" {
            continue;
        }
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| format!(".{}", e.to_lowercase()));
        let ext_ref = ext.as_deref().unwrap_or("");
        if !STATIC_HTML_ALLOWED_EXTENSIONS.contains(&ext_ref) {
            bail!(
                "Static-html layout does not allow file type: {} (extension {})",
                rel,
                if ext_ref.is_empty() {
                    "<none>"
                } else {
                    ext_ref
                }
            );
        }
        files.push(path);
    }
    Ok(())
}

fn read_project_manifest(project_root: &Path) -> Result<ProjectManifest> {
    let manifest_path = project_root.join("manifest.json");
    let raw = fs::read_to_string(&manifest_path)
        .with_context(|| format!("read {}", manifest_path.display()))?;
    serde_json::from_str(&raw).with_context(|| format!("parse {}", manifest_path.display()))
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct BundleManifestFile {
    path: String,
    bytes: u64,
}

fn build_bundle_manifest(
    project_root: &Path,
    bundle_files: &[PathBuf],
    name: &str,
    version: &str,
    description: &str,
    layout: BundleLayout,
    constraints: Option<&serde_json::Value>,
) -> serde_json::Value {
    let mut file_entries: Vec<BundleManifestFile> = bundle_files
        .iter()
        .filter_map(|file| {
            let rel = file
                .strip_prefix(project_root)
                .ok()?
                .to_string_lossy()
                .replace('\\', "/");
            let bytes = fs::metadata(file).ok()?.len();
            Some(BundleManifestFile { path: rel, bytes })
        })
        .collect();
    file_entries.sort_by(|a, b| a.path.cmp(&b.path));

    let layout_constraints = match layout {
        BundleLayout::Constrained => constraints
            .cloned()
            .unwrap_or_else(|| serde_json::json!({ "type": "default" })),
        BundleLayout::StaticHtml => serde_json::json!({ "type": "static-html" }),
    };

    serde_json::json!({
        "name": name,
        "version": version,
        "description": description,
        "createdAt": chrono_now_iso(),
        "layout": match layout {
            BundleLayout::Constrained => "constrained",
            BundleLayout::StaticHtml => "static-html",
        },
        "constraints": layout_constraints,
        "entry": "index.html",
        "files": file_entries,
    })
}

fn chrono_now_iso() -> String {
    use std::time::SystemTime;
    let now = SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    // Simple ISO 8601 without chrono dependency
    let secs = now.as_secs();
    let days = secs / 86400;
    let time_of_day = secs % 86400;
    let hours = time_of_day / 3600;
    let minutes = (time_of_day % 3600) / 60;
    let seconds = time_of_day % 60;
    let millis = now.subsec_millis();

    // Approximate date calculation (good enough for timestamps)
    let mut y = 1970i64;
    let mut remaining_days = days as i64;
    loop {
        let leap = y % 4 == 0 && (y % 100 != 0 || y % 400 == 0);
        let days_in_year: i64 = if leap { 366 } else { 365 };
        if remaining_days < days_in_year {
            break;
        }
        remaining_days -= days_in_year;
        y += 1;
    }
    let leap = y % 4 == 0 && (y % 100 != 0 || y % 400 == 0);
    let month_days: [i64; 12] = [
        31,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    let mut m = 0usize;
    for (i, &md) in month_days.iter().enumerate() {
        if remaining_days < md {
            m = i;
            break;
        }
        remaining_days -= md;
    }

    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z",
        y,
        m + 1,
        remaining_days + 1,
        hours,
        minutes,
        seconds,
        millis
    )
}

fn write_bundle(
    project_root: &Path,
    out_dir: &Path,
    bundle_files: &[PathBuf],
    manifest_json: &serde_json::Value,
) -> Result<()> {
    if out_dir.exists() {
        fs::remove_dir_all(out_dir)
            .with_context(|| format!("clean bundle dir {}", out_dir.display()))?;
    }
    fs::create_dir_all(out_dir)
        .with_context(|| format!("create bundle dir {}", out_dir.display()))?;

    for file in bundle_files {
        let rel = file
            .strip_prefix(project_root)
            .with_context(|| format!("strip prefix for {}", file.display()))?;
        let dest = out_dir.join(rel);
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(file, &dest)
            .with_context(|| format!("copy {} to {}", file.display(), dest.display()))?;
    }

    let manifest_str =
        serde_json::to_string_pretty(manifest_json).context("serialize bundle manifest")?;
    fs::write(out_dir.join("manifest.json"), manifest_str).context("write bundle manifest")?;

    Ok(())
}

fn build_upload_form(
    out_dir: &Path,
    field_name: &str,
) -> Result<reqwest::blocking::multipart::Form> {
    let mut form = reqwest::blocking::multipart::Form::new();

    let files = bundle::walk_files(out_dir)?;
    // Also include manifest.json which walk_files skips
    let manifest_path = out_dir.join("manifest.json");
    let mut all_files = files;
    if manifest_path.is_file() {
        all_files.push(manifest_path);
    }

    for file in &all_files {
        let rel = file
            .strip_prefix(out_dir)
            .unwrap_or(file)
            .to_string_lossy()
            .replace('\\', "/");
        let data = fs::read(file)
            .with_context(|| format!("read file for IPFS upload: {}", file.display()))?;
        let part = reqwest::blocking::multipart::Part::bytes(data).file_name(rel);
        form = form.part(field_name.to_string(), part);
    }
    Ok(form)
}

fn parse_ipfs_add_cid(body: &str) -> Result<String> {
    let lines: Vec<&str> = body.trim().split('\n').filter(|l| !l.is_empty()).collect();
    if lines.is_empty() {
        bail!("IPFS add returned empty response");
    }
    let last: serde_json::Value =
        serde_json::from_str(lines[lines.len() - 1]).context("parse IPFS add response")?;
    let cid = last
        .get("Hash")
        .or_else(|| last.get("Cid").and_then(|c| c.get("/")))
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("IPFS add response missing CID"))?;
    Ok(cid.to_string())
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProtocolRelayUploadResponse {
    root_cid: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProtocolRelayErrorResponse {
    error: ProtocolRelayErrorDetail,
    #[serde(default)]
    request_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ProtocolRelayErrorDetail {
    #[serde(default)]
    code: Option<String>,
    #[serde(default)]
    message: Option<String>,
}

fn protocol_relay_upload_url(endpoint: &str) -> Result<reqwest::Url> {
    let mut url = reqwest::Url::parse(endpoint)
        .with_context(|| format!("invalid protocol relay endpoint URL: {}", endpoint))?;
    if !matches!(url.scheme(), "http" | "https") {
        bail!("Protocol relay endpoint must use http or https");
    }
    if url.path() != "/" && !url.path().is_empty() {
        bail!(
            "Protocol relay endpoint must be a base URL without a path (example: https://ipfs.vibefi.dev)"
        );
    }
    if url.query().is_some() || url.fragment().is_some() {
        bail!(
            "Protocol relay endpoint must be a base URL without query/fragment (example: https://ipfs.vibefi.dev)"
        );
    }
    url.set_path("/v1/uploads");
    Ok(url)
}

fn parse_protocol_relay_error(status: reqwest::StatusCode, body: &str) -> String {
    if let Ok(parsed) = serde_json::from_str::<ProtocolRelayErrorResponse>(body) {
        let code = parsed
            .error
            .code
            .unwrap_or_else(|| "UNKNOWN_ERROR".to_string());
        let message = parsed
            .error
            .message
            .unwrap_or_else(|| "relay upload failed".to_string());
        if let Some(request_id) = parsed.request_id.filter(|v| !v.trim().is_empty()) {
            return format!(
                "Protocol relay upload failed ({status}) [{code}]: {message} (requestId: {request_id})"
            );
        }
        return format!("Protocol relay upload failed ({status}) [{code}]: {message}");
    }
    let fallback_body = body.trim();
    if fallback_body.is_empty() {
        return format!("Protocol relay upload failed ({status})");
    }
    format!("Protocol relay upload failed ({status}): {fallback_body}")
}

fn upload_via_protocol_relay(
    out_dir: &Path,
    endpoint: &str,
    api_key: Option<&str>,
) -> Result<String> {
    let endpoint = endpoint.trim();
    if endpoint.is_empty() {
        bail!("Protocol relay endpoint is required");
    }
    let url = protocol_relay_upload_url(endpoint)?;
    let form = build_upload_form(out_dir, "file")?;
    let client = reqwest::blocking::Client::new();
    let mut request = client.post(url.clone()).multipart(form);
    if let Some(token) = api_key.map(str::trim).filter(|value| !value.is_empty()) {
        request = request.header("Authorization", format!("Bearer {}", token));
    }

    let response = request
        .send()
        .with_context(|| format!("Protocol relay upload request to {} failed", url))?;
    let status = response.status();
    let body = response
        .text()
        .context("read protocol relay response body")?;
    if !status.is_success() {
        bail!("{}", parse_protocol_relay_error(status, &body));
    }
    let parsed: ProtocolRelayUploadResponse =
        serde_json::from_str(&body).context("parse protocol relay response JSON")?;
    if parsed.root_cid.trim().is_empty() {
        bail!("Protocol relay response missing rootCid");
    }
    Ok(parsed.root_cid)
}

fn upload_via_ipfs_add(
    out_dir: &Path,
    endpoint: &str,
    bearer_token: Option<&str>,
    provider_label: &str,
) -> Result<String> {
    let endpoint = endpoint.trim();
    if endpoint.is_empty() {
        bail!("{provider_label} endpoint is required");
    }
    let endpoint = endpoint.trim_end_matches('/');
    let url = format!(
        "{}/api/v0/add?recursive=true&wrap-with-directory=true&cid-version=1&pin=true",
        endpoint
    );

    let form = build_upload_form(out_dir, "file")?;
    let client = reqwest::blocking::Client::new();
    let mut request = client.post(&url).multipart(form);
    if let Some(token) = bearer_token
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        request = request.header("Authorization", format!("Bearer {}", token));
    }

    let response = request
        .send()
        .with_context(|| format!("{provider_label} IPFS add request to {} failed", endpoint))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().unwrap_or_default();
        bail!("{provider_label} IPFS add failed: {} {}", status, body);
    }

    let body = response.text().context("read IPFS add response body")?;
    parse_ipfs_add_cid(&body)
}

fn upload_via_pinata(out_dir: &Path, endpoint: &str, api_key: Option<&str>) -> Result<String> {
    let endpoint = endpoint.trim();
    if endpoint.is_empty() {
        bail!("Pinata endpoint is required");
    }
    let token = api_key
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow!("Pinata API key/token is required"))?;
    let url = format!("{}/pinning/pinFileToIPFS", endpoint.trim_end_matches('/'));
    let form = build_upload_form(out_dir, "file")?;
    let response = reqwest::blocking::Client::new()
        .post(&url)
        .header("Authorization", format!("Bearer {}", token))
        .multipart(form)
        .send()
        .with_context(|| format!("Pinata upload request to {} failed", endpoint))?;
    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().unwrap_or_default();
        bail!("Pinata upload failed: {} {}", status, body);
    }
    let body = response.text().context("read Pinata response body")?;
    let value: serde_json::Value =
        serde_json::from_str(&body).context("parse Pinata response JSON")?;
    let cid = value
        .get("IpfsHash")
        .or_else(|| value.get("cid"))
        .or_else(|| value.get("Cid").and_then(|inner| inner.get("/")))
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("Pinata response missing CID"))?;
    Ok(cid.to_string())
}

fn upload_bundle(out_dir: &Path, upload_config: &UploadConfig) -> Result<String> {
    match upload_config.provider {
        UploadProvider::ProtocolRelay => {
            if upload_config.protocol_relay.endpoint.trim().is_empty() {
                bail!(
                    "Protocol relay is selected but no endpoint is configured. \
                     Set an endpoint in Publish settings, or choose 4EVERLAND, Pinata, or Local IPFS Node."
                );
            }
            upload_via_protocol_relay(
                out_dir,
                &upload_config.protocol_relay.endpoint,
                upload_config.protocol_relay.api_key.as_deref(),
            )
        }
        UploadProvider::FourEverland => {
            let token = upload_config
                .four_everland
                .access_token
                .as_deref()
                .ok_or_else(|| anyhow!("4EVERLAND access token is required"))?;
            upload_via_ipfs_add(
                out_dir,
                &upload_config.four_everland.endpoint,
                Some(token),
                "4EVERLAND",
            )
        }
        UploadProvider::Pinata => upload_via_pinata(
            out_dir,
            &upload_config.pinata.endpoint,
            upload_config.pinata.api_key.as_deref(),
        ),
        UploadProvider::LocalNode => upload_via_ipfs_add(
            out_dir,
            &upload_config.local_node.endpoint,
            None,
            "Local IPFS node",
        ),
    }
}

/// Full pipeline: validate → package → upload to IPFS → return result.
pub fn package_and_upload(
    project_root: &Path,
    upload_config: &UploadConfig,
    progress: &mut dyn FnMut(&str, u8, &str),
) -> Result<PackageResult> {
    // 1. Validate
    progress("validate", 5, "Validating project...");
    let errors = validator::validate_project(project_root).context("project validation failed")?;
    if !validator::is_valid(&errors) {
        let error_messages: Vec<String> = errors
            .iter()
            .filter(|e| e.severity == validator::ValidationSeverity::Error)
            .map(|e| {
                let location = e.file.as_deref().unwrap_or("<project>");
                format!("{}: {}", location, e.message)
            })
            .collect();
        bail!(
            "Project has validation errors:\n{}",
            error_messages.join("\n")
        );
    }

    // 2. Read manifest metadata
    progress("manifest", 10, "Reading project manifest...");
    let manifest = read_project_manifest(project_root)?;
    let name = manifest
        .name
        .as_deref()
        .filter(|s| !s.is_empty())
        .unwrap_or("unnamed")
        .to_string();
    let version = manifest
        .version
        .as_deref()
        .filter(|s| !s.is_empty())
        .unwrap_or("0.0.0")
        .to_string();
    let description = manifest.description.as_deref().unwrap_or("").to_string();
    let dapp_id = manifest.fork_of.as_ref().and_then(|f| f.dapp_id.clone());

    // 3. Detect layout and collect files
    progress("package", 20, "Detecting bundle layout...");
    let layout = detect_layout(project_root)?;
    let bundle_files = match layout {
        BundleLayout::Constrained => collect_constrained_files(project_root)?,
        BundleLayout::StaticHtml => collect_static_html_files(project_root)?,
    };
    progress(
        "package",
        30,
        &format!("Packaging {} files...", bundle_files.len()),
    );

    // 4. Build manifest and write bundle to temp dir
    let bundle_manifest = build_bundle_manifest(
        project_root,
        &bundle_files,
        &name,
        &version,
        &description,
        layout,
        manifest.constraints.as_ref(),
    );
    let out_dir = project_root.join(".vibefi").join("bundle");
    write_bundle(project_root, &out_dir, &bundle_files, &bundle_manifest)?;
    progress("package", 40, "Bundle written to temp directory");

    // 5. Upload to IPFS
    let upload_label = upload_config.provider.label();
    progress("upload", 50, &format!("Uploading via {}...", upload_label));
    let root_cid = upload_bundle(&out_dir, upload_config).context("upload failed")?;
    progress(
        "upload",
        95,
        &format!("Uploaded via {}: {}", upload_label, root_cid),
    );

    // 6. Clean up
    let _ = fs::remove_dir_all(&out_dir);
    progress("complete", 100, &format!("Published as {}", root_cid));

    Ok(PackageResult {
        root_cid,
        name,
        version,
        description,
        dapp_id,
    })
}
