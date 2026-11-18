# 0021. Encrypt secrets at rest with envelope AEAD under a pluggable KEK

- Status: Proposed
- Date: 2026-11-18

## Context

R14 requires BMC credentials to be encrypted at rest, never logged and redacted in every API
response. The control plane also holds CA keys, the catalog signing key, boot-token keys and
first-boot user-data. It must stay one binary with no external secret manager required
([system design §11.5](../design/system-design.md#115-credentials-at-rest-r14-and-redaction)).

Options considered: **file permissions only**; **whole-database encryption**; **field-level AEAD
under a key-encryption key (KEK)** with pluggable KEK sources; **an external manager (Vault)
required**.

## Decision

- **Envelope encryption:** a KEK wraps versioned data-encryption keys (DEKs, random 256-bit, stored
  wrapped in the store). Each secret field is encrypted with XChaCha20-Poly1305 and a random 192-bit
  nonce, with `AAD = "racksmith/v1|{kind}|{resource_id}|{field}|dek_v{n}"`, so ciphertexts cannot be
  swapped between records or fields.
- **`KekProvider` port, M1 implementations:** a key file (0400, outside the data directory, generated
  by `racksmith init`) and systemd credentials through `$CREDENTIALS_DIRECTORY` (TPM2 or host-key
  sealing, no extra code). Argon2id passphrases, Vault Transit, cloud KMS and PKCS#11 come in M2 or
  later behind the same port.
- **Rotation:** `racksmith admin kek rotate` re-wraps DEKs; DEK rotation re-encrypts lazily.
- **Redaction enforced by code:** `Secret<T>` in the domain has no `Serialize` or `Display` and a
  redacting `Debug`; `expose_secret` is source-banned outside the crates that must use secrets
  ([ADR 0011](0011-evolve-workspace-into-ports-core-adapters.md)); proto fields marked `sensitive` are
  input-only (`cargo xtask proto-check`); a tracing layer drops fields named like secrets.
- **Canary test:** the lab registers a BMC with a random canary password and user-data holding a second
  canary, runs commission, deploy and release, and searches logs, traces, metrics, API responses, the
  store file and a snapshot. Any hit fails the build.

## Consequences

- Snapshots and the data directory contain only ciphertext; losing the KEK makes them unreadable, so
  the docs pair snapshot shipping with KEK custody.
- The default key file is only as safe as the host; systemd-creds with TPM2 is the recommended
  production setup.
- One more key hierarchy to operate, with rotation commands and tests.
