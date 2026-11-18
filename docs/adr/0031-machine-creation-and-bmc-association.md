# 0031. Create machines from BMCs, learn boot MACs on a token-bound boot, merge enrollments by identity

- Status: Proposed
- Date: 2026-11-18

## Context

The quickstart, an M1 gate, runs `init → serve → bmc add → image import → machine deploy`, but the
first design draft had no step that creates a machine or commissions it, and `deploy` needs a
`ready` machine whose root disk was resolved from a commissioning inventory. The boot lookup works
by MAC, yet IPMI reports no MACs and Redfish `EthernetInterfaces` is often empty or lists NICs that
are not on the provisioning network. A machine registered by its BMC would therefore first PXE-boot
as an unknown MAC and create a duplicate `EnrollmentRequest`, and a machine auto-enrolled over PXE
(R8) would have no BMC to power it for its next deploy
([system design §8.9](../design/system-design.md#89-machine-creation-bmc-binding-and-boot-mac-learning)).
`MachineSpec.bmc` is optional, but no behaviour was defined for a machine without one.

Options considered: **BMC registration only** (the operator creates machines separately and types
boot MACs); **explicit machine creation only**; **three creation paths with boot-MAC learning and
enrollment merge**.

## Decision

- **Three paths, one result.** `racksmith bmc add … --create-machine` reads the BMC identity and
  creates a `new` machine bound to the BMC (the quickstart path); `racksmith machine create [--bmc]
  [--boot-mac]` or `apply -f` creates one explicitly; an approved `EnrollmentRequest` creates one
  from the enrolled identity. Uniqueness is enforced with `Absent` compares on the UUID and serial
  indexes.
- **Boot-MAC learning.** A machine without `spec.boot_mac` is commissioned anyway. Inside an open
  commission operation, the first script fetch from an unowned MAC whose query carries the
  machine's UUID and serial records that MAC as the boot MAC, in the same transaction that issues
  the boot token; the usual one-tuple and conflict rules apply. Deploys require a known boot MAC.
- **Merge, not duplicate.** Approving an `EnrollmentRequest` whose UUID and serial match an
  existing machine merges the request into it (adds MACs, records the boot MAC). A UUID that matches
  one machine and a serial that matches another is refused with `IdentityConflict`.
- **BMC binding for PXE-first machines** is set by the operator, or automatically when exactly one
  registered BMC reports the same UUID and serial.
- **Machines without a BMC** can be commissioned through a manual-power path (phase
  `AwaitingManualPower`, condition `ManualPowerRequired`, advanced by a DHCP sighting of the boot
  MAC). `allocate` is rejected without a BMC (condition `NoPowerDriver`), because deploy, release and
  hand-off retries all need a one-time override and a power cycle. `decide()` has explicit rules for
  `bmc = None`, so its rule table stays total.
- The quickstart becomes `init → serve → bmc add --create-machine → machine commission → image
  import → machine deploy` (commissioning is automatic under `init --lab`).

## Consequences

- The quickstart is complete and testable, and boot MACs need not be typed or trusted from BMCs.
- Enrollment never produces two machines for one box, and auto-enrolled machines can get a BMC.
- A machine without a BMC is a commissioning-only inventory record until a BMC is bound, which is
  stated rather than discovered at the first deploy.
