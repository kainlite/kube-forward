#[cfg(test)]
mod tests {
    use kube_forward::config::{ForwardConfig, ForwardOptions, PodSelector, PortMapping};
    use kube_forward::validate::{validate_configs, validate_selector};

    fn base_config(name: &str, local: u16, remote: u16) -> ForwardConfig {
        ForwardConfig {
            name: name.to_string(),
            target: "svc.ns".to_string(),
            ports: PortMapping {
                protocol: Some("TCP".to_string()),
                local,
                remote,
            },
            options: ForwardOptions::default(),
            pod_selector: PodSelector::default(),
        }
    }

    #[test]
    fn test_valid_config_passes() {
        let configs = vec![base_config("a", 8080, 80), base_config("b", 8081, 80)];
        let report = validate_configs(&configs);
        assert!(report.is_valid(), "expected valid, got {:?}", report.errors);
        assert!(report.warnings.is_empty());
    }

    #[test]
    fn test_empty_config_warns_but_is_valid() {
        let report = validate_configs(&[]);
        assert!(report.is_valid());
        assert_eq!(report.warnings.len(), 1);
        assert!(report.warnings[0].contains("no port-forwards"));
    }

    #[test]
    fn test_duplicate_name_is_error() {
        let configs = vec![base_config("dup", 8080, 80), base_config("dup", 8081, 80)];
        let report = validate_configs(&configs);
        assert!(!report.is_valid());
        assert!(
            report
                .errors
                .iter()
                .any(|e| e.contains("duplicate forward name 'dup'")),
            "errors: {:?}",
            report.errors
        );
    }

    #[test]
    fn test_duplicate_local_port_same_protocol_is_error() {
        let configs = vec![base_config("a", 8080, 80), base_config("b", 8080, 90)];
        let report = validate_configs(&configs);
        assert!(!report.is_valid());
        assert!(
            report
                .errors
                .iter()
                .any(|e| e.contains("local TCP port 8080 is used by multiple forwards")),
            "errors: {:?}",
            report.errors
        );
    }

    #[test]
    fn test_same_local_port_different_protocol_is_allowed() {
        let mut udp = base_config("dns", 5353, 53);
        udp.ports.protocol = Some("udp".to_string());
        let tcp = base_config("web", 5353, 80);
        let report = validate_configs(&[tcp, udp]);
        assert!(report.is_valid(), "errors: {:?}", report.errors);
    }

    #[test]
    fn test_invalid_protocol_is_error() {
        let mut c = base_config("a", 8080, 80);
        c.ports.protocol = Some("sctp".to_string());
        let report = validate_configs(&[c]);
        assert!(!report.is_valid());
        assert!(
            report
                .errors
                .iter()
                .any(|e| e.contains("invalid protocol 'sctp'"))
        );
    }

    #[test]
    fn test_zero_ports_are_errors() {
        let report = validate_configs(&[base_config("a", 0, 0)]);
        assert!(!report.is_valid());
        assert!(
            report
                .errors
                .iter()
                .any(|e| e.contains("local port must be non-zero"))
        );
        assert!(
            report
                .errors
                .iter()
                .any(|e| e.contains("remote port must be non-zero"))
        );
    }

    #[test]
    fn test_empty_target_is_error() {
        let mut c = base_config("a", 8080, 80);
        c.target = "".to_string();
        let report = validate_configs(&[c]);
        assert!(!report.is_valid());
        assert!(report.errors.iter().any(|e| e.contains("target is empty")));
    }

    #[test]
    fn test_unparseable_target_is_error() {
        let mut c = base_config("a", 8080, 80);
        c.target = ".".to_string();
        let report = validate_configs(&[c]);
        assert!(!report.is_valid());
        assert!(report.errors.iter().any(|e| e.contains("invalid target")));
    }

    #[test]
    fn test_malformed_selector_is_error() {
        let mut c = base_config("a", 8080, 80);
        c.pod_selector.label = Some("no-equals".to_string());
        let report = validate_configs(&[c]);
        assert!(!report.is_valid());
        assert!(
            report
                .errors
                .iter()
                .any(|e| e.contains("pod_selector label") && e.contains("malformed")),
            "errors: {:?}",
            report.errors
        );
    }

    #[test]
    fn test_empty_name_is_error() {
        let report = validate_configs(&[base_config("  ", 8080, 80)]);
        assert!(!report.is_valid());
        assert!(report.errors.iter().any(|e| e.contains("name is empty")));
    }

    #[test]
    fn test_validate_selector_directly() {
        assert!(validate_selector("app=web").is_ok());
        assert!(validate_selector("app.kubernetes.io/name=web").is_ok());
        assert!(validate_selector("=web").is_err());
        assert!(validate_selector("no-equals").is_err());
    }
}
