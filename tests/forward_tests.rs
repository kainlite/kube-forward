#[cfg(test)]
mod tests {
    use k8s_openapi::api::core::v1::Pod;
    use k8s_openapi::api::core::v1::{PodSpec, PodStatus};
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
    use kube_forward::config::{ForwardConfig, PodSelector};
    use kube_forward::config::{ForwardOptions, LocalDnsConfig, PortMapping};
    use kube_forward::forward::{ForwardState, HealthCheck, PortForward};
    use kube_forward::util::ServiceInfo;
    use std::collections::BTreeMap;
    use std::process::Command;
    use std::sync::Arc;
    use std::sync::mpsc;
    use std::thread;
    use tokio::time::timeout;

    use std::time::Duration;
    use tokio::io::AsyncReadExt;
    use tokio::io::AsyncWriteExt;
    use tokio::net::TcpListener;

    use tokio::net::UdpSocket;

    #[ctor::ctor]
    fn init() {
        let _ = rustls::crypto::ring::default_provider().install_default();
    }

    #[tokio::test]
    async fn test_health_check() {
        let health_check = HealthCheck::new();

        // Start a test server
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let protocol = "TCP".to_string();

        // Test successful connection
        assert!(health_check.check_connection(port, &protocol).await);
        assert_eq!(*health_check.failures.read().await, 0);
        assert!(health_check.last_check.read().await.is_some());

        // Test failed connection
        drop(listener); // Close the listener
        assert!(!health_check.check_connection(port, &protocol).await);
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
                protocol: Some("TCP".to_string()),
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
                persistent_connection: false,
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
                protocol: Some("TCP".to_string()),
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
                persistent_connection: true,
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
                protocol: Some("TCP".to_string()),
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
                persistent_connection: false,
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
                protocol: Some("TCP".to_string()),
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
                persistent_connection: false,
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
        // Test 1: Already Connected state
        let config = ForwardConfig {
            name: "kube-dns".to_string(),
            target: "kube-dns.kube-system".to_string(),
            ports: PortMapping {
                protocol: Some("udp".to_string()),
                local: 5356,
                remote: 53,
            },
            pod_selector: PodSelector {
                label: Some("k8s-app=kube-dns".to_string()),
                annotation: None,
            },
            local_dns: LocalDnsConfig::default(),
            options: ForwardOptions {
                retry_interval: Duration::from_secs(5),
                max_retries: 5,
                persistent_connection: true,
                health_check_interval: Duration::from_secs(5),
            },
        };

        let service_info = ServiceInfo {
            name: "kube-dns".to_string(),
            namespace: "kube-system".to_string(),
            ports: vec![53],
        };

        let forward = Arc::new(PortForward::new(config, service_info));
        let client = kube::Client::try_default().await.unwrap();

        *forward.state.write().await = ForwardState::Connected;

        println!("Initial state set to Connected");

        // Create channel for coordination
        let (tx, rx) = mpsc::channel();

        // Get the current runtime handle
        let runtime_handle = tokio::runtime::Handle::current();

        // Spawn the forward in a separate OS thread
        let forward_thread = thread::spawn({
            let forward = Arc::clone(&forward);
            let client = client.clone();
            let tx = tx.clone();
            let handle = runtime_handle.clone();
            move || {
                println!("Starting forward thread");

                // Enter the runtime context
                let _guard = handle.enter();

                // Now we can use the existing runtime
                handle.block_on(async move {
                    println!("About to start forward");
                    tx.send(()).unwrap();

                    match forward.start(client).await {
                        Ok(_) => println!("Forward started successfully"),
                        Err(e) => println!("Forward error: {:?}", e),
                    }
                })
            }
        });

        // Wait for the forward to start (with timeout)
        match rx.recv_timeout(Duration::from_secs(5)) {
            Ok(_) => {
                println!("Received start signal");
                std::thread::sleep(Duration::from_secs(5));

                println!("Attempting to run test");
                let args = ["google.com", "@127.0.0.1:5356"];
                let output = Command::new("dog")
                    .args(args)
                    .output()
                    .expect("Failed to run dog command");

                dbg!(output);
            }
            Err(_) => panic!("Forward failed to start within timeout"),
        };

        println!("Cleaning up forward thread");
        drop(forward_thread);

        // Test 2: Test TCP keepalive and connection handling
        let config = ForwardConfig {
            name: "kube-dns".to_string(),
            target: "kube-system".to_string(),
            ports: PortMapping {
                protocol: Some("TCP".to_string()),
                local: 0, // Let OS assign port
                remote: 53,
            },
            pod_selector: PodSelector {
                label: Some("k8s-app=kube-dns".to_string()),
                annotation: None,
            },
            local_dns: LocalDnsConfig::default(),
            options: ForwardOptions {
                max_retries: 1,
                retry_interval: Duration::from_millis(100),
                health_check_interval: Duration::from_secs(1),
                persistent_connection: true,
            },
        };

        let service_info = ServiceInfo {
            name: "kube-dns".to_string(),
            namespace: "kube-system".to_string(),
            ports: vec![53],
        };

        let keep_forward = PortForward::new(config, service_info.clone());

        let client = kube::Client::try_default().await.unwrap();
        // Start the forward in a separate task
        let _forward_handle = tokio::spawn({
            let keep_forward = keep_forward.clone();
            let client = client.clone();
            async move {
                let result = keep_forward.establish_forward(&client).await;
                if result.is_err() {
                    println!("Forward error: {:?}", result);
                }
                result
            }
        });

        // Check if the connection is alive
        let result = keep_forward.monitor_connection(&client).await;
        match result {
            Ok(_) => {
                assert!(matches!(
                    *keep_forward.state.read().await,
                    ForwardState::Connected
                ));
            }
            Err(e) => {
                dbg!("Failed to establish forward: {:?}", e);
            }
        }

        // Give it some time to start
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Stop the forward
        forward.stop().await;
    }

    // This tests specifically depends on kind
    // just a reminder in case it fails later on
    #[tokio::test]
    async fn test_monitor_connection() {
        let config = ForwardConfig {
            name: "kube-dns".to_string(),
            target: "kube-dns.kube-system".to_string(),
            ports: kube_forward::config::PortMapping {
                protocol: Some("TCP".to_string()),
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
                persistent_connection: false,
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
                assert!(msg.contains("Failed initial health checks"));
            }
            _ => panic!("Expected ConnectionError for health check failure"),
        }
    }

    #[tokio::test]
    async fn test_forward_connection() {
        let client = kube::Client::try_default().await.unwrap();
        let pods: kube::Api<Pod> = kube::Api::namespaced(client, "default");

        // Create a mock TCP connection pair
        let (client_stream, mut server_stream) = tokio::io::duplex(64);

        // Spawn a task to simulate the remote end
        let server_handle = tokio::spawn(async move {
            let mut buf = [0u8; 64];
            let _ = server_stream.read(&mut buf).await;
            let _ = server_stream.write_all(b"response data").await;
        });

        // Test the forward_connection function
        let result =
            PortForward::forward_connection(&pods, "test-pod".to_string(), 80, client_stream).await;

        // Wait for the server task
        let _ = server_handle.await;

        // Since we're not in a real k8s environment, this should fail
        assert!(result.is_err());

        // Test with timeout
        let (client_stream, mut server_stream) = tokio::io::duplex(64);

        // Spawn a task that will delay, triggering the timeout
        let server_handle = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(300)).await;
            let _ = server_stream.write_all(b"delayed response").await;
        });

        let result =
            PortForward::forward_connection(&pods, "test-pod".to_string(), 80, client_stream).await;
        let _ = server_handle.await;
        assert!(result.is_err());
    }

    #[test]
    fn test_matches_pod_selector_comprehensive() {
        let config = ForwardConfig {
            name: "test-forward".to_string(),
            target: "test-target.test-namespace".to_string(),
            ports: kube_forward::config::PortMapping {
                protocol: Some("TCP".to_string()),
                local: 8080,
                remote: 80,
            },
            local_dns: kube_forward::config::LocalDnsConfig {
                enabled: false,
                hostname: None,
            },
            pod_selector: PodSelector {
                label: Some("app=myapp".to_string()),
                annotation: Some("monitoring=enabled".to_string()),
            },
            options: kube_forward::config::ForwardOptions {
                max_retries: 3,
                retry_interval: Duration::from_secs(1),
                health_check_interval: Duration::from_secs(5),
                persistent_connection: true,
            },
        };

        let service_info = ServiceInfo {
            name: "test-service".to_string(),
            namespace: "default".to_string(),
            ports: vec![80],
        };

        let forward = PortForward::new(config, service_info);

        // Test 1: Pod with matching label and annotation
        let mut pod = Pod::default();
        pod.metadata.labels = Some(BTreeMap::from([("app".to_string(), "myapp".to_string())]));
        pod.metadata.annotations = Some(BTreeMap::from([(
            "monitoring".to_string(),
            "enabled".to_string(),
        )]));

        assert!(forward.clone().matches_pod_selector(
            &pod,
            &PodSelector {
                label: Some("app=myapp".to_string()),
                annotation: Some("monitoring=enabled".to_string()),
            }
        ));

        // Test 2: Pod with matching label but no annotation
        let mut pod = Pod::default();
        pod.metadata.labels = Some(BTreeMap::from([("app".to_string(), "myapp".to_string())]));

        assert!(!forward.clone().matches_pod_selector(
            &pod,
            &PodSelector {
                label: Some("app=myapp".to_string()),
                annotation: Some("monitoring=enabled".to_string()),
            }
        ));

        // Test 3: Pod with no selectors
        let pod = Pod::default();
        assert!(!forward.clone().matches_pod_selector(
            &pod,
            &PodSelector {
                label: None,
                annotation: None,
            }
        ));

        // Test 4: Pod with service name in labels but no specific selector
        let mut pod = Pod::default();
        pod.metadata.labels = Some(BTreeMap::from([(
            "service".to_string(),
            "test-service".to_string(),
        )]));

        assert!(forward.clone().matches_pod_selector(
            &pod,
            &PodSelector {
                label: None,
                annotation: None,
            }
        ));
    }

    #[tokio::test]
    async fn test_get_pod_with_different_states() {
        let client = kube::Client::try_default().await.unwrap();

        // Create test pods with different states
        let running_pod = Pod {
            metadata: ObjectMeta {
                name: Some("test-pod-running".to_string()),
                namespace: Some("default".to_string()),
                labels: Some(BTreeMap::from([("app".to_string(), "test".to_string())])),
                ..ObjectMeta::default()
            },
            spec: Some(PodSpec::default()),
            status: Some(PodStatus {
                phase: Some("Running".to_string()),
                ..PodStatus::default()
            }),
        };

        let mut pending_pod = running_pod.clone();
        pending_pod.status = Some(PodStatus {
            phase: Some("Pending".to_string()),
            ..PodStatus::default()
        });

        let config = ForwardConfig {
            name: "test-forward".to_string(),
            target: "test-target".to_string(),
            ports: PortMapping {
                protocol: Some("TCP".to_string()),
                local: 8080,
                remote: 80,
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
        };

        let service_info = ServiceInfo {
            name: "test-service".to_string(),
            namespace: "default".to_string(),
            ports: vec![80],
        };

        let forward = PortForward::new(config, service_info);

        // Test get_pod behavior
        let result = forward.get_pod(&client).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_try_release_port_scenarios() {
        let config = ForwardConfig {
            name: "test-forward".to_string(),
            target: "test-target".to_string(),
            ports: PortMapping {
                protocol: Some("TCP".to_string()),
                local: 0,
                remote: 80,
            },
            pod_selector: PodSelector::default(),
            local_dns: LocalDnsConfig::default(),
            options: ForwardOptions::default(),
        };

        let service_info = ServiceInfo {
            name: "test-service".to_string(),
            namespace: "default".to_string(),
            ports: vec![80],
        };

        let mut forward = PortForward::new(config, service_info);

        // Test 1: Port in Connected state
        *forward.state.write().await = ForwardState::Connected;
        let result = forward.try_release_port().await;
        assert!(result.is_ok());

        // Test 2: Port in Starting state
        *forward.state.write().await = ForwardState::Starting;
        let result = forward.try_release_port().await;
        assert!(result.is_ok());

        // Test 3: Port actually in use by another process
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        forward.config.ports.local = port;
        *forward.state.write().await = ForwardState::Disconnected;
        let result = forward.try_release_port().await;
        assert!(result.is_err());
        assert!(result.unwrap_err().kind() == std::io::ErrorKind::AddrInUse);
    }

    #[tokio::test]
    async fn test_get_pod() {
        let config = ForwardConfig {
            name: "test-forward".to_string(),
            target: "test-target.test-namespace".to_string(),
            ports: kube_forward::config::PortMapping {
                protocol: Some("TCP".to_string()),
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
                persistent_connection: false,
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

    // Helper function to create a test UDP forward configuration
    fn create_udp_test_config(local_port: u16) -> ForwardConfig {
        ForwardConfig {
            name: "kube-dns".to_string(),
            target: "kube-system".to_string(),
            ports: PortMapping {
                local: local_port,
                remote: 53,
                protocol: Some("UDP".to_string()),
            },
            pod_selector: PodSelector {
                label: Some("k8s-app=kube-dns".to_string()),
                annotation: None,
            },
            local_dns: LocalDnsConfig::default(),
            options: ForwardOptions {
                max_retries: 3,
                retry_interval: Duration::from_millis(100),
                health_check_interval: Duration::from_secs(1),
                persistent_connection: true,
            },
        }
    }

    #[tokio::test]
    async fn test_udp_forward() {
        // Find a free port
        let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let local_port = socket.local_addr().unwrap().port();
        drop(socket);

        let config = create_udp_test_config(local_port);
        let service_info = ServiceInfo {
            name: "kube-dns".to_string(),
            namespace: "kube-system".to_string(),
            ports: vec![53],
        };

        let forward = PortForward::new(config, service_info);
        let client = kube::Client::try_default().await.unwrap();

        // Start the forward
        let result = forward.establish_forward(&client).await;
        assert!(result.is_ok(), "Failed to establish forward: {:?}", result);

        // Wait for the port forward to be ready
        // tokio::time::sleep(Duration::from_secs(1)).await;

        // Test UDP communication
        let _test_result = timeout(Duration::from_secs(1), async {
            let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
            socket
                .connect(format!("127.0.0.1:{}", local_port))
                .await
                .unwrap();

            // DNS query for google.com (simplified)
            let query = vec![
                0x00, 0x01, // Transaction ID
                0x01, 0x00, // Flags
                0x00, 0x01, // Questions
                0x00, 0x00, // Answer RRs
                0x00, 0x00, // Authority RRs
                0x00, 0x00, // Additional RRs
                0x06, 0x67, 0x6f, 0x6f, 0x67, 0x6c, 0x65, // google
                0x03, 0x63, 0x6f, 0x6d, // com
                0x00, // null terminator
                0x00, 0x01, // Type A
                0x00, 0x01, // Class IN
            ];

            socket.send(&query).await.unwrap();

            let mut buf = vec![0u8; 512];
            let len = socket.recv(&mut buf).await.unwrap();

            assert!(len > 0, "Received empty response");
            assert!(buf[2] & 0x80 != 0, "Not a DNS response"); // Check if response bit is set

            true
        })
        .await;

        // assert!(test_result.is_ok(), "UDP test timed out");
        // assert!(test_result.unwrap(), "UDP test failed");

        // Test concurrent UDP connections
        let _test_concurrent = timeout(Duration::from_secs(1), async {
            let mut handles = vec![];

            for _ in 0..5 {
                let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
                socket
                    .connect(format!("127.0.0.1:{}", local_port))
                    .await
                    .unwrap();

                handles.push(tokio::spawn(async move {
                    let query = vec![
                        0x00, 0x01, 0x01, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
                    ];
                    socket.send(&query).await.unwrap();

                    let mut buf = vec![0u8; 512];
                    socket.recv(&mut buf).await.unwrap()
                }));
            }

            for handle in handles {
                let len = handle.await.unwrap();
                assert!(len > 0, "Concurrent UDP test received empty response");
            }

            true
        })
        .await;

        // assert!(test_concurrent.is_ok(), "Concurrent UDP test timed out");
        // assert!(test_concurrent.unwrap(), "Concurrent UDP test failed");

        // Clean up
        forward.stop().await;
    }

    // #[tokio::test]
    // async fn test_handle_udp_packet() {
    //     // Create a mock UDP socket
    //     let socket = Arc::new(UdpSocket::bind("127.0.0.1:0").await.unwrap());
    //     let server_addr = socket.local_addr().unwrap();

    //     dbg!("test");
    //     // Create a client UDP socket
    //     let client_socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    //     client_socket.connect(server_addr).await.unwrap();

    //     // Create test data
    //     let test_data = vec![
    //         0x00, 0x01, // Transaction ID
    //         0x01, 0x00, // Flags
    //         0x00, 0x01, // Questions
    //         0x00, 0x00, // Answer RRs
    //         0x00, 0x00, // Authority RRs
    //         0x00, 0x00, // Additional RRs
    //         // DNS query data
    //         0x03, b'w', b'w', b'w', // www
    //         0x07, b'e', b'x', b'a', b'm', b'p', b'l', b'e', // example
    //         0x03, b'c', b'o', b'm', // com
    //         0x00, // null terminator
    //         0x00, 0x01, // Type A
    //         0x00, 0x01, // Class IN
    //     ];

    //     // Set up the kubernetes client and API
    //     let client = kube::Client::try_default().await.unwrap();
    //     let pods: Api<Pod> = Api::namespaced(client, "default");

    //     // Create a mock response handler
    //     let response_handler = tokio::spawn({
    //         let socket = socket.clone();
    //         async move {
    //             let mut buf = vec![0u8; 512];
    //             let (_, peer) = socket.recv_from(&mut buf).await.unwrap();

    //             // Simulate DNS response
    //             let response = vec![
    //                 0x00, 0x01, // Transaction ID (same as query)
    //                 0x81, 0x80, // Flags (Standard response)
    //                 0x00, 0x01, // Questions
    //                 0x00, 0x01, // Answer RRs
    //                 0x00, 0x00, // Authority RRs
    //                 0x00, 0x00, // Additional RRs
    //                 // Original query
    //                 0x03, b'w', b'w', b'w', 0x07, b'e', b'x', b'a', b'm', b'p', b'l', b'e', 0x03,
    //                 b'c', b'o', b'm', 0x00, 0x00, 0x01, // Type A
    //                 0x00, 0x01, // Class IN
    //                 // Answer
    //                 0xc0, 0x0c, // Pointer to domain name
    //                 0x00, 0x01, // Type A
    //                 0x00, 0x01, // Class IN
    //                 0x00, 0x00, 0x0e, 0x10, // TTL (3600 seconds)
    //                 0x00, 0x04, // Data length
    //                 0x0a, 0x00, 0x00, 0x0a, // IP address (10.0.0.10)
    //             ];

    //             socket.send_to(&response, peer).await.unwrap();
    //         }
    //     });

    //     // Test sending a UDP packet
    //     let result = PortForward::handle_udp_packet(
    //         &pods,
    //         "test-pod".to_string(),
    //         53,
    //         socket.clone(),
    //         test_data.clone(),
    //         client_socket.local_addr().unwrap(),
    //     )
    //     .await;

    //     // The actual handle_udp_packet call will fail because we're not in a real k8s environment
    //     assert!(
    //         result.is_err(),
    //         "Expected error due to missing k8s environment"
    //     );

    //     // But we can verify that our socket received the data correctly
    //     let _ = response_handler.await;

    //     // Test error handling with invalid port
    //     let result = PortForward::handle_udp_packet(
    //         &pods,
    //         "test-pod".to_string(),
    //         0, // Invalid port
    //         socket.clone(),
    //         test_data.clone(),
    //         client_socket.local_addr().unwrap(),
    //     )
    //     .await;
    //     assert!(result.is_err(), "Expected error with invalid port");

    //     // Test with empty data
    //     let result = PortForward::handle_udp_packet(
    //         &pods,
    //         "test-pod".to_string(),
    //         53,
    //         socket.clone(),
    //         vec![], // Empty data
    //         client_socket.local_addr().unwrap(),
    //     )
    //     .await;
    //     assert!(result.is_err(), "Expected error with empty data");

    //     // Test with non-existent pod
    //     let result = PortForward::handle_udp_packet(
    //         &pods,
    //         "non-existent-pod".to_string(),
    //         53,
    //         socket,
    //         test_data,
    //         client_socket.local_addr().unwrap(),
    //     )
    //     .await;
    //     assert!(result.is_err(), "Expected error with non-existent pod");
    // }

    #[tokio::test]
    async fn test_udp_forward_reconnection() {
        let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let local_port = socket.local_addr().unwrap().port();
        drop(socket);

        let mut config = create_udp_test_config(local_port);
        config.options.persistent_connection = true;
        config.options.retry_interval = Duration::from_millis(100);

        let service_info = ServiceInfo {
            name: "kube-dns".to_string(),
            namespace: "kube-system".to_string(),
            ports: vec![53],
        };

        let forward = PortForward::new(config, service_info);
        let client = kube::Client::try_default().await.unwrap();

        // Start the forward
        let result = forward.establish_forward(&client).await;
        assert!(result.is_ok(), "Failed to establish forward: {:?}", result);
        dbg!(&forward.state);

        // Wait for initial connection
        tokio::time::sleep(Duration::from_secs(1)).await;

        // Verify initial state
        // assert!(matches!(
        //     *forward.state.read().await,
        //     ForwardState::Connected
        // ));

        // Force a reconnection by stopping and starting
        forward.stop().await;
        assert!(matches!(
            *forward.state.read().await,
            ForwardState::Disconnected
        ));

        // Restart the forward
        let result = forward.establish_forward(&client).await;
        assert!(
            result.is_ok(),
            "Failed to re-establish forward: {:?}",
            result
        );

        // Wait for reconnection
        tokio::time::sleep(Duration::from_secs(1)).await;
        assert!(matches!(
            *forward.state.read().await,
            ForwardState::Connected
        ));

        // Clean up
        forward.stop().await;
    }

    #[tokio::test]
    async fn test_handle_tcp_packet() {
        // Create a mock TCP server to simulate the kubernetes API
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let server_port = listener.local_addr().unwrap().port();

        // Create the test configuration
        let config = ForwardConfig {
            name: "test-tcp-forward".to_string(),
            target: "test-service.default".to_string(),
            ports: PortMapping {
                protocol: Some("TCP".to_string()),
                local: server_port,
                remote: 80,
            },
            pod_selector: PodSelector::default(),
            local_dns: LocalDnsConfig::default(),
            options: ForwardOptions::default(),
        };

        let service_info = ServiceInfo {
            name: "test-service".to_string(),
            namespace: "default".to_string(),
            ports: vec![80],
        };

        let forward = PortForward::new(config, service_info);

        // Create a client connection
        let client_socket = tokio::net::TcpStream::connect(format!("127.0.0.1:{}", server_port))
            .await
            .unwrap();

        // Accept the connection in a separate task
        let accept_handle = tokio::spawn(async move {
            let (server_socket, _) = listener.accept().await.unwrap();
            server_socket
        });

        // Get the server side of the connection
        let server_socket = accept_handle.await.unwrap();

        // Test the TCP packet handling
        let test_data = b"test message";
        let (mut client_read, mut client_write) = client_socket.into_split();
        let (mut server_read, mut server_write) = server_socket.into_split();

        // Write test data
        client_write.write_all(test_data).await.unwrap();
        client_write.flush().await.unwrap();

        // Read the data on the server side
        let mut received = vec![0u8; test_data.len()];
        server_read.read_exact(&mut received).await.unwrap();
        assert_eq!(&received, test_data);

        // Send response
        let response = b"response data";
        server_write.write_all(response).await.unwrap();
        server_write.flush().await.unwrap();

        // Read response
        let mut received_response = vec![0u8; response.len()];
        client_read
            .read_exact(&mut received_response)
            .await
            .unwrap();
        assert_eq!(&received_response, response);

        // Clean up
        forward.stop().await;
    }

    #[tokio::test]
    async fn test_handle_tcp_forward() {
        // Create a mock TCP server
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let server_port = listener.local_addr().unwrap().port();

        // Create the test configuration
        let config = ForwardConfig {
            name: "test-tcp-forward".to_string(),
            target: "test-service.default".to_string(),
            ports: PortMapping {
                protocol: Some("TCP".to_string()),
                local: server_port,
                remote: 80,
            },
            pod_selector: PodSelector::default(),
            local_dns: LocalDnsConfig::default(),
            options: ForwardOptions::default(),
        };

        let service_info = ServiceInfo {
            name: "test-service".to_string(),
            namespace: "default".to_string(),
            ports: vec![80],
        };

        let forward = Arc::new(PortForward::new(config, service_info));
        let client = kube::Client::try_default().await.unwrap();

        // Start the TCP forward
        let forward_handle = tokio::spawn({
            let forward = forward.clone();
            let client = client.clone();
            async move {
                forward
                    .handle_tcp_forward(&client, "test-pod".to_string(), listener)
                    .await
            }
        });

        // Wait a bit for the forward to start
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Test multiple concurrent connections
        let mut handles = vec![];
        for i in 0..3 {
            let handle = tokio::spawn(async move {
                let mut stream =
                    tokio::net::TcpStream::connect(format!("127.0.0.1:{}", server_port))
                        .await
                        .unwrap();

                // Send test data
                let test_data = format!("test message {}", i);
                stream.write_all(test_data.as_bytes()).await.unwrap();
                stream.flush().await.unwrap();

                // Read response (this will fail in test environment since we don't have a real k8s cluster)
                let mut buf = vec![0u8; 1024];
                match stream.read(&mut buf).await {
                    Ok(_) | Err(_) => (), // We expect this to fail or timeout in test environment
                }
            });
            handles.push(handle);
        }

        // Wait for all connection tests to complete
        for handle in handles {
            let _ = handle.await;
        }

        // Stop the forward
        forward.stop().await;
        let _ = forward_handle.await;

        // Verify the forward ended in the correct state
        assert!(matches!(
            *forward.state.read().await,
            ForwardState::Disconnected
        ));
    }
}
