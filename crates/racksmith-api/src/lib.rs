//! The HTTP API, built on axum.
//!
//! This crate is layer 1 of the architecture (see `architecture.toml`): an adapter that exposes
//! the pure domain model of `racksmith_domain` over HTTP. It owns the wire format and the v0
//! in-memory machine store, and never contains lifecycle rules of its own.
//!
//! # Endpoints
//!
//! | Method | Path                           | Success | Errors        |
//! |--------|--------------------------------|---------|---------------|
//! | `GET`  | `/healthz`                     | 200     |               |
//! | `GET`  | `/api/v1/machines`             | 200     |               |
//! | `POST` | `/api/v1/machines`             | 201     | 409, 422      |
//! | `GET`  | `/api/v1/machines/{id}`        | 200     | 404           |
//! | `POST` | `/api/v1/machines/{id}/events` | 200     | 404, 409, 422 |
//!
//! Every error response has the body `{"error": {"code": "...", "message": "..."}}`; the codes
//! are listed on [`ApiError::code`].
//!
//! # Example
//!
//! ```no_run
//! use racksmith_api::{AppState, DEFAULT_DRAIN_TIMEOUT};
//! use tokio::net::TcpListener;
//!
//! # async fn run() -> Result<(), Box<dyn std::error::Error>> {
//! let listener = TcpListener::bind("127.0.0.1:5240").await?;
//! let shutdown = async {
//!     tokio::signal::ctrl_c().await.ok();
//! };
//! racksmith_api::serve(listener, AppState::default(), shutdown, DEFAULT_DRAIN_TIMEOUT).await?;
//! # Ok(())
//! # }
//! ```

mod dto;
mod error;
mod registry;
mod routes;

use std::io;
use std::sync::Arc;
use std::time::Duration;

use tokio::net::TcpListener;
use tokio::sync::oneshot;

pub use error::ApiError;
use registry::MachineRegistry;
pub use registry::RegistryError;
pub use routes::router;

/// Shared state of every request handler: the v0 in-memory machine registry.
///
/// Cloning is cheap; clones share the same registry. `AppState::default()` starts empty.
#[derive(Debug, Clone, Default)]
pub struct AppState {
    registry: Arc<MachineRegistry>,
}

impl AppState {
    /// Returns the machine registry.
    pub(crate) fn registry(&self) -> &MachineRegistry {
        &self.registry
    }
}

/// How long [`serve`] waits for open connections to finish after shutdown is requested, unless
/// the caller chooses otherwise.
pub const DEFAULT_DRAIN_TIMEOUT: Duration = Duration::from_secs(10);

/// Why the HTTP server stopped with an error.
#[derive(Debug, thiserror::Error)]
pub enum ServeError {
    /// The listener failed.
    #[error("HTTP server failed")]
    Io(#[from] io::Error),
    /// Connections were still open when the drain timeout expired; they were closed abruptly.
    #[error("connections were still open {timeout:?} after shutdown was requested")]
    DrainTimedOut {
        /// The drain timeout that expired.
        timeout: Duration,
    },
}

/// Serves the API on `listener` until `shutdown` completes.
///
/// When `shutdown` resolves the server stops accepting connections and lets in-flight requests
/// finish. Connections still open after `drain_timeout`, such as a client that never finishes
/// sending its request, are dropped so that shutdown always completes.
///
/// # Errors
///
/// Returns [`ServeError::Io`] if the listener fails, or [`ServeError::DrainTimedOut`] if
/// connections had to be dropped.
pub async fn serve<F>(
    listener: TcpListener,
    state: AppState,
    shutdown: F,
    drain_timeout: Duration,
) -> Result<(), ServeError>
where
    F: Future<Output = ()> + Send + 'static,
{
    let address = listener.local_addr()?;
    tracing::info!(%address, "serving the HTTP API");
    let (requested, on_request) = oneshot::channel();
    let signal = async move {
        shutdown.await;
        // The receiver is gone only if the server already stopped, so there is nothing to tell.
        requested.send(()).ok();
    };
    let server = axum::serve(listener, router(state)).with_graceful_shutdown(signal);
    let mut server = std::pin::pin!(server.into_future());
    tokio::select! {
        result = &mut server => return Ok(result?),
        Ok(()) = on_request => {}
    }
    tokio::time::timeout(drain_timeout, server)
        .await
        .map_err(|_elapsed| ServeError::DrainTimedOut {
            timeout: drain_timeout,
        })??;
    Ok(())
}

// Dev-dependencies used only by the integration tests in `tests/`.
#[cfg(test)]
use http_body_util as _;
#[cfg(test)]
use serde_json as _;
#[cfg(test)]
use tower as _;
