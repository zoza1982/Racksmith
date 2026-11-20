//! The machine lifecycle state machine.
//!
//! [`MachineState::apply`] is the single source of truth for which events are valid in which
//! state. [`TRANSITIONS`] restates the valid transitions as data, and a test keeps the two in
//! agreement. `cargo xtask docs` generates both the table in `docs/lifecycle.md` and the
//! `Transitions` set of the TLA+ model (`specs/tla/MachineLifecycle.tla`) from [`TRANSITIONS`],
//! and fails CI if either is stale.

use std::fmt;

use serde::{Deserialize, Serialize};

/// Lifecycle state of a machine.
///
/// Serialized as a `snake_case` string, for example `"commissioning"`. It is output only: a
/// state is reached by applying events, never read from a client, so it does not implement
/// `Deserialize`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[cfg_attr(kani, derive(kani::Arbitrary))]
#[serde(rename_all = "snake_case")]
pub enum MachineState {
    /// Discovered (for example by PXE boot) but never commissioned.
    New,
    /// Hardware inventory and tests are running.
    Commissioning,
    /// Commissioned and available for allocation.
    Ready,
    /// Reserved by a user but not yet deployed.
    Allocated,
    /// An operating system is being installed.
    Deploying,
    /// Running the deployed operating system.
    Deployed,
    /// Being wiped and returned to the pool.
    Releasing,
    /// Failed or taken out of service; needs repair or re-commissioning.
    Broken,
}

impl MachineState {
    /// Every state, in declaration order.
    pub const ALL: [Self; 8] = [
        Self::New,
        Self::Commissioning,
        Self::Ready,
        Self::Allocated,
        Self::Deploying,
        Self::Deployed,
        Self::Releasing,
        Self::Broken,
    ];

    /// Returns the `snake_case` name used on the wire and in `Display` output.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::New => "new",
            Self::Commissioning => "commissioning",
            Self::Ready => "ready",
            Self::Allocated => "allocated",
            Self::Deploying => "deploying",
            Self::Deployed => "deployed",
            Self::Releasing => "releasing",
            Self::Broken => "broken",
        }
    }

    /// Returns the state reached by applying `event` in this state.
    ///
    /// The match below is exhaustive over every `(state, event)` pair with no wildcard arm, so
    /// adding a state or an event fails to compile until each new pair is classified.
    ///
    /// # Errors
    ///
    /// Returns [`TransitionError`] if `event` is not valid in this state.
    #[expect(
        clippy::too_many_lines,
        reason = "one exhaustive (state, event) match is the reviewable source of truth; \
                  splitting it would hide how each pair is classified"
    )]
    pub const fn apply(self, event: MachineEvent) -> Result<Self, TransitionError> {
        use MachineEvent as E;
        use MachineState as S;

        let next = match (self, event) {
            // Valid transitions, grouped by target state.
            (S::New | S::Ready | S::Broken, E::StartCommissioning) => S::Commissioning,
            (S::Commissioning, E::CommissioningSucceeded)
            | (S::Releasing, E::ReleaseCompleted)
            | (S::Broken, E::MarkFixed) => S::Ready,
            (S::Ready, E::Allocate) => S::Allocated,
            (S::Allocated, E::Deploy) => S::Deploying,
            (S::Deploying, E::DeploymentSucceeded) => S::Deployed,
            (S::Allocated | S::Deployed, E::Release) => S::Releasing,
            (S::Commissioning, E::CommissioningFailed)
            | (S::Deploying, E::DeploymentFailed)
            | (
                S::New
                | S::Commissioning
                | S::Ready
                | S::Allocated
                | S::Deploying
                | S::Deployed
                | S::Releasing,
                E::MarkBroken,
            ) => S::Broken,

            // Every remaining pair, grouped by event, is rejected.
            (
                S::Commissioning | S::Allocated | S::Deploying | S::Deployed | S::Releasing,
                E::StartCommissioning,
            )
            | (
                S::New
                | S::Ready
                | S::Allocated
                | S::Deploying
                | S::Deployed
                | S::Releasing
                | S::Broken,
                E::CommissioningSucceeded | E::CommissioningFailed,
            )
            | (
                S::New
                | S::Commissioning
                | S::Allocated
                | S::Deploying
                | S::Deployed
                | S::Releasing
                | S::Broken,
                E::Allocate,
            )
            | (
                S::New
                | S::Commissioning
                | S::Ready
                | S::Deploying
                | S::Deployed
                | S::Releasing
                | S::Broken,
                E::Deploy,
            )
            | (
                S::New
                | S::Commissioning
                | S::Ready
                | S::Allocated
                | S::Deployed
                | S::Releasing
                | S::Broken,
                E::DeploymentSucceeded | E::DeploymentFailed,
            )
            | (
                S::New | S::Commissioning | S::Ready | S::Deploying | S::Releasing | S::Broken,
                E::Release,
            )
            | (
                S::New
                | S::Commissioning
                | S::Ready
                | S::Allocated
                | S::Deploying
                | S::Deployed
                | S::Broken,
                E::ReleaseCompleted,
            )
            | (S::Broken, E::MarkBroken)
            | (
                S::New
                | S::Commissioning
                | S::Ready
                | S::Allocated
                | S::Deploying
                | S::Deployed
                | S::Releasing,
                E::MarkFixed,
            ) => return Err(TransitionError { from: self, event }),
        };
        Ok(next)
    }
}

impl fmt::Display for MachineState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Something that happened to a machine and may move it to another [`MachineState`].
///
/// Serialized as a `snake_case` string, for example `"start_commissioning"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(kani, derive(kani::Arbitrary))]
#[serde(rename_all = "snake_case")]
pub enum MachineEvent {
    /// Begin (or repeat) hardware commissioning.
    StartCommissioning,
    /// Commissioning finished successfully.
    CommissioningSucceeded,
    /// Commissioning found a problem.
    CommissioningFailed,
    /// Reserve the machine for a user.
    Allocate,
    /// Begin installing an operating system.
    Deploy,
    /// The operating system was installed and booted.
    DeploymentSucceeded,
    /// Installing the operating system failed.
    DeploymentFailed,
    /// Give the machine back to the pool.
    Release,
    /// Wiping and cleanup after a release finished.
    ReleaseCompleted,
    /// Take the machine out of service.
    MarkBroken,
    /// An operator repaired the machine and returns it to service.
    MarkFixed,
}

impl MachineEvent {
    /// Every event, in declaration order.
    pub const ALL: [Self; 11] = [
        Self::StartCommissioning,
        Self::CommissioningSucceeded,
        Self::CommissioningFailed,
        Self::Allocate,
        Self::Deploy,
        Self::DeploymentSucceeded,
        Self::DeploymentFailed,
        Self::Release,
        Self::ReleaseCompleted,
        Self::MarkBroken,
        Self::MarkFixed,
    ];

    /// Returns the `snake_case` name used on the wire and in `Display` output.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::StartCommissioning => "start_commissioning",
            Self::CommissioningSucceeded => "commissioning_succeeded",
            Self::CommissioningFailed => "commissioning_failed",
            Self::Allocate => "allocate",
            Self::Deploy => "deploy",
            Self::DeploymentSucceeded => "deployment_succeeded",
            Self::DeploymentFailed => "deployment_failed",
            Self::Release => "release",
            Self::ReleaseCompleted => "release_completed",
            Self::MarkBroken => "mark_broken",
            Self::MarkFixed => "mark_fixed",
        }
    }
}

impl fmt::Display for MachineEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// An event that is not valid in the machine's current state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("event `{event}` is not valid in state `{from}`")]
pub struct TransitionError {
    /// The state the machine was (and still is) in.
    pub from: MachineState,
    /// The rejected event.
    pub event: MachineEvent,
}

/// Every valid transition as `(from, event, to)`, grouped by event in declaration order.
///
/// This is data derived from [`MachineState::apply`], which remains the source of truth; a test
/// asserts that the two agree on all `(state, event)` pairs. The docs table and the TLA+ model's
/// transition set are generated from it.
pub const TRANSITIONS: &[(MachineState, MachineEvent, MachineState)] = {
    use MachineEvent as E;
    use MachineState as S;
    &[
        (S::New, E::StartCommissioning, S::Commissioning),
        (S::Ready, E::StartCommissioning, S::Commissioning),
        (S::Broken, E::StartCommissioning, S::Commissioning),
        (S::Commissioning, E::CommissioningSucceeded, S::Ready),
        (S::Commissioning, E::CommissioningFailed, S::Broken),
        (S::Ready, E::Allocate, S::Allocated),
        (S::Allocated, E::Deploy, S::Deploying),
        (S::Deploying, E::DeploymentSucceeded, S::Deployed),
        (S::Deploying, E::DeploymentFailed, S::Broken),
        (S::Allocated, E::Release, S::Releasing),
        (S::Deployed, E::Release, S::Releasing),
        (S::Releasing, E::ReleaseCompleted, S::Ready),
        (S::New, E::MarkBroken, S::Broken),
        (S::Commissioning, E::MarkBroken, S::Broken),
        (S::Ready, E::MarkBroken, S::Broken),
        (S::Allocated, E::MarkBroken, S::Broken),
        (S::Deploying, E::MarkBroken, S::Broken),
        (S::Deployed, E::MarkBroken, S::Broken),
        (S::Releasing, E::MarkBroken, S::Broken),
        (S::Broken, E::MarkFixed, S::Ready),
    ]
};

#[cfg(test)]
mod tests {
    use super::{MachineEvent, MachineState, TRANSITIONS, TransitionError};

    /// Position of each state in [`MachineState::ALL`]; the exhaustive match makes this test
    /// fail to compile when a variant is added without updating `ALL`.
    const fn state_index(state: MachineState) -> usize {
        match state {
            MachineState::New => 0,
            MachineState::Commissioning => 1,
            MachineState::Ready => 2,
            MachineState::Allocated => 3,
            MachineState::Deploying => 4,
            MachineState::Deployed => 5,
            MachineState::Releasing => 6,
            MachineState::Broken => 7,
        }
    }

    /// Position of each event in [`MachineEvent::ALL`]; see [`state_index`].
    const fn event_index(event: MachineEvent) -> usize {
        match event {
            MachineEvent::StartCommissioning => 0,
            MachineEvent::CommissioningSucceeded => 1,
            MachineEvent::CommissioningFailed => 2,
            MachineEvent::Allocate => 3,
            MachineEvent::Deploy => 4,
            MachineEvent::DeploymentSucceeded => 5,
            MachineEvent::DeploymentFailed => 6,
            MachineEvent::Release => 7,
            MachineEvent::ReleaseCompleted => 8,
            MachineEvent::MarkBroken => 9,
            MachineEvent::MarkFixed => 10,
        }
    }

    #[test]
    fn all_lists_every_state_once_in_order() {
        for (position, state) in MachineState::ALL.into_iter().enumerate() {
            assert_eq!(state_index(state), position, "{state:?}");
        }
    }

    #[test]
    fn all_lists_every_event_once_in_order() {
        for (position, event) in MachineEvent::ALL.into_iter().enumerate() {
            assert_eq!(event_index(event), position, "{event:?}");
        }
    }

    #[test]
    fn transitions_table_agrees_with_apply() {
        for state in MachineState::ALL {
            for event in MachineEvent::ALL {
                let listed: Vec<MachineState> = TRANSITIONS
                    .iter()
                    .filter(|&&(from, on, _)| from == state && on == event)
                    .map(|&(_, _, to)| to)
                    .collect();
                let expected = state.apply(event).ok().into_iter().collect::<Vec<_>>();
                assert_eq!(listed, expected, "{state:?} --{event:?}-->");
            }
        }
    }

    #[test]
    fn errors_report_the_rejected_pair() {
        let error = MachineState::New
            .apply(MachineEvent::Deploy)
            .expect_err("cannot deploy a new machine");
        assert_eq!(
            error,
            TransitionError {
                from: MachineState::New,
                event: MachineEvent::Deploy,
            },
            "error carries the rejected pair"
        );
        assert_eq!(
            error.to_string(),
            "event `deploy` is not valid in state `new`"
        );
    }

    #[test]
    fn display_matches_serde_names() {
        for state in MachineState::ALL {
            let json = serde_json::to_string(&state).expect("serialization is infallible");
            assert_eq!(json, format!("\"{state}\""), "{state:?}");
        }
        for event in MachineEvent::ALL {
            let json = serde_json::to_string(&event).expect("serialization is infallible");
            assert_eq!(json, format!("\"{event}\""), "{event:?}");
        }
    }

    #[test]
    fn deserializes_snake_case_names() {
        let event: MachineEvent =
            serde_json::from_str(r#""start_commissioning""#).expect("known event");
        assert_eq!(event, MachineEvent::StartCommissioning, "snake_case event");
        assert!(
            serde_json::from_str::<MachineEvent>(r#""StartCommissioning""#).is_err(),
            "PascalCase is not accepted"
        );
    }
}

/// Kani bounded-model-checking proofs (run with `cargo kani -p racksmith-domain`).
///
/// States and events are chosen with `kani::any()`, which the `kani::Arbitrary` derives above
/// make range over every variant.
#[cfg(kani)]
mod verification {
    use std::num::NonZeroU64;

    use super::{MachineEvent, MachineState};
    use crate::{MacAddress, Machine, MachineId};

    /// Events per sequence in [`handle_is_atomic`]: enough to reach every state from `New`
    /// (`New` to `Releasing` takes six events) and then apply one more event there.
    const SEQUENCE_LEN: usize = 7;

    /// I4: `apply` never panics, and a rejection reports exactly the pair it was given.
    #[kani::proof]
    fn apply_never_panics() {
        let state: MachineState = kani::any();
        let event: MachineEvent = kani::any();
        if let Err(error) = state.apply(event) {
            assert!(
                error.from == state && error.event == event,
                "a rejection must report the pair it was given"
            );
        }
    }

    /// I4: over every sequence of [`SEQUENCE_LEN`] events, `handle` agrees with `apply` and is
    /// atomic: a rejected event leaves the whole machine unchanged, and an accepted one changes
    /// only its state.
    #[kani::proof]
    #[kani::unwind(8)]
    fn handle_is_atomic() {
        let mut machine = Machine::new(MachineId::new(NonZeroU64::MIN), MacAddress::new([0; 6]));
        for _ in 0..SEQUENCE_LEN {
            let before = machine.clone();
            let event: MachineEvent = kani::any();
            match machine.handle(event) {
                Ok(next) => {
                    assert!(
                        before.state().apply(event) == Ok(next) && machine.state() == next,
                        "an accepted event must move to the state `apply` returns"
                    );
                    assert!(
                        machine.id() == before.id()
                            && machine.mac() == before.mac()
                            && machine.hostname() == before.hostname(),
                        "an accepted event must change nothing but the state"
                    );
                }
                Err(_) => assert!(machine == before, "a rejected event must change nothing"),
            }
        }
    }

    /// I1: `Deployed` is entered only from `Deploying`, via `DeploymentSucceeded`.
    #[kani::proof]
    fn deployed_only_entered_from_deploying() {
        let state: MachineState = kani::any();
        let event: MachineEvent = kani::any();
        if state.apply(event) == Ok(MachineState::Deployed) {
            assert!(
                state == MachineState::Deploying,
                "`deployed` must only be entered from `deploying`"
            );
            assert!(
                event == MachineEvent::DeploymentSucceeded,
                "`deployed` must only be entered via `deployment_succeeded`"
            );
        }
    }
}
