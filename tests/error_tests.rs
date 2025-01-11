#[cfg(test)]
mod tests {
    use kube_forward::error::PortForwardError;
    use kube_forward::error::Result;

    #[test]
    fn test_error_creation() {
        // Test DNS error
        let dns_error = PortForwardError::DnsError("invalid name".to_string());
        assert!(matches!(dns_error, PortForwardError::DnsError(_)));
        assert_eq!(dns_error.to_string(), "invalid DNS name: invalid name");

        // Test Connection error
        let conn_error = PortForwardError::ConnectionError("timeout".to_string());
        assert!(matches!(conn_error, PortForwardError::ConnectionError(_)));
        assert_eq!(conn_error.to_string(), "connection error: timeout");
    }

    #[test]
    fn test_result_type() {
        // Test Ok result
        let ok_result: Result<i32> = Ok(42);
        assert!(ok_result.is_ok());
        assert_eq!(ok_result.unwrap(), 42);

        // Test Err result
        let err_result: Result<i32> = Err(PortForwardError::DnsError("test error".to_string()));
        assert!(err_result.is_err());
        assert!(matches!(
            err_result.unwrap_err(),
            PortForwardError::DnsError(_)
        ));
    }
}
