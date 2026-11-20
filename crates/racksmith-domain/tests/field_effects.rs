//! Field-effect tests for the domain enum that derives `Deserialize`: `MachineEvent`, the event
//! a client sends to `POST /api/v1/machines/{id}/events`.
//!
//! Every variant read from its wire name must change an observable outcome compared with any
//! other variant: here, the result of [`MachineState::apply`]. A variant that parsed but behaved
//! exactly like another would be an input that is accepted and ignored.

use serde as _;
use thiserror as _;

#[cfg(test)]
mod tests {
    use proptest::prelude::*;
    use proptest::sample::select;
    use racksmith_domain::{MachineEvent, MachineState};

    /// Deserializes an event from its JSON wire name.
    fn event_from_wire(event: MachineEvent) -> MachineEvent {
        serde_json::from_value(serde_json::json!(event.as_str())).unwrap()
    }

    /// The outcome of `event` in every state: the observable behavior of the event.
    fn event_column(event: MachineEvent) -> Vec<Option<MachineState>> {
        MachineState::ALL
            .into_iter()
            .map(|state| state.apply(event).ok())
            .collect()
    }

    proptest! {
        /// Two distinct events read from the wire lead to different results in some state.
        #[test]
        fn event_wire_name_changes_the_resulting_state(
            event in select(MachineEvent::ALL.to_vec()),
            other in select(MachineEvent::ALL.to_vec()),
        ) {
            prop_assume!(event != other);
            let (event, other) = (event_from_wire(event), event_from_wire(other));
            prop_assert_ne!(event_column(event), event_column(other));
        }
    }

    /// The proptest samples pairs; this pins every pair, so no variant can hide behind sampling.
    #[test]
    fn every_event_variant_is_distinguishable() {
        for event in MachineEvent::ALL {
            for other in MachineEvent::ALL
                .into_iter()
                .filter(|&other| other != event)
            {
                assert_ne!(
                    event_column(event_from_wire(event)),
                    event_column(event_from_wire(other)),
                    "`{event}` behaves exactly like `{other}`"
                );
            }
        }
    }
}
