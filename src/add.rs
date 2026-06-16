use std::collections::{BTreeMap, HashSet};

use anyhow::{Context, Result};
use inquire::{Select, Text};
use k8s_openapi::api::core::v1::{Namespace, Service};
use kube::{Api, Client, api::ListParams};

use crate::config::{ForwardConfig, PodSelector, PortMapping, default_forward_options};
use crate::validate::validate_configs;

/// Keys we prefer when turning a service's selector into a single `key=value`
/// pod selector, most-specific first.
const PREFERRED_SELECTOR_KEYS: [&str; 4] = [
    "app.kubernetes.io/name",
    "app.kubernetes.io/instance",
    "app",
    "k8s-app",
];

/// Turn a service's label selector map into a single `key=value` pod selector,
/// preferring well-known app keys and otherwise the first key. Pure helper.
pub fn derive_selector_from_service(selector: &BTreeMap<String, String>) -> Option<String> {
    if selector.is_empty() {
        return None;
    }
    for key in PREFERRED_SELECTOR_KEYS {
        if let Some(value) = selector.get(key) {
            return Some(format!("{}={}", key, value));
        }
    }
    selector
        .iter()
        .next()
        .map(|(key, value)| format!("{}={}", key, value))
}

/// Suggest a free local port: the remote port if available, otherwise the next
/// higher port not already taken. Pure helper.
pub fn suggest_local_port(remote: u16, taken: &HashSet<u16>) -> u16 {
    let mut port = remote.max(1);
    while taken.contains(&port) {
        match port.checked_add(1) {
            Some(next) => port = next,
            None => break,
        }
    }
    port
}

/// Normalize a service port protocol to the lowercase form kube-forward uses.
pub fn normalize_protocol(protocol: Option<&str>) -> String {
    match protocol {
        Some(p) if p.eq_ignore_ascii_case("udp") => "udp".to_string(),
        _ => "tcp".to_string(),
    }
}

/// Build a `ForwardConfig` from the collected answers. Pure helper.
pub fn build_forward_config(
    name: String,
    target: String,
    local: u16,
    remote: u16,
    protocol: String,
    selector_label: Option<String>,
) -> ForwardConfig {
    ForwardConfig {
        name,
        target,
        ports: PortMapping {
            protocol: Some(protocol),
            local,
            remote,
        },
        options: default_forward_options(),
        pod_selector: PodSelector {
            label: selector_label,
            annotation: None,
        },
    }
}

/// Render a forward as a minimal YAML list item and append it to the existing
/// config text, preserving whatever comments/formatting are already there.
/// Pure helper: deterministic and independent of the cluster.
pub fn append_entry(existing: &str, config: &ForwardConfig) -> String {
    let mut block = String::new();
    block.push_str(&format!("- name: \"{}\"\n", config.name));
    block.push_str(&format!("  target: \"{}\"\n", config.target));
    block.push_str("  ports:\n");
    if let Some(protocol) = &config.ports.protocol
        && !protocol.eq_ignore_ascii_case("tcp")
    {
        block.push_str(&format!("    protocol: {}\n", protocol.to_lowercase()));
    }
    block.push_str(&format!("    local: {}\n", config.ports.local));
    block.push_str(&format!("    remote: {}\n", config.ports.remote));
    if let Some(label) = &config.pod_selector.label {
        block.push_str("  pod_selector:\n");
        block.push_str(&format!("    label: \"{}\"\n", label));
    }

    let mut out = existing.to_string();
    if !out.is_empty() {
        if !out.ends_with('\n') {
            out.push('\n');
        }
        out.push('\n');
    }
    out.push_str(&block);
    out
}

/// Interactive flow: discover a service via the cluster and append a forward
/// entry to `config_path`. Discovery is read-only; only the config file is written.
pub async fn run(client: Client, config_path: &std::path::Path) -> Result<()> {
    let existing_text = match tokio::fs::read_to_string(config_path).await {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => {
            return Err(e).with_context(|| format!("failed to read {}", config_path.display()));
        }
    };
    let existing: Vec<ForwardConfig> = if existing_text.trim().is_empty() {
        Vec::new()
    } else {
        serde_yaml::from_str(&existing_text)
            .with_context(|| format!("failed to parse {}", config_path.display()))?
    };

    // Namespace
    let namespaces: Api<Namespace> = Api::all(client.clone());
    let mut ns_names: Vec<String> = namespaces
        .list(&ListParams::default())
        .await
        .context("failed to list namespaces (is the cluster reachable?)")?
        .into_iter()
        .filter_map(|n| n.metadata.name)
        .collect();
    ns_names.sort();
    let namespace = Select::new("Namespace:", ns_names)
        .prompt()
        .context("namespace selection cancelled")?;

    // Service
    let services: Api<Service> = Api::namespaced(client.clone(), &namespace);
    let svc_list = services
        .list(&ListParams::default())
        .await
        .context("failed to list services")?;
    let mut svc_names: Vec<String> = svc_list
        .iter()
        .filter_map(|s| s.metadata.name.clone())
        .collect();
    svc_names.sort();
    if svc_names.is_empty() {
        anyhow::bail!("no services found in namespace '{}'", namespace);
    }
    let service_name = Select::new("Service:", svc_names)
        .prompt()
        .context("service selection cancelled")?;
    let service = svc_list
        .into_iter()
        .find(|s| s.metadata.name.as_deref() == Some(service_name.as_str()))
        .context("selected service disappeared")?;

    // Remote port
    let ports = service
        .spec
        .as_ref()
        .and_then(|spec| spec.ports.clone())
        .unwrap_or_default();
    if ports.is_empty() {
        anyhow::bail!("service '{}' exposes no ports", service_name);
    }
    let port_labels: Vec<String> = ports
        .iter()
        .map(|p| {
            let name = p.name.clone().unwrap_or_else(|| "port".to_string());
            let proto = normalize_protocol(p.protocol.as_deref());
            format!("{} ({} {})", p.port, proto, name)
        })
        .collect();
    let chosen_idx = if ports.len() == 1 {
        0
    } else {
        let chosen = Select::new("Remote port:", port_labels.clone())
            .prompt()
            .context("port selection cancelled")?;
        port_labels.iter().position(|l| l == &chosen).unwrap_or(0)
    };
    let remote = ports[chosen_idx].port as u16;
    let protocol = normalize_protocol(ports[chosen_idx].protocol.as_deref());

    // Local port (suggest a free one given what the config already uses)
    let taken: HashSet<u16> = existing.iter().map(|c| c.ports.local).collect();
    let suggested = suggest_local_port(remote, &taken);
    let local_text = Text::new("Local port:")
        .with_default(&suggested.to_string())
        .prompt()
        .context("local port entry cancelled")?;
    let local: u16 = local_text
        .trim()
        .parse()
        .with_context(|| format!("'{}' is not a valid port", local_text))?;

    // Name (default to the service name, kept unique)
    let default_name = unique_name(&service_name, &existing);
    let name = Text::new("Name:")
        .with_default(&default_name)
        .prompt()
        .context("name entry cancelled")?;

    // Pod selector, pre-filled from the service's own selector when possible.
    let derived = service
        .spec
        .as_ref()
        .and_then(|spec| spec.selector.clone())
        .and_then(|sel| derive_selector_from_service(&sel.into_iter().collect()));
    let selector_input = match derived {
        Some(d) => Text::new("Pod selector (key=value, blank to skip):")
            .with_default(&d)
            .prompt(),
        None => Text::new("Pod selector (key=value, blank to skip):").prompt(),
    }
    .context("selector entry cancelled")?;
    let selector_label = {
        let trimmed = selector_input.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    };

    let target = format!("{}.{}", service_name, namespace);
    let new_config = build_forward_config(
        name.trim().to_string(),
        target,
        local,
        remote,
        protocol,
        selector_label,
    );

    // Validate the whole resulting set before writing anything.
    let mut candidate = existing.clone();
    candidate.push(new_config.clone());
    let report = validate_configs(&candidate);
    if !report.is_valid() {
        for error in &report.errors {
            eprintln!("error: {}", error);
        }
        anyhow::bail!("the new entry would make the config invalid; nothing was written");
    }

    let updated = append_entry(&existing_text, &new_config);
    tokio::fs::write(config_path, updated)
        .await
        .with_context(|| format!("failed to write {}", config_path.display()))?;

    println!(
        "+ appended '{}' to {} \u{2713}",
        new_config.name,
        config_path.display()
    );
    Ok(())
}

/// Ensure a name does not collide with an existing forward by suffixing `-2`, `-3`, ...
fn unique_name(base: &str, existing: &[ForwardConfig]) -> String {
    let names: HashSet<&str> = existing.iter().map(|c| c.name.as_str()).collect();
    if !names.contains(base) {
        return base.to_string();
    }
    let mut n = 2;
    loop {
        let candidate = format!("{}-{}", base, n);
        if !names.contains(candidate.as_str()) {
            return candidate;
        }
        n += 1;
    }
}
