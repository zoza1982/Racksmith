//! Field-effect tests: every command-line argument changes an observable outcome.
//!
//! Each test runs the built `racksmith` binary twice with one argument changed and asserts that
//! the output differs. An argument whose change no test can observe is accepted and ignored,
//! which the completeness layer treats as a defect.

// Dependencies of the binary that these tests do not use directly.
use anyhow as _;
use clap as _;
use racksmith_api as _;
use racksmith_domain as _;
use tokio as _;
use tracing as _;
use tracing_subscriber as _;

#[cfg(test)]
mod tests {
    use std::io::{BufRead as _, BufReader, Read as _, Write as _};
    use std::net::{SocketAddr, TcpListener, TcpStream};
    use std::process::{Child, Command, Output, Stdio};
    use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
    use std::thread;
    use std::time::{Duration, Instant};

    /// Runs `racksmith` with `args` and returns its output.
    fn racksmith(args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_racksmith"))
            .args(args)
            .env_remove("RUST_LOG")
            .output()
            .unwrap()
    }

    /// The observable outcome of a run: exit code, stdout and stderr.
    fn outcome(args: &[&str]) -> (Option<i32>, String, String) {
        let output = racksmith(args);
        (
            output.status.code(),
            String::from_utf8(output.stdout).unwrap(),
            String::from_utf8(output.stderr).unwrap(),
        )
    }

    #[test]
    fn command_selects_the_subcommand_that_runs() {
        let lifecycle = outcome(&["lifecycle"]);
        let validate = outcome(&["validate", "mac", "52:54:00:12:34:56"]);
        assert_eq!(lifecycle.0, Some(0), "lifecycle exit code");
        assert_eq!(validate.0, Some(0), "validate exit code");
        assert!(
            lifecycle.1.starts_with("| From"),
            "lifecycle prints the table"
        );
        assert_eq!(validate.1, "52:54:00:12:34:56\n", "validate prints the MAC");
    }

    #[test]
    fn lifecycle_format_selects_the_output_syntax() {
        let default = outcome(&["lifecycle"]);
        let markdown = outcome(&["lifecycle", "--format", "markdown"]);
        let tla = outcome(&["lifecycle", "--format", "tla"]);
        assert_eq!(markdown.0, Some(0), "markdown exit code");
        assert_eq!(tla.0, Some(0), "tla exit code");
        assert_eq!(default, markdown, "markdown is the default format");
        assert!(
            markdown.1.starts_with("| From"),
            "markdown table: {}",
            markdown.1
        );
        assert!(tla.1.starts_with("Transitions =="), "TLA+ set: {}", tla.1);
        assert_ne!(markdown.1, tla.1, "the format changes the output");
    }

    #[test]
    fn validate_target_selects_the_parser() {
        // Valid both as a MAC address and as a hostname, with a different canonical form.
        let value = "52-54-00-AB-CD-EF";
        let mac = outcome(&["validate", "mac", value]);
        let hostname = outcome(&["validate", "hostname", value]);
        assert_eq!(
            mac,
            (Some(0), "52:54:00:ab:cd:ef\n".to_owned(), String::new())
        );
        assert_eq!(
            hostname,
            (Some(0), "52-54-00-ab-cd-ef\n".to_owned(), String::new())
        );
    }

    #[test]
    fn validate_mac_value_changes_the_output() {
        let first = outcome(&["validate", "mac", "52:54:00:00:00:01"]);
        let second = outcome(&["validate", "mac", "52:54:00:00:00:02"]);
        let invalid = outcome(&["validate", "mac", "52:54:00:00:00"]);
        assert_eq!(first.1, "52:54:00:00:00:01\n", "first MAC");
        assert_eq!(second.1, "52:54:00:00:00:02\n", "second MAC");
        assert_eq!(invalid.0, Some(1), "an invalid MAC fails");
    }

    #[test]
    fn validate_hostname_value_changes_the_output() {
        let first = outcome(&["validate", "hostname", "node-01"]);
        let second = outcome(&["validate", "hostname", "node-02.rack-a"]);
        let invalid = outcome(&["validate", "hostname", "node_01"]);
        assert_eq!(first.1, "node-01\n", "first hostname");
        assert_eq!(second.1, "node-02.rack-a\n", "second hostname");
        assert_eq!(invalid.0, Some(1), "an invalid hostname fails");
    }

    /// Upper bound for anything that should be quick, so a hang fails the test instead of CI.
    const PATIENCE: Duration = Duration::from_secs(10);

    /// A loopback address that was free a moment ago: bound to port 0, read, and released.
    fn free_address() -> SocketAddr {
        TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
    }

    /// A running `racksmith serve` whose stderr lines arrive on a channel, so every wait for
    /// one is bounded by [`PATIENCE`]. The process is killed on drop.
    struct Server {
        child: Child,
        lines: Receiver<String>,
    }

    impl Server {
        fn start(listen: &str, rust_log: Option<&str>) -> Self {
            let mut command = Command::new(env!("CARGO_BIN_EXE_racksmith"));
            command
                .args(["serve", "--listen", listen])
                .env_remove("RUST_LOG")
                .stderr(Stdio::piped());
            if let Some(filter) = rust_log {
                command.env("RUST_LOG", filter);
            }
            let mut child = command.spawn().unwrap();
            let stderr = child.stderr.take().unwrap();
            let (sender, lines) = mpsc::channel();
            thread::spawn(move || {
                for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                    if sender.send(line).is_err() {
                        break;
                    }
                }
            });
            Self { child, lines }
        }

        /// The next stderr line containing `needle`, or a failure after [`PATIENCE`].
        fn wait_for(&self, needle: &str) -> String {
            let deadline = Instant::now() + PATIENCE;
            loop {
                let left = deadline.saturating_duration_since(Instant::now());
                match self.lines.recv_timeout(left) {
                    Ok(line) if line.contains(needle) => return line,
                    Ok(_) => {}
                    Err(error) => panic!("no stderr line with `{needle}`: {error}"),
                }
            }
        }

        /// Waits until the process exits on its own, failing after [`PATIENCE`]; returns its
        /// exit code and stderr.
        fn wait_for_exit(mut self) -> (Option<i32>, String) {
            let mut stderr = Vec::new();
            let deadline = Instant::now() + PATIENCE;
            loop {
                let left = deadline.saturating_duration_since(Instant::now());
                match self.lines.recv_timeout(left) {
                    Ok(line) => stderr.push(line),
                    Err(RecvTimeoutError::Disconnected) => break,
                    Err(RecvTimeoutError::Timeout) => {
                        panic!("the server is still running: {}", stderr.join("\n"))
                    }
                }
            }
            let status = self.child.wait().unwrap();
            (status.code(), stderr.join("\n"))
        }

        /// Kills the process and returns every stderr line not consumed yet.
        fn stop(mut self) -> String {
            self.child.kill().unwrap();
            self.child.wait().unwrap();
            let rest: Vec<String> = self.lines.try_iter().collect();
            rest.join("\n")
        }
    }

    impl Drop for Server {
        fn drop(&mut self) {
            // Already gone after `stop` or `wait_for_exit`; otherwise a failed test must not
            // leave a server running.
            self.child.kill().ok();
            self.child.wait().ok();
        }
    }

    /// Sends one HTTP/1.1 request to `address` and returns the raw response.
    fn http(address: SocketAddr, method: &str, path: &str, body: &str) -> String {
        let mut stream = TcpStream::connect_timeout(&address, PATIENCE).unwrap();
        stream.set_read_timeout(Some(PATIENCE)).unwrap();
        write!(
            stream,
            "{method} {path} HTTP/1.1\r\nHost: test\r\nContent-Type: application/json\r\n\
             Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
        .unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        response
    }

    /// `serve --listen` binds exactly the given address, and an address already in use fails
    /// and names it. A server that ignored the flag would bind the default 127.0.0.1:5240: the
    /// first half would then not see the requested port, and the second would keep running
    /// and fail after [`PATIENCE`] instead of hanging. The server is stopped with `kill`, so
    /// no signal handling is involved; only loopback is used.
    #[test]
    fn listen_changes_the_bound_address() {
        let requested = free_address();
        let server = Server::start(&requested.to_string(), None);
        let started = server.wait_for("serving the HTTP API");
        server.stop();
        assert!(
            started.contains(&format!("address={requested}")),
            "the server listens on {requested}: {started}"
        );

        let occupied = TcpListener::bind("127.0.0.1:0").unwrap();
        let taken = occupied.local_addr().unwrap().to_string();
        let (code, stderr) = Server::start(&taken, None).wait_for_exit();
        assert_ne!(code, Some(0), "binding a used address fails: {stderr}");
        assert!(
            stderr.contains(&format!("failed to listen on {taken}")),
            "the error names the address: {stderr}"
        );
    }

    /// `RUST_LOG` selects which log lines are written: by default (`info`) a registration is
    /// logged, and a filter that silences the routes module drops that line while the startup
    /// line, from another module, stays. The HTTP response arrives only after the handler has
    /// logged, so the line is in the pipe before the server is killed.
    #[test]
    fn rust_log_filters_the_log_lines() {
        let logs = |rust_log: Option<&str>| {
            let address = free_address();
            let server = Server::start(&address.to_string(), rust_log);
            server.wait_for("serving the HTTP API");
            let response = http(
                address,
                "POST",
                "/api/v1/machines",
                "{\"mac\":\"52:54:00:12:34:56\"}",
            );
            assert!(response.starts_with("HTTP/1.1 201"), "{response}");
            server.stop()
        };
        let default = logs(None);
        let filtered = logs(Some("info,racksmith_api::routes=off"));
        assert!(
            default.contains("registered machine"),
            "info is the default level: {default}"
        );
        assert!(
            !filtered.contains("registered machine"),
            "RUST_LOG silenced the routes module: {filtered}"
        );
    }
}
