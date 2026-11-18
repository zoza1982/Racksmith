//! Spec-conformance and property tests for the machine lifecycle.
//!
//! - `apply_matches_spec_table` pins `MachineState::apply` to the transition table in the
//!   project spec, cell by cell, for all 8 x 11 `(state, event)` pairs.
//! - `ready_is_reachable_from_every_state` checks invariant I3 by breadth-first search.
//! - `random_event_sequences_preserve_invariants` drives a `Machine` with random events and checks
//!   invariants I1, I2 and I4 against the recorded history.

use serde as _;
use serde_json as _;
use thiserror as _;

#[cfg(test)]
mod tests {
    use std::collections::{BTreeSet, VecDeque};
    use std::num::NonZeroU64;

    use proptest::collection::vec;
    use proptest::prelude::*;
    use proptest::sample::select;
    use racksmith_domain::{MacAddress, Machine, MachineEvent, MachineId, MachineState};

    type S = MachineState;

    const NO: Option<S> = None;
    const COMMISSIONING: Option<S> = Some(S::Commissioning);
    const READY: Option<S> = Some(S::Ready);
    const ALLOCATED: Option<S> = Some(S::Allocated);
    const DEPLOYING: Option<S> = Some(S::Deploying);
    const DEPLOYED: Option<S> = Some(S::Deployed);
    const RELEASING: Option<S> = Some(S::Releasing);
    const BROKEN: Option<S> = Some(S::Broken);

    /// The spec's transition table. Rows follow `MachineState::ALL` and columns follow
    /// `MachineEvent::ALL` (start commissioning, commissioning succeeded, commissioning failed,
    /// allocate, deploy, deployment succeeded, deployment failed, release, release completed,
    /// mark broken, mark fixed). `NO` marks a rejected event.
    const SPEC: [(S, [Option<S>; 11]); 8] = [
        (
            S::New,
            [COMMISSIONING, NO, NO, NO, NO, NO, NO, NO, NO, BROKEN, NO],
        ),
        (
            S::Commissioning,
            [NO, READY, BROKEN, NO, NO, NO, NO, NO, NO, BROKEN, NO],
        ),
        (
            S::Ready,
            [
                COMMISSIONING,
                NO,
                NO,
                ALLOCATED,
                NO,
                NO,
                NO,
                NO,
                NO,
                BROKEN,
                NO,
            ],
        ),
        (
            S::Allocated,
            [NO, NO, NO, NO, DEPLOYING, NO, NO, RELEASING, NO, BROKEN, NO],
        ),
        (
            S::Deploying,
            [NO, NO, NO, NO, NO, DEPLOYED, BROKEN, NO, NO, BROKEN, NO],
        ),
        (
            S::Deployed,
            [NO, NO, NO, NO, NO, NO, NO, RELEASING, NO, BROKEN, NO],
        ),
        (
            S::Releasing,
            [NO, NO, NO, NO, NO, NO, NO, NO, READY, BROKEN, NO],
        ),
        (
            S::Broken,
            [COMMISSIONING, NO, NO, NO, NO, NO, NO, NO, NO, NO, READY],
        ),
    ];

    #[test]
    fn apply_matches_spec_table() {
        assert_eq!(
            SPEC.map(|(state, _)| state),
            MachineState::ALL,
            "spec rows follow MachineState::ALL"
        );
        for (state, row) in SPEC {
            for (event, expected) in MachineEvent::ALL.into_iter().zip(row) {
                match (state.apply(event), expected) {
                    (Ok(actual), Some(expected)) => {
                        assert_eq!(actual, expected, "{state:?} --{event:?}-->");
                    }
                    (Err(error), None) => {
                        assert_eq!((error.from, error.event), (state, event), "error payload");
                    }
                    (actual, expected) => {
                        panic!("{state:?} --{event:?}--> gave {actual:?}, spec says {expected:?}")
                    }
                }
            }
        }
    }

    /// Returns every state reachable from `start` (including `start`) by valid events.
    fn reachable_from(start: MachineState) -> BTreeSet<MachineState> {
        let mut seen = BTreeSet::from([start]);
        let mut queue = VecDeque::from([start]);
        while let Some(state) = queue.pop_front() {
            for next in MachineEvent::ALL
                .into_iter()
                .filter_map(|e| state.apply(e).ok())
            {
                if seen.insert(next) {
                    queue.push_back(next);
                }
            }
        }
        seen
    }

    #[test]
    fn ready_is_reachable_from_every_state() {
        for state in MachineState::ALL {
            assert!(
                reachable_from(state).contains(&MachineState::Ready),
                "I3: Ready must be reachable from {state:?}"
            );
        }
    }

    fn new_machine() -> Machine {
        Machine::new(
            MachineId::new(NonZeroU64::MIN),
            MacAddress::new([0x52, 0x54, 0, 0, 0, 1]),
        )
    }

    fn events() -> impl Strategy<Value = Vec<MachineEvent>> {
        vec(select(MachineEvent::ALL.to_vec()), 0..64)
    }

    proptest! {
        #[test]
        fn random_event_sequences_preserve_invariants(events in events()) {
            let mut machine = new_machine();
            // I2 bookkeeping: whether `Allocated` was entered since the machine was last `Ready`.
            let mut allocated_since_ready = false;

            for event in events {
                let before = machine.state();
                match machine.handle(event) {
                    Ok(after) => {
                        prop_assert_eq!(after, machine.state());
                        if after == MachineState::Deployed {
                            prop_assert_eq!(before, MachineState::Deploying, "I1");
                        }
                        match after {
                            MachineState::Ready => allocated_since_ready = false,
                            MachineState::Allocated => allocated_since_ready = true,
                            MachineState::Deploying | MachineState::Deployed => {
                                prop_assert!(allocated_since_ready, "I2: {:?} without Allocated", after);
                            }
                            MachineState::New
                            | MachineState::Commissioning
                            | MachineState::Releasing
                            | MachineState::Broken => {}
                        }
                    }
                    Err(error) => {
                        prop_assert_eq!(machine.state(), before, "I4: state unchanged on error");
                        prop_assert_eq!((error.from, error.event), (before, event));
                    }
                }
            }
        }
    }
}
