use socket2::{SockRef, TcpKeepalive};
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;

use kube::{Client, api::Api};

use crate::{
    backoff::backoff_delay,
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
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{RwLock, broadcast, mpsc};
use tokio_util::sync::CancellationToken;

use tracing::{debug, error, info, warn};

use futures::TryStreamExt;

/// Maximum consecutive health check failures before reconnecting (TCP)
const MAX_TCP_HEALTH_FAILURES: u32 = 3;
/// Maximum consecutive health check failures before reconnecting (UDP)
const MAX_UDP_HEALTH_FAILURES: u32 = 5;
/// Maximum number of retries when binding to a local port
const MAX_BIND_RETRIES: u32 = 3;
/// Delay between bind retries
const BIND_RETRY_DELAY: std::time::Duration = std::time::Duration::from_secs(1);
/// Initial delay before starting health checks to let the connection establish
const HEALTH_CHECK_INITIAL_DELAY: std::time::Duration = std::time::Duration::from_secs(2);
/// Maximum attempts for the initial health check
const MAX_INITIAL_HEALTH_ATTEMPTS: u32 = 3;
/// Delay between initial health check retries
const INITIAL_HEALTH_RETRY_DELAY: std::time::Duration = std::time::Duration::from_millis(500);

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

/// Copy data in both directions between two streams until either side closes,
/// returning the byte counts `(a_to_b, b_to_a)`.
///
/// `idle_timeout` bounds inactivity, not total lifetime: the timer resets on
/// every chunk transferred in either direction. A `TimedOut` error is returned
/// only when nothing moves for the whole `idle_timeout` window.
pub async fn copy_bidirectional_with_idle_timeout<A, B>(
    a: &mut A,
    b: &mut B,
    idle_timeout: std::time::Duration,
) -> std::io::Result<(u64, u64)>
where
    A: AsyncRead + AsyncWrite + Unpin,
    B: AsyncRead + AsyncWrite + Unpin,
{
    let mut a_to_b: u64 = 0;
    let mut b_to_a: u64 = 0;
    let mut buf_a = vec![0u8; 8192];
    let mut buf_b = vec![0u8; 8192];
    let mut a_eof = false;
    let mut b_eof = false;

    while !(a_eof && b_eof) {
        tokio::select! {
            res = a.read(&mut buf_a), if !a_eof => {
                let n = res?;
                if n == 0 {
                    a_eof = true;
                    let _ = b.shutdown().await;
                } else {
                    b.write_all(&buf_a[..n]).await?;
                    b.flush().await?;
                    a_to_b += n as u64;
                }
            }
            res = b.read(&mut buf_b), if !b_eof => {
                let n = res?;
                if n == 0 {
                    b_eof = true;
                    let _ = a.shutdown().await;
                } else {
                    a.write_all(&buf_b[..n]).await?;
                    a.flush().await?;
                    b_to_a += n as u64;
                }
            }
            _ = tokio::time::sleep(idle_timeout) => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    format!("no activity for {:?}", idle_timeout),
                ));
            }
        }
    }

    Ok((a_to_b, b_to_a))
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
    /// Cancellation token for the listener tasks of the *current* connection
    /// attempt. Replaced with a fresh token on every (re)connect so the previous
    /// attempt's listener releases its local port before we rebind.
    pub session: Arc<RwLock<CancellationToken>>,
    /// Name of the pod the listener is currently bound to. The health loop checks
    /// that this specific pod is still running, so a pod replacement triggers a
    /// reconnect that rebinds to the new pod instead of serving a stale one.
    pub active_pod: Arc<RwLock<Option<String>>>,
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
            session: Arc::new(RwLock::new(CancellationToken::new())),
            active_pod: Arc::new(RwLock::new(None)),
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

            // Start a fresh session, cancelling any listener tasks left over from a
            // previous attempt so the local port is free to rebind.
            let session = {
                let mut guard = self.session.write().await;
                guard.cancel();
                let fresh = CancellationToken::new();
                *guard = fresh.clone();
                fresh
            };

            self.metrics.record_connection_attempt();

            match self.establish_forward(&client).await {
                Ok(()) => {
                    retry_count = 0;
                    *self.state.write().await = ForwardState::Connected;
                    self.metrics.record_connection_success();
                    self.metrics.set_connection_status(true);
                    info!("Port-forward established for {}", self.config.name);

                    // Monitor the connection until it is lost or we are asked to stop.
                    tokio::select! {
                        _ = shutdown_rx.recv() => {
                            info!("Received shutdown signal for {}", self.config.name);
                            session.cancel();
                            break;
                        }
                        _ = self.monitor_connection(&client) => {
                            warn!("Connection lost for {}, attempting to reconnect", self.config.name);
                            *self.state.write().await = ForwardState::Disconnected;
                            self.metrics.set_connection_status(false);
                            // The next iteration cancels this session and rebinds.
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
                    let delay = backoff_delay(self.config.options.retry_interval, retry_count);
                    debug!(
                        "Retrying {} in {:?} (attempt {})",
                        self.config.name, delay, retry_count
                    );
                    tokio::time::sleep(delay).await;
                    continue;
                }
            }
        }

        Ok(())
    }

    pub async fn monitor_connection(&self, client: &Client) -> Result<()> {
        let health_check = HealthCheck::new();
        let mut interval = tokio::time::interval(self.config.options.health_check_interval);
        let mut consecutive_failures = 0u32;
        let protocol = self.config.ports.protocol.as_deref().unwrap_or("TCP");
        let max_failures = if protocol.eq_ignore_ascii_case("UDP") {
            MAX_UDP_HEALTH_FAILURES
        } else {
            MAX_TCP_HEALTH_FAILURES
        };

        // The specific pod this connection is bound to. Without it there is nothing
        // meaningful to probe, which also covers the no-op establish case.
        let pod_name = match self.active_pod.read().await.clone() {
            Some(name) => name,
            None => {
                return Err(PortForwardError::ConnectionError(
                    "No active pod recorded for connection monitoring".to_string(),
                ));
            }
        };

        // Add initial delay to allow the connection to fully establish
        tokio::time::sleep(HEALTH_CHECK_INITIAL_DELAY).await;

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
        while initial_attempts < MAX_INITIAL_HEALTH_ATTEMPTS {
            if self
                .check_health(client, &health_check, &pod_name, protocol)
                .await
            {
                debug!("Initial health check passed for {}", self.config.name);
                break;
            }
            initial_attempts += 1;
            if initial_attempts < MAX_INITIAL_HEALTH_ATTEMPTS {
                debug!(
                    "Initial health check attempt {}/{} failed for {}, retrying...",
                    initial_attempts, MAX_INITIAL_HEALTH_ATTEMPTS, self.config.name
                );
                tokio::time::sleep(INITIAL_HEALTH_RETRY_DELAY).await;
            }
        }

        if initial_attempts >= MAX_INITIAL_HEALTH_ATTEMPTS {
            return Err(PortForwardError::ConnectionError(
                "Failed initial health checks".to_string(),
            ));
        }

        loop {
            interval.tick().await;

            if self
                .check_health(client, &health_check, &pod_name, protocol)
                .await
            {
                consecutive_failures = 0; // Reset counter on successful check
                continue;
            }

            consecutive_failures += 1;
            if consecutive_failures >= max_failures {
                return Err(PortForwardError::ConnectionError(format!(
                    "health check failed {} consecutive times for {}",
                    max_failures, self.config.name
                )));
            }
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
        }
    }

    /// One health probe: the local listener is alive, the bound pod is still
    /// running, and the upstream port-forward path actually opens. Any failing
    /// signal makes the probe fail, which counts toward the reconnect threshold.
    async fn check_health(
        &self,
        client: &Client,
        health_check: &HealthCheck,
        pod_name: &str,
        protocol: &str,
    ) -> bool {
        if !health_check
            .check_connection(self.config.ports.local, protocol)
            .await
        {
            return false;
        }
        if !self.is_pod_running(client, pod_name).await {
            return false;
        }
        self.probe_upstream(client, pod_name).await
    }

    /// Whether the specific bound pod still exists and reports the Running phase.
    /// A pod replacement makes this false, so the forward reconnects and rebinds
    /// to the new pod instead of serving a stale name.
    async fn is_pod_running(&self, client: &Client, pod_name: &str) -> bool {
        let pods: Api<Pod> = Api::namespaced(client.clone(), &self.service_info.namespace);
        match pods.get_opt(pod_name).await {
            Ok(Some(pod)) => pod
                .status
                .and_then(|s| s.phase)
                .map(|phase| phase == "Running")
                .unwrap_or(false),
            _ => false,
        }
    }

    /// Open a real port-forward stream to the bound pod's remote port to confirm
    /// the data path works, not just that our local listener accepts connections.
    async fn probe_upstream(&self, client: &Client, pod_name: &str) -> bool {
        let pods: Api<Pod> = Api::namespaced(client.clone(), &self.service_info.namespace);
        let remote = self.config.ports.remote;
        match pods.portforward(pod_name, &[remote]).await {
            Ok(mut pf) => pf.take_stream(remote).is_some(),
            Err(_) => false,
        }
    }

    pub async fn establish_forward(&self, client: &Client) -> Result<()> {
        // Check current state first and update atomically. Only an already
        // established connection short-circuits; a fresh Starting state must
        // proceed to actually bind.
        let mut current_state = self.state.write().await;
        match *current_state {
            ForwardState::Connected => {
                debug!("Port forward {} is already connected", self.config.name);
                return Ok(());
            }
            _ => {
                *current_state = ForwardState::Starting;
            }
        }
        drop(current_state); // Release the lock

        // Get pod for the service
        let pod = self.get_pod(client).await?;
        // Clone the name to avoid lifetime issues
        let pod_name = pod.metadata.name.clone().ok_or_else(|| {
            self.metrics.record_connection_failure();
            PortForwardError::ConnectionError("Pod name not found".to_string())
        })?;

        // Remember which pod we are bound to so the health loop can detect a
        // replacement and reconnect to the new pod.
        *self.active_pod.write().await = Some(pod_name.clone());

        // Create Api instance for the namespace
        let _pods: Api<Pod> = Api::namespaced(client.clone(), &self.service_info.namespace);

        // Try to bind to the port with retries
        let mut retry_count = 0;

        // Create TCP listener for the local port
        debug!(
            "Creating TCP listener for the local port: {}",
            self.config.ports.local
        );

        // Create listener based on protocol
        let protocol = self.config.ports.protocol.as_deref().unwrap_or("TCP");
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
                            if retry_count >= MAX_BIND_RETRIES {
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
                                self.config.ports.local, BIND_RETRY_DELAY
                            );
                            tokio::time::sleep(BIND_RETRY_DELAY).await;
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
        connection_timeout: std::time::Duration,
    ) -> anyhow::Result<()> {
        debug!("Starting port forward for port {}", port);

        // Create port forward
        let mut pf = pods
            .portforward(&pod_name, &[port])
            .await
            .map_err(|e| anyhow::anyhow!("Failed to create portforward: {}", e))?;

        // Get the stream for our port
        let mut upstream_conn = pf.take_stream(port).ok_or_else(|| {
            anyhow::anyhow!("Failed to get port forward stream for port {}", port)
        })?;

        debug!("Port forward stream established for port {}", port);

        // Copy data bidirectionally. `connection_timeout` is an *idle* timeout: it
        // only fires when neither side has transferred data for that long, so a
        // healthy long-lived connection (psql, a stream) is never torn down while
        // it is actively in use.
        match copy_bidirectional_with_idle_timeout(
            &mut client_conn,
            &mut upstream_conn,
            connection_timeout,
        )
        .await
        {
            Ok((sent, received)) => {
                debug!(
                    "Connection closed for port {} (client->pod {} bytes, pod->client {} bytes)",
                    port, sent, received
                );
            }
            Err(e) if e.kind() == std::io::ErrorKind::TimedOut => {
                warn!(
                    "Connection idle for port {} after {:?}, closing",
                    port, connection_timeout
                );
                return Err(anyhow::anyhow!(
                    "Connection idle timeout after {:?}",
                    connection_timeout
                ));
            }
            Err(e) => {
                warn!("Error during data transfer for port {}: {}", port, e);
                return Err(anyhow::anyhow!("Data transfer error: {}", e));
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
        let name = self.config.name.clone();
        let remote_port = self.config.ports.remote;
        let idle_timeout = self.config.options.connection_timeout;
        let token = self.session.read().await.clone();
        let metrics = self.metrics.clone();
        let pods: Api<Pod> = Api::namespaced(client.clone(), &self.service_info.namespace);

        tokio::spawn(async move {
            let socket = Arc::new(socket);
            let mut buf = vec![0u8; 65535]; // Max UDP packet size
            // One reusable upstream session per client peer, keyed by source address.
            let mut sessions: HashMap<SocketAddr, mpsc::Sender<Vec<u8>>> = HashMap::new();

            loop {
                tokio::select! {
                    result = socket.recv_from(&mut buf) => {
                        match result {
                            Ok((len, peer)) => {
                                let data = buf[..len].to_vec();

                                // Reuse the peer's session, recreating it if absent or dead.
                                let delivered = match sessions.get(&peer) {
                                    Some(tx) => tx.send(data.clone()).await.is_ok(),
                                    None => false,
                                };
                                if !delivered {
                                    let (tx, rx) = mpsc::channel::<Vec<u8>>(64);
                                    Self::spawn_udp_session(
                                        pods.clone(),
                                        pod_name.clone(),
                                        remote_port,
                                        socket.clone(),
                                        peer,
                                        rx,
                                        idle_timeout,
                                        token.clone(),
                                        metrics.clone(),
                                    );
                                    let _ = tx.send(data).await;
                                    sessions.insert(peer, tx);
                                }
                            }
                            Err(e) => {
                                error!("UDP receive error for {}: {}", name, e);
                                metrics.record_connection_failure();
                                break;
                            }
                        }
                    }
                    _ = token.cancelled() => {
                        debug!("UDP forward {} cancelled", name);
                        break;
                    }
                }
            }
        });

        Ok(())
    }

    /// Spawn a per-peer UDP session that owns one upstream port-forward stream and
    /// reuses it across datagrams, reopening on error and expiring when idle.
    #[allow(clippy::too_many_arguments)]
    fn spawn_udp_session(
        pods: Api<Pod>,
        pod_name: String,
        remote_port: u16,
        socket: Arc<tokio::net::UdpSocket>,
        peer: SocketAddr,
        mut rx: mpsc::Receiver<Vec<u8>>,
        idle_timeout: std::time::Duration,
        token: CancellationToken,
        metrics: ForwardMetrics,
    ) {
        tokio::spawn(async move {
            // Lazily (re)opened stream, reused across datagrams while healthy.
            let mut upstream = None;

            loop {
                tokio::select! {
                    maybe = rx.recv() => {
                        let Some(data) = maybe else { break };

                        let mut succeeded = false;
                        // Try on the existing stream, then once more on a fresh one.
                        for _ in 0..2 {
                            if upstream.is_none() {
                                match pods.portforward(&pod_name, &[remote_port]).await {
                                    Ok(mut pf) => match pf.take_stream(remote_port) {
                                        Some(stream) => upstream = Some(stream),
                                        None => break,
                                    },
                                    Err(e) => {
                                        error!("Failed to open UDP portforward: {}", e);
                                        break;
                                    }
                                }
                            }
                            if let Some(stream) = upstream.as_mut() {
                                match Self::udp_exchange(stream, &socket, peer, &data).await {
                                    Ok(()) => {
                                        succeeded = true;
                                        break;
                                    }
                                    Err(e) => {
                                        debug!("UDP exchange failed, reopening stream: {}", e);
                                        upstream = None;
                                    }
                                }
                            }
                        }

                        if succeeded {
                            metrics.record_connection_success();
                        } else {
                            metrics.record_connection_failure();
                        }
                    }
                    _ = tokio::time::sleep(idle_timeout) => {
                        debug!("UDP session for {} idle, closing", peer);
                        break;
                    }
                    _ = token.cancelled() => break,
                }
            }
        });
    }

    /// Perform one length-prefixed request/response exchange over an upstream
    /// stream. This matches DNS-over-TCP framing, which is how UDP services like
    /// kube-dns are reached through a (TCP) port-forward.
    async fn udp_exchange<S>(
        upstream: &mut S,
        socket: &tokio::net::UdpSocket,
        peer: SocketAddr,
        data: &[u8],
    ) -> anyhow::Result<()>
    where
        S: AsyncRead + AsyncWrite + Unpin,
    {
        let len_bytes = (data.len() as u16).to_be_bytes();
        upstream.write_all(&len_bytes).await?;
        upstream.write_all(data).await?;
        upstream.flush().await?;

        let mut len_buf = [0u8; 2];
        match upstream.read_exact(&mut len_buf).await {
            Ok(_) => {
                let response_length = u16::from_be_bytes(len_buf) as usize;
                let mut response = vec![0u8; response_length];
                match upstream.read_exact(&mut response).await {
                    Ok(_) => {
                        socket.send_to(&response, peer).await?;
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                        debug!("No response data received from upstream");
                    }
                    Err(e) => return Err(e.into()),
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                debug!("Upstream closed after sending data");
            }
            Err(e) => return Err(e.into()),
        }

        Ok(())
    }

    pub async fn handle_tcp_forward(
        &self,
        client: &Client,
        pod_name: String,
        listener: TcpListener,
    ) -> Result<()> {
        let name = self.config.name.clone();
        let remote_port = self.config.ports.remote;
        let connection_timeout = self.config.options.connection_timeout;
        let token = self.session.read().await.clone();
        let metrics = self.metrics.clone();
        let pods: Api<Pod> = Api::namespaced(client.clone(), &self.service_info.namespace);

        tokio::spawn(async move {
            let mut listener_stream = TcpListenerStream::new(listener);

            loop {
                tokio::select! {
                    maybe_conn = listener_stream.try_next() => {
                        match maybe_conn {
                            Ok(Some(client_conn)) => {
                                if let Ok(peer_addr) = client_conn.peer_addr() {
                                    debug!(%peer_addr, "New TCP connection for {}", name);
                                    metrics.record_connection_attempt();
                                }
                                let pods = pods.clone();
                                let pod_name = pod_name.clone();
                                let metrics = metrics.clone();

                                tokio::spawn(async move {
                                    match Self::forward_connection(&pods, pod_name, remote_port, client_conn, connection_timeout).await {
                                        Err(e) => {
                                            error!("Failed to forward TCP connection: {}", e);
                                            metrics.record_connection_failure();
                                        }
                                        _ => {
                                            metrics.record_connection_success();
                                        }
                                    }
                                });
                            }
                            Ok(None) | Err(_) => {
                                debug!("TCP listener for {} closed", name);
                                break;
                            }
                        }
                    }
                    _ = token.cancelled() => {
                        debug!("TCP forward {} cancelled", name);
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
            if self.matches_pod_selector(&pod, &self.config.pod_selector)
                && let Some(status) = &pod.status
                && let Some(phase) = &status.phase
                && phase == "Running"
            {
                return Ok(pod);
            }
        }

        Err(PortForwardError::ConnectionError(format!(
            "No ready pods found matching selector for service {} (label: {:?}, annotation: {:?})",
            self.service_info.name,
            self.config.pod_selector.label,
            self.config.pod_selector.annotation,
        )))
    }

    pub fn matches_pod_selector(&self, pod: &Pod, selector: &PodSelector) -> bool {
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
            let (key, value) = Self::parse_selector(label_selector);
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
            let (key, value) = Self::parse_selector(annotation_selector);
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

    pub fn parse_selector(selector: &str) -> (&str, &str) {
        let parts: Vec<&str> = selector.split('=').collect();
        match parts.as_slice() {
            [key, value] if !key.is_empty() => (*key, *value),
            _ => {
                warn!(
                    "malformed pod selector '{}', expected 'key=value'; it will match no pods",
                    selector
                );
                ("", "") // Return empty strings if format is invalid
            }
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

        // Cancel listener tasks and break the supervisor's monitor loop.
        self.session.read().await.cancel();
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

    /// Poll each forward's state until all have settled (Connected or Failed) or
    /// `timeout` elapses, then return a `(name, state)` snapshot. Used to build the
    /// startup summary so it reflects real outcomes rather than fire-and-forget.
    pub async fn wait_for_initial_states(
        &self,
        timeout: std::time::Duration,
    ) -> Vec<(String, ForwardState)> {
        let start = tokio::time::Instant::now();
        loop {
            let mut snapshot = Vec::new();
            let mut all_settled = true;
            {
                let forwards = self.forwards.read().await;
                for forward in forwards.iter() {
                    let state = forward.state.read().await.clone();
                    if !matches!(state, ForwardState::Connected | ForwardState::Failed(_)) {
                        all_settled = false;
                    }
                    snapshot.push((forward.config.name.clone(), state));
                }
            }

            if all_settled || start.elapsed() >= timeout {
                return snapshot;
            }
            tokio::time::sleep(std::time::Duration::from_millis(150)).await;
        }
    }
}
