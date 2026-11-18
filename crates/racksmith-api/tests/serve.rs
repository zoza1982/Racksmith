//! Tests of `racksmith_api::serve` over a real loopback socket: requests are served, and
//! shutdown completes even when a client stalls.

// Dependencies of the library that these tests do not use directly.
use axum as _;
use http_body_util as _;
use racksmith_domain as _;
use serde as _;
use serde_json as _;
use thiserror as _;
use tower as _;
use tracing as _;

#[cfg(test)]
mod tests {
    use std::io::{Read as _, Write as _};
    use std::net::{SocketAddr, TcpStream};
    use std::time::Duration;

    use racksmith_api::{AppState, ServeError, serve};
    use tokio::net::TcpListener;
    use tokio::sync::oneshot;
    use tokio::task::{self, JoinHandle};

    const REQUEST: &[u8] = b"GET /healthz HTTP/1.1\r\nHost: test\r\n\r\n";
    const RESPONSE_END: &[u8] = b"{\"status\":\"ok\"}";
    /// Upper bound for anything that should be quick, so a hang fails the test instead of CI.
    const PATIENCE: Duration = Duration::from_secs(10);

    /// A server running on an ephemeral loopback port.
    struct Server {
        address: SocketAddr,
        shutdown: oneshot::Sender<()>,
        handle: JoinHandle<Result<(), ServeError>>,
    }

    async fn start(drain_timeout: Duration) -> Server {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (shutdown, requested) = oneshot::channel::<()>();
        let signal = async {
            requested.await.ok();
        };
        let handle = tokio::spawn(serve(listener, AppState::default(), signal, drain_timeout));
        Server {
            address,
            shutdown,
            handle,
        }
    }

    impl Server {
        /// Requests shutdown and returns how `serve` finished.
        async fn stop(self) -> Result<(), ServeError> {
            self.shutdown.send(()).unwrap();
            tokio::time::timeout(PATIENCE, self.handle)
                .await
                .expect("serve must return after shutdown")
                .unwrap()
        }
    }

    /// Connects, sends one complete request and reads its response, keeping the connection
    /// open. Runs on a blocking thread because it uses a blocking socket.
    async fn connect_and_get(address: SocketAddr) -> (TcpStream, String) {
        task::spawn_blocking(move || {
            let mut stream = TcpStream::connect(address).unwrap();
            stream.set_read_timeout(Some(PATIENCE)).unwrap();
            stream.write_all(REQUEST).unwrap();
            let mut response = Vec::new();
            let mut buffer = [0; 1024];
            while !response.ends_with(RESPONSE_END) {
                let read = stream.read(&mut buffer).unwrap();
                assert_ne!(read, 0, "connection closed before the response ended");
                response.extend_from_slice(&buffer[..read]);
            }
            (stream, String::from_utf8(response).unwrap())
        })
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn serves_requests_and_stops_gracefully() {
        let server = start(PATIENCE).await;
        let (stream, response) = connect_and_get(server.address).await;
        assert!(
            response.starts_with("HTTP/1.1 200 OK"),
            "response: {response}"
        );
        drop(stream);
        server.stop().await.expect("an idle server drains cleanly");
    }

    #[tokio::test]
    async fn stalled_connection_cannot_block_shutdown() {
        let drain_timeout = Duration::from_millis(200);
        let server = start(drain_timeout).await;
        let address = server.address;
        let stalled = task::spawn_blocking(move || {
            let mut stream = TcpStream::connect(address).unwrap();
            // The headers never end: the request is incomplete forever.
            stream
                .write_all(b"GET /healthz HTTP/1.1\r\nHost: test\r\n")
                .unwrap();
            stream
        })
        .await
        .unwrap();
        // Connections are accepted in order, so once a later one is served the stalled one has
        // been accepted too.
        let (_served, _) = connect_and_get(address).await;
        let error = server
            .stop()
            .await
            .expect_err("a stalled connection outlives the drain timeout");
        assert!(
            matches!(error, ServeError::DrainTimedOut { timeout } if timeout == drain_timeout),
            "error: {error:?}"
        );
        drop(stalled);
    }
}
