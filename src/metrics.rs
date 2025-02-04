use metrics::{counter, gauge};

#[derive(Clone, Debug, PartialEq)]
pub struct ForwardMetrics {
    pub(crate) name: String,
}

impl ForwardMetrics {
    pub fn new(name: String) -> Self {
        Self { name }
    }

    pub fn record_connection_attempt(&self) {
        counter!("port_forward_connection_attempts_total", "forward" => self.name.clone())
            .increment(1);
    }

    pub fn record_connection_success(&self) {
        counter!("port_forward_connection_successes_total", "forward" => self.name.clone())
            .increment(1);
    }

    pub fn record_connection_failure(&self) {
        counter!("port_forward_connection_failures_total","forward" => self.name.clone())
            .increment(1);
    }

    pub fn set_connection_status(&self, connected: bool) {
        let gauge = gauge!("port_forward_connected", "forward" => self.name.clone());
        gauge.set(if connected { 1.0 } else { 0.0 });
    }
}
