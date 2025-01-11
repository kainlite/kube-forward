# kube-forward

[![ci](https://github.com/kainlite/kube-forward/actions/workflows/release.yaml/badge.svg)](https://github.com/kainlite/kube-forward/actions/workflows/release.yaml)
[![codecov](https://codecov.io/gh/kainlite/kube-forward/branch/master/graph/badge.svg)](https://codecov.io/gh/kainlite/kube-forward)

### kubectl persistent port-forward
This is a tiny tool to simplify `kubectl port-forward` written in Rust, basically the idea is to have a more persistent
configuration and also be able to spin up multiple port-forwards with just one common, that don't get interrupted or
reconnect automatically, more features will be coming soon.

Sample configuration for different services:
```yaml
- name: "jaeger-ui"
  # <deployment name>.<namespace>
  target: "simple-query.observability"
  ports:
    local: 8686    # Same port as remote since it's the UI standard port
    remote: 16686   # Jaeger UI default port
  options:
    retry_interval: 5s
    max_retries: 30
    health_check_interval: 10s
  # local_dns:
  #   enabled: true
  #   hostname: "jaeger.localhost"  # Optional (it doesn't work yet), if you want to access it through a nice local domain
  pod_selector:
    label: "app.kubernetes.io/instance=simple"

- name: "argocd-ui"
  target: "argocd-server.argocd"
  ports:
    local: 8080    # custom port
    remote: 8080   # https
  options:
    retry_interval: 5s
    max_retries: 30
    health_check_interval: 10s
  # local_dns:
  #   enabled: true
  #   hostname: "argocd.localhost"  # Optional (it doesn't work yet), if you want to access it through a nice local domain
  pod_selector:
    label: "app.kubernetes.io/name=argocd-server"

- name: "grafana-ui"
  target: "grafana.monitoring"
  ports:
    local: 3001  
    remote: 3000   
  options:
    retry_interval: 5s
    max_retries: 30
    health_check_interval: 10s
  # local_dns:
  #   enabled: true
  #   hostname: "grafana.localhost"  # Optional (it doesn't work yet), if you want to access it through a nice local domain
  pod_selector:
    label: "app.kubernetes.io/name=grafana"

# Given the case that the service name matches the pod name then you don't need to use the pod_selector 
# to specify labels
- name: "postgres"
  target: "postgres.tr"
  ports:
    local: 5434 
    remote: 5432   
  options:
    retry_interval: 5s
    max_retries: 30
    health_check_interval: 10s
  # local_dns:
  #   enabled: true
  #   hostname: "grafana.localhost"  # Optional (it doesn't work yet), if you want to access it through a nice local domain
  # pod_selector:
  #   label: "app=postgres"
```

### Install instructions
Grab or build the binary from the releases and move it to and executable location in your $PATH.

```bash
git clone 
cargo build --release
```

### Run
To run it and be able to use your port-forwards, run:
```bash
❯ RUST_LOG=info kube-forward -c config.yaml
2025-01-10T00:42:57.001808Z  INFO kube_forward: Setting up port-forward for jaeger-ui
2025-01-10T00:42:57.632541Z  INFO kube_forward: Setting up port-forward for argocd-ui
2025-01-10T00:42:57.801798Z  INFO kube_forward: Setting up port-forward for grafana-ui
Creating TCP listener for the local port: 8686
2025-01-10T00:42:58.119131Z  INFO kube_forward::forward: Port-forward established for jaeger-ui
2025-01-10T00:42:58.120442Z  INFO kube_forward::forward: New connection for jaeger-ui peer_addr=127.0.0.1:37674
2025-01-10T00:42:58.327776Z  INFO kube_forward: Setting up port-forward for postgres
Creating TCP listener for the local port: 5434
2025-01-10T00:42:58.656643Z  INFO kube_forward::forward: Port-forward established for postgres
Creating TCP listener for the local port: 3001
2025-01-10T00:42:58.657695Z  INFO kube_forward::forward: Port-forward established for grafana-ui
2025-01-10T00:42:58.657941Z  INFO kube_forward::forward: New connection for postgres peer_addr=127.0.0.1:33474
2025-01-10T00:42:58.659193Z  INFO kube_forward::forward: New connection for grafana-ui peer_addr=127.0.0.1:51300
Creating TCP listener for the local port: 8080
2025-01-10T00:42:58.705469Z  INFO kube_forward::forward: Port-forward established for argocd-ui
2025-01-10T00:42:58.706768Z  INFO kube_forward::forward: New connection for argocd-ui peer_addr=127.0.0.1:57198
2025-01-10T00:42:59.085888Z  INFO kube_forward::forward: New connection for postgres peer_addr=127.0.0.1:33488
```

### Logging

You can change the log level by changing the environment variable `RUST_LOG`, if something doesn't work try `debug` to
see more information.
