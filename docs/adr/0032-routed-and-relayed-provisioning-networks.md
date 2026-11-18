# 0032. Support routed provisioning subnets behind DHCP relays

- Status: Proposed
- Date: 2026-11-18

## Context

RackSmith targets hyperscale and GPU clusters ([PRD](../prd.md)), which run L3 leaf-spine fabrics
where each rack is its own subnet behind DHCP relays. The first design draft assumed the control
plane sits on the same L2 segment as every machine: listeners bind per provisioning interface, a
`BootNetwork` is defined per interface, and ProxyOffers are answered on the local segment. It did not
say how full mode handles relayed, `giaddr`-tagged DHCPDISCOVER packets, whether proxy mode works
behind a relay, or which control-plane address goes into bootfile URLs, `rs.cp` and the OS-ready
URL for machines on remote subnets. The [ADR 0008](0008-pluggable-network-services.md) default (proxyDHCP)
was unproven for the main target topology, and the lab's single bridge never exercised it
([system design §8.10](../design/system-design.md#810-routed-and-relayed-provisioning-networks)).

Options considered: **single L2 only** (document it as a limitation); **relayed subnets** with
`giaddr` subnet selection and a per-subnet advertised address.

## Decision

- A `BootNetwork` is either `local { interface }` or `relayed { subnets[], relay_addresses[] }`.
  The DHCP listener also binds a routed socket on the provisioning-facing address and accepts
  unicast DHCP only from configured relay addresses.
- **Full mode:** the `Subnet` is selected by the prefix that contains `giaddr` (option 82 is
  recorded and may refine the choice in M2); replies go to the relay; a `giaddr` that matches no
  subnet is dropped and counted.
- **Proxy mode:** supported when the relay forwards to both the site DHCP and RackSmith; RackSmith
  sends its ProxyOffer to the relay and receives the port-4011 follow-up by unicast. Relays with a
  single helper use `external` mode.
- Each relayed subnet has a `cp_address` (default: the control-plane address that routes to
  `giaddr`) used in bootfile URLs, `rs.cp`, the OS-ready URL and `EnrollResponse.cp_endpoints`.
  `racksmith doctor` checks that each `cp_address` is bound and that relays are reachable.
- The nightly lab adds a relayed topology (a second bridge behind a router VM running a DHCP relay)
  in both full and proxy mode.

## Consequences

- The default DHCP mode is tested on the topology most target sites use.
- Boot services gain a second socket kind and a subnet-selection rule, both in the sans-IO
  `racksmith-netboot-core` and fuzzed.
- Sites must configure relays (two helper addresses for proxy mode), which the docs describe.
