use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ForwardConfig {
    pub name: String,
    pub target: String,
    pub ports: PortMapping,
    #[serde(default = "default_forward_options")]
    pub options: ForwardOptions,
    #[serde(default)]
    pub local_dns: LocalDnsConfig,
    #[serde(default)]
    pub pod_selector: PodSelector,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct PortMapping {
    pub local: u16,
    pub remote: u16,
}

#[derive(Clone, Default, Debug, Serialize, Deserialize, PartialEq)]
pub struct ForwardOptions {
    #[serde(with = "humantime_serde", default = "default_retry_interval")]
    pub retry_interval: Duration,
    #[serde(default = "default_max_retries")]
    pub max_retries: u32,
    #[serde(default = "default_persistent_connection")]
    pub persistent_connection: bool,
    #[serde(with = "humantime_serde", default = "default_health_check_interval")]
    pub health_check_interval: Duration,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq)]
pub struct LocalDnsConfig {
    pub enabled: bool,
    pub hostname: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Default)]
pub struct PodSelector {
    pub label: Option<String>,      // e.g. "app.kubernetes.io/name=simple"
    pub annotation: Option<String>, // e.g. "prometheus.io/scrape=true"
}

pub fn default_forward_options() -> ForwardOptions {
    ForwardOptions {
        retry_interval: default_retry_interval(),
        max_retries: default_max_retries(),
        persistent_connection: default_persistent_connection(),
        health_check_interval: default_health_check_interval(),
    }
}

pub fn default_retry_interval() -> Duration {
    Duration::from_secs(5)
}

pub fn default_max_retries() -> u32 {
    3
}

pub fn default_health_check_interval() -> Duration {
    Duration::from_secs(10)
}

pub fn default_persistent_connection() -> bool {
    true
}
