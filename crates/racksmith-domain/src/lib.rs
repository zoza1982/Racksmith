//! Pure domain model: validated identifiers and the machine lifecycle state machine.
//!
//! This crate is layer 0 of the architecture (see `architecture.toml`): it contains only
//! value types and the machine lifecycle state machine. It performs no I/O, spawns no tasks and
//! depends on nothing but `serde` and `thiserror`, so every rule here can be tested exhaustively
//! and checked formally.
//!
//! - [`MachineId`], [`MacAddress`] and [`Hostname`] are validated newtypes: once constructed they
//!   are guaranteed well-formed, and their `Display` output always parses back to an equal value.
//! - [`MachineState`] and [`MachineEvent`] form the lifecycle state machine. [`MachineState::apply`]
//!   is the single source of truth for transitions; [`TRANSITIONS`] lists the valid ones, and
//!   the docs table and the TLA+ model's transition set are generated from it.
//! - [`Machine`] ties an identity to a lifecycle state and applies events atomically.
//!
//! # Example
//!
//! ```
//! use std::num::NonZeroU64;
//!
//! use racksmith_domain::{Machine, MachineEvent, MachineId, MachineState};
//!
//! let id = MachineId::new(NonZeroU64::MIN);
//! let mut machine = Machine::new(id, "52:54:00:12:34:56".parse()?);
//! assert_eq!(machine.state(), MachineState::New);
//!
//! machine.handle(MachineEvent::StartCommissioning)?;
//! machine.handle(MachineEvent::CommissioningSucceeded)?;
//! assert_eq!(machine.state(), MachineState::Ready);
//!
//! // Invalid transitions are rejected and leave the machine untouched.
//! let error = machine.handle(MachineEvent::Deploy).unwrap_err();
//! assert_eq!(error.from, MachineState::Ready);
//! assert_eq!(machine.state(), MachineState::Ready);
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

mod hex;
mod hostname;
mod id;
mod lifecycle;
mod mac;
mod machine;
mod serde_str;

pub use hostname::{Hostname, ParseHostnameError};
pub use id::{MachineId, ParseMachineIdError};
pub use lifecycle::{MachineEvent, MachineState, TRANSITIONS, TransitionError};
pub use mac::{MacAddress, ParseMacAddressError};
pub use machine::Machine;

// `proptest` is a dev-dependency used only by the integration tests in `tests/`.
#[cfg(test)]
use proptest as _;
