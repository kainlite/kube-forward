#[cfg(test)]
mod tests {
    use kube_forward::metrics::ForwardMetrics;

    #[test]
    fn test_connection_metrics() {
        let metrics = ForwardMetrics::new("test-forward".to_string());

        // Record various metrics
        metrics.record_connection_attempt();
        metrics.record_connection_success();
        metrics.record_connection_failure();

        // Set connection status
        metrics.set_connection_status(true);
        metrics.set_connection_status(false);
    }

    #[test]
    fn test_metrics_clone_and_debug() {
        let metrics = ForwardMetrics::new("test-forward".to_string());
        let cloned = metrics.clone();

        assert_eq!(metrics, cloned);
        assert!(format!("{:?}", metrics).contains("test-forward"));
    }
}
