//! SPF (Sender Policy Framework) checking — best-effort per spec.
//!
//! DNS lookup for SPF records, validates sending IP against sender domain.
//! Logs result but does NOT reject on softfail (best-effort per spec §11).

use std::net::IpAddr;
use tracing::{info, warn};
use trust_dns_resolver::TokioAsyncResolver;

/// Result of an SPF check.
#[derive(Debug, Clone)]
pub struct SpfResult {
    /// The SPF record found (if any).
    pub record: Option<String>,
    /// Whether the sender IP is authorized.
    pub passed: bool,
    /// Human-readable result message.
    pub message: String,
}

/// Check SPF for a sender domain and connecting IP.
///
/// This is best-effort: if DNS fails or no SPF record exists, we return
/// a soft-fail result but still accept the mail (per spec §11).
pub async fn check_spf(sender_domain: &str, sender_ip: IpAddr) -> SpfResult {
    // Default: if we can't check, we accept (best-effort)
    let default_pass = SpfResult {
        record: None,
        passed: true,
        message: "No SPF record found, accepting (best-effort)".to_string(),
    };

    if sender_domain.is_empty() {
        return SpfResult {
            record: None,
            passed: true,
            message: "Empty sender domain (bounce), accepting".to_string(),
        };
    }

    // Resolve TXT records for the domain
    let resolver = match TokioAsyncResolver::tokio_from_system_conf() {
        Ok(r) => r,
        Err(e) => {
            warn!(error = ?e, "Failed to create DNS resolver for SPF check");
            return default_pass;
        }
    };

    let txt_lookup = match resolver.txt_lookup(sender_domain).await {
        Ok(lookup) => lookup,
        Err(e) => {
            warn!(domain = %sender_domain, error = ?e, "DNS TXT lookup failed for SPF");
            return default_pass;
        }
    };

    // Find the SPF record (starts with "v=spf1")
    let spf_record = txt_lookup.iter().find_map(|record| {
        let data = record.txt_data();
        let text = data
            .iter()
            .map(|b| String::from_utf8_lossy(b).to_string())
            .collect::<String>();
        if text.starts_with("v=spf1") {
            Some(text)
        } else {
            None
        }
    });

    let spf_record = match spf_record {
        Some(r) => r,
        None => {
            info!(domain = %sender_domain, "No SPF record found");
            return default_pass;
        }
    };

    info!(domain = %sender_domain, record = %spf_record, "Found SPF record");

    // Parse and evaluate the SPF record
    let passed = evaluate_spf(&spf_record, sender_ip);

    SpfResult {
        record: Some(spf_record.clone()),
        passed,
        message: if passed {
            format!("SPF pass for {}", sender_ip)
        } else {
            format!("SPF fail for {}", sender_ip)
        },
    }
}

/// Evaluate an SPF record against a sender IP.
///
/// Supports basic mechanisms: +all, -all, ~all, ?all, ip4, ip6, a, mx, include
/// For v1, we handle the common cases and log the rest.
fn evaluate_spf(record: &str, sender_ip: IpAddr) -> bool {
    let mechanisms: Vec<&str> = record.split_whitespace().collect();

    for mechanism in mechanisms.iter().skip(1) {
        // Skip "v=spf1"
        if mechanism.starts_with("all") {
            // +all = pass, -all = fail, ~all = softfail, ?all = neutral
            return if mechanism.starts_with("+all") || mechanism.starts_with("?all") {
                true
            } else if mechanism.starts_with("-all") {
                false
            } else {
                // ~all (softfail) — we pass but log
                info!(mechanism = %mechanism, "SPF softfail — accepting mail");
                true
            };
        }

        if let Some(cidr) = mechanism.strip_prefix("ip4:") {
            if let Some((network, prefix)) = parse_cidr(cidr) {
                if sender_ip_matches(sender_ip, network, prefix) {
                    return true;
                }
            }
        }

        if let Some(cidr) = mechanism.strip_prefix("ip6:") {
            if let Some((network, prefix)) = parse_cidr_v6(cidr) {
                if sender_ip_matches_v6(sender_ip, network, prefix) {
                    return true;
                }
            }
        }

        // a, mx, include — complex lookups, log and skip for v1
        if mechanism.starts_with("a")
            || mechanism.starts_with("mx")
            || mechanism.starts_with("include:")
        {
            info!(mechanism = %mechanism, "SPF mechanism not fully supported in v1, skipping");
            continue;
        }
    }

    // Default: if no mechanism matched, neutral
    true
}

fn parse_cidr(cidr: &str) -> Option<(IpAddr, u32)> {
    let parts: Vec<&str> = cidr.split('/').collect();
    if parts.len() != 2 {
        return None;
    }
    let ip: IpAddr = parts[0].parse().ok()?;
    let prefix: u32 = parts[1].parse().ok()?;
    Some((ip, prefix))
}

fn parse_cidr_v6(cidr: &str) -> Option<(IpAddr, u32)> {
    let parts: Vec<&str> = cidr.split('/').collect();
    if parts.len() != 2 {
        return None;
    }
    let ip: IpAddr = parts[0].parse().ok()?;
    let prefix: u32 = parts[1].parse().ok()?;
    Some((ip, prefix))
}

fn sender_ip_matches(sender_ip: IpAddr, network: IpAddr, prefix: u32) -> bool {
    match (sender_ip, network) {
        (IpAddr::V4(s), IpAddr::V4(n)) => {
            let s = s.octets();
            let n = n.octets();
            let mask = !0u32 << (32 - prefix);
            let s_int = u32::from_be_bytes(s);
            let n_int = u32::from_be_bytes(n);
            (s_int & mask) == (n_int & mask)
        }
        _ => false,
    }
}

fn sender_ip_matches_v6(sender_ip: IpAddr, network: IpAddr, prefix: u32) -> bool {
    match (sender_ip, network) {
        (IpAddr::V6(s), IpAddr::V6(n)) => {
            let s = s.octets();
            let n = n.octets();
            let full_prefix = prefix as usize;
            let bytes_to_check = full_prefix / 8;
            let bits_remaining = full_prefix % 8;

            if bytes_to_check < 16 {
                for i in 0..bytes_to_check {
                    if s[i] != n[i] {
                        return false;
                    }
                }
                if bits_remaining > 0 {
                    let mask = !0u8 << (8 - bits_remaining);
                    if (s[bytes_to_check] & mask) != (n[bytes_to_check] & mask) {
                        return false;
                    }
                }
            }
            true
        }
        _ => false,
    }
}
