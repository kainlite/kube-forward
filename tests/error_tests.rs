use kube_forward::{
    config::{ForwardConfig, ForwardOptions, LocalDnsConfig, PodSelector, PortMapping},
    error::PortForwardError,
    forward::PortForward,
    util::ServiceInfo,
};
use std::time::Duration;
use tokio::net::TcpListener;

#[tokio::test]
async fn test_port_already_in_use() {
    // Bind to a port first
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let bound_port = listener.local_addr().unwrap().port();

    // Create config using the already bound port
    let config = ForwardConfig {
        name: "test-forward".to_string(),
        target: "test-target".to_string(),
        ports: PortMapping {
            local: bound_port,
            remote: 80,
        },
        pod_selector: PodSelector {
            label: None,
            annotation: None,
        },
        local_dns: LocalDnsConfig {
            enabled: false,
            hostname: None,
        },
        options: ForwardOptions {
            max_retries: 1,
            retry_interval: Duration::from_millis(100),
            health_check_interval: Duration::from_secs(5),
            persistent_connection: false,
        },
    };

    let service_info = ServiceInfo {
        name: "test-service".to_string(),
        namespace: "default".to_string(),
        ports: vec![80],
    };

    let forward = PortForward::new(config, service_info);

    // Try to start the forward
    let client = kube::Client::try_default().await.unwrap();
    let result = forward.start(client).await;

    // Verify we get the appropriate error
    assert!(result.is_err());
    match result {
        Err(PortForwardError::ConnectionError(msg)) => {
            assert!(msg.contains("Max retry attempts reached"));
        }
        _ => panic!("Expected ConnectionError"),
    }
}

#[tokio::test]
async fn test_invalid_pod_selector() {
    let config = ForwardConfig {
        name: "test-forward".to_string(),
        target: "test-target".to_string(),
        ports: PortMapping {
            local: 8080,
            remote: 80,
        },
        pod_selector: PodSelector {
            label: Some("invalid=selector=format".to_string()),
            annotation: None,
        },
        local_dns: LocalDnsConfig {
            enabled: false,
            hostname: None,
        },
        options: ForwardOptions {
            max_retries: 1,
            retry_interval: Duration::from_millis(100),
            health_check_interval: Duration::from_secs(5),
            persistent_connection: false,
        },
    };

    let service_info = ServiceInfo {
        name: "test-service".to_string(),
        namespace: "default".to_string(),
        ports: vec![80],
    };

    let forward = PortForward::new(config, service_info);

    // Create a test pod
    let pod = k8s_openapi::api::core::v1::Pod::default();

    // Test pod selector matching
    assert!(!forward.clone().matches_pod_selector(
        &pod,
        &PodSelector {
            label: Some("invalid=selector=format".to_string()),
            annotation: None,
        }
    ));
}

#[tokio::test]
async fn test_max_retries_exceeded() {
    let config = ForwardConfig {
        name: "test-forward".to_string(),
        target: "test-target".to_string(),
        ports: PortMapping {
            local: 8080,
            remote: 80,
        },
        pod_selector: PodSelector {
            label: None,
            annotation: None,
        },
        local_dns: LocalDnsConfig {
            enabled: false,
            hostname: None,
        },
        options: ForwardOptions {
            max_retries: 2,
            retry_interval: Duration::from_millis(100),
            health_check_interval: Duration::from_secs(5),
            persistent_connection: false,
        },
    };

    let service_info = ServiceInfo {
        name: "nonexistent-service".to_string(),
        namespace: "default".to_string(),
        ports: vec![80],
    };

    let forward = PortForward::new(config, service_info);

    let client = kube::Client::try_default().await.unwrap();
    let result = forward.start(client).await;

    assert!(result.is_err());
    match result {
        Err(PortForwardError::ConnectionError(msg)) => {
            assert!(msg.contains("Max retry attempts reached"));
        }
        _ => panic!("Expected ConnectionError"),
    }
}
