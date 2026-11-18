//! Fuzzes the machine lifecycle with arbitrary event sequences.
//!
//! Properties checked for every scenario:
//! - `MachineState::apply` never panics, from any start state;
//! - `apply` accepts exactly the transitions listed in `TRANSITIONS`, and a rejection reports the
//!   pair it was given;
//! - `Deployed` is only ever entered from `Deploying` (invariant I1);
//! - `Machine::handle` agrees with `apply`, and a failed event leaves the machine unchanged
//!   (invariant I4).

#![no_main]

use std::num::NonZeroU64;
use std::ops::ControlFlow;

use libfuzzer_sys::arbitrary::{self, Arbitrary, Unstructured};
use libfuzzer_sys::fuzz_target;
use racksmith_domain::{
    MacAddress, Machine, MachineEvent, MachineId, MachineState, TRANSITIONS, TransitionError,
};

/// Upper bound on events per scenario; longer sequences add run time without new coverage.
const MAX_EVENTS: u32 = 256;

/// A start state and the events to apply from it.
#[derive(Debug)]
struct Scenario {
    start: MachineState,
    events: Vec<MachineEvent>,
}

impl<'a> Arbitrary<'a> for Scenario {
    /// Picks states and events by index into their `ALL` lists, so the domain crate needs no
    /// fuzzing-specific trait implementations.
    fn arbitrary(u: &mut Unstructured<'a>) -> arbitrary::Result<Self> {
        let start = *u.choose(&MachineState::ALL)?;
        let mut events = Vec::new();
        u.arbitrary_loop(None, Some(MAX_EVENTS), |u| {
            events.push(*u.choose(&MachineEvent::ALL)?);
            Ok(ControlFlow::Continue(()))
        })?;
        Ok(Self { start, events })
    }
}

fuzz_target!(|scenario: Scenario| {
    check_state_sequence(scenario.start, &scenario.events);
    check_machine_sequence(&scenario.events);
});

/// Drives a bare `MachineState` from `start`, which may be any state.
fn check_state_sequence(start: MachineState, events: &[MachineEvent]) {
    events.iter().fold(start, |state, &event| {
        checked_apply(state, event).unwrap_or(state)
    });
}

/// Drives a `Machine`, which always starts in `New`, and checks that `handle` is atomic.
fn check_machine_sequence(events: &[MachineEvent]) {
    let mut machine = Machine::new(
        MachineId::new(NonZeroU64::MIN),
        MacAddress::new([0x52, 0x54, 0x00, 0x00, 0x00, 0x01]),
    );
    for &event in events {
        let before = machine.clone();
        let expected = checked_apply(before.state(), event);
        let result = machine.handle(event);
        assert_eq!(result, expected, "handle disagrees with apply for {event}");
        match result {
            Ok(to) => assert_eq!(machine.state(), to, "handle did not move to {to}"),
            Err(_) => assert_eq!(machine, before, "failed {event} modified the machine"),
        }
    }
}

/// Applies `event` in state `from` and checks the outcome against `TRANSITIONS`.
fn checked_apply(from: MachineState, event: MachineEvent) -> Result<MachineState, TransitionError> {
    let result = from.apply(event);
    let listed = TRANSITIONS
        .iter()
        .find(|&&(state, listed_event, _)| state == from && listed_event == event)
        .map(|&(_, _, to)| to);
    assert_eq!(
        result.ok(),
        listed,
        "apply({from}, {event}) disagrees with TRANSITIONS"
    );
    if let Err(error) = result {
        assert_eq!(
            error,
            TransitionError { from, event },
            "rejection of ({from}, {event}) reports the wrong pair"
        );
    }
    if result == Ok(MachineState::Deployed) {
        assert_eq!(
            from,
            MachineState::Deploying,
            "Deployed entered from {from} via {event}"
        );
    }
    result
}
