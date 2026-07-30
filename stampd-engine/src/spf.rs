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
        let m = *mechanism; // dereference &&str to &str
                            // Check for "all" qualifiers: +all, -all, ~all, ?all, bare "all"
        if m == "all" || m == "+all" || m == "-all" || m == "~all" || m == "?all" {
            // +all = pass, -all = fail, ~all = softfail, ?all = neutral
            return m == "+all" || m == "?all" || m == "~all";
        }

        if let Some(cidr) = m.strip_prefix("ip4:") {
            if let Some((network, prefix)) = parse_cidr(cidr) {
                if sender_ip_matches(sender_ip, network, prefix) {
                    return true;
                }
            }
        }

        if let Some(cidr) = m.strip_prefix("ip6:") {
            if let Some((network, prefix)) = parse_cidr_v6(cidr) {
                if sender_ip_matches_v6(sender_ip, network, prefix) {
                    return true;
                }
            }
        }

        // a, mx, include — complex lookups, log and skip for v1
        if m.starts_with("a") || m.starts_with("mx") || m.starts_with("include:") {
            info!(mechanism = %m, "SPF mechanism not fully supported in v1, skipping");
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{Ipv4Addr, Ipv6Addr};

    #[test]
    fn test_parse_cidr_valid() {
        let (ip, prefix) = parse_cidr("10.0.0.0/8").unwrap();
        assert_eq!(ip, IpAddr::V4(Ipv4Addr::new(10, 0, 0, 0)));
        assert_eq!(prefix, 8);
    }

    #[test]
    fn test_parse_cidr_no_slash() {
        assert!(parse_cidr("10.0.0.0").is_none());
    }

    #[test]
    fn test_parse_cidr_invalid_ip() {
        assert!(parse_cidr("999.999.999.999/24").is_none());
    }

    #[test]
    fn test_parse_cidr_invalid_prefix() {
        assert!(parse_cidr("10.0.0.0/abc").is_none());
    }

    #[test]
    fn test_parse_cidr_v6_valid() {
        let (ip, prefix) = parse_cidr_v6("::1/128").unwrap();
        assert_eq!(ip, IpAddr::V6(Ipv6Addr::LOCALHOST));
        assert_eq!(prefix, 128);
    }

    #[test]
    fn test_sender_ip_matches_exact() {
        let ip = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 100));
        let net = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 100));
        assert!(sender_ip_matches(ip, net, 32));
    }

    #[test]
    fn test_sender_ip_matches_subnet() {
        let ip = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 100));
        let net = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 0));
        assert!(sender_ip_matches(ip, net, 24));
    }

    #[test]
    fn test_sender_ip_matches_different_subnet() {
        let ip = IpAddr::V4(Ipv4Addr::new(192, 168, 2, 100));
        let net = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 0));
        assert!(!sender_ip_matches(ip, net, 24));
    }

    #[test]
    fn test_sender_ip_matches_v6() {
        let ip = IpAddr::V6(Ipv6Addr::new(0x2001, 0xdb8, 0, 0, 0, 0, 0, 1));
        let net = IpAddr::V6(Ipv6Addr::new(0x2001, 0xdb8, 0, 0, 0, 0, 0, 0));
        assert!(sender_ip_matches_v6(ip, net, 32));
    }

    #[test]
    fn test_sender_ip_matches_v6_different() {
        let ip = IpAddr::V6(Ipv6Addr::new(0x2001, 0xdb8, 1, 0, 0, 0, 0, 1));
        let net = IpAddr::V6(Ipv6Addr::new(0x2001, 0xdb8, 0, 0, 0, 0, 0, 0));
        assert!(!sender_ip_matches_v6(ip, net, 48)); // /48 differentiates on 3rd hextet
    }

    #[test]
    fn test_evaluate_spf_minus_all() {
        let record = "v=spf1 -all";
        let ip = IpAddr::V4(Ipv4Addr::new(1, 2, 3, 4));
        assert!(!evaluate_spf(record, ip));
    }

    #[test]
    fn test_evaluate_spf_plus_all() {
        let record = "v=spf1 +all";
        let ip = IpAddr::V4(Ipv4Addr::new(1, 2, 3, 4));
        assert!(evaluate_spf(record, ip));
    }

    #[test]
    fn test_evaluate_spf_question_all() {
        let record = "v=spf1 ?all";
        let ip = IpAddr::V4(Ipv4Addr::new(1, 2, 3, 4));
        assert!(evaluate_spf(record, ip));
    }

    #[test]
    fn test_evaluate_spf_tilde_all() {
        let record = "v=spf1 ~all";
        let ip = IpAddr::V4(Ipv4Addr::new(1, 2, 3, 4));
        assert!(evaluate_spf(record, ip)); // softfail = pass
    }

    #[test]
    fn test_evaluate_spf_ip4_match() {
        let record = "v=spf1 ip4:10.0.0.0/8 -all";
        let ip = IpAddr::V4(Ipv4Addr::new(10, 1, 2, 3));
        assert!(evaluate_spf(record, ip));
    }

    #[test]
    fn test_evaluate_spf_ip4_no_match() {
        let record = "v=spf1 ip4:10.0.0.0/8 -all";
        let ip = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1));
        assert!(!evaluate_spf(record, ip));
    }

    #[test]
    fn test_evaluate_spf_no_mechanism_neutral() {
        let record = "v=spf1";
        let ip = IpAddr::V4(Ipv4Addr::new(1, 2, 3, 4));
        assert!(evaluate_spf(record, ip)); // default neutral = pass
    }
}
