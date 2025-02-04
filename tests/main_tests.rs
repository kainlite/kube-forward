use std::fs;
use tempfile::tempdir;
use tokio::fs::File;
use tokio::io::AsyncWriteExt;

#[ctor::ctor]
fn init() {
    let _ = rustls::crypto::ring::default_provider().install_default();
}

#[tokio::test]
async fn test_main_config_loading() {
    // Create a temporary directory
    let temp_dir = tempdir().unwrap();
    let config_path = temp_dir.path().join("test_config.yaml");

    // Create a test configuration file
    let config_content = r#"
- name: test-forward
  target: test-service.default
  ports:
    local: 8080
    remote: 80
  pod_selector:
    label: app=test
  options:
    retry_interval: 1s
    max_retries: 2
    health_check_interval: 5s
    persistent_connection: false
"#;

    fs::write(&config_path, config_content).unwrap();

    // Test configuration loading
    let config_str = tokio::fs::read_to_string(&config_path).await.unwrap();
    let config: Vec<kube_forward::config::ForwardConfig> =
        serde_yaml::from_str(&config_str).unwrap();

    assert_eq!(config.len(), 1);
    assert_eq!(config[0].name, "test-forward");
    assert_eq!(config[0].target, "test-service.default");
}

#[tokio::test]
async fn test_main_metrics_initialization() {
    // Test metrics initialization
    let builder = metrics_exporter_prometheus::PrometheusBuilder::new();
    let result = builder
        .with_http_listener(([0, 0, 0, 0], 19292))
        .add_global_label("service", "kube-forward")
        .install();

    assert!(result.is_ok());
}

#[tokio::test]
async fn test_main_invalid_config() {
    // Create a temporary directory
    let temp_dir = tempdir().unwrap();
    let config_path = temp_dir.path().join("invalid_config.yaml");

    // Create an invalid configuration file
    let invalid_config = "invalid: yaml: content";
    let mut file = File::create(&config_path).await.unwrap();
    file.write_all(invalid_config.as_bytes()).await.unwrap();

    // Test loading invalid configuration
    let config_str = tokio::fs::read_to_string(&config_path).await.unwrap();
    let result: Result<Vec<kube_forward::config::ForwardConfig>, _> =
        serde_yaml::from_str(&config_str);
    assert!(result.is_err());
}

#[tokio::test]
async fn test_main_kubernetes_client() {
    // Test Kubernetes client initialization
    let client_result = kube::Client::try_default().await;
    assert!(client_result.is_ok());
}
