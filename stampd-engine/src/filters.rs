//! Filter hook system — delegates to Transit Python bridge via gateway.
//!
//! When `gateway_url` is configured, filters are executed via the gateway's
//! Transit Python bridge (resident process, fast). Otherwise, falls back to
//! spawning scripts directly.
//!
//! Gateway endpoint: POST /internal/filters/hook
//! Body: { "hook": "mail_from"|"rcpt_to"|"data", "context": {...}, "filters": [...] }
//! Response: { "ok": true } or { "ok": false, "reason": "..." }

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;
use tracing::{error, info, warn};

/// Which SMTP hook point a filter handles.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookPoint {
    MailFrom,
    RcptTo,
    Data,
}

/// Context passed to a filter script.
#[derive(Debug, Serialize)]
pub struct FilterContext {
    /// The hook point being called.
    pub hook: HookPoint,
    /// Sender address (from MAIL FROM).
    pub sender: String,
    /// Recipient address (from RCPT TO).
    pub recipient: String,
    /// Client HELO/EHLO domain.
    pub helo_domain: String,
    /// Client IP address.
    pub client_ip: String,
    /// Whether connection is TLS.
    pub tls: bool,
    /// Message headers (only for DATA hook).
    pub headers: Option<String>,
    /// Message body (only for DATA hook).
    pub body: Option<String>,
}

/// Result from a filter script.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilterResult {
    /// "accept" or "reject".
    pub action: String,
    /// Human-readable reason.
    #[serde(default)]
    pub reason: String,
}

/// A loaded filter configuration.
#[derive(Debug, Clone)]
pub struct Filter {
    pub name: String,
    pub path: PathBuf,
    pub hooks: Vec<HookPoint>,
    pub enabled: bool,
}

/// Run all enabled filters for a given hook point.
///
/// If `gateway_url` is set, delegates to the gateway's Transit Python bridge.
/// Otherwise, falls back to spawning scripts directly.
///
/// Returns Ok(()) if all filters accept, or Err(reason) if any rejects.
pub async fn run_filters(
    filters_dir: &Path,
    hook: HookPoint,
    context: &FilterContext,
    timeout_ms: u64,
    gateway_url: Option<&str>,
) -> Result<(), String> {
    // Try Transit delegation first
    if let Some(gw_url) = gateway_url {
        match run_filters_via_transit(gw_url, hook, context, filters_dir).await {
            Ok(()) => return Ok(()),
            Err(e) => {
                warn!(error = %e, "Transit filter delegation failed, falling back to script spawning");
            }
        }
    }

    // Fallback: script spawning
    run_filters_via_scripts(filters_dir, hook, context, timeout_ms).await
}

/// Run filters via the gateway's Transit Python bridge.
async fn run_filters_via_transit(
    gateway_url: &str,
    hook: HookPoint,
    context: &FilterContext,
    filters_dir: &Path,
) -> Result<(), String> {
    // Load filter configs to get enabled filter function names
    let filters =
        load_filters(filters_dir).map_err(|e| format!("Failed to load filters: {}", e))?;

    let hook_name = match hook {
        HookPoint::MailFrom => "mail_from",
        HookPoint::RcptTo => "rcpt_to",
        HookPoint::Data => "data",
    };

    // Map filter names to Transit function names
    let enabled_filters: Vec<String> = filters
        .iter()
        .filter(|f| f.enabled && f.hooks.contains(&hook))
        .map(|f| {
            // Convert filter name to camelCase function name
            // e.g., "block-sender" -> "blockSender", "spam-keywords" -> "spamKeywords"
            let name = f.name.replace('-', "_");
            let parts: Vec<&str> = name.split('_').collect();
            let mut result = String::new();
            for (i, part) in parts.iter().enumerate() {
                if i == 0 {
                    result.push_str(part);
                } else {
                    let mut chars = part.chars();
                    if let Some(first) = chars.next() {
                        result.push(first.to_uppercase().next().unwrap_or(first));
                        result.push_str(chars.as_str());
                    }
                }
            }
            result
        })
        .collect();

    if enabled_filters.is_empty() {
        return Ok(()); // No filters for this hook
    }

    let context_json =
        serde_json::to_value(context).map_err(|e| format!("Failed to serialize context: {}", e))?;

    let request_body = serde_json::json!({
        "hook": hook_name,
        "context": context_json,
        "filters": enabled_filters,
    });

    let url = format!("{}/internal/filters/hook", gateway_url);

    let client = reqwest::Client::builder()
        .timeout(Duration::from_millis(5000))
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {}", e))?;

    let response = client
        .post(&url)
        .json(&request_body)
        .send()
        .await
        .map_err(|e| format!("Failed to call gateway filter endpoint: {}", e))?;

    let status = response.status();
    let body: serde_json::Value = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse gateway response: {}", e))?;

    if !status.is_success() {
        return Err(format!("Gateway returned error status: {}", status));
    }

    if body.get("ok").and_then(|v| v.as_bool()) == Some(false) {
        let reason = body
            .get("reason")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");
        return Err(reason.to_string());
    }

    Ok(())
}

/// Run filters by spawning scripts directly (legacy fallback).
async fn run_filters_via_scripts(
    filters_dir: &Path,
    hook: HookPoint,
    context: &FilterContext,
    timeout_ms: u64,
) -> Result<(), String> {
    if !filters_dir.exists() {
        return Ok(()); // No filters directory
    }

    let filters =
        load_filters(filters_dir).map_err(|e| format!("Failed to load filters: {}", e))?;

    for filter in &filters {
        if !filter.enabled {
            continue;
        }
        if !filter.hooks.contains(&hook) {
            continue;
        }

        let script_name = match hook {
            HookPoint::MailFrom => "mail_from",
            HookPoint::RcptTo => "rcpt_to",
            HookPoint::Data => "data",
        };

        // Look for script with common extensions
        let script_path = find_script(&filter.path, script_name);
        if let Some(script_path) = script_path {
            info!(filter = %filter.name, hook = ?hook, script = %script_path.display(), "Running filter via script");

            match run_filter_script(&script_path, context, timeout_ms).await {
                Ok(result) => {
                    if result.action == "reject" {
                        warn!(
                            filter = %filter.name,
                            hook = ?hook,
                            reason = %result.reason,
                            "Filter rejected"
                        );
                        return Err(result.reason);
                    }
                    info!(filter = %filter.name, hook = ?hook, "Filter accepted");
                }
                Err(e) => {
                    error!(
                        filter = %filter.name,
                        hook = ?hook,
                        error = %e,
                        "Filter execution failed"
                    );
                    // On error, continue (don't block mail delivery)
                }
            }
        }
    }

    Ok(())
}

/// Load all filters from the filters directory.
fn load_filters(filters_dir: &Path) -> anyhow::Result<Vec<Filter>> {
    let mut filters = Vec::new();

    if let Ok(entries) = std::fs::read_dir(filters_dir) {
        for entry in entries.flatten() {
            let dir = entry.path();
            if !dir.is_dir() {
                continue;
            }

            let config_path = dir.join("config.toml");
            if config_path.exists() {
                if let Ok(content) = std::fs::read_to_string(&config_path) {
                    if let Ok(config) = toml::from_str::<FilterConfig>(&content) {
                        filters.push(Filter {
                            name: config.name.unwrap_or_else(|| {
                                dir.file_name().unwrap().to_string_lossy().to_string()
                            }),
                            path: dir,
                            hooks: config.hooks,
                            enabled: config.enabled.unwrap_or(true),
                        });
                    }
                }
            }
        }
    }

    Ok(filters)
}

/// Find an executable script with common extensions.
fn find_script(filter_dir: &Path, name: &str) -> Option<PathBuf> {
    let extensions = ["", ".py", ".sh", ".js", ".ts"];
    for ext in &extensions {
        let path = filter_dir.join(format!("{}{}", name, ext));
        if path.exists() {
            return Some(path);
        }
    }
    None
}

/// Run a filter script with timeout.
async fn run_filter_script(
    script: &Path,
    context: &FilterContext,
    timeout_ms: u64,
) -> Result<FilterResult, String> {
    let json = serde_json::to_string(context)
        .map_err(|e| format!("Failed to serialize context: {}", e))?;

    let ext = script.extension().and_then(|e| e.to_str()).unwrap_or("");
    let (program, args) = match ext {
        "py" => (
            "python3".to_string(),
            vec![script.to_string_lossy().to_string()],
        ),
        "sh" => (
            "bash".to_string(),
            vec![script.to_string_lossy().to_string()],
        ),
        "js" => (
            "node".to_string(),
            vec![script.to_string_lossy().to_string()],
        ),
        "ts" => (
            "bun".to_string(),
            vec!["run".to_string(), script.to_string_lossy().to_string()],
        ),
        _ => {
            // Try as executable directly
            return run_executable(script, &json, timeout_ms).await;
        }
    };

    let mut child = Command::new(&program)
        .args(&args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| format!("Failed to spawn {}: {}", program, e))?;

    // Write context to stdin
    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(json.as_bytes())
            .await
            .map_err(|e| format!("Failed to write to stdin: {}", e))?;
        stdin
            .shutdown()
            .await
            .map_err(|e| format!("Failed to close stdin: {}", e))?;
    }

    // Wait with timeout
    let output = tokio::time::timeout(Duration::from_millis(timeout_ms), child.wait_with_output())
        .await
        .map_err(|_| format!("Filter timed out after {}ms", timeout_ms))?
        .map_err(|e| format!("Failed to wait for filter: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("Filter exited with {}: {}", output.status, stderr));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let result: FilterResult = serde_json::from_str(&stdout)
        .map_err(|e| format!("Failed to parse filter output: {} — stdout: {}", e, stdout))?;

    Ok(result)
}

/// Run an executable directly (no interpreter needed).
async fn run_executable(
    script: &Path,
    json: &str,
    timeout_ms: u64,
) -> Result<FilterResult, String> {
    let mut child = Command::new(script)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| format!("Failed to spawn {}: {}", script.display(), e))?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(json.as_bytes())
            .await
            .map_err(|e| format!("Failed to write to stdin: {}", e))?;
        stdin
            .shutdown()
            .await
            .map_err(|e| format!("Failed to close stdin: {}", e))?;
    }

    let output = tokio::time::timeout(Duration::from_millis(timeout_ms), child.wait_with_output())
        .await
        .map_err(|_| format!("Filter timed out after {}ms", timeout_ms))?
        .map_err(|e| format!("Failed to wait for filter: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("Filter exited with {}: {}", output.status, stderr));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let result: FilterResult = serde_json::from_str(&stdout)
        .map_err(|e| format!("Failed to parse filter output: {} — stdout: {}", e, stdout))?;

    Ok(result)
}

/// Config structure for a filter's config.toml.
#[derive(Debug, Deserialize)]
struct FilterConfig {
    name: Option<String>,
    hooks: Vec<HookPoint>,
    enabled: Option<bool>,
}
