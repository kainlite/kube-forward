#[cfg(test)]
mod tests {
    use kube_forward::config::{
        ForwardConfig, ForwardOptions, LocalDnsConfig, PodSelector, PortMapping,
        default_forward_options, default_health_check_interval, default_max_retries,
        default_persistent_connection, default_retry_interval,
    };
    use std::time::Duration;

    #[ctor::ctor]
    fn init() {
        let _ = rustls::crypto::ring::default_provider().install_default();
    }

    #[test]
    fn test_config_defaults() {
        // Test default values
        assert_eq!(default_retry_interval(), Duration::from_secs(5));
        assert_eq!(default_max_retries(), 3);
        assert_eq!(default_health_check_interval(), Duration::from_secs(10));
        assert!(default_persistent_connection());

        // Test default ForwardOptions
        let default_options = default_forward_options();
        assert_eq!(default_options.retry_interval, default_retry_interval());
        assert_eq!(default_options.max_retries, default_max_retries());
        assert_eq!(
            default_options.health_check_interval,
            default_health_check_interval()
        );
        assert_eq!(
            default_options.persistent_connection,
            default_persistent_connection()
        );
    }

    #[test]
    fn test_config_serialization() {
        let config = ForwardConfig {
            name: "test".to_string(),
            target: "test-service".to_string(),
            ports: PortMapping {
                protocol: Some("TCP".to_string()),
                local: 8080,
                remote: 80,
            },
            options: ForwardOptions {
                retry_interval: Duration::from_secs(5),
                max_retries: 3,
                health_check_interval: Duration::from_secs(10),
                persistent_connection: true,
            },
            local_dns: LocalDnsConfig {
                enabled: true,
                hostname: Some("test.local".to_string()),
            },
            pod_selector: PodSelector {
                label: Some("app=test".to_string()),
                annotation: Some("env=prod".to_string()),
            },
        };

        // Test serialization
        let yaml = serde_yaml::to_string(&config).unwrap();

        // Test deserialization
        let deserialized: ForwardConfig = serde_yaml::from_str(&yaml).unwrap();

        assert_eq!(config.name, deserialized.name);
        assert_eq!(config.target, deserialized.target);
        assert_eq!(config.ports.local, deserialized.ports.local);
        assert_eq!(config.ports.remote, deserialized.ports.remote);
        assert_eq!(config.options.max_retries, deserialized.options.max_retries);
        assert_eq!(
            config.options.persistent_connection,
            deserialized.options.persistent_connection
        );
        assert_eq!(config.local_dns.enabled, deserialized.local_dns.enabled);
        assert_eq!(config.local_dns.hostname, deserialized.local_dns.hostname);
        assert_eq!(config.pod_selector.label, deserialized.pod_selector.label);
        assert_eq!(
            config.pod_selector.annotation,
            deserialized.pod_selector.annotation
        );
    }

    #[test]
    fn test_config_partial_eq() {
        let config1 = ForwardConfig {
            name: "test".to_string(),
            target: "test-service".to_string(),
            ports: PortMapping {
                protocol: Some("TCP".to_string()),
                local: 8080,
                remote: 80,
            },
            options: ForwardOptions::default(),
            local_dns: LocalDnsConfig::default(),
            pod_selector: PodSelector::default(),
        };

        let config2 = config1.clone();
        assert_eq!(config1, config2);

        let different_config = ForwardConfig {
            name: "different".to_string(),
            ..config1.clone()
        };
        assert_ne!(config1, different_config);
    }
}
