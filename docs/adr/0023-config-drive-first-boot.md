# 0023. Deliver first-boot configuration on an end-of-disk config drive

- Status: Proposed
- Date: 2026-11-18

## Context

R19 requires cloud-init (NoCloud), Ignition (Flatcar, Talos) and SSH keys. The agent must not ship GPL
filesystem tools or mount and edit guest filesystems on the hot path, and the five recipes must not
each need their own injection trick. The control plane also needs an OS-ready signal to end the
deploy ([system design §10.6](../design/system-design.md#106-first-boot-configuration-and-the-os-ready-signal-r19)).

Options considered: **a metadata service** reached over the network by the booted OS; **per-OS
injection** (a pre-allocated Ignition file extent in Flatcar images, writing Talos's META partition);
**a config-drive partition** that cloud-init and Ignition already read.

## Decision

- After relocating the backup GPT, the agent appends a 16 MiB config-drive partition at the end of the
  disk and raw-writes a vfat image built in RAM with the pure-Rust `fatfs` crate (without default
  features, so it does not pull the unmaintained `chrono`; timestamps are set explicitly). Nothing is
  mounted.
  - `NOCLOUD` (label `CIDATA`, `meta-data`, `user-data`, optional `network-config`) for Ubuntu,
    Debian, Rocky and Talos (nocloud platform image).
  - `CONFIGDRIVE2` (label `config-2`, `openstack/latest/user_data` holding Ignition JSON) for
    Flatcar's OpenStack image.
- The OS's `growpart` grows the root partition into the space between the image and the config drive.
- **First-boot configuration is a resource:** `FirstBootConfig { format: CLOUD_CONFIG | IGNITION |
  TALOS_MACHINE_CONFIG, payload }`, stored at `fbc/<id>` with the payload AEAD-encrypted, served by
  `FirstBootConfigService` and the `racksmith firstboot` commands, and referenced by
  `Assignment.first_boot_config`.
- **Composition per format:** cloud-config becomes MIME multipart with RackSmith's part merged after
  the user's (a user-defined `phone_home` is kept and RackSmith's moves to `runcmd`); Ignition gets a
  RackSmith config that merges the user's config through `ignition.config.merge` and adds the
  phone-home unit and `core`'s SSH keys; a Talos machine config is written unchanged, and a deploy
  that sets SSH keys for Talos is rejected because Talos has no SSH.
- **OS-ready:** recipes post to `http://<ready address>/ready/<token>` (cloud-init or a systemd unit
  in the Ignition config) with a 256-bit single-use token bound to the machine and the deploy
  operation, which can only complete that deploy. The handler commits the signal before it answers
  200 and accepts a late or replayed post while the operation is open. The ready address and the
  TCP-probe target for recipes without a hook (Talos) are defined in
  [ADR 0033](0033-provisioning-network-boundary-and-os-ready-path.md). The signal used is recorded with
  the timings.

## Consequences

- One code path serves every recipe, and the agent needs no filesystem tools.
- Recipes must pick an image variant that reads a config drive (for example the Talos nocloud and
  Flatcar OpenStack images); recipe CI tests this nightly.
- The phone-home endpoint is plain HTTP, acceptable because the token can only move one deploy to
  `deployed`.
