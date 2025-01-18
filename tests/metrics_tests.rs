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

    #[test]
    fn test_metrics_recording() {
        let metrics = ForwardMetrics::new("test-metrics".to_string());

        // Test multiple attempts
        for _ in 0..3 {
            metrics.record_connection_attempt();
        }

        // Test success and failure recording
        metrics.record_connection_success();
        metrics.record_connection_failure();

        // Test connection status changes
        metrics.set_connection_status(true);
        metrics.set_connection_status(false);
        metrics.set_connection_status(true);
    }

    #[test]
    fn test_metrics_labels() {
        let metrics = ForwardMetrics::new("test-labels".to_string());
        let debug_output = format!("{:?}", metrics);

        assert!(debug_output.contains("test-labels"));

        // Test different connection states
        metrics.set_connection_status(true);
        metrics.set_connection_status(false);
    }
}
