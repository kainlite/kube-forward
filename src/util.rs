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
    let parts: Vec<&str> = target.split('.').collect();

    let client = client.clone();
    // Fix the temporary value issue by getting the namespace first
    let default_ns = client.default_namespace();
    let (service_name, namespace) = match parts.len() {
        1 => (parts[0], default_ns),
        2 => (parts[0], parts[1]),
        _ => parse_full_dns_name(&parts)?,
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
