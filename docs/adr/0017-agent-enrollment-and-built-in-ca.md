# 0017. Enroll agents with HMAC boot tokens, BMC corroboration and short-lived certificates

- Status: Proposed
- Date: 2026-11-18

## Context

R8 requires auto-enrollment of unknown machines and R27 requires mTLS between the control plane and
agents, with the agent identity bound to the enrollment. Until Secure Boot (R29, M2), stage 1 of the
network boot is unauthenticated, and MAC addresses, SMBIOS UUIDs and serials are not secrets. Real
UEFI and iPXE stacks re-fetch after transient errors, so a strictly single-issue token would fail
real machines and cost a POST of up to 4 minutes on GPU servers
([system design §8.5, §10.3](../design/system-design.md#85-boot-decisions-and-the-boot-token)).

Options considered: **identity claims only** (MAC/UUID; Tinkerbell's known weakness);
**stored random tokens** (state per boot); **derived HMAC tokens** plus BMC corroboration and
short-lived certificates; **TPM-only identity** (needs EK infrastructure and M2 attestation work).

## Decision

We will use:

- **Boot tokens** `"v<k>." + HMAC-SHA256(boot_key_k, "rs-boot/v1" ‖ machine ‖ op ‖ attempt)`,
  recomputable, never stored; the store records only issuance (tuple and time). TTL 15 minutes. The
  key version is embedded so a rotation keeps the previous key valid for 1 hour and never breaks boots
  in flight.
- **Idempotent re-issue** of the same token to the same `(mac, ip, uuid)` tuple within 120 s.
- **Fail-closed conflicts:** a fetch from a different tuple, or an `Enroll` with an already consumed
  token, makes the reconciler emit `mark_broken` (reason `boot_token_conflict`), which supersedes the
  operation and bumps `enrollment_generation`, killing every certificate and session for the machine.
- **Plan push rule:** a plan is pushed only for an attempt whose token was issued to exactly one
  tuple, consumed exactly once, and first issued at least 5 s earlier, and only on the session created
  by the `Enroll` that consumed it.
- **Per-attempt sessions:** a token-consuming `Enroll` bumps `enrollment_generation` (killing every
  older certificate and session) and records the new session's key in the operation. Only one live
  session per machine is allowed; a second is a conflict that closes the attempt. `Renew` works only
  over the bound session, while an operation is open, and never past its deadline.
- **Corroboration:** for machines with a registered BMC, the claimed UUID, serial and MACs must match
  the BMC's inventory. Corroboration is a consistency check on non-secret claims, never a substitute
  for a token-bound boot.
- **Certificates:** an ephemeral P-256 key generated in agent RAM; a 12-hour client certificate with
  SAN `urn:racksmith:machine:<id>:gen:<generation>`, renewed over the session at 50%. Certificates
  bind `(machine, generation)`, not the operation; operation fencing is per message. Revocation is a
  generation bump checked on every stream and report, with no CRL.
- **Unknown machines** become `EnrollmentRequest` objects, not machines, under a per-network policy
  (`auto-accept` for lab installs, `require-approval` by default, `bmc-corroborated-only`, `deny`).
  A tokenless `Enroll` is accepted only from a MAC that boot services sighted and served an
  `EnrollUnknown` script, within per-MAC, per-IP, per-network and global caps. Its agent holds a
  1-hour enrollment-scoped certificate that permits only inventory upload. Only under `auto-accept`
  does it receive a machine certificate over the same session; otherwise the accepted machine is
  commissioned through a control-plane-initiated, token-bound boot, and power phases are never
  skipped for a tokenless session. `bmc-corroborated-only` is documented as equivalent to
  `auto-accept` for the enrolling session until TPM EK binding (M2).
- **The CA:** separate server and client hierarchies. A P-256 server root (10 years, encrypted under
  the KEK, used only at `init` and issuing-CA rotation) signs only a P-256 server issuing CA (1 year)
  for server certificates (90 days, serverAuth); it is the root that iPXE, agents (`rs.ca`) and the CLI
  pin. A separate P-256 client CA (1 year) issues agent (12 hours) and enrollment (1 hour)
  certificates (clientAuth) and is trusted only by the agent gateway. Every certificate is built from
  a fixed template: the CSR contributes only its public key, and subject, SANs, EKU, basicConstraints
  and extensions come from the template. Verifiers check EKU and SAN. Hostile CSRs are fuzzed. Built
  with rcgen on rustls with the ring provider. There is no parking in M1:
  agents power off after commissioning or a wipe. TPM EK capture is M2, with proto fields reserved.

## Consequences

- No per-boot secret is stored; a crash or restore cannot leak or replay tokens, and a rotation does
  not disturb boots in flight.
- Transient re-fetches no longer fail real machines, while a second claimant closes the attempt.
- **Residual risk, documented:** an on-link attacker who clones the tuple and fetches first can enroll
  and receive the plan, including user-data, before detection. Until Secure Boot (M2), user-data is
  exposed to such an attacker, which in M1 can be any deployed OS on the provisioning network
  ([ADR 0033](0033-provisioning-network-boundary-and-os-ready-path.md)); DHCP snooping is recommended.
- An earlier certificate (from commissioning, or from a race an attacker once won) can no longer
  receive a later deploy plan, because the deploy's token-consuming `Enroll` bumps the generation and
  binds the plan to its own session.
- A boot-token conflict leaves a tainted machine `broken` until an operator releases it.
