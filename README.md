# kube-forward

[![ci](https://github.com/kainlite/kube-forward/actions/workflows/release.yaml/badge.svg)](https://github.com/kainlite/kube-forward/actions/workflows/release.yaml)
[![codecov](https://codecov.io/gh/kainlite/kube-forward/branch/master/graph/badge.svg)](https://codecov.io/gh/kainlite/kube-forward)

### kubectl persistent port-forward
This is a tiny tool to simplify `kubectl port-forward` written in Rust, basically the idea is to have a more persistent
configuration and also be able to spin up multiple port-forwards with just one common, that don't get interrupted or
reconnect automatically, more features will be coming soon.

### Configurations
Given that not all configurations are the same and there are a few possible options, there are some examples so you can
get the most out of it, in some cases you might want a port-forward to die, in others you might want kube-forward to
keep trying forever, maybe your connection can vary in speed or reliability (fine tune or test changing the retry
interval), in a future release `retry_interval` and `health_check_interval` will become optional and their defaults will be
5s and 10s.

#### Sample entry
```yaml
- name: "identifier"
  target: "<svc-name>.<namespace>"
  ports:
    protocol: <optional | tcp or udp, tcp is the default if not configured>
    local: <any free local port> 
    remote: <remote port on the pod> 
  options: <optional | define it if you need to tweak the default behavior>
    retry_interval: <optional> (define it in seconds for example 5s)
    max_retries: <optional> (define it as a number for example 20)
    health_check_interval: <optional> (define it in seconds for example 10s)
    persistent_connection: <optional | defaults to true and ignores max_retries>
  pod_selector:
    label: <optional> if the <svc-name>" doesn't match the pod name you need to use a label like: app.kubernetes.io/instance=simple"
```

#### Minimal configuration example
Assuming we have an instance called "simple" we can easily match the pod using that label.
```yaml
- name: "jaeger-ui"
  target: "simple-query.observability"
  ports:
    local: 16686
    remote: 16686
  options:
    retry_interval: 1s
    health_check_interval: 30s
  pod_selector:
    label: "app.kubernetes.io/instance=simple"

- name: "postgres"
  target: "postgres.tr"
  ports:
    local: 5434 
    remote: 5432   

- name: "coredns"
  target: "kube-dns.kube-system"
  ports:
    protocol: udp
    local: 5454 
    remote: 53
  pod_selector:
    label: "k8s-app=kube-dns"
```

To test it you can do:
```bash
❯ dog google.com @127.0.0.1:5454
A google.com. 30s   172.253.122.139
A google.com. 30s   172.253.122.102
A google.com. 30s   172.253.122.101
A google.com. 30s   172.253.122.113
A google.com. 30s   172.253.122.100
A google.com. 30s   172.253.122.138
```

#### Full configuration example
Full configuration example for different services:
```yaml
- name: "jaeger-ui"
  target: "simple-query.observability"
  ports:
    local: 8686
    remote: 16686
  options:
    retry_interval: 5s
    health_check_interval: 10s
    max_retries: 3
    persistent_connection: false
  pod_selector:
    label: "app.kubernetes.io/instance=simple"

- name: "argocd-ui"
  target: "argocd-server.argocd"
  ports:
    local: 8080
    remote: 8080
  options:
    retry_interval: 5s
    health_check_interval: 10s
  pod_selector:
    label: "app.kubernetes.io/name=argocd-server"

- name: "grafana-ui"
  target: "grafana.monitoring"
  ports:
    local: 3001  
    remote: 3000   
  options:
    retry_interval: 5s
    health_check_interval: 10s
  pod_selector:
    label: "app.kubernetes.io/name=grafana"

- name: "postgres"
  target: "postgres.tr"
  ports:
    local: 5434 
    remote: 5432   

- name: "coredns"
  target: "kube-dns.kube-system"
  ports:
    protocol: udp
    local: 5454 
    remote: 53
  pod_selector:
    label: "k8s-app=kube-dns"
```

### Install instructions
```bash
curl -sSL https://raw.githubusercontent.com/kainlite/kube-forward/master/scripts/install.sh | sh
```
#### Manual Installation
You can also download the binary directly from the releases page.

### Usage
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
