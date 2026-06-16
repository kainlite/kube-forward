use std::collections::{BTreeMap, HashSet};

use crate::config::ForwardConfig;
use crate::util::parse_target;

/// Result of validating a set of forward configs.
///
/// `errors` are fatal (the program should not start). `warnings` are advisory.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ValidationReport {
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

impl ValidationReport {
    pub fn is_valid(&self) -> bool {
        self.errors.is_empty()
    }
}

/// Validate a list of forward configs without touching the cluster.
///
/// Catches the mistakes that otherwise only surface as confusing runtime
/// failures: duplicate names (which collide as metric labels), two forwards
/// fighting over the same local port, bad protocols/ports, unparseable targets,
/// and malformed pod selectors.
pub fn validate_configs(configs: &[ForwardConfig]) -> ValidationReport {
    let mut errors = Vec::new();
    let mut warnings = Vec::new();

    if configs.is_empty() {
        warnings.push("configuration defines no port-forwards".to_string());
    }

    // Local ports keyed by (protocol, port) so TCP and UDP may reuse a number.
    let mut ports_in_use: BTreeMap<(String, u16), Vec<String>> = BTreeMap::new();

    for config in configs {
        let label = if config.name.trim().is_empty() {
            "<unnamed forward>".to_string()
        } else {
            config.name.clone()
        };

        if config.name.trim().is_empty() {
            errors.push(format!("{}: name is empty", label));
        }

        if config.target.trim().is_empty() {
            errors.push(format!("{}: target is empty", label));
        } else if let Err(e) = parse_target(&config.target) {
            errors.push(format!("{}: {}", label, e));
        }

        let protocol = config.ports.protocol.as_deref().unwrap_or("TCP");
        let protocol_upper = protocol.to_uppercase();
        if protocol_upper != "TCP" && protocol_upper != "UDP" {
            errors.push(format!(
                "{}: invalid protocol '{}', expected 'tcp' or 'udp'",
                label, protocol
            ));
        }

        if config.ports.local == 0 {
            errors.push(format!("{}: local port must be non-zero", label));
        }
        if config.ports.remote == 0 {
            errors.push(format!("{}: remote port must be non-zero", label));
        }

        if let Some(selector) = &config.pod_selector.label
            && let Err(msg) = validate_selector(selector)
        {
            errors.push(format!("{}: pod_selector label {}", label, msg));
        }
        if let Some(selector) = &config.pod_selector.annotation
            && let Err(msg) = validate_selector(selector)
        {
            errors.push(format!("{}: pod_selector annotation {}", label, msg));
        }

        if config.ports.local != 0 && (protocol_upper == "TCP" || protocol_upper == "UDP") {
            ports_in_use
                .entry((protocol_upper, config.ports.local))
                .or_default()
                .push(label);
        }
    }

    // Duplicate names, reported in declaration order.
    let mut seen_names = HashSet::new();
    let mut reported_names = HashSet::new();
    for config in configs {
        let name = config.name.trim();
        if name.is_empty() {
            continue;
        }
        if !seen_names.insert(name) && reported_names.insert(name) {
            errors.push(format!(
                "duplicate forward name '{}'; names must be unique",
                name
            ));
        }
    }

    for ((protocol, port), names) in &ports_in_use {
        if names.len() > 1 {
            errors.push(format!(
                "local {} port {} is used by multiple forwards: {}",
                protocol,
                port,
                names.join(", ")
            ));
        }
    }

    ValidationReport { errors, warnings }
}

/// Validate a `key=value` selector, the form used by pod label/annotation matching.
pub fn validate_selector(selector: &str) -> Result<(), String> {
    match selector.split_once('=') {
        Some((key, _)) if !key.trim().is_empty() => Ok(()),
        Some(_) => Err(format!(
            "'{}' has an empty key, expected 'key=value'",
            selector
        )),
        None => Err(format!("'{}' is malformed, expected 'key=value'", selector)),
    }
}
