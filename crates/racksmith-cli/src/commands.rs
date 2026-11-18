//! Subcommand implementations, one module per subcommand.
//!
//! Commands write to explicit writers rather than printing, so their output can be tested.

mod lifecycle;
mod serve;
mod validate;

use std::io;
use std::process::ExitCode;

use crate::cli::Command;

/// Runs `command` against the process's stdout and stderr.
///
/// # Errors
///
/// Returns an error if the command fails or its output cannot be written.
pub(crate) fn run(command: Command) -> anyhow::Result<ExitCode> {
    match command {
        Command::Serve { listen } => {
            serve::run(listen)?;
            Ok(ExitCode::SUCCESS)
        }
        Command::Lifecycle { format } => {
            lifecycle::run(format, &mut io::stdout().lock())?;
            Ok(ExitCode::SUCCESS)
        }
        Command::Validate { target } => {
            let code = validate::run(&target, &mut io::stdout().lock(), &mut io::stderr().lock())?;
            Ok(code)
        }
    }
}
