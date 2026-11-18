//! End-to-end tests that run the built `racksmith` binary.

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
    use std::process::{Command, Output};

    /// Runs `racksmith` with `args` and returns its output.
    fn racksmith(args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_racksmith"))
            .args(args)
            .env_remove("RUST_LOG")
            .output()
            .unwrap()
    }

    fn stdout(output: &Output) -> &str {
        std::str::from_utf8(&output.stdout).unwrap()
    }

    fn stderr(output: &Output) -> &str {
        std::str::from_utf8(&output.stderr).unwrap()
    }

    #[test]
    fn lifecycle_prints_markdown_table_on_stdout_only() {
        let output = racksmith(&["lifecycle"]);
        assert!(output.status.success(), "exit status: {}", output.status);
        assert_eq!(stderr(&output), "", "stderr");
        let table = stdout(&output);
        assert!(
            table
                .starts_with("| From            | Event                     | To              |\n"),
            "heading row: {table}"
        );
        assert!(
            table.contains("| `allocated`     | `deploy`                  | `deploying`     |\n"),
            "allocate -> deploy row: {table}"
        );
        assert!(
            table.ends_with("|\n"),
            "ends with a complete row and newline"
        );
    }

    #[test]
    fn validate_mac_accepts_and_canonicalizes() {
        let output = racksmith(&["validate", "mac", "52-54-00-AB-CD-EF"]);
        assert!(output.status.success(), "exit status: {}", output.status);
        assert_eq!(stdout(&output), "52:54:00:ab:cd:ef\n", "stdout");
    }

    #[test]
    fn validate_mac_rejects_with_failure_status() {
        let output = racksmith(&["validate", "mac", "not-a-mac"]);
        assert_eq!(output.status.code(), Some(1), "exit status");
        assert_eq!(stdout(&output), "", "stdout");
        assert!(
            stderr(&output).contains("invalid MAC address"),
            "stderr: {}",
            stderr(&output)
        );
    }

    #[test]
    fn validate_hostname_accepts_and_lowercases() {
        let output = racksmith(&["validate", "hostname", "Node-01"]);
        assert!(output.status.success(), "exit status: {}", output.status);
        assert_eq!(stdout(&output), "node-01\n", "stdout");
    }

    #[test]
    fn lifecycle_prints_the_tla_transition_set() {
        let output = racksmith(&["lifecycle", "--format", "tla"]);
        assert!(output.status.success(), "exit status: {}", output.status);
        let tla = stdout(&output);
        assert!(
            tla.starts_with("Transitions ==\n    { <<\"New\","),
            "first row: {tla}"
        );
        assert!(tla.ends_with(">> }\n"), "closing brace: {tla}");
    }

    #[test]
    fn validate_explains_values_that_start_with_a_hyphen() {
        for (args, reason) in [
            (
                ["validate", "hostname", "-node"],
                "must not start or end with `-`",
            ),
            (
                ["validate", "mac", "-52:54:00:00:00:01"],
                "invalid MAC address",
            ),
        ] {
            let output = racksmith(&args);
            assert_eq!(output.status.code(), Some(1), "exit status for {args:?}");
            assert!(
                stderr(&output).contains(reason),
                "stderr: {}",
                stderr(&output)
            );
        }
    }

    #[test]
    fn validate_hostname_rejects_with_failure_status() {
        let output = racksmith(&["validate", "hostname", "node_01"]);
        assert_eq!(output.status.code(), Some(1), "exit status");
        assert_eq!(stdout(&output), "", "stdout");
        assert!(
            stderr(&output).contains("invalid hostname"),
            "stderr: {}",
            stderr(&output)
        );
    }

    /// Tests of `racksmith serve`, which send real signals to the binary.
    #[cfg(unix)]
    mod serve {
        use std::io::{BufRead, BufReader, Read as _, Write as _};
        use std::net::TcpStream;
        use std::process::{Child, ChildStderr, Command, Stdio};

        /// A running `racksmith serve` on an ephemeral port, with its log stream.
        struct Server {
            child: Child,
            log: BufReader<ChildStderr>,
            address: String,
        }

        impl Server {
            fn start() -> Self {
                let mut child = Command::new(env!("CARGO_BIN_EXE_racksmith"))
                    .args(["serve", "--listen", "127.0.0.1:0"])
                    .env_remove("RUST_LOG")
                    .stderr(Stdio::piped())
                    .spawn()
                    .unwrap();
                let mut log = BufReader::new(child.stderr.take().unwrap());
                let started = read_until(&mut log, "serving the HTTP API");
                assert!(
                    !started.contains('\x1b'),
                    "no colour codes in piped logs: {started:?}"
                );
                let address = started
                    .split_once("address=")
                    .map(|(_, address)| address.trim().to_owned())
                    .unwrap();
                Self {
                    child,
                    log,
                    address,
                }
            }

            fn signal(&self, name: &str) {
                let status = Command::new("kill")
                    .args([format!("-{name}"), self.child.id().to_string()])
                    .status()
                    .unwrap();
                assert!(status.success(), "kill -{name}");
            }
        }

        /// Reads log lines until one contains `needle`, and returns it.
        fn read_until(log: &mut impl BufRead, needle: &str) -> String {
            let mut line = String::new();
            loop {
                line.clear();
                assert_ne!(
                    log.read_line(&mut line).unwrap(),
                    0,
                    "log ended before {needle:?}"
                );
                if line.contains(needle) {
                    return line;
                }
            }
        }

        #[test]
        fn serve_drains_and_exits_cleanly_on_sigint_and_sigterm() {
            for signal in ["INT", "TERM"] {
                let mut server = Server::start();
                server.signal(signal);
                read_until(&mut server.log, "server stopped");
                let status = server.child.wait().unwrap();
                assert!(status.success(), "exit status after SIG{signal}: {status}");
            }
        }

        #[test]
        fn second_signal_stops_serve_without_waiting_for_stalled_clients() {
            let mut server = Server::start();
            let mut stalled = TcpStream::connect(&server.address).unwrap();
            stalled
                .write_all(b"GET /healthz HTTP/1.1\r\nHost: test\r\n")
                .unwrap();
            // Connections are accepted in order, so once a later one is answered the stalled one
            // has been accepted too.
            let mut probe = TcpStream::connect(&server.address).unwrap();
            probe
                .write_all(b"GET /healthz HTTP/1.1\r\nHost: test\r\nConnection: close\r\n\r\n")
                .unwrap();
            let mut response = String::new();
            probe.read_to_string(&mut response).unwrap();
            assert!(
                response.starts_with("HTTP/1.1 200 OK"),
                "probe response: {response}"
            );
            server.signal("TERM");
            read_until(&mut server.log, "finishing in-flight requests");
            server.signal("TERM");
            let status = server.child.wait().unwrap();
            assert_eq!(status.code(), Some(1), "forced stop is a failure: {status}");
        }
    }
}
