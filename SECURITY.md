# Security policy

RackSmith provisions bare metal. A vulnerability here can mean a wiped disk, a hijacked boot or
leaked BMC credentials across an entire rack, so we take reports seriously and handle them
privately.

## Supported versions

RackSmith is pre-1.0 and has no releases yet. Only the `main` branch is supported; fixes land
there. Once releases begin, this section will list which release lines receive security fixes.

| Version       | Supported |
|---------------|-----------|
| `main`        | Yes       |
| anything else | No        |

## Reporting a vulnerability

**Do not open a public issue, pull request or discussion for a vulnerability.**

Report it through GitHub private vulnerability reporting:
<https://github.com/zoza1982/Racksmith/security/advisories/new>

Please include:

- the affected component (crate, endpoint, command or protocol) and commit,
- the impact you believe it has and who can trigger it (network position, credentials needed),
- steps or a proof of concept that reproduce it,
- any suggested fix or mitigation.

## What to expect

| Step                               | Target                         |
|------------------------------------|--------------------------------|
| Acknowledgement of your report     | within 3 business days         |
| Initial assessment and severity    | within 10 business days        |
| Fix for critical and high severity | within 30 days of confirmation |
| Fix for medium and low severity    | within 90 days of confirmation |

This is a volunteer-run project, so these are targets rather than guarantees; we will tell you if
we expect to miss one. We coordinate disclosure with you, publish a GitHub security advisory
(with a CVE where appropriate) once a fix is available, and credit you unless you prefer
otherwise.

## Scope

Bare-metal provisioning has a large blast radius. These areas are in scope and especially
interesting:

- **BMC credentials and power control** (IPMI, Redfish): credential storage, exposure in logs,
  API responses or error messages, and unauthorized power or boot-device changes.
- **Network boot** (DHCP, PXE, TFTP, iPXE, HTTP boot): rogue or spoofed clients, boot-file
  injection, path traversal, and parsing of untrusted packets.
- **DHCP**: lease exhaustion, option injection and spoofing that redirects machines.
- **Image deployment**: tampered or unverified images, and anything that writes to the wrong disk
  or the wrong machine.
- **Lifecycle bypass**: any way to deploy, release or wipe a machine outside the transitions in
  [docs/lifecycle.md](docs/lifecycle.md).
- **The API and CLI**: authentication and authorization, injection, denial of service.
- **Supply chain**: the build, CI workflows and dependencies (see [deny.toml](deny.toml)).

Out of scope: vulnerabilities in third-party dependencies that are already publicly known (report
those upstream; we track them with cargo-deny and Dependabot), attacks that require full control
of the host RackSmith runs on, and findings from automated scanners without a demonstrated impact.

## Security posture

The project's guardrails are described in [docs/trust-pyramid.md](docs/trust-pyramid.md). In
short: `unsafe` code is forbidden, dependencies are restricted to crates.io with an allowlist of
permissive licenses and no known advisories, CI actions are pinned by commit SHA, and the machine
lifecycle is model-checked.
