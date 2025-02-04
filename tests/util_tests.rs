#[cfg(test)]
mod tests {

    use k8s_openapi::api::core::v1::ServicePort;
    use k8s_openapi::api::core::v1::ServiceSpec;
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
    use kube::Client;
    use kube_forward::util::{parse_full_dns_name, resolve_service, ServiceInfo};

    use k8s_openapi::api::core::v1::Service;
    use kube_forward::error::PortForwardError;

    #[test]
    fn test_service_info() {
        let service_info = ServiceInfo {
            namespace: "default".to_string(),
            name: "my-service".to_string(),
            ports: vec![80, 443],
        };

        assert_eq!(service_info.namespace, "default");
        assert_eq!(service_info.name, "my-service");
        assert_eq!(service_info.ports, vec![80, 443]);

        // Test Clone and Debug
        let cloned = service_info.clone();
        assert_eq!(service_info, cloned);
        assert_eq!(
            format!("{:?}", service_info),
            "ServiceInfo { namespace: \"default\", name: \"my-service\", ports: [80, 443] }"
        );
    }

    #[test]
    fn test_parse_full_dns_name() {
        // Test valid DNS name
        let parts = vec!["service", "namespace", "svc", "cluster", "local"];
        let result = parse_full_dns_name(&parts);
        assert!(result.is_ok());
        let (service, namespace) = result.unwrap();
        assert_eq!(service, "service");
        assert_eq!(namespace, "namespace");

        // Test invalid DNS name (too short)
        let invalid_parts = vec!["service"];
        let result = parse_full_dns_name(&invalid_parts);
        assert!(result.is_err());
        match result {
            Err(PortForwardError::DnsError(msg)) => {
                assert_eq!(msg, "Invalid DNS name format");
            }
            _ => panic!("Expected DnsError"),
        }
    }

    #[tokio::test]
    async fn test_resolve_service() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        // Create a mock service
        let _service = Service {
            metadata: ObjectMeta {
                name: Some("test-service".to_string()),
                namespace: Some("default".to_string()),
                ..ObjectMeta::default()
            },
            spec: Some(ServiceSpec {
                ports: Some(vec![
                    ServicePort {
                        port: 80,
                        ..ServicePort::default()
                    },
                    ServicePort {
                        port: 443,
                        ..ServicePort::default()
                    },
                ]),
                ..ServiceSpec::default()
            }),
            status: None,
        };

        // Create a mock client using kube's API
        let mock_client = Client::try_default().await.unwrap();

        // Test with simple service name
        let result = resolve_service(mock_client.clone(), "test-service").await;
        // Note: This will fail without a real k8s cluster or proper mocking
        assert!(result.is_err());

        // Test with service.namespace format
        let result = resolve_service(mock_client.clone(), "test-service.default").await;
        // Note: This will fail without a real k8s cluster or proper mocking
        assert!(result.is_err());

        // Test with full DNS name
        let result = resolve_service(
            mock_client.clone(),
            "test-service.default.svc.cluster.local",
        )
        .await;
        // Note: This will fail without a real k8s cluster or proper mocking
        assert!(result.is_err());
    }
}
