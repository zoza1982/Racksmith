//! `racksmith serve`: run the HTTP API until a shutdown signal.
//!
//! The first Ctrl-C (SIGINT), or SIGTERM on Unix as sent by systemd and Kubernetes, starts a
//! graceful shutdown: no new connections, in-flight requests finish, and connections still open
//! after [`DEFAULT_DRAIN_TIMEOUT`] are dropped. A second signal stops at once with an error.

use std::io;
use std::net::SocketAddr;

use anyhow::Context as _;
use racksmith_api::{AppState, DEFAULT_DRAIN_TIMEOUT};
use tokio::net::TcpListener;
use tokio::runtime;
use tokio::sync::oneshot;

/// Starts an async runtime and serves the API on `listen` until a shutdown signal.
///
/// # Errors
///
/// Returns an error if the runtime or the signal handlers cannot start, the address cannot be
/// bound, the server fails, connections outlive the drain timeout, or a second signal arrives
/// before shutdown completes.
pub(crate) fn run(listen: SocketAddr) -> anyhow::Result<()> {
    let runtime = runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("failed to start the async runtime")?;
    runtime.block_on(serve(listen))
}

/// Binds `listen` and serves the API, shutting down as described in the module docs.
async fn serve(listen: SocketAddr) -> anyhow::Result<()> {
    let signals = Signals::install().context("failed to install the shutdown signal handlers")?;
    let listener = TcpListener::bind(listen)
        .await
        .with_context(|| format!("failed to listen on {listen}"))?;
    let (requested, on_request) = oneshot::channel();
    let shutdown = async {
        // The sender is dropped only when signal handling ends, which also ends this command.
        on_request.await.ok();
    };
    let server = racksmith_api::serve(
        listener,
        AppState::default(),
        shutdown,
        DEFAULT_DRAIN_TIMEOUT,
    );
    tokio::select! {
        result = server => result?,
        error = handle_signals(signals, requested) => return Err(error),
    }
    tracing::info!("server stopped");
    Ok(())
}

/// Requests a graceful shutdown on the first signal and returns an error on the second.
async fn handle_signals(mut signals: Signals, requested: oneshot::Sender<()>) -> anyhow::Error {
    let first = signals.recv().await;
    tracing::info!("received {first}; finishing in-flight requests (send it again to stop now)");
    // The receiver is gone only if the server already stopped, so there is nothing to tell.
    requested.send(()).ok();
    let second = signals.recv().await;
    anyhow::anyhow!("received {second} during shutdown; stopped without finishing requests")
}

/// The signals that stop the server: SIGINT and SIGTERM.
#[cfg(unix)]
struct Signals {
    interrupt: tokio::signal::unix::Signal,
    terminate: tokio::signal::unix::Signal,
}

#[cfg(unix)]
impl Signals {
    fn install() -> io::Result<Self> {
        use tokio::signal::unix::{SignalKind, signal};

        Ok(Self {
            interrupt: signal(SignalKind::interrupt())?,
            terminate: signal(SignalKind::terminate())?,
        })
    }

    /// Waits for the next signal and returns its name.
    async fn recv(&mut self) -> &'static str {
        tokio::select! {
            _ = self.interrupt.recv() => "SIGINT",
            _ = self.terminate.recv() => "SIGTERM",
        }
    }
}

/// The signal that stops the server: Ctrl-C.
#[cfg(windows)]
struct Signals {
    ctrl_c: tokio::signal::windows::CtrlC,
}

#[cfg(windows)]
impl Signals {
    fn install() -> io::Result<Self> {
        Ok(Self {
            ctrl_c: tokio::signal::windows::ctrl_c()?,
        })
    }

    /// Waits for the next signal and returns its name.
    async fn recv(&mut self) -> &'static str {
        self.ctrl_c.recv().await;
        "Ctrl-C"
    }
}
