//! The machine aggregate.

use serde::Serialize;

use crate::{Hostname, MacAddress, MachineEvent, MachineId, MachineState, TransitionError};

/// A physical machine under management.
///
/// The lifecycle state can only change through [`Machine::handle`], which enforces the
/// transition rules of [`MachineState::apply`]. For that reason `Machine` implements
/// `Serialize` but not `Deserialize`: deserializing would let callers forge any state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Machine {
    id: MachineId,
    mac: MacAddress,
    hostname: Option<Hostname>,
    state: MachineState,
}

impl Machine {
    /// Creates a newly discovered machine in [`MachineState::New`] with no hostname.
    ///
    /// `mac` is the address of the primary boot interface.
    #[must_use]
    pub const fn new(id: MachineId, mac: MacAddress) -> Self {
        Self {
            id,
            mac,
            hostname: None,
            state: MachineState::New,
        }
    }

    /// Returns this machine with `hostname` set.
    #[must_use]
    pub fn with_hostname(mut self, hostname: Hostname) -> Self {
        self.hostname = Some(hostname);
        self
    }

    /// Returns the machine's identifier.
    #[must_use]
    pub const fn id(&self) -> MachineId {
        self.id
    }

    /// Returns the MAC address of the primary boot interface.
    #[must_use]
    pub const fn mac(&self) -> MacAddress {
        self.mac
    }

    /// Returns the hostname, if one has been assigned.
    #[must_use]
    pub const fn hostname(&self) -> Option<&Hostname> {
        self.hostname.as_ref()
    }

    /// Returns the current lifecycle state.
    #[must_use]
    pub const fn state(&self) -> MachineState {
        self.state
    }

    /// Applies `event` and returns the new state.
    ///
    /// The update is atomic: on error the machine is left exactly as it was.
    ///
    /// # Errors
    ///
    /// Returns [`TransitionError`] if `event` is not valid in the current state.
    pub const fn handle(&mut self, event: MachineEvent) -> Result<MachineState, TransitionError> {
        match self.state.apply(event) {
            Ok(next) => {
                self.state = next;
                Ok(next)
            }
            Err(error) => Err(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU64;

    use super::Machine;
    use crate::{MacAddress, MachineEvent, MachineId, MachineState};

    fn machine() -> Machine {
        Machine::new(
            MachineId::new(NonZeroU64::MIN),
            MacAddress::new([0x52, 0x54, 0, 0, 0, 1]),
        )
    }

    #[test]
    fn starts_new_without_hostname() {
        let machine = machine();
        assert_eq!(machine.state(), MachineState::New, "initial state");
        assert_eq!(machine.hostname(), None, "no hostname by default");
    }

    #[test]
    fn handle_moves_through_the_happy_path() {
        let mut machine = machine();
        let path = [
            (
                MachineEvent::StartCommissioning,
                MachineState::Commissioning,
            ),
            (MachineEvent::CommissioningSucceeded, MachineState::Ready),
            (MachineEvent::Allocate, MachineState::Allocated),
            (MachineEvent::Deploy, MachineState::Deploying),
            (MachineEvent::DeploymentSucceeded, MachineState::Deployed),
            (MachineEvent::Release, MachineState::Releasing),
            (MachineEvent::ReleaseCompleted, MachineState::Ready),
        ];
        for (event, expected) in path {
            assert_eq!(machine.handle(event), Ok(expected), "{event:?}");
            assert_eq!(machine.state(), expected, "state after {event:?}");
        }
    }

    #[test]
    fn rejected_event_leaves_machine_unchanged() {
        let mut machine = machine();
        let before = machine.clone();
        let error = machine
            .handle(MachineEvent::Deploy)
            .expect_err("new machines cannot deploy");
        assert_eq!(
            error.from,
            MachineState::New,
            "error reports the current state"
        );
        assert_eq!(machine, before, "machine is untouched");
    }

    #[test]
    fn serializes_all_fields() {
        let machine = machine().with_hostname("node-01".parse().expect("valid hostname"));
        let json = serde_json::to_value(&machine).expect("serialization is infallible");
        assert_eq!(
            json,
            serde_json::json!({
                "id": "m-0000000000000001",
                "mac": "52:54:00:00:00:01",
                "hostname": "node-01",
                "state": "new",
            }),
            "wire format"
        );
    }
}
