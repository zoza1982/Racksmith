//! Command-line arguments.

use std::net::SocketAddr;

use clap::{Parser, Subcommand, ValueEnum};

/// Bare-metal provisioning for racks of physical servers.
#[derive(Debug, Parser)]
#[command(name = "racksmith", version, about)]
pub(crate) struct Cli {
    /// The subcommand to run.
    #[command(subcommand)]
    pub(crate) command: Command,
}

/// A `racksmith` subcommand.
#[derive(Debug, Subcommand)]
pub(crate) enum Command {
    /// Run the HTTP API server until Ctrl-C or SIGTERM (a second signal stops it at once).
    Serve {
        /// Address to listen on (5240 is the port MAAS uses).
        #[arg(long, default_value = "127.0.0.1:5240")]
        listen: SocketAddr,
    },
    /// Print the machine lifecycle transition table.
    Lifecycle {
        /// Output format.
        #[arg(long, value_enum, default_value_t = LifecycleFormat::Markdown)]
        format: LifecycleFormat,
    },
    /// Check that a value is well-formed and print its canonical form.
    Validate {
        /// What to validate.
        #[command(subcommand)]
        target: ValidateTarget,
    },
}

/// A value that `racksmith validate` can check.
#[derive(Debug, Subcommand)]
pub(crate) enum ValidateTarget {
    /// A MAC address, such as `52:54:00:12:34:56` or `52-54-00-12-34-56`.
    Mac {
        /// The MAC address to check.
        #[arg(allow_hyphen_values = true)]
        value: String,
    },
    /// An RFC 1123 hostname, such as `node-01.rack-a`.
    Hostname {
        /// The hostname to check.
        #[arg(allow_hyphen_values = true)]
        value: String,
    },
}

/// How `racksmith lifecycle` prints the transition table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(crate) enum LifecycleFormat {
    /// A Markdown table, embedded in `docs/lifecycle.md`.
    Markdown,
    /// The TLA+ `Transitions` definition, embedded in `specs/tla/MachineLifecycle.tla`.
    Tla,
}
