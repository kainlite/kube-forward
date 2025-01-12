#[cfg(test)]
mod tests {
    use k8s_openapi::api::core::v1::Pod;
    use kube_forward::config::{ForwardConfig, PodSelector};
    use kube_forward::forward::{ForwardState, HealthCheck, PortForward};
    use kube_forward::util::ServiceInfo;
    use std::time::Duration;
    use tokio::net::TcpListener;

    #[tokio::test]
    async fn test_health_check() {
        let health_check = HealthCheck::new();

        // Start a test server
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();

        // Test successful connection
        assert!(health_check.check_connection(port).await);
        assert_eq!(*health_check.failures.read().await, 0);
        assert!(health_check.last_check.read().await.is_some());

        // Test failed connection
        drop(listener); // Close the listener
        assert!(!health_check.check_connection(port).await);
        assert_eq!(*health_check.failures.read().await, 1);
    }

    #[test]
    fn test_forward_state() {
        assert_eq!(ForwardState::Starting, ForwardState::Starting);
        assert_ne!(ForwardState::Starting, ForwardState::Connected);
        assert_ne!(
            ForwardState::Failed("error1".to_string()),
            ForwardState::Failed("error2".to_string())
        );
    }

    #[tokio::test]
    async fn test_port_forward_new() {
        let config = ForwardConfig {
            name: "test-forward".to_string(),
            target: "test-target.test-namespace".to_string(),
            ports: kube_forward::config::PortMapping {
                local: 8080,
                remote: 80,
            },
            pod_selector: PodSelector {
                label: None,
                annotation: None,
            },
            local_dns: kube_forward::config::LocalDnsConfig {
                enabled: false,
                hostname: None,
            },
            options: kube_forward::config::ForwardOptions {
                max_retries: 3,
                retry_interval: Duration::from_secs(1),
                health_check_interval: Duration::from_secs(5),
            },
        };

        let service_info = ServiceInfo {
            name: "test-service".to_string(),
            namespace: "default".to_string(),
            ports: vec![80],
        };

        let forward = PortForward::new(config.clone(), service_info.clone());
        assert_eq!(*forward.state.read().await, ForwardState::Starting);
        assert_eq!(forward.config.name, "test-forward");
        assert_eq!(forward.service_info.name, "test-service");
    }

    #[test]
    fn test_matches_pod_selector() {
        let config = ForwardConfig {
            name: "test-forward".to_string(),
            target: "test-target.test-namespace".to_string(),
            ports: kube_forward::config::PortMapping {
                local: 8080,
                remote: 80,
            },
            local_dns: kube_forward::config::LocalDnsConfig {
                enabled: false,
                hostname: None,
            },
            pod_selector: PodSelector {
                label: Some("app=myapp".to_string()),
                annotation: None,
            },
            options: kube_forward::config::ForwardOptions {
                max_retries: 3,
                retry_interval: Duration::from_secs(1),
                health_check_interval: Duration::from_secs(5),
            },
        };

        let service_info = ServiceInfo {
            name: "test-service".to_string(),
            namespace: "default".to_string(),
            ports: vec![80],
        };

        let forward = PortForward::new(config, service_info);

        // Create a test pod with matching label
        let mut pod = Pod::default();
        pod.metadata.labels = Some(std::collections::BTreeMap::from([(
            "app".to_string(),
            "myapp".to_string(),
        )]));

        assert!(forward.clone().matches_pod_selector(
            &pod,
            &PodSelector {
                label: Some("app=myapp".to_string()),
                annotation: None,
            }
        ));

        // Test non-matching label
        assert!(!forward.clone().matches_pod_selector(
            &pod,
            &PodSelector {
                label: Some("app=different".to_string()),
                annotation: None,
            }
        ));
    }

    #[test]
    fn test_parse_selector() {
        let config = ForwardConfig {
            name: "test-forward".to_string(),
            target: "test-target.test-namespace".to_string(),
            local_dns: kube_forward::config::LocalDnsConfig {
                enabled: false,
                hostname: None,
            },
            ports: kube_forward::config::PortMapping {
                local: 8080,
                remote: 80,
            },
            pod_selector: PodSelector {
                label: None,
                annotation: None,
            },
            options: kube_forward::config::ForwardOptions {
                max_retries: 3,
                retry_interval: Duration::from_secs(1),
                health_check_interval: Duration::from_secs(5),
            },
        };

        let service_info = ServiceInfo {
            name: "test-service".to_string(),
            namespace: "default".to_string(),
            ports: vec![80],
        };

        let forward = PortForward::new(config, service_info);

        let (key, value) = forward.clone().parse_selector("app=myapp");
        assert_eq!(key, "app");
        assert_eq!(value, "myapp");

        // Test invalid format
        let (key, value) = forward.parse_selector("invalid-format");
        assert_eq!(key, "");
        assert_eq!(value, "");
    }

    #[tokio::test]
    async fn test_port_forward_stop() {
        let config = ForwardConfig {
            name: "test-forward".to_string(),
            target: "test-target.test-namespace".to_string(),
            ports: kube_forward::config::PortMapping {
                local: 8080,
                remote: 80,
            },
            pod_selector: PodSelector {
                label: None,
                annotation: None,
            },
            local_dns: kube_forward::config::LocalDnsConfig {
                enabled: false,
                hostname: None,
            },
            options: kube_forward::config::ForwardOptions {
                max_retries: 3,
                retry_interval: Duration::from_secs(1),
                health_check_interval: Duration::from_secs(5),
            },
        };

        let service_info = ServiceInfo {
            name: "test-service".to_string(),
            namespace: "default".to_string(),
            ports: vec![80],
        };

        let forward = PortForward::new(config, service_info);

        // Initial state should be Starting
        assert_eq!(*forward.state.read().await, ForwardState::Starting);

        // Stop the forward
        forward.stop().await;

        // State should be Disconnected after stopping
        assert_eq!(*forward.state.read().await, ForwardState::Disconnected);

        // Check that a subscriber receives the shutdown signal
        let mut rx = forward.shutdown.subscribe();
        assert!(rx.try_recv().is_err()); // Should be empty after stop
    }

    // This tests specifically depends on kind
    // just a reminder in case it fails later on
    #[tokio::test]
    async fn test_establish_forward() {
        let config = ForwardConfig {
            name: "test-forward".to_string(),
            target: "test-target.test-namespace".to_string(),
            ports: kube_forward::config::PortMapping {
                local: 0, // Use port 0 to let OS assign random port
                remote: 80,
            },
            pod_selector: PodSelector {
                label: None,
                annotation: None,
            },
            local_dns: kube_forward::config::LocalDnsConfig {
                enabled: false,
                hostname: None,
            },
            options: kube_forward::config::ForwardOptions {
                max_retries: 3,
                retry_interval: Duration::from_secs(1),
                health_check_interval: Duration::from_secs(5),
            },
        };

        let service_info = ServiceInfo {
            name: "kube-dns".to_string(),
            namespace: "kube-system".to_string(),
            ports: vec![53],
        };

        let forward = PortForward::new(config, service_info);
        let client = kube::Client::try_default().await.unwrap();

        // Test port already in use
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();

        let mut config_with_used_port = forward.config.clone();
        config_with_used_port.ports.local = port;
        let forward_with_used_port =
            PortForward::new(config_with_used_port, forward.service_info.clone());

        let result = forward_with_used_port.establish_forward(&client).await;
        assert!(result.is_err());
        match result {
            Err(kube_forward::error::PortForwardError::ConnectionError(msg)) => {
                assert!(msg.contains("already in use"));
            }
            _ => panic!("Expected ConnectionError for port in use"),
        }
    }

    // This tests specifically depends on kind
    // just a reminder in case it fails later on
    #[tokio::test]
    async fn test_monitor_connection() {
        let config = ForwardConfig {
            name: "kube-dns".to_string(),
            target: "kube-dns.kube-system".to_string(),
            ports: kube_forward::config::PortMapping {
                local: 53,
                remote: 53,
            },
            pod_selector: PodSelector {
                label: None,
                annotation: None,
            },
            local_dns: kube_forward::config::LocalDnsConfig {
                enabled: false,
                hostname: None,
            },
            options: kube_forward::config::ForwardOptions {
                max_retries: 3,
                retry_interval: Duration::from_secs(1),
                health_check_interval: Duration::from_millis(100), // Use shorter interval for testing
            },
        };

        let service_info = ServiceInfo {
            name: "kube-dns".to_string(),
            namespace: "kube-system".to_string(),
            ports: vec![53],
        };

        let mut forward = PortForward::new(config, service_info);
        let client = kube::Client::try_default().await.unwrap();

        // Create a listener that we'll close to simulate connection failure
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        forward.config.ports.local = port;

        // Start monitoring in a separate task
        let monitor_handle = tokio::spawn({
            let forward = forward.clone();
            let client = client.clone();
            async move { forward.monitor_connection(&client).await }
        });

        // Wait a bit to ensure monitoring has started
        tokio::time::sleep(Duration::from_millis(50)).await;

        // Close the listener to simulate connection failure
        drop(listener);

        // Wait for the monitor to detect the failure
        let result = monitor_handle.await.unwrap();
        assert!(result.is_err());
        match result {
            Err(kube_forward::error::PortForwardError::ConnectionError(msg)) => {
                assert!(msg.contains("Connection health check failed"));
            }
            _ => panic!("Expected ConnectionError for health check failure"),
        }
    }

    #[tokio::test]
    async fn test_forward_connection() {
        let pods: kube::Api<Pod> =
            kube::Api::namespaced(kube::Client::try_default().await.unwrap(), "default");

        // Create a mock TCP connection
        let (client_conn, _server_conn) = tokio::io::duplex(64);

        // Test the forward_connection function
        let result =
            PortForward::forward_connection(&pods, "test-pod".to_string(), 80, client_conn).await;

        // Since we're not in a real k8s environment, this should fail
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_get_pod() {
        let config = ForwardConfig {
            name: "test-forward".to_string(),
            target: "test-target.test-namespace".to_string(),
            ports: kube_forward::config::PortMapping {
                local: 8080,
                remote: 80,
            },
            pod_selector: PodSelector {
                label: Some("app=test".to_string()),
                annotation: None,
            },
            local_dns: kube_forward::config::LocalDnsConfig {
                enabled: false,
                hostname: None,
            },
            options: kube_forward::config::ForwardOptions {
                max_retries: 3,
                retry_interval: Duration::from_secs(1),
                health_check_interval: Duration::from_secs(5),
            },
        };

        let service_info = ServiceInfo {
            name: "test-service".to_string(),
            namespace: "default".to_string(),
            ports: vec![80],
        };

        let forward = PortForward::new(config, service_info);
        let client = kube::Client::try_default().await.unwrap();

        // Test get_pod without a real cluster
        let result = forward.get_pod(&client).await;
        assert!(result.is_err());
        match result {
            Err(kube_forward::error::PortForwardError::ConnectionError(msg)) => {
                assert!(msg.contains("No ready pods found"));
            }
            _ => panic!("Expected ConnectionError for no pods found"),
        }
    }
}
