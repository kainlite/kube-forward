use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ForwardConfig {
    pub name: String,
    pub target: String,
    pub ports: PortMapping,
    #[serde(default)]
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

fn default_retry_interval() -> Duration {
    Duration::from_secs(5)
}

fn default_max_retries() -> u32 {
    3
}

fn default_health_check_interval() -> Duration {
    Duration::from_secs(10)
}
