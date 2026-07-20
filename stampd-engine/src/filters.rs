//! Filter hook system — spawns external scripts at SMTP stages.
//!
//! Filters are directories in `filters_dir` containing:
//! - `config.toml`: name, hooks (mail_from/rcpt_to/data), enabled
//! - Executable scripts named `mail_from.py`, `rcpt_to.py`, `data.py`
//!
//! Context passed via stdin as JSON, result read from stdout as JSON:
//! {"action": "accept"|"reject", "reason": "..."}
//!
//! Timeout enforced per-filter via `filters.timeout_ms`.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;
use tracing::{info, warn, error};
use serde::{Deserialize, Serialize};

/// Which SMTP hook point a filter handles.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
/// Returns Ok(()) if all filters accept, or Err(reason) if any rejects.
pub async fn run_filters(
    filters_dir: &Path,
    hook: HookPoint,
    context: &FilterContext,
    timeout_ms: u64,
) -> Result<(), String> {
    if !filters_dir.exists() {
        return Ok(()); // No filters directory
    }

    let filters = load_filters(filters_dir)
        .map_err(|e| format!("Failed to load filters: {}", e))?;

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
            info!(filter = %filter.name, hook = ?hook, script = %script_path.display(), "Running filter");

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
                            name: config.name.unwrap_or_else(|| dir.file_name().unwrap().to_string_lossy().to_string()),
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
        "py" => ("python3".to_string(), vec![script.to_string_lossy().to_string()]),
        "sh" => ("bash".to_string(), vec![script.to_string_lossy().to_string()]),
        "js" => ("node".to_string(), vec![script.to_string_lossy().to_string()]),
        "ts" => ("bun".to_string(), vec!["run".to_string(), script.to_string_lossy().to_string()]),
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
        stdin.write_all(json.as_bytes()).await
            .map_err(|e| format!("Failed to write to stdin: {}", e))?;
        stdin.shutdown().await
            .map_err(|e| format!("Failed to close stdin: {}", e))?;
    }

    // Wait with timeout
    let output = tokio::time::timeout(
        Duration::from_millis(timeout_ms),
        child.wait_with_output(),
    )
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
        stdin.write_all(json.as_bytes()).await
            .map_err(|e| format!("Failed to write to stdin: {}", e))?;
        stdin.shutdown().await
            .map_err(|e| format!("Failed to close stdin: {}", e))?;
    }

    let output = tokio::time::timeout(
        Duration::from_millis(timeout_ms),
        child.wait_with_output(),
    )
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
