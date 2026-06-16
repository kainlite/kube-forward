# kube-forward

[![ci](https://github.com/kainlite/kube-forward/actions/workflows/release.yaml/badge.svg)](https://github.com/kainlite/kube-forward/actions/workflows/release.yaml)
[![codecov](https://codecov.io/gh/kainlite/kube-forward/branch/master/graph/badge.svg)](https://codecov.io/gh/kainlite/kube-forward)

### kubectl persistent port-forward
A small tool, written in Rust, to simplify `kubectl port-forward`. It runs many port-forwards from a single
configuration file and supervises each one: connections that drop are reconnected automatically, so your forwards stay
up across pod restarts and redeploys. Both TCP and UDP are supported.

### Configurations
Given that not all configurations are the same and there are a few possible options, there are some examples so you can
get the most out of it, in some cases you might want a port-forward to die, in others you might want kube-forward to
keep trying forever, maybe your connection can vary in speed or reliability (fine tune or test changing the retry
interval). Every option is optional: `retry_interval` defaults to 5s, `health_check_interval` to 10s, `max_retries` to 3,
`persistent_connection` to true, and `connection_timeout` to 1h.

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
    connection_timeout: <optional | idle timeout per connection, defaults to 1h; an active connection is never closed>
  pod_selector:
    label: <optional> if the <svc-name> doesn't match the pod name you need to use a label like "app.kubernetes.io/instance=simple"
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

### Validating a configuration
You can check a configuration file without connecting to a cluster:
```bash
❯ kube-forward validate -c config.yaml
✓ configuration is valid
```
It reports duplicate names, two forwards sharing the same local port, invalid protocols or ports, unparseable targets,
and malformed pod selectors, exiting non-zero when any are found. The same checks run automatically at startup.

### Adding a service interactively
Writing an entry by hand means knowing the namespace, service, ports and selector up front. The `add` subcommand
discovers them from the cluster (read-only) and appends a validated entry to your config file:
```bash
❯ kube-forward add -c config.yaml
? Namespace:   monitoring
? Service:     grafana
? Remote port: 3000 (tcp http)
? Local port:  3001
? Name:        grafana
? Pod selector (key=value, blank to skip): app.kubernetes.io/name=grafana
+ appended 'grafana' to config.yaml ✓
```
It suggests a free local port, pre-fills the pod selector from the service's own selector, keeps the name unique, and
re-validates the whole file before writing. Nothing is written if you cancel or the result would be invalid. Only the
config file is modified; your kubeconfig is used read-only for discovery.

### Reliability
Each forward is supervised: if the connection is lost it reconnects on its own with exponential backoff and jitter.
Health checks verify the *specific* pod the forward is bound to is still running and that the upstream port-forward path
actually opens, so a pod restart or redeploy triggers a clean reconnect that rebinds to the new pod instead of serving a
stale one. The previous attempt's local listener is always released before rebinding, so forwards recover instead of
wedging on "port already in use".

### Install instructions
```bash
curl -sSL https://raw.githubusercontent.com/kainlite/kube-forward/master/scripts/install.sh | sh
```
#### Manual Installation
You can also download the binary directly from the releases page.

### Usage
To run it and be able to use your port-forwards, run:
```bash
❯ kube-forward -c config.yaml
kube-forward 0.6.0 — 4 forwards configured

  argocd-ui   127.0.0.1:8080 -> argocd-server.argocd:8080  TCP
  grafana-ui  127.0.0.1:3001 -> grafana.monitoring:3000    TCP
  postgres    127.0.0.1:5434 -> postgres.tr:5432           TCP
  coredns     127.0.0.1:5454 -> kube-dns.kube-system:53    UDP

✓ 4/4 established
watching for changes, press Ctrl-C to stop
```

When a forward cannot start, the summary stays honest and names what failed:
```bash
✓ 3/4 established  ✗ 1 failed
    ✗ longhorn: no ready pods found matching selector
```

#### Commands and options
- `kube-forward [-c config.yaml]` — start all forwards (this is the default with no subcommand).
- `kube-forward validate [-c config.yaml]` — validate the config without contacting a cluster; exits non-zero on errors.
- `kube-forward add [-c config.yaml]` — interactively discover a service and append a forward to the config file.

Options apply to every command:

| Flag | Default | Description |
| --- | --- | --- |
| `-c, --config <path>` | `config.yaml` | Path to the configuration file. |
| `-e, --expose-metrics` | off | Serve Prometheus metrics over HTTP. |
| `-m, --metrics-port <port>` | `9292` | Port for the metrics endpoint. |

### Logging

The startup banner and summary are printed to stdout. Diagnostic detail goes through `tracing`: set `RUST_LOG` to change
the level (it is honored, defaulting to `info`), for example `RUST_LOG=debug kube-forward -c config.yaml` to see
per-connection activity and reconnection attempts. Color is emitted only when stdout is a terminal and `NO_COLOR` is unset.

### Metrics
Pass `--expose-metrics` to serve Prometheus metrics on `0.0.0.0:9292` (override the port with `--metrics-port`). Every
series carries a `service="kube-forward"` label and a `forward="<name>"` label so you can break results down per forward:

| Metric | Type | Meaning |
| --- | --- | --- |
| `port_forward_connection_attempts_total` | counter | Connection attempts made. |
| `port_forward_connection_successes_total` | counter | Successful connections. |
| `port_forward_connection_failures_total` | counter | Failed connections. |
| `port_forward_connected` | gauge | `1` while connected, `0` otherwise. |

```bash
❯ kube-forward -c config.yaml --expose-metrics --metrics-port 9292
❯ curl -s localhost:9292/metrics | grep port_forward
```

### Releasing
Releases are automated with [release-please](https://github.com/googleapis/release-please). Land changes on `master`
using [Conventional Commits](https://www.conventionalcommits.org/) (`feat:`, `fix:`, and `feat!:` or a `BREAKING CHANGE:`
footer for a major bump). release-please maintains a release PR that bumps the version in `Cargo.toml` and updates
`CHANGELOG.md`; merging that PR tags the release, and the workflow builds and uploads the binaries. No manual version
bumps or tags are needed.
