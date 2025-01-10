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
}
