use thiserror::Error;

#[derive(Error, Debug)]
pub enum PortForwardError {
    #[error("kubernetes error: {0}")]
    KubeError(#[from] kube::Error),

    #[error("invalid DNS name: {0}")]
    DnsError(String),

    // #[error("configuration error: {0}")]
    // ConfigError(String),
    #[error("connection error: {0}")]
    ConnectionError(String),
}

pub type Result<T> = std::result::Result<T, PortForwardError>;
