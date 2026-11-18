-------------------------- MODULE MachineLifecycle --------------------------
(***************************************************************************)
(* The RackSmith machine lifecycle, model-checked by TLC.                  *)
(*                                                                         *)
(* This module mirrors `MachineState::apply` in                            *)
(* crates/racksmith-domain/src/lifecycle.rs. The transition table          *)
(* `Transitions` below is generated from the Rust `TRANSITIONS` table by   *)
(* `cargo xtask docs --fix`, and `cargo xtask docs` fails in CI if it is   *)
(* stale, so the model always checks the table the code implements.        *)
(*                                                                         *)
(* Each action corresponds to exactly one `MachineEvent` variant:          *)
(*                                                                         *)
(*   TLA+ action                Rust `MachineEvent`                        *)
(*   -------------------------  --------------------------------------     *)
(*   StartCommissioning(m)      MachineEvent::StartCommissioning           *)
(*   CommissioningSucceeded(m)  MachineEvent::CommissioningSucceeded       *)
(*   CommissioningFailed(m)     MachineEvent::CommissioningFailed          *)
(*   Allocate(m)                MachineEvent::Allocate                     *)
(*   Deploy(m)                  MachineEvent::Deploy                       *)
(*   DeploymentSucceeded(m)     MachineEvent::DeploymentSucceeded          *)
(*   DeploymentFailed(m)        MachineEvent::DeploymentFailed             *)
(*   Release(m)                 MachineEvent::Release                      *)
(*   ReleaseCompleted(m)        MachineEvent::ReleaseCompleted             *)
(*   MarkBroken(m)              MachineEvent::MarkBroken                   *)
(*   MarkFixed(m)               MachineEvent::MarkFixed                    *)
(*                                                                         *)
(* A rejected event (a `TransitionError` in Rust) changes nothing, so it   *)
(* is a stuttering step and needs no action of its own.                    *)
(***************************************************************************)
EXTENDS FiniteSets

CONSTANT Machines  \* The machines under management, as model values.

ASSUME MachinesNonEmpty == Machines # {} /\ IsFiniteSet(Machines)

States == {"New", "Commissioning", "Ready", "Allocated",
           "Deploying", "Deployed", "Releasing", "Broken"}

Events == {"StartCommissioning", "CommissioningSucceeded", "CommissioningFailed",
           "Allocate", "Deploy", "DeploymentSucceeded", "DeploymentFailed",
           "Release", "ReleaseCompleted", "MarkBroken", "MarkFixed"}

(***************************************************************************)
(* The transition table, as <<from, event, to>> triples. Pairs absent from *)
(* this set are rejected. Generated; do not edit by hand.                  *)
(***************************************************************************)
\* BEGIN GENERATED: transitions
Transitions ==
    { <<"New",           "StartCommissioning",     "Commissioning">>,
      <<"Ready",         "StartCommissioning",     "Commissioning">>,
      <<"Broken",        "StartCommissioning",     "Commissioning">>,
      <<"Commissioning", "CommissioningSucceeded", "Ready">>,
      <<"Commissioning", "CommissioningFailed",    "Broken">>,
      <<"Ready",         "Allocate",               "Allocated">>,
      <<"Allocated",     "Deploy",                 "Deploying">>,
      <<"Deploying",     "DeploymentSucceeded",    "Deployed">>,
      <<"Deploying",     "DeploymentFailed",       "Broken">>,
      <<"Allocated",     "Release",                "Releasing">>,
      <<"Deployed",      "Release",                "Releasing">>,
      <<"Releasing",     "ReleaseCompleted",       "Ready">>,
      <<"New",           "MarkBroken",             "Broken">>,
      <<"Commissioning", "MarkBroken",             "Broken">>,
      <<"Ready",         "MarkBroken",             "Broken">>,
      <<"Allocated",     "MarkBroken",             "Broken">>,
      <<"Deploying",     "MarkBroken",             "Broken">>,
      <<"Deployed",      "MarkBroken",             "Broken">>,
      <<"Releasing",     "MarkBroken",             "Broken">>,
      <<"Broken",        "MarkFixed",              "Ready">> }
\* END GENERATED: transitions

(***************************************************************************)
(* Static checks on the table itself, evaluated once by TLC before it      *)
(* explores any behavior.                                                  *)
(***************************************************************************)

\* The table only mentions known states and events.
ASSUME TableWellTyped == Transitions \subseteq States \X Events \X States

\* At most one target per (state, event): `apply` is a function.
ASSUME TableDeterministic ==
    \A t, u \in Transitions : (t[1] = u[1] /\ t[2] = u[2]) => t[3] = u[3]

\* The states reachable from `from` by following any sequence of transitions,
\* including `from` itself.
ReachableFrom(from) ==
    LET Step(seen) == seen \cup {t[3] : t \in {t \in Transitions : t[1] \in seen}}
        RECURSIVE Closure(_)
        Closure(seen) == IF Step(seen) = seen THEN seen ELSE Closure(Step(seen))
    IN  Closure({from})

\* I3: Ready is reachable from every state, so no state is a trap.
ASSUME ReadyReachableFromEveryState ==
    \A s \in States : "Ready" \in ReachableFrom(s)

-----------------------------------------------------------------------------

VARIABLES
    state,               \* state[m]: the current lifecycle state of m.
    previous,            \* previous[m]: the state m left on its last transition.
    allocatedSinceReady  \* allocatedSinceReady[m]: Allocate fired since m last entered Ready.

vars == <<state, previous, allocatedSinceReady>>

\* `previous` holds this sentinel until a machine's first transition.
NoState == "NoState"

TypeOK ==
    /\ state \in [Machines -> States]
    /\ previous \in [Machines -> States \cup {NoState}]
    /\ allocatedSinceReady \in [Machines -> BOOLEAN]

Init ==
    /\ state = [m \in Machines |-> "New"]
    /\ previous = [m \in Machines |-> NoState]
    /\ allocatedSinceReady = [m \in Machines |-> FALSE]

(***************************************************************************)
(* Machine m handles event e according to the table. Machines are          *)
(* independent, so a step changes exactly one machine.                     *)
(***************************************************************************)
Fire(m, e) ==
    \E t \in Transitions :
        /\ t[1] = state[m]
        /\ t[2] = e
        /\ state' = [state EXCEPT ![m] = t[3]]
        /\ previous' = [previous EXCEPT ![m] = t[1]]
        /\ allocatedSinceReady' =
               [allocatedSinceReady EXCEPT ![m] =
                    CASE e = "Allocate" -> TRUE
                      [] t[3] = "Ready" -> FALSE
                      [] OTHER          -> @]

StartCommissioning(m)     == Fire(m, "StartCommissioning")
CommissioningSucceeded(m) == Fire(m, "CommissioningSucceeded")
CommissioningFailed(m)    == Fire(m, "CommissioningFailed")
Allocate(m)               == Fire(m, "Allocate")
Deploy(m)                 == Fire(m, "Deploy")
DeploymentSucceeded(m)    == Fire(m, "DeploymentSucceeded")
DeploymentFailed(m)       == Fire(m, "DeploymentFailed")
Release(m)                == Fire(m, "Release")
ReleaseCompleted(m)       == Fire(m, "ReleaseCompleted")
MarkBroken(m)             == Fire(m, "MarkBroken")
MarkFixed(m)              == Fire(m, "MarkFixed")

Next ==
    \E m \in Machines :
        \/ StartCommissioning(m)
        \/ CommissioningSucceeded(m)
        \/ CommissioningFailed(m)
        \/ Allocate(m)
        \/ Deploy(m)
        \/ DeploymentSucceeded(m)
        \/ DeploymentFailed(m)
        \/ Release(m)
        \/ ReleaseCompleted(m)
        \/ MarkBroken(m)
        \/ MarkFixed(m)

(***************************************************************************)
(* Fairness: the environment assumptions under which I3 holds as a         *)
(* liveness property, stated per machine.                                  *)
(*                                                                         *)
(*  - Commissioning terminates: a machine does not stay in Commissioning   *)
(*    forever; it eventually succeeds or fails (weak fairness).            *)
(*  - Operators eventually repair: a machine that is Broken infinitely     *)
(*    often is eventually marked fixed (strong fairness). Strong, not      *)
(*    weak, because commissioning may keep failing, so MarkFixed is        *)
(*    enabled only intermittently on a Broken -> Commissioning -> Broken   *)
(*    loop.                                                                *)
(***************************************************************************)
Fairness ==
    \A m \in Machines :
        /\ WF_vars(CommissioningSucceeded(m) \/ CommissioningFailed(m))
        /\ SF_vars(MarkFixed(m))

Spec == Init /\ [][Next]_vars /\ Fairness

-----------------------------------------------------------------------------

\* I1: Deployed is only ever entered from Deploying.
DeployedOnlyFromDeploying ==
    \A m \in Machines : state[m] = "Deployed" => previous[m] = "Deploying"

\* I2: Deploying or Deployed only if Allocated since the machine was last Ready.
DeployRequiresAllocation ==
    \A m \in Machines :
        state[m] \in {"Deploying", "Deployed"} => allocatedSinceReady[m]

\* I3 as liveness: every Broken machine eventually becomes Ready again.
BrokenEventuallyReady ==
    \A m \in Machines : state[m] = "Broken" ~> state[m] = "Ready"

=============================================================================
