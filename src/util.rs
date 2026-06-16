use crate::error::{PortForwardError, Result};
use k8s_openapi::api::core::v1::Service;
use kube::{Api, Client};

#[derive(Clone, Debug, PartialEq)]
pub struct ServiceInfo {
    pub namespace: String,
    pub name: String,
    pub ports: Vec<u16>,
}

pub async fn resolve_service(client: Client, target: &str) -> Result<ServiceInfo> {
    let client = client.clone();
    // Get the default namespace before borrowing the client mutably below.
    let default_ns = client.default_namespace();
    let (service_name, namespace) = match parse_target(target)? {
        (name, Some(ns)) => (name, ns),
        (name, None) => (name, default_ns),
    };

    let client = client.clone();
    let services: Api<Service> = Api::namespaced(client, namespace);
    let service = services
        .get(service_name)
        .await
        .map_err(PortForwardError::KubeError)?;

    let ports = service
        .spec
        .as_ref()
        .and_then(|spec| spec.ports.as_ref())
        .map(|ports| ports.iter().map(|port| port.port as u16).collect())
        .unwrap_or_default();

    Ok(ServiceInfo {
        namespace: namespace.to_string(),
        name: service_name.to_string(),
        ports,
    })
}

pub fn parse_full_dns_name<'a>(parts: &'a [&'a str]) -> Result<(&'a str, &'a str)> {
    if parts.len() >= 2 {
        Ok((parts[0], parts[1]))
    } else {
        Err(PortForwardError::DnsError(
            "Invalid DNS name format".to_string(),
        ))
    }
}

/// Parse a `target` string into a service name and optional namespace.
///
/// Accepts `service`, `service.namespace`, or a longer DNS-style name where only
/// the first two labels are significant. A `None` namespace means the caller
/// should fall back to the client's default namespace. This is pure (no cluster
/// access) so it can be reused by config validation.
pub fn parse_target(target: &str) -> Result<(&str, Option<&str>)> {
    let parts: Vec<&str> = target.split('.').collect();
    match parts.as_slice() {
        [name] if !name.is_empty() => Ok((name, None)),
        [name, ns, ..] if !name.is_empty() && !ns.is_empty() => Ok((name, Some(ns))),
        _ => Err(PortForwardError::DnsError(format!(
            "invalid target '{}', expected 'service' or 'service.namespace'",
            target
        ))),
    }
}
