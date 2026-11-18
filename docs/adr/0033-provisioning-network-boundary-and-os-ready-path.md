# 0033. Keep deployed machines on the provisioning network in M1 and drop the isolation claim

- Status: Proposed
- Date: 2026-11-18

## Context

In M1 the deployed OS uses DHCP on its boot interface, and the OS-ready phone-home posts to the
boot HTTP listener on the provisioning network. Deployed tenant OSes therefore sit on the
provisioning network, as the trust-boundary diagram already showed. The first design draft, and
ADRs [0017](0017-agent-enrollment-and-built-in-ca.md) and [0018](0018-serve-time-ipxe-trust-anchor.md),
nevertheless answered the stage-1 and cloned-tuple risks with an install prerequisite that the
provisioning VLAN "carries no tenant or user traffic", which the design itself cannot meet. Every
deployed host is an on-link party: it sees other machines' MACs and SMBIOS UUIDs in broadcast DHCP,
as a former tenant it knows serials, and it can reach the boot, enrollment and DHCP listeners. The
draft also left the TCP-probe ready signal (used for Talos) without a target address, because in
proxy mode RackSmith never learns the deployed OS's address
([system design §10.6, §11.9](../design/system-design.md#119-isolation-statement-and-install-prerequisites)).

Options considered:

1. **Require a separate tenant network in M1** (a second NIC or a VLAN flip after hand-off). Keeps
   the claim, but excludes single-NIC machines and needs switch integration that M1 does not have.
2. **Keep deployed machines on the provisioning network in M1**, drop the claim, re-rate the
   threats, and harden what an on-link deployed host can do.

## Decision

We will take option 2:

- The "no tenant traffic" prerequisite is replaced by "no traffic from parties outside the
  installation's trust domain"; M1 is a single trust domain ([PRD](../prd.md) scope, system design
  §1.5) and claims no isolation of boot and enrollment services from deployed machines. Threats T1
  and T3 are rated with any deployed OS as a possible attacker.
- What limits such an attacker in M1: plans bound to the session of the token-consuming `Enroll`, a
  generation bump on that `Enroll`, one live session per machine, separate server and client CAs
  with template-only issuance, tokenless enrollment only from DHCP-sighted MACs with per-source caps
  (ADR 0017), and DHCP snooping where available.
- **OS-ready path:** a `ready_listen` setting (default: the boot HTTP address on the provisioning
  network; may be a separate, routed address) serves only `POST /ready/<token>`. The signal is
  committed before the 200 response.
- **TCP-probe target:** the full-mode reservation if there is one; otherwise the address the agent
  reported from its boot lease, checked per recipe in the nightly lab; otherwise the deploy sets
  `ReadyProbeAddressUnknown` and fails at its deadline. Static addressing through IPAM (M2) removes
  the guess.
- Sites that need more give deployed machines a separate tenant interface or VLAN through
  `network-config` and declare `tenant_subnets`; `racksmith doctor` fails when a provisioning address
  or relayed subnet overlaps one. A VLAN flip after hand-off through switch integration is M2.

## Consequences

- The documentation stops promising an isolation the design cannot deliver, and the threat model
  matches the topology.
- User-data stays exposed to a first-fetching attacker on the provisioning network until Secure Boot
  (M2); the docs say so and advise against long-lived secrets in user-data.
- Talos deploys in proxy mode depend on the deployed OS getting its boot-time lease again, which is
  checked nightly and can be made certain with a DHCP reservation.
