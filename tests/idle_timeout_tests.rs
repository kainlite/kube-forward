#[cfg(test)]
mod tests {
    use kube_forward::forward::copy_bidirectional_with_idle_timeout;
    use std::time::Duration;
    use tokio::io::{AsyncReadExt, AsyncWriteExt, duplex};

    #[ctor::ctor(unsafe)]
    fn init() {
        let _ = rustls::crypto::ring::default_provider().install_default();
    }

    #[tokio::test]
    async fn test_copies_both_directions_and_counts_bytes() {
        let (mut client, mut a) = duplex(1024);
        let (mut b, mut server) = duplex(1024);

        let handle = tokio::spawn(async move {
            copy_bidirectional_with_idle_timeout(&mut a, &mut b, Duration::from_secs(5)).await
        });

        client.write_all(b"hello").await.unwrap();
        let mut buf = [0u8; 5];
        server.read_exact(&mut buf).await.unwrap();
        assert_eq!(&buf, b"hello");

        server.write_all(b"hi").await.unwrap();
        let mut buf2 = [0u8; 2];
        client.read_exact(&mut buf2).await.unwrap();
        assert_eq!(&buf2, b"hi");

        drop(client);
        drop(server);

        let (a_to_b, b_to_a) = handle.await.unwrap().unwrap();
        assert_eq!(a_to_b, 5);
        assert_eq!(b_to_a, 2);
    }

    #[tokio::test]
    async fn test_idle_connection_times_out() {
        let (_client, mut a) = duplex(1024);
        let (mut b, _server) = duplex(1024);

        // No data ever flows; the idle timer should fire.
        let res =
            copy_bidirectional_with_idle_timeout(&mut a, &mut b, Duration::from_millis(100)).await;

        let err = res.expect_err("expected an idle timeout");
        assert_eq!(err.kind(), std::io::ErrorKind::TimedOut);
    }

    #[tokio::test]
    async fn test_active_connection_survives_past_idle_timeout() {
        let (mut client, mut a) = duplex(1024);
        let (mut b, mut server) = duplex(1024);

        let handle = tokio::spawn(async move {
            copy_bidirectional_with_idle_timeout(&mut a, &mut b, Duration::from_millis(200)).await
        });

        // Keep traffic flowing every 50ms for ~600ms, well past the 200ms idle
        // timeout. Because the timer resets on activity, the copy must stay alive.
        for _ in 0..12 {
            client.write_all(b"x").await.unwrap();
            let mut buf = [0u8; 1];
            server.read_exact(&mut buf).await.unwrap();
            tokio::time::sleep(Duration::from_millis(50)).await;
        }

        drop(client);
        drop(server);

        let (a_to_b, _b_to_a) = handle
            .await
            .unwrap()
            .expect("active connection should not idle-timeout");
        assert_eq!(a_to_b, 12);
    }
}
