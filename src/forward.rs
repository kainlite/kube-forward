use socket2::{SockRef, TcpKeepalive};
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;

use kube::{api::Api, Client};

use crate::{
    config::ForwardConfig,
    config::PodSelector,
    error::{PortForwardError, Result},
    metrics::ForwardMetrics,
    util::ServiceInfo,
};
use anyhow;
use chrono::DateTime;
use chrono::Utc;
use k8s_openapi::api::core::v1::Pod;
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};

use tracing::{debug, error, info, warn};

use futures::TryStreamExt;

use std::net::SocketAddr;
use tokio::{
    io::{AsyncRead, AsyncWrite},
    net::TcpListener,
};
use tokio_stream::wrappers::TcpListenerStream;

#[derive(Debug)]
pub struct HealthCheck {
    pub last_check: Arc<RwLock<Option<DateTime<Utc>>>>,
    pub failures: Arc<RwLock<u32>>,
}

impl Default for HealthCheck {
    fn default() -> Self {
        Self::new()
    }
}

impl HealthCheck {
    pub fn new() -> Self {
        Self {
            last_check: Arc::new(RwLock::new(None)),
            failures: Arc::new(RwLock::new(0)),
        }
    }

    pub async fn check_connection(&self, local_port: u16, protocol: &str) -> bool {
        match protocol.to_uppercase().as_str() {
            "UDP" => {
                // For UDP, try to send a test packet and verify we can't bind to the port
                use tokio::net::UdpSocket;

                // Try to create a temporary socket for sending test packet
                match UdpSocket::bind("127.0.0.1:0").await {
                    Ok(test_socket) => {
                        // Try to connect to the forwarded port
                        if test_socket
                            .connect(format!("127.0.0.1:{}", local_port))
                            .await
                            .is_ok()
                        {
                            // Send a test packet (DNS query format)
                            let test_packet = vec![
                                0x00, 0x01, // Transaction ID
                                0x01, 0x00, // Flags
                                0x00, 0x01, // Questions
                                0x00, 0x00, // Answer RRs
                                0x00, 0x00, // Authority RRs
                                0x00, 0x00, // Additional RRs
                            ];

                            match test_socket.send(&test_packet).await {
                                Ok(_) => {
                                    // Now verify we can't bind to the forwarded port
                                    match UdpSocket::bind(format!("127.0.0.1:{}", local_port)).await
                                    {
                                        Ok(_) => {
                                            // If we can bind, the forward is not active
                                            let mut failures = self.failures.write().await;
                                            *failures += 1;
                                            false
                                        }
                                        Err(_) => {
                                            // If we can't bind, the forward is active
                                            *self.failures.write().await = 0;
                                            *self.last_check.write().await = Some(Utc::now());
                                            true
                                        }
                                    }
                                }
                                Err(_) => {
                                    let mut failures = self.failures.write().await;
                                    *failures += 1;
                                    false
                                }
                            }
                        } else {
                            let mut failures = self.failures.write().await;
                            *failures += 1;
                            false
                        }
                    }
                    Err(_) => {
                        let mut failures = self.failures.write().await;
                        *failures += 1;
                        false
                    }
                }
            }
            _ => {
                // Default TCP check
                use tokio::net::TcpStream;
                match TcpStream::connect(format!("127.0.0.1:{}", local_port)).await {
                    Ok(_) => {
                        *self.failures.write().await = 0;
                        *self.last_check.write().await = Some(Utc::now());
                        true
                    }
                    Err(_) => {
                        let mut failures = self.failures.write().await;
                        *failures += 1;
                        false
                    }
                }
            }
        }
    }
}

// Represents the state of a port-forward
#[derive(Debug, Clone, PartialEq)]
pub enum ForwardState {
    Starting,
    Connected,
    Disconnected,
    Failed(String),
    Stopping,
}

#[derive(Debug, Clone)]
pub struct PortForward {
    pub config: ForwardConfig,
    pub service_info: ServiceInfo,
    pub state: Arc<RwLock<ForwardState>>,
    pub shutdown: broadcast::Sender<()>,
    pub metrics: ForwardMetrics,
}

impl PortForward {
    pub fn new(config: ForwardConfig, service_info: ServiceInfo) -> Self {
        let (shutdown_tx, _) = broadcast::channel(1);
        Self {
            metrics: ForwardMetrics::new(config.name.clone()),
            config,
            service_info,
            state: Arc::new(RwLock::new(ForwardState::Starting)),
            shutdown: shutdown_tx,
        }
    }

    pub async fn start(&self, client: Client) -> Result<()> {
        let mut retry_count = 0;
        let mut shutdown_rx = self.shutdown.subscribe();

        loop {
            if retry_count >= self.config.options.max_retries
                && !self.config.options.persistent_connection
            {
                let err_msg = "Max retry attempts reached".to_string();
                *self.state.write().await = ForwardState::Failed(err_msg.clone());
                return Err(PortForwardError::ConnectionError(err_msg));
            }

            self.metrics.record_connection_attempt();

            match self.establish_forward(&client).await {
                Ok(()) => {
                    *self.state.write().await = ForwardState::Connected;
                    self.metrics.record_connection_success();
                    self.metrics.set_connection_status(true);
                    info!("Port-forward established for {}", self.config.name);

                    // Monitor the connection
                    tokio::select! {
                        _ = shutdown_rx.recv() => {
                            info!("Received shutdown signal for {}", self.config.name);
                            break;
                        }
                        _ = self.monitor_connection(&client) => {
                            warn!("Connection lost for {}, attempting to reconnect", self.config.name);
                            *self.state.write().await = ForwardState::Disconnected;
                        }
                    }
                }
                Err(e) => {
                    warn!(
                        "Failed to establish port-forward for {}: {}",
                        self.config.name, e
                    );
                    self.metrics.record_connection_failure();
                    self.metrics.set_connection_status(false);
                    retry_count += 1;
                    tokio::time::sleep(self.config.options.retry_interval).await;
                    continue;
                }
            }
        }

        Ok(())
    }

    pub async fn monitor_connection(&self, client: &Client) -> Result<()> {
        let health_check = HealthCheck::new();
        let mut interval = tokio::time::interval(self.config.options.health_check_interval);
        let mut consecutive_failures = 0;
        let protocol = self
            .config
            .ports
            .protocol
            .clone()
            .unwrap_or_else(|| "TCP".to_string());
        let max_failures = if protocol.to_uppercase() == "UDP" {
            5
        } else {
            3
        }; // More lenient with UDP

        // Add initial delay to allow the connection to fully establish
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;

        // Verify we're in the correct state to start monitoring
        let state = self.state.read().await;
        if !matches!(*state, ForwardState::Connected | ForwardState::Starting) {
            return Err(PortForwardError::ConnectionError(
                "Cannot monitor connection: not in Connected or Starting state".to_string(),
            ));
        }
        drop(state);

        // Initial health check with retry
        let mut initial_attempts = 0;
        let max_initial_attempts = 3;
        while initial_attempts < max_initial_attempts {
            if health_check
                .check_connection(
                    self.config.ports.local,
                    &self
                        .config
                        .ports
                        .protocol
                        .clone()
                        .expect("Protocol configuration"),
                )
                .await
            {
                debug!("Initial health check passed for {}", self.config.name);
                break;
            }
            initial_attempts += 1;
            if initial_attempts < max_initial_attempts {
                debug!(
                    "Initial health check attempt {}/{} failed for {}, retrying...",
                    initial_attempts, max_initial_attempts, self.config.name
                );
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            }
        }

        if initial_attempts >= max_initial_attempts {
            return Err(PortForwardError::ConnectionError(
                "Failed initial health checks".to_string(),
            ));
        }

        loop {
            interval.tick().await;

            // Check TCP connection
            let protocol = self
                .config
                .ports
                .protocol
                .clone()
                .unwrap_or_else(|| "TCP".to_string());
            if !health_check
                .check_connection(self.config.ports.local, &protocol)
                .await
            {
                consecutive_failures += 1;
                if consecutive_failures > 1 {
                    warn!(
                        "Health check failed for {} ({}/{})",
                        self.config.name, consecutive_failures, max_failures
                    );
                } else {
                    debug!(
                        "Health check failed for {} ({}/{})",
                        self.config.name, consecutive_failures, max_failures
                    );
                }
                continue;
            }
            consecutive_failures = 0; // Reset counter on successful check

            // Check pod status
            if let Ok(pod) = self.get_pod(client).await {
                if let Some(status) = &pod.status {
                    if let Some(phase) = &status.phase {
                        if phase != "Running" {
                            return Err(PortForwardError::ConnectionError(
                                "Pod is no longer running".to_string(),
                            ));
                        }
                    }
                }
            } else {
                return Err(PortForwardError::ConnectionError(
                    "Pod not found".to_string(),
                ));
            }
        }
    }

    pub async fn establish_forward(&self, client: &Client) -> Result<()> {
        // Check current state first and update atomically
        let mut current_state = self.state.write().await;
        match *current_state {
            ForwardState::Connected => {
                debug!("Port forward {} is already connected", self.config.name);
                return Ok(());
            }
            ForwardState::Starting => {
                debug!("Port forward {} is already starting", self.config.name);
                return Ok(());
            }
            _ => {
                *current_state = ForwardState::Starting;
            }
        }
        drop(current_state); // Release the lock

        self.metrics.record_connection_attempt();
        // Get pod for the service
        let pod = self.get_pod(client).await?;
        // Clone the name to avoid lifetime issues
        let pod_name = pod.metadata.name.clone().ok_or_else(|| {
            self.metrics.record_connection_failure();
            PortForwardError::ConnectionError("Pod name not found".to_string())
        })?;

        // Create Api instance for the namespace
        let _pods: Api<Pod> = Api::namespaced(client.clone(), &self.service_info.namespace);

        // Try to bind to the port with retries
        let mut retry_count = 0;
        let max_bind_retries = 3;
        let bind_retry_delay = std::time::Duration::from_secs(1);

        // Create TCP listener for the local port
        debug!(
            "Creating TCP listener for the local port: {}",
            self.config.ports.local
        );

        // Create listener based on protocol
        let protocol = self
            .config
            .ports
            .protocol
            .clone()
            .unwrap_or_else(|| "TCP".to_string());
        debug!(
            "Creating {} listener for the local port: {}",
            protocol, self.config.ports.local
        );

        let addr = SocketAddr::from(([127, 0, 0, 1], self.config.ports.local));

        match protocol.to_uppercase().as_str() {
            "TCP" => {
                let listener = loop {
                    match TcpListener::bind(addr).await {
                        Ok(listener) => break listener,
                        Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => {
                            if retry_count >= max_bind_retries {
                                self.metrics.record_connection_failure();
                                return Err(PortForwardError::ConnectionError(format!(
                            "Port {} is already in use. Please choose a different local port",
                            self.config.ports.local
                        )));
                            }
                            // Try to forcefully release the port
                            if let Err(release_err) = self.try_release_port().await {
                                warn!(
                                    "Failed to release port {}: {}",
                                    self.config.ports.local, release_err
                                );
                            }
                            retry_count += 1;
                            debug!(
                                "Port {} in use, retrying in {:?}...",
                                self.config.ports.local, bind_retry_delay
                            );
                            tokio::time::sleep(bind_retry_delay).await;
                            continue;
                        }
                        Err(e) => {
                            self.metrics.record_connection_failure();
                            return Err(PortForwardError::ConnectionError(format!(
                                "Failed to bind to port: {}",
                                e
                            )));
                        }
                    }
                };

                // Set TCP keepalive
                let ka = TcpKeepalive::new().with_time(std::time::Duration::from_secs(30));
                let sf = SockRef::from(&listener);
                let _ = sf.set_tcp_keepalive(&ka);

                self.handle_tcp_forward(client, pod_name, listener).await?;
            }
            "UDP" => {
                let socket = tokio::net::UdpSocket::bind(addr).await.map_err(|e| {
                    PortForwardError::ConnectionError(format!("Failed to bind UDP socket: {}", e))
                })?;

                self.handle_udp_forward(client, pod_name, socket).await?;
            }
            _ => {
                return Err(PortForwardError::ConnectionError(format!(
                    "Unsupported protocol: {}",
                    protocol
                )));
            }
        };

        // Set state to connected
        *self.state.write().await = ForwardState::Connected;
        self.metrics.record_connection_success();
        self.metrics.set_connection_status(true);

        Ok(())
    }

    pub async fn forward_connection(
        pods: &Api<Pod>,
        pod_name: String,
        port: u16,
        mut client_conn: impl AsyncRead + AsyncWrite + Unpin,
    ) -> anyhow::Result<()> {
        debug!("Starting port forward for port {}", port);

        // Create port forward
        let mut pf = pods
            .portforward(&pod_name, &[port])
            .await
            .map_err(|e| anyhow::anyhow!("Failed to create portforward: {}", e))?;

        // Get the stream for our port
        let mut upstream_conn = pf
            .take_stream(port) // Use port instead of 0
            .ok_or_else(|| {
                anyhow::anyhow!("Failed to get port forward stream for port {}", port)
            })?;

        debug!("Port forward stream established for port {}", port);

        // Copy data bidirectionally with timeout
        match tokio::time::timeout(
            std::time::Duration::from_secs(30), // 30 second timeout
            tokio::io::copy_bidirectional(&mut client_conn, &mut upstream_conn),
        )
        .await
        {
            Ok(Ok(_)) => {
                debug!("Connection closed normally for port {}", port);
            }
            Ok(Err(e)) => {
                warn!("Error during data transfer for port {}: {}", port, e);
                return Err(anyhow::anyhow!("Data transfer error: {}", e));
            }
            Err(_) => {
                warn!("Connection timeout for port {}", port);
                return Err(anyhow::anyhow!("Connection timeout"));
            }
        }

        // Clean up
        drop(upstream_conn);

        // Wait for the port forwarder to finish
        if let Err(e) = pf.join().await {
            warn!("Port forwarder join error: {}", e);
        }

        Ok(())
    }

    async fn handle_udp_forward(
        &self,
        client: &Client,
        pod_name: String,
        socket: tokio::net::UdpSocket,
    ) -> Result<()> {
        let state = self.state.clone();
        let name = self.config.name.clone();
        let remote_port = self.config.ports.remote;
        let mut shutdown = self.shutdown.subscribe();
        let metrics = self.metrics.clone();
        let pods: Api<Pod> = Api::namespaced(client.clone(), &self.service_info.namespace);

        tokio::spawn(async move {
            let mut buf = vec![0u8; 65535]; // Max UDP packet size
            let socket = Arc::new(socket);

            loop {
                tokio::select! {
                    result = socket.recv_from(&mut buf) => {
                        match result {
                            Ok((len, peer)) => {
                                let pods = pods.clone();
                                let pod_name = pod_name.clone();
                                let metrics = metrics.clone();
                                let socket = socket.clone();
                                let data = buf[..len].to_vec();

                                tokio::spawn(async move {
                                    if let Err(e) = Self::handle_udp_packet(&pods, pod_name, remote_port, socket, data, peer).await {
                                        error!("Failed to forward UDP packet: {}", e);
                                        metrics.record_connection_failure();
                                    } else {
                                        metrics.record_connection_success();
                                    }
                                });
                            }
                            Err(e) => {
                                error!("UDP receive error: {}", e);
                                metrics.record_connection_failure();
                                break;
                            }
                        }
                    }
                    _ = shutdown.recv() => {
                        info!("Received shutdown signal for UDP forward {}", name);
                        *state.write().await = ForwardState::Disconnected;
                        metrics.set_connection_status(false);
                        break;
                    }
                }
            }
        });

        Ok(())
    }

    async fn handle_udp_packet(
        pods: &Api<Pod>,
        pod_name: String,
        port: u16,
        socket: Arc<tokio::net::UdpSocket>,
        data: Vec<u8>,
        peer: SocketAddr,
    ) -> anyhow::Result<()> {
        // Create port forward
        let mut pf = pods
            .portforward(&pod_name, &[port])
            .await
            .map_err(|e| anyhow::anyhow!("Failed to create UDP portforward: {}", e))?;

        // Get the stream for our port
        let mut upstream_conn = pf.take_stream(port).ok_or_else(|| {
            anyhow::anyhow!("Failed to get UDP port forward stream for port {}", port)
        })?;

        // Send the data length as a u16 in network byte order (big-endian)
        let len_bytes = (data.len() as u16).to_be_bytes();
        upstream_conn.write_all(&len_bytes).await?;

        // Send the actual data
        upstream_conn.write_all(&data).await?;
        upstream_conn.flush().await?;

        // Read response length
        let mut len_buf = [0u8; 2];
        match upstream_conn.read_exact(&mut len_buf).await {
            Ok(_) => {
                let response_length = u16::from_be_bytes(len_buf) as usize;

                // Read response data
                let mut response = vec![0u8; response_length];
                match upstream_conn.read_exact(&mut response).await {
                    Ok(_) => {
                        // Send response back to client
                        socket.send_to(&response, peer).await?;
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                        // If we get EOF here, it might be a response without data
                        debug!("No response data received from upstream");
                    }
                    Err(e) => return Err(e.into()),
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                // If we get EOF here, the connection was closed normally
                debug!("Connection closed by upstream after sending data");
            }
            Err(e) => return Err(e.into()),
        }

        Ok(())
    }

    async fn handle_tcp_forward(
        &self,
        client: &Client,
        pod_name: String,
        listener: TcpListener,
    ) -> Result<()> {
        let state = self.state.clone();
        let name = self.config.name.clone();
        let remote_port = self.config.ports.remote;
        let mut shutdown = self.shutdown.subscribe();
        let metrics = self.metrics.clone();
        let pods: Api<Pod> = Api::namespaced(client.clone(), &self.service_info.namespace);

        tokio::spawn(async move {
            let mut listener_stream = TcpListenerStream::new(listener);

            loop {
                tokio::select! {
                    Ok(Some(client_conn)) = listener_stream.try_next() => {
                        if let Ok(peer_addr) = client_conn.peer_addr() {
                            info!(%peer_addr, "New TCP connection for {}", name);
                            metrics.record_connection_attempt();
                        }
                        let pods = pods.clone();
                        let pod_name = pod_name.clone();
                        let metrics = metrics.clone();

                        tokio::spawn(async move {
                            if let Err(e) = Self::forward_connection(&pods, pod_name, remote_port, client_conn).await {
                                error!("Failed to forward TCP connection: {}", e);
                                metrics.record_connection_failure();
                            } else {
                                metrics.record_connection_success();
                            }
                        });
                    }
                    _ = shutdown.recv() => {
                        info!("Received shutdown signal for TCP forward {}", name);
                        *state.write().await = ForwardState::Disconnected;
                        metrics.set_connection_status(false);
                        break;
                    }
                    else => {
                        error!("Port forward {} listener closed", name);
                        *state.write().await = ForwardState::Failed("Listener closed unexpectedly".to_string());
                        metrics.set_connection_status(false);
                        metrics.record_connection_failure();
                        break;
                    }
                }
            }
        });

        Ok(())
    }

    pub async fn get_pod(&self, client: &Client) -> Result<Pod> {
        let pods: Api<Pod> = Api::namespaced(client.clone(), &self.service_info.namespace);

        // Get all pods in the namespace
        let pod_list = pods
            .list(&kube::api::ListParams::default())
            .await
            .map_err(PortForwardError::KubeError)?;

        for pod in pod_list.items {
            if self
                .clone()
                .matches_pod_selector(&pod, &self.config.pod_selector)
            {
                if let Some(status) = &pod.status {
                    if let Some(phase) = &status.phase {
                        if phase == "Running" {
                            return Ok(pod);
                        }
                    }
                }
            }
        }

        Err(PortForwardError::ConnectionError(format!(
            "No ready pods found matching selector for service {}",
            self.service_info.name
        )))
    }

    pub fn matches_pod_selector(self, pod: &Pod, selector: &PodSelector) -> bool {
        // If no selector is specified, fall back to checking if service name is in any label
        if selector.label.is_none() && selector.annotation.is_none() {
            return pod
                .metadata
                .labels
                .as_ref()
                .is_some_and(|labels| labels.values().any(|v| v == &self.service_info.name));
        }

        // Check label if specified
        if let Some(label_selector) = &selector.label {
            let (key, value) = self.clone().parse_selector(label_selector);
            if pod
                .metadata
                .labels
                .as_ref()
                .is_none_or(|labels| labels.get(key).is_none_or(|v| v != value))
            {
                return false;
            }
        }

        // Check annotation if specified
        if let Some(annotation_selector) = &selector.annotation {
            let (key, value) = self.clone().parse_selector(annotation_selector);
            if pod
                .metadata
                .annotations
                .as_ref()
                .is_none_or(|annotations| annotations.get(key).is_none_or(|v| v != value))
            {
                return false;
            }
        }

        true
    }

    pub fn parse_selector(self, selector: &str) -> (&str, &str) {
        let parts: Vec<&str> = selector.split('=').collect();
        match parts.as_slice() {
            [key, value] => (*key, *value),
            _ => ("", ""), // Return empty strings if format is invalid
        }
    }

    pub async fn try_release_port(&self) -> std::io::Result<()> {
        let addr = SocketAddr::from(([127, 0, 0, 1], self.config.ports.local));

        // First check if this is our own active connection
        let state = self.state.read().await;
        match *state {
            ForwardState::Connected | ForwardState::Starting => {
                debug!(
                    "Port {} is in use by our own active connection (state: {:?})",
                    self.config.ports.local, state
                );
                return Ok(());
            }
            _ => drop(state),
        }

        // Try to create a temporary connection to verify the port status
        let socket = tokio::net::TcpSocket::new_v4()?;

        // Try to connect first to check if our process is actually using it
        match tokio::net::TcpStream::connect(addr).await {
            Ok(_) => {
                // Double check our state again as it might have changed
                let state = self.state.read().await;
                if matches!(*state, ForwardState::Connected | ForwardState::Starting) {
                    debug!(
                        "Port {} is in use by our active connection (verified)",
                        self.config.ports.local
                    );
                    Ok(())
                } else {
                    debug!(
                        "Port {} is in use by another process",
                        self.config.ports.local
                    );
                    Err(std::io::Error::new(
                        std::io::ErrorKind::AddrInUse,
                        "Port is actively in use by another process",
                    ))
                }
            }
            Err(_) => {
                // If connect failed, try to bind
                match socket.bind(addr) {
                    Ok(_) => {
                        debug!("Port {} is free", self.config.ports.local);
                        Ok(())
                    }
                    Err(e) => {
                        debug!("Port {} bind error: {}", self.config.ports.local, e);
                        Err(e)
                    }
                }
            }
        }
    }

    pub async fn stop(&self) {
        // Set state to stopping
        *self.state.write().await = ForwardState::Stopping;

        // Send shutdown signal
        let _ = self.shutdown.send(());

        // Wait a moment for cleanup
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        // Try to release the port
        if let Err(e) = self.try_release_port().await {
            warn!(
                "Failed to release port {} during shutdown: {}",
                self.config.ports.local, e
            );
        }

        // Set final state
        *self.state.write().await = ForwardState::Disconnected;

        // Update metrics
        self.metrics.set_connection_status(false);
    }
}

// Manager to handle multiple port-forwards
pub struct PortForwardManager {
    pub forwards: Arc<RwLock<Vec<Arc<PortForward>>>>,
    pub client: Client,
}

impl PortForwardManager {
    pub fn new(client: Client) -> Self {
        Self {
            forwards: Arc::new(RwLock::new(Vec::new())),
            client,
        }
    }

    pub async fn add_forward(
        &self,
        config: ForwardConfig,
        service_info: ServiceInfo,
    ) -> Result<()> {
        let forward = Arc::new(PortForward::new(config, service_info));
        self.forwards.write().await.push(forward.clone());

        // Start the port-forward in a separate task
        let client = self.client.clone();
        tokio::spawn(async move {
            if let Err(e) = forward.start(client).await {
                error!("Port-forward failed: {}", e);
            }
        });

        Ok(())
    }

    pub async fn stop_all(&self) {
        for forward in self.forwards.read().await.iter() {
            forward.stop().await;
        }
    }
}
