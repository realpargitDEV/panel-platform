# Security Model and Threat Model

Project Host runs code the user supplies, as a privileged service, on a machine
the user cares about. That is the whole security problem in one sentence: the
product's purpose is to execute untrusted-by-default workloads, so the design
question is never "can code be run" but "what can that code reach".

Docker-specific controls live in `docs/docker.md`. This document covers the
threat model, the trust boundaries, and the controls that are not Docker's.

---

## 1. What is being defended

| Asset                     | Why it matters                                     |
| ------------------------- | -------------------------------------------------- |
| The host operating system | Container escape means total compromise            |
| The agent's database      | Password hash, session hashes, encrypted secrets   |
| The secret encryption key | Unlocks every stored project secret                |
| Project files             | The user's work                                    |
| Backups                   | Historical copies of the above                     |
| The agent's API           | Authenticated access is equivalent to host control |
| The Docker socket         | Access to it is equivalent to root on the host     |
| Other projects' data      | Cross-project isolation is a core promise          |

---

## 2. Who the attacker is

Five actors, in descending likelihood.

1. **A malicious or compromised project.** The most probable and most important.
   A dependency in a Discord bot is backdoored; the bot now runs hostile code
   inside a container that Project Host started. Assumed to happen.
2. **A malicious archive.** A ZIP crafted to escape extraction, exhaust disk, or
   plant files outside the project directory.
3. **An attacker on the LAN.** Reachable only when the user has explicitly
   enabled LAN binding. Wants to reach the agent API or a project's port.
4. **A local unprivileged user.** Another account on the same machine, trying to
   read the database, the key, or the bootstrap token.
5. **A malicious update or template.** Supply-chain: a tampered agent binary or
   an untrusted Docker template.

Explicitly **out of scope for version one**: a hostile administrator (they own
the machine), physical access with disk encryption off, and a compromised host
kernel. These are stated rather than silently ignored — a threat model that
claims to cover everything covers nothing.

---

## 3. Trust boundaries

```
  ┌─ UNTRUSTED ────────────────────────────────────────┐
  │  project container code, ZIP contents, .env files, │
  │  file names, project display names, remote peers   │
  └────────────────────┬───────────────────────────────┘
                       │  validation, canonicalisation, allow-lists
  ┌────────────────────▼───────────────────────────────┐
  │  SEMI-TRUSTED: the webview (renders untrusted data)│
  │  no tokens, no network, no filesystem              │
  └────────────────────┬───────────────────────────────┘
                       │  typed Tauri IPC, closed command set
  ┌────────────────────▼───────────────────────────────┐
  │  TRUSTED: desktop Rust core — holds credentials    │
  └────────────────────┬───────────────────────────────┘
                       │  TLS 1.3, pinned cert, bearer/device-key auth
  ┌────────────────────▼───────────────────────────────┐
  │  PRIVILEGED: the agent — Docker, database, files   │
  └────────────────────────────────────────────────────┘
```

Data crosses boundaries only through validation. Privilege never flows upward.

---

## 4. Threats and controls

### 4.1 Command injection

**Threat.** A project name, file path, environment value or version string
becomes part of a shell command.

**Control.** There is no shell. Every process invocation uses a structured
argument array (`Command::new(...).arg(...)`), never a formatted string, and
never `sh -c`. Docker operations go through the Docker **API** via `bollard`, not
the `docker` CLI, so there is no command line to inject into at all. The few
places that must run a host binary — `netsh`, `ufw`, `systemctl` — take
fixed argument vectors with values drawn from validated enums and integers, and
are the subject of a dedicated review item in Phase 11.

### 4.2 Path traversal and symlink escape

**Threat.** `../../../etc/shadow`, an absolute path, a symlink pointing out of
the project, a Windows junction, or a path that changes between check and use.

**Control.** A single `SafePath` type in `file-manager` is the only way to
address a file. Construction is fallible and does, in order:

1. Reject absolute paths, drive letters, UNC prefixes, and any `..` component —
   before normalisation, not after.
2. Reject NUL bytes, control characters, Windows reserved device names, trailing
   dots and spaces, and any `:` beyond a drive prefix.
3. Join to the canonical project root and canonicalise, resolving every link.
4. Verify the result is still beneath the root, comparing case-folded on
   Windows.
5. Open with `O_NOFOLLOW` on the final component; on Linux 5.6+ use `openat2`
   with `RESOLVE_BENEATH` so the **kernel** enforces containment.

Step 5 is what addresses TOCTOU: steps 1–4 validate a name, and a name can be
swapped for a symlink between validation and use. `RESOLVE_BENEATH` closes the
window; where unavailable, the file is opened first and the handle's identity
verified before use, so the check applies to the object rather than the path.

No raw `&str` path from a request ever reaches `std::fs`. The types make it
impossible: the filesystem functions in `file-manager` accept `SafePath` only.

### 4.3 Zip Slip, ZIP bombs and malicious archives

**Threat.** Entries named `../../agent.toml`, `C:\Windows\…`, `\\host\share\…`;
symlink entries; a 10 MB archive expanding to 500 GB; a million tiny files.

**Control.** Streaming extraction with per-entry validation and running totals:

| Check              | Limit                           |
| ------------------ | ------------------------------- |
| Archive size       | 2 GB (configurable)             |
| Entry count        | 50,000                          |
| Uncompressed total | 10 GB                           |
| Compression ratio  | 100:1 overall, 1000:1 per entry |
| Single file size   | 1 GB                            |
| Path depth         | 32                              |

Entry names go through the same `SafePath` construction as everything else.
Symlink, hardlink, device, FIFO and socket entries are rejected outright — not
skipped silently, but treated as a failed import, because their presence is
evidence of intent rather than accident. Extraction targets a UUID-named temp
directory on the same filesystem as the projects directory and is renamed into
place only after full success; a failure removes the temp tree entirely. Nothing
is ever extracted directly into a live project directory.

The archive is never loaded into memory. Ratio counters are checked as bytes
are written, so a bomb is caught during extraction rather than after.

### 4.4 Secret exposure

**Threat.** Secrets in logs, in API responses, in backups, in `.env.example`, in
audit metadata, in error messages, in duplicated projects.

**Control.**

- Values are encrypted at rest with **XChaCha20-Poly1305**, per-value random
  nonce, key held in the OS keychain (`docs/platform-support.md` §4).
- A `Secret<T>` wrapper implements `Debug` and `Display` as `[redacted]` and
  zeroises on drop. A secret cannot be logged by accident because there is no
  formatting impl that prints it.
- The logging layer additionally redacts by key name (`password`, `token`,
  `secret`, `authorization`, `api_key`, …) as defence in depth.
- No API route returns a decrypted secret (`docs/api-design.md` §3).
- Backups store environment **metadata** — key names and secret flags — never
  values. A backup archive is therefore not a secret store.
- `.env.example` export emits keys with empty values.
- Duplicating a project copies non-secret variables and leaves secrets unset.
- Audit metadata is built from an allow-list of fields, never by serialising a
  whole request body.

### 4.5 Authentication attacks

| Threat           | Control                                                                                             |
| ---------------- | --------------------------------------------------------------------------------------------------- |
| Brute force      | Argon2id (19 MiB, t=2); 5 attempts then exponential lockout to 15 min; per-account and per-source   |
| User enumeration | Identical response and timing for unknown account and wrong password                                |
| Session fixation | Tokens are server-generated only; a new session is issued on every login and on password change     |
| Session theft    | Opaque 256-bit tokens, stored only as SHA-256, bound to a client, individually revocable            |
| Token leakage    | Never in URLs, never in the webview, never in logs; sent as a header including on WebSocket upgrade |
| CSRF             | Not applicable — bearer tokens, no cookies, no ambient authority, no browser                        |
| Replay (remote)  | Nonce challenge signed by a device key; nonces are single-use and time-bounded                      |

### 4.6 Remote access

**Threat.** An unauthenticated LAN peer reaching the agent; a man in the middle;
a revoked laptop still connecting.

**Control.** Loopback-only by default. LAN binding is an explicit setting that
writes an audit entry and, on consent, a firewall rule scoped to the local
subnet and private profiles. TLS 1.3 with a pinned self-signed certificate — a
changed fingerprint is a hard stop requiring explicit human confirmation, never
a click-through. Pairing needs a short-lived single-use code entered on the
host. Revocation deletes the device key and kills its sessions immediately.

The agent is never exposed to the internet by design; `docs/remote-management.md`
covers Tailscale as the supported way to cross networks, because it moves the
authentication problem to a system built for it rather than inventing one.

### 4.7 Cross-project access

**Threat.** Project A reads project B's files, reaches B over the network, or
sees B's secrets.

**Control.** Per-project Docker network; no shared volumes; per-project bind
mount of that project's directory only; environment variables injected per
container. The agent's own directories — config, database, backups — are never
mounted into any container. Full detail in `docs/docker.md`.

### 4.8 Local privilege escalation

**Threat.** An unprivileged local user reads the database, the encryption key,
or the bootstrap token, and gains agent control.

**Control.** Data directories are administrator/root-only with inheritance
disabled (Windows) and `0750` with a dedicated service user (Linux). The key
lives in the OS keychain, not on disk, where the platform supports it. The agent
verifies permissions at startup, logs a warning if they are wrong, and refuses
LAN binding while they are wrong — a misconfigured install should not
additionally expose itself to the network.

The Linux service user is in the `docker` group, which is root-equivalent. This
is unavoidable for a service that manages Docker, and is documented plainly
rather than hidden: anyone who can run code as `project-host` owns the host. The
mitigation is that nothing user-supplied runs as that user — user code runs in
containers, and the agent's own attack surface is a validated typed API.

### 4.9 Resource exhaustion

**Threat.** A project fills the disk, eats all RAM, spawns unbounded processes,
or the agent itself leaks memory through log buffers.

**Control.** Per-container memory, CPU and PID limits; bounded log retention
with rotation; bounded in-memory ring buffers with visible drop counters; upload
and archive limits; a configurable project ceiling; disk-space checks before
backup and import, refusing rather than filling the volume; metric sampling at
fixed intervals with batched writes.

### 4.10 Race conditions and duplicate operations

**Threat.** Two restores at once; delete during rebuild; a retried request
creating two projects; two log followers on one container.

**Control.** `project_locks` with `project_id` as primary key, so concurrency is
refused by a uniqueness constraint rather than by checking. Idempotency keys with
stored responses for 24 hours. Reference-counted log followers. Port allocation
protected by a `UNIQUE` constraint. Operation states persisted so an interrupted
operation is recoverable rather than ambiguous.

### 4.11 Supply chain and updates

**Threat.** A tampered agent binary, a malicious dependency, an untrusted
container template.

**Control.** Version one supports **manual, signature-verified updates only** —
no silent auto-update. Signatures are verified before anything is written;
configuration is backed up first; a failed update rolls back. Only templates
shipped in `docker/templates/` are usable; arbitrary images and user Dockerfiles
are not permitted in version one. Dependencies are pinned with lockfiles and
audited in CI (`cargo audit`, `pnpm audit`), with the review recorded in Phase 11.

---

## 5. Input handling summary

| Input                | Treatment                                                     |
| -------------------- | ------------------------------------------------------------- |
| Project display name | Length-bounded, control characters stripped; **display only** |
| Project identifiers  | Server-generated UUID → slug, directory, container name       |
| File paths           | `SafePath` only                                               |
| ZIP entries          | `SafePath` + type and limit checks                            |
| Environment keys     | `[A-Za-z_][A-Za-z0-9_]*`, enforced by database `CHECK`        |
| Environment values   | Length-bounded; encrypted when secret; never logged           |
| Ports                | Integer, 1024–65535, uniqueness-constrained                   |
| Runtime versions     | Allow-list from the template manifest                         |
| Commands             | Allow-listed template commands with validated arguments       |
| Docker image         | Never user-supplied                                           |
| Network addresses    | Parsed to typed IP/CIDR; no string interpolation              |

---

## 6. Verification plan

Phase 11 reviews each control; Phase 12 runs the tests. The distinction the
specification insists on — verified versus assumed — is tracked in
`docs/security-review.md`, which is written during Phase 11 and lists each item
as **verified by test**, **verified by inspection**, or **unverified**, with the
reason.

Security tests that must exist and pass before any completeness claim:

- Path traversal corpus: `..`, encoded variants, absolute, UNC, drive-relative,
  reserved names, trailing dot/space, deep nesting.
- Symlink and junction escape, including a link created _after_ validation
  (TOCTOU).
- ZIP corpus: slip, bomb, ratio, symlink entry, device entry, huge count, deep
  path, absolute entry.
- Secret redaction: assert no secret value appears in any log sink, API
  response, backup archive, or audit row.
- Container hardening assertions: no socket mount, no privileged flag, no host
  network, non-root user, `no-new-privileges`, dropped capabilities.
- Auth: lockout, timing equivalence, session revocation, expiry, pin mismatch
  rejection.
- Concurrency: simultaneous restore attempts, delete-during-rebuild, idempotent
  retry.

On the current development machine the Docker-dependent items in that list
cannot run. They will be skipped with an explicit reason and reported as
**not verified** — never as passing.
