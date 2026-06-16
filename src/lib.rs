//! kube-forward: a persistent, self-healing port-forward manager for Kubernetes.
//!
//! Forwards are declared in a YAML config (see [`config`]) and supervised by
//! [`forward::PortForwardManager`]: each one reconnects on its own with backoff
//! ([`backoff`]) and rebinds to a fresh pod after a restart. The binary wires
//! these together with config validation ([`validate`]), terminal output
//! ([`ui`]), an interactive config builder ([`add`]), and Prometheus metrics
//! ([`metrics`]).
//!
//! ## Modules
//! - [`add`] — interactive `add` subcommand: discover a service and append a forward.
//! - [`backoff`] — exponential backoff with jitter for reconnects.
//! - [`config`] — serde model for the YAML configuration and its defaults.
//! - [`error`] — crate error type.
//! - [`forward`] — core port-forward, health checks, and the supervising manager.
//! - [`metrics`] — Prometheus counters and gauges per forward.
//! - [`ui`] — startup banner, tables, summaries, and the validation report.
//! - [`util`] — service/target resolution helpers.
//! - [`validate`] — cluster-free configuration validation.

pub mod add;
pub mod backoff;
pub mod config;
pub mod error;
pub mod forward;
pub mod metrics;
pub mod ui;
pub mod util;
pub mod validate;
