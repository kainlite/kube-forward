use metrics::{counter, gauge};

#[derive(Clone, Debug, PartialEq)]
pub struct ForwardMetrics {
    name: String,
}

impl ForwardMetrics {
    pub fn new(name: String) -> Self {
        Self { name }
    }

    pub fn record_connection_attempt(&self) {
        counter!("port_forward_connection_attempts_total", 1, "forward" => self.name.clone());
    }

    pub fn record_connection_success(&self) {
        counter!("port_forward_connection_successes_total", 1, "forward" => self.name.clone());
    }

    pub fn record_connection_failure(&self) {
        counter!("port_forward_connection_failures_total", 1, "forward" => self.name.clone());
    }

    pub fn set_connection_status(&self, connected: bool) {
        gauge!("port_forward_connected", if connected { 1.0 } else { 0.0 }, "forward" => self.name.clone());
    }
}
