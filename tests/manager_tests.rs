use kube::Client;
use kube_forward::{
    config::{ForwardConfig, ForwardOptions, LocalDnsConfig, PodSelector, PortMapping},
    forward::PortForwardManager,
    util::ServiceInfo,
};
use std::time::Duration;

async fn create_test_config(name: &str, local_port: u16, remote_port: u16) -> ForwardConfig {
    ForwardConfig {
        name: name.to_string(),
        target: format!("{}.test-namespace", name),
        ports: PortMapping {
            protocol: Some("TCP".to_string()),
            local: local_port,
            remote: remote_port,
        },
        pod_selector: PodSelector {
            label: Some("app=test".to_string()),
            annotation: None,
        },
        local_dns: LocalDnsConfig {
            enabled: false,
            hostname: None,
        },
        options: ForwardOptions {
            max_retries: 3,
            retry_interval: Duration::from_secs(1),
            health_check_interval: Duration::from_secs(5),
            persistent_connection: false,
        },
    }
}

async fn create_test_service(name: &str) -> ServiceInfo {
    ServiceInfo {
        name: name.to_string(),
        namespace: "default".to_string(),
        ports: vec![80],
    }
}

#[tokio::test]
async fn test_manager_creation() {
    let client = Client::try_default()
        .await
        .expect("Failed to create client");
    let manager = PortForwardManager::new(client);

    let forwards = manager.forwards.read().await;
    assert_eq!(forwards.len(), 0);
}

#[tokio::test]
async fn test_manager_add_forward() {
    let client = Client::try_default()
        .await
        .expect("Failed to create client");
    let manager = PortForwardManager::new(client);

    let config = create_test_config("test-forward", 8080, 80).await;
    let service_info = create_test_service("test-service").await;

    manager
        .add_forward(config.clone(), service_info.clone())
        .await
        .expect("Failed to add forward");

    let forwards = manager.forwards.read().await;
    assert_eq!(forwards.len(), 1);
    assert_eq!(forwards[0].config.name, "test-forward");
}

#[tokio::test]
async fn test_manager_multiple_forwards() {
    let client = Client::try_default()
        .await
        .expect("Failed to create client");
    let manager = PortForwardManager::new(client);

    // Add multiple forwards
    for i in 0..3 {
        let config = create_test_config(&format!("test-forward-{}", i), 8080 + i, 80).await;
        let service_info = create_test_service(&format!("test-service-{}", i)).await;

        manager
            .add_forward(config, service_info)
            .await
            .expect("Failed to add forward");
    }

    let forwards = manager.forwards.read().await;
    assert_eq!(forwards.len(), 3);
}

#[tokio::test]
async fn test_manager_stop_all() {
    let client = Client::try_default()
        .await
        .expect("Failed to create client");
    let manager = PortForwardManager::new(client);

    // Add a few forwards
    for i in 0..2 {
        let config = create_test_config(&format!("test-forward-{}", i), 8080 + i, 80).await;
        let service_info = create_test_service(&format!("test-service-{}", i)).await;

        manager
            .add_forward(config, service_info)
            .await
            .expect("Failed to add forward");
    }

    // Stop all forwards
    manager.stop_all().await;

    // Give some time for the shutdown to propagate
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Verify all forwards are stopped
    let forwards = manager.forwards.read().await;
    for forward in forwards.iter() {
        // The forward should either be in Disconnected or Failed state
        let state = forward.state.read().await;
        assert!(matches!(
            *state,
            kube_forward::forward::ForwardState::Disconnected
                | kube_forward::forward::ForwardState::Failed(_)
        ));
    }
}
