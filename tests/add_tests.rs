#[cfg(test)]
mod tests {
    use kube_forward::add::{
        append_entry, build_forward_config, derive_selector_from_service, normalize_protocol,
        suggest_local_port,
    };
    use kube_forward::config::{ForwardConfig, PodSelector, PortMapping};
    use std::collections::{BTreeMap, HashSet};

    fn selector(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn test_derive_selector_prefers_well_known_keys() {
        let sel = selector(&[("foo", "bar"), ("app.kubernetes.io/name", "grafana")]);
        assert_eq!(
            derive_selector_from_service(&sel),
            Some("app.kubernetes.io/name=grafana".to_string())
        );
    }

    #[test]
    fn test_derive_selector_falls_back_to_first_key() {
        let sel = selector(&[("custom", "x"), ("zzz", "y")]);
        // BTreeMap is sorted, so "custom" comes first deterministically.
        assert_eq!(
            derive_selector_from_service(&sel),
            Some("custom=x".to_string())
        );
    }

    #[test]
    fn test_derive_selector_empty_is_none() {
        assert_eq!(derive_selector_from_service(&BTreeMap::new()), None);
    }

    #[test]
    fn test_suggest_local_port() {
        let mut taken = HashSet::new();
        assert_eq!(suggest_local_port(3000, &taken), 3000);
        taken.insert(3000);
        taken.insert(3001);
        assert_eq!(suggest_local_port(3000, &taken), 3002);
    }

    #[test]
    fn test_normalize_protocol() {
        assert_eq!(normalize_protocol(Some("UDP")), "udp");
        assert_eq!(normalize_protocol(Some("tcp")), "tcp");
        assert_eq!(normalize_protocol(None), "tcp");
        assert_eq!(normalize_protocol(Some("SCTP")), "tcp");
    }

    #[test]
    fn test_append_entry_tcp_minimal() {
        let cfg = build_forward_config(
            "grafana".to_string(),
            "grafana.monitoring".to_string(),
            3001,
            3000,
            "tcp".to_string(),
            Some("app=grafana".to_string()),
        );
        let out = append_entry("", &cfg);
        assert!(out.contains("- name: \"grafana\""));
        assert!(out.contains("target: \"grafana.monitoring\""));
        assert!(out.contains("local: 3001"));
        assert!(out.contains("remote: 3000"));
        assert!(out.contains("label: \"app=grafana\""));
        // TCP is the default, so protocol is omitted for brevity.
        assert!(!out.contains("protocol:"));
    }

    #[test]
    fn test_append_entry_udp_includes_protocol() {
        let cfg = build_forward_config(
            "dns".to_string(),
            "kube-dns.kube-system".to_string(),
            5353,
            53,
            "udp".to_string(),
            None,
        );
        let out = append_entry("", &cfg);
        assert!(out.contains("protocol: udp"));
        // No selector provided, so no pod_selector block.
        assert!(!out.contains("pod_selector:"));
    }

    #[test]
    fn test_append_entry_preserves_existing_and_reparses() {
        let existing =
            "- name: \"keep\"\n  target: \"svc.ns\"\n  ports:\n    local: 8080\n    remote: 80\n";
        let cfg = build_forward_config(
            "added".to_string(),
            "other.ns".to_string(),
            9090,
            90,
            "tcp".to_string(),
            None,
        );
        let out = append_entry(existing, &cfg);
        // Existing entry is preserved.
        assert!(out.contains("- name: \"keep\""));
        // The combined text still parses as a list of two forwards.
        let parsed: Vec<ForwardConfig> = serde_yaml::from_str(&out).unwrap();
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].name, "keep");
        assert_eq!(parsed[1].name, "added");
        assert_eq!(parsed[1].ports.local, 9090);
    }

    #[test]
    fn test_build_forward_config_uses_defaults() {
        let cfg = build_forward_config(
            "a".to_string(),
            "svc.ns".to_string(),
            1,
            2,
            "tcp".to_string(),
            None,
        );
        assert_eq!(cfg.options, kube_forward::config::default_forward_options());
        assert_eq!(
            cfg.pod_selector,
            PodSelector {
                label: None,
                annotation: None
            }
        );
        assert_eq!(
            cfg.ports,
            PortMapping {
                protocol: Some("tcp".to_string()),
                local: 1,
                remote: 2
            }
        );
    }
}
