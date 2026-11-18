//! Terminal output. `println!`/`eprintln!` are banned workspace-wide, so all output goes
//! through explicit, fallible writes to locked handles.

use std::io::{self, Write as _};

/// Writes an informational message to stdout.
///
/// # Errors
///
/// Returns an error if stdout cannot be written (e.g. a closed pipe).
pub(crate) fn info(message: &str) -> io::Result<()> {
    writeln!(io::stdout().lock(), "{message}")
}

/// Writes a section header for a quality-gate step to stderr.
///
/// # Errors
///
/// Returns an error if stderr cannot be written.
pub(crate) fn header(title: &str) -> io::Result<()> {
    writeln!(io::stderr().lock(), "\n==> {title}")
}

/// Writes a warning to stderr.
///
/// # Errors
///
/// Returns an error if stderr cannot be written.
pub(crate) fn warn(message: &str) -> io::Result<()> {
    writeln!(io::stderr().lock(), "warning: {message}")
}

/// Writes an error to stderr.
///
/// # Errors
///
/// Returns an error if stderr cannot be written.
pub(crate) fn error(message: &str) -> io::Result<()> {
    writeln!(io::stderr().lock(), "error: {message}")
}
