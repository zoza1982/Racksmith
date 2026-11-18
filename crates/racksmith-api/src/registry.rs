//! The v0 machine store: an in-memory registry of machines keyed by id.
//!
//! The registry owns every [`Machine`] and is the only place lifecycle events are applied, so
//! uniqueness checks and state changes happen under one write lock and cannot interleave.

use std::collections::BTreeMap;
use std::num::NonZeroU64;

use racksmith_domain::{Hostname, MacAddress, Machine, MachineEvent, MachineId, TransitionError};
use tokio::sync::RwLock;

/// Why a registry operation was rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum RegistryError {
    /// No machine has the given id.
    #[error("machine `{0}` does not exist")]
    NotFound(MachineId),
    /// Another machine already uses the given MAC address.
    #[error("a machine with MAC address `{0}` is already registered")]
    DuplicateMac(MacAddress),
    /// The event is not valid in the machine's current state.
    #[error(transparent)]
    Transition(#[from] TransitionError),
    /// Every machine id has been allocated.
    #[error("no machine ids are left to allocate")]
    IdSpaceExhausted,
}

/// In-memory store of machines, shared by all request handlers.
///
/// Ids are allocated sequentially starting at 1 and never reused. Listing returns machines in
/// id order, so output is deterministic.
#[derive(Debug)]
pub(crate) struct MachineRegistry {
    inner: RwLock<Inventory>,
}

/// The data guarded by the registry's lock.
#[derive(Debug)]
struct Inventory {
    machines: BTreeMap<MachineId, Machine>,
    /// The id the next registered machine receives, or `None` once ids are exhausted.
    next_id: Option<NonZeroU64>,
}

impl Default for MachineRegistry {
    fn default() -> Self {
        Self {
            inner: RwLock::new(Inventory {
                machines: BTreeMap::new(),
                next_id: Some(NonZeroU64::MIN),
            }),
        }
    }
}

impl MachineRegistry {
    /// Returns every machine, ordered by id.
    pub(crate) async fn list(&self) -> Vec<Machine> {
        self.inner.read().await.machines.values().cloned().collect()
    }

    /// Returns the machine with the given id.
    ///
    /// # Errors
    ///
    /// Returns [`RegistryError::NotFound`] if no machine has that id.
    pub(crate) async fn get(&self, id: MachineId) -> Result<Machine, RegistryError> {
        self.inner
            .read()
            .await
            .machines
            .get(&id)
            .cloned()
            .ok_or(RegistryError::NotFound(id))
    }

    /// Registers a newly discovered machine and returns it, in [`MachineState::New`].
    ///
    /// [`MachineState::New`]: racksmith_domain::MachineState::New
    ///
    /// # Errors
    ///
    /// Returns [`RegistryError::DuplicateMac`] if `mac` is already registered, or
    /// [`RegistryError::IdSpaceExhausted`] if no ids are left.
    pub(crate) async fn register(
        &self,
        mac: MacAddress,
        hostname: Option<Hostname>,
    ) -> Result<Machine, RegistryError> {
        let mut inventory = self.inner.write().await;
        if inventory
            .machines
            .values()
            .any(|machine| machine.mac() == mac)
        {
            return Err(RegistryError::DuplicateMac(mac));
        }
        let id = inventory.next_id.ok_or(RegistryError::IdSpaceExhausted)?;
        inventory.next_id = id.checked_add(1);

        let id = MachineId::new(id);
        let machine = match hostname {
            Some(hostname) => Machine::new(id, mac).with_hostname(hostname),
            None => Machine::new(id, mac),
        };
        inventory.machines.insert(id, machine.clone());
        Ok(machine)
    }

    /// Applies `event` to the machine with the given id and returns the updated machine.
    ///
    /// The update is atomic: on error the stored machine is unchanged.
    ///
    /// # Errors
    ///
    /// Returns [`RegistryError::NotFound`] if no machine has that id, or
    /// [`RegistryError::Transition`] if `event` is not valid in the machine's current state.
    pub(crate) async fn apply(
        &self,
        id: MachineId,
        event: MachineEvent,
    ) -> Result<Machine, RegistryError> {
        let mut inventory = self.inner.write().await;
        let machine = inventory
            .machines
            .get_mut(&id)
            .ok_or(RegistryError::NotFound(id))?;
        machine.handle(event)?;
        Ok(machine.clone())
    }
}

#[cfg(test)]
mod tests {
    use racksmith_domain::MachineState;

    use super::*;

    fn mac(last: u8) -> MacAddress {
        MacAddress::new([0x52, 0x54, 0x00, 0x00, 0x00, last])
    }

    #[tokio::test]
    async fn ids_are_sequential_from_one() {
        let registry = MachineRegistry::default();
        let first = registry.register(mac(1), None).await.unwrap();
        let second = registry.register(mac(2), None).await.unwrap();
        assert_eq!(first.id().get().get(), 1, "first id is 1");
        assert_eq!(second.id().get().get(), 2, "ids increase by one");
    }

    #[tokio::test]
    async fn duplicate_mac_is_rejected_without_consuming_an_id() {
        let registry = MachineRegistry::default();
        registry.register(mac(1), None).await.unwrap();
        let error = registry.register(mac(1), None).await.unwrap_err();
        assert_eq!(error, RegistryError::DuplicateMac(mac(1)), "duplicate MAC");
        let next = registry.register(mac(2), None).await.unwrap();
        assert_eq!(next.id().get().get(), 2, "the rejected request used no id");
    }

    #[tokio::test]
    async fn exhausted_ids_are_reported_not_wrapped() {
        let registry = MachineRegistry::default();
        registry.inner.write().await.next_id = Some(NonZeroU64::MAX);
        let last = registry.register(mac(1), None).await.unwrap();
        assert_eq!(last.id().get(), NonZeroU64::MAX, "the largest id is usable");
        let error = registry.register(mac(2), None).await.unwrap_err();
        assert_eq!(
            error,
            RegistryError::IdSpaceExhausted,
            "no id after the largest"
        );
    }

    #[tokio::test]
    async fn rejected_event_leaves_stored_machine_unchanged() {
        let registry = MachineRegistry::default();
        let id = registry.register(mac(1), None).await.unwrap().id();
        let error = registry.apply(id, MachineEvent::Deploy).await.unwrap_err();
        assert!(
            matches!(error, RegistryError::Transition(_)),
            "deploying a new machine is a transition error, got {error:?}"
        );
        let stored = registry.get(id).await.unwrap();
        assert_eq!(stored.state(), MachineState::New, "state is unchanged");
    }
}
