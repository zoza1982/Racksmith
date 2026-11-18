# 0008. Make network services pluggable, with built-in defaults and proxyDHCP by default

- Status: Accepted
- Date: 2026-11-18

## Context

Network boot needs DHCP, TFTP and/or HTTP boot, and iPXE. Deployed machines also need IP
addresses and DNS. Greenfield users want this to work with zero dependencies. Hyperscale sites
already run DHCP (for example Kea), IPAM and DNS (for example NetBox, Infoblox, PowerDNS), and
will not hand those over. Options considered:

1. **Own everything.** Built-in DHCP, IPAM and DNS. Full control, but it clashes with existing
   infrastructure.
2. **Own boot only, and always integrate external IPAM.** Easy for brownfield sites, but
   greenfield users need an external IPAM before they can boot anything.
3. **Pluggable everything.** Traits for each service, with a built-in default for each and
   adapters for external systems.

## Decision

We will take option 3 ([PRD](../prd.md) R7–R10).

- **Ports** are traits in the core: `Dhcp`, `Ipam`, `Dns`, plus boot-file serving and the power
  drivers (Redfish and IPMI implement the same power port).
- **Built-in defaults in M1:** proxyDHCP, full DHCPv4 server mode, TFTP, HTTP(S) boot, iPXE
  chainloading, a built-in IPAM, and a DNS record publisher.
- **proxyDHCP is the default mode.** RackSmith answers only the PXE-specific part of DHCP and
  leaves IP addressing to the existing DHCP server, so it can be dropped into a running network
  without conflicting with it. Full DHCP server mode has to be turned on explicitly.
- **External adapters** (Kea, NetBox, RFC 2136 dynamic DNS/PowerDNS) arrive in M2. Each adapter
  must pass the same contract tests as the built-in implementation of its port.
- **Security:** every protocol parser (DHCP, TFTP) has a fuzz target. Listeners bind only to the
  interfaces they are configured for.

## Consequences

- Works out of the box on a laptop or in a lab, and fits into existing site infrastructure
  without anyone having to rip and replace.
- Every port needs a stable trait and a contract test suite, which is more design work up front.
  The port boundaries belong in the system design.
- Running DHCP and TFTP widens the attack surface. proxyDHCP by default, interface-scoped
  listeners and fuzzing are the mitigations.
- Supporting both proxyDHCP and full DHCP doubles the boot-path test matrix. The virtual lab
  ([ADR 0010](0010-virtual-lab-ci-testbed.md)) must cover both.
