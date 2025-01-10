use anyhow::Result;
use clap::Parser;
use kube::Client;
use metrics_exporter_prometheus::PrometheusBuilder;
use std::path::PathBuf;
use tracing::{error, info};

use kube_forward::{config::ForwardConfig, forward::PortForwardManager, util::resolve_service};

#[derive(Parser)]
#[command(author, version, about)]
struct Cli {
    #[arg(short, long, default_value = "config.yaml")]
    config: PathBuf,

    #[arg(short, long)]
    expose_metrics: bool,

    #[arg(short, long, default_value = "9292")]
    metrics_port: u16,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::fmt::init();

    // Parse command line arguments
    let cli = Cli::parse();

    // Initialize metrics
    if cli.expose_metrics {
        let builder = PrometheusBuilder::new();
        builder
            .with_http_listener(([0, 0, 0, 0], cli.metrics_port))
            .add_global_label("service", "kube-forward")
            .install()?;
    }

    // Load configuration
    let config_content = tokio::fs::read_to_string(&cli.config).await?;
    let config: Vec<ForwardConfig> = serde_yaml::from_str(&config_content)?;

    // Initialize Kubernetes client
    let client = Client::try_default().await?;

    // Create port-forward manager
    let manager = PortForwardManager::new(client.clone());

    // Set up signal handling
    let (shutdown_tx, mut shutdown_rx) = tokio::sync::broadcast::channel(1);
    let shutdown_tx_clone = shutdown_tx.clone();

    ctrlc::set_handler(move || {
        let _ = shutdown_tx_clone.send(());
    })?;

    // Start port-forwards
    for forward_config in config {
        info!("Setting up port-forward for {}", forward_config.name);

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
                }
            }
            Err(e) => {
                error!(
                    "Failed to resolve service for {}: {}",
                    forward_config.name, e
                );
            }
        }
    }

    // Wait for shutdown signal
    shutdown_rx.recv().await?;
    info!("Shutting down...");

    // Stop all port-forwards
    manager.stop_all().await;

    Ok(())
}
