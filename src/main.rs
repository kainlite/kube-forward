use anyhow::Result;
use clap::{Parser, Subcommand};
use kube::Client;
use metrics_exporter_prometheus::PrometheusBuilder;
use std::collections::HashMap;
use std::path::PathBuf;
use tracing::{debug, error, info, warn};
use tracing_subscriber::EnvFilter;

use kube_forward::{
    config::ForwardConfig,
    forward::{ForwardState, PortForwardManager},
    ui,
    util::resolve_service,
    validate::validate_configs,
};

/// How long to wait for forwards to settle before printing the startup summary.
const STARTUP_SETTLE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

#[derive(Parser)]
#[command(author, version, about)]
struct Cli {
    #[arg(short, long, default_value = "config.yaml", global = true)]
    config: PathBuf,

    #[arg(short, long, global = true)]
    expose_metrics: bool,

    #[arg(short, long, default_value = "9292", global = true)]
    metrics_port: u16,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Validate the configuration file without connecting to the cluster.
    Validate,
    /// Interactively discover a service and append a forward to the config file.
    Add,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize rustls for the metrics exporter :head_exploding:
    let _ = rustls::crypto::ring::default_provider().install_default();

    // Initialize logging, honoring RUST_LOG when set (falling back to "info").
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let cli = Cli::parse();

    match cli.command {
        Some(Command::Validate) => {
            let config = load_config(&cli.config).await?;
            let report = validate_configs(&config);
            if ui::print_validation(&report) {
                Ok(())
            } else {
                std::process::exit(1);
            }
        }
        Some(Command::Add) => {
            let client = Client::try_default().await?;
            kube_forward::add::run(client, &cli.config).await
        }
        None => {
            let config = load_config(&cli.config).await?;
            run(cli, config).await
        }
    }
}

async fn load_config(path: &PathBuf) -> Result<Vec<ForwardConfig>> {
    let content = tokio::fs::read_to_string(path)
        .await
        .map_err(|e| anyhow::anyhow!("failed to read config '{}': {}", path.display(), e))?;
    serde_yaml::from_str(&content)
        .map_err(|e| anyhow::anyhow!("failed to parse config '{}': {}", path.display(), e))
}

async fn run(cli: Cli, config: Vec<ForwardConfig>) -> Result<()> {
    // Validate before touching the cluster so mistakes fail fast and clearly.
    let report = validate_configs(&config);
    if !report.is_valid() {
        ui::print_validation(&report);
        anyhow::bail!("invalid configuration; fix the errors above or run 'kube-forward validate'");
    }
    for warning in &report.warnings {
        warn!("{}", warning);
    }

    // Initialize metrics
    if cli.expose_metrics {
        PrometheusBuilder::new()
            .with_http_listener(([0, 0, 0, 0], cli.metrics_port))
            .add_global_label("service", "kube-forward")
            .install()?;
        info!("metrics exposed on 0.0.0.0:{}", cli.metrics_port);
    }

    ui::print_startup(env!("CARGO_PKG_VERSION"), &config);

    // Initialize Kubernetes client
    let client = Client::try_default().await?;
    let manager = PortForwardManager::new(client.clone());

    // Set up signal handling
    let (shutdown_tx, mut shutdown_rx) = tokio::sync::broadcast::channel(1);
    let shutdown_tx_clone = shutdown_tx.clone();
    ctrlc::set_handler(move || {
        let _ = shutdown_tx_clone.send(());
    })?;

    // Start port-forwards, remembering any that fail to resolve so the summary is honest.
    let mut resolve_failures: Vec<ui::ForwardStatus> = Vec::new();
    for forward_config in &config {
        debug!("Setting up port-forward for {}", forward_config.name);

        match resolve_service(client.clone(), &forward_config.target).await {
            Ok(service_info) => {
                if let Err(e) = manager
                    .add_forward(forward_config.clone(), service_info)
                    .await
                {
                    error!(
                        "Failed to set up port-forward {}: {}",
                        forward_config.name, e
                    );
                    resolve_failures.push(ui::ForwardStatus {
                        name: forward_config.name.clone(),
                        established: false,
                        detail: Some(e.to_string()),
                    });
                }
            }
            Err(e) => {
                error!(
                    "Failed to resolve service for {}: {}",
                    forward_config.name, e
                );
                resolve_failures.push(ui::ForwardStatus {
                    name: forward_config.name.clone(),
                    established: false,
                    detail: Some(e.to_string()),
                });
            }
        }
    }

    // Wait for the forwards we did start to settle, then print one honest summary.
    let states = manager
        .wait_for_initial_states(STARTUP_SETTLE_TIMEOUT)
        .await;
    let mut by_name: HashMap<String, ui::ForwardStatus> = HashMap::new();
    for (name, state) in states {
        let (established, detail) = match state {
            ForwardState::Connected => (true, None),
            ForwardState::Failed(msg) => (false, Some(msg)),
            _ => (false, Some("still connecting (timed out)".to_string())),
        };
        by_name.insert(
            name.clone(),
            ui::ForwardStatus {
                name,
                established,
                detail,
            },
        );
    }
    for failure in resolve_failures {
        by_name.insert(failure.name.clone(), failure);
    }
    let statuses: Vec<ui::ForwardStatus> = config
        .iter()
        .filter_map(|c| by_name.remove(&c.name))
        .collect();
    ui::print_summary(&statuses);

    // Wait for shutdown signal
    shutdown_rx.recv().await?;
    info!("Shutting down...");

    // Stop all port-forwards
    manager.stop_all().await;

    Ok(())
}
