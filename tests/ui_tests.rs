#[cfg(test)]
mod tests {
    use kube_forward::config::{ForwardConfig, ForwardOptions, PodSelector, PortMapping};
    use kube_forward::ui::{ForwardStatus, forward_table, startup_summary};

    fn config(name: &str, local: u16, remote: u16, proto: &str, target: &str) -> ForwardConfig {
        ForwardConfig {
            name: name.to_string(),
            target: target.to_string(),
            ports: PortMapping {
                protocol: Some(proto.to_string()),
                local,
                remote,
            },
            options: ForwardOptions::default(),
            pod_selector: PodSelector::default(),
        }
    }

    #[test]
    fn test_forward_table_empty() {
        assert_eq!(forward_table(&[]), "");
    }

    #[test]
    fn test_forward_table_aligns_and_includes_fields() {
        let configs = vec![
            config("argocd-ui", 8080, 8080, "tcp", "argocd-server.argocd"),
            config("coredns", 5454, 53, "udp", "kube-dns.kube-system"),
        ];
        let table = forward_table(&configs);
        let lines: Vec<&str> = table.lines().collect();
        assert_eq!(lines.len(), 2);

        assert!(lines[0].contains("argocd-ui"));
        assert!(lines[0].contains("127.0.0.1:8080 -> argocd-server.argocd:8080"));
        assert!(lines[0].contains("TCP"));

        assert!(lines[1].contains("coredns"));
        assert!(lines[1].contains("127.0.0.1:5454 -> kube-dns.kube-system:53"));
        // protocol is uppercased even when configured lowercase
        assert!(lines[1].contains("UDP"));

        // The address column starts at the same offset on every row (aligned).
        let offset0 = lines[0].find("127.0.0.1").unwrap();
        let offset1 = lines[1].find("127.0.0.1").unwrap();
        assert_eq!(offset0, offset1);
    }

    #[test]
    fn test_startup_summary_all_established() {
        let statuses = vec![
            ForwardStatus {
                name: "a".to_string(),
                established: true,
                detail: None,
            },
            ForwardStatus {
                name: "b".to_string(),
                established: true,
                detail: None,
            },
        ];
        let summary = startup_summary(&statuses, false);
        assert_eq!(summary, "\u{2713} 2/2 established");
    }

    #[test]
    fn test_startup_summary_with_failures() {
        let statuses = vec![
            ForwardStatus {
                name: "a".to_string(),
                established: true,
                detail: None,
            },
            ForwardStatus {
                name: "longhorn".to_string(),
                established: false,
                detail: Some("no ready pods".to_string()),
            },
        ];
        let summary = startup_summary(&statuses, false);
        assert!(summary.contains("1/2 established"));
        assert!(summary.contains("1 failed"));
        assert!(summary.contains("longhorn: no ready pods"));
    }

    #[test]
    fn test_startup_summary_no_color_has_no_escapes() {
        let statuses = vec![ForwardStatus {
            name: "a".to_string(),
            established: true,
            detail: None,
        }];
        let summary = startup_summary(&statuses, false);
        assert!(!summary.contains('\x1b'));
    }

    #[test]
    fn test_startup_summary_color_has_escapes() {
        let statuses = vec![ForwardStatus {
            name: "a".to_string(),
            established: true,
            detail: None,
        }];
        let summary = startup_summary(&statuses, true);
        assert!(summary.contains('\x1b'));
    }
}
