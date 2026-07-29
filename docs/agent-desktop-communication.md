# Agent ↔ Desktop Communication

Three hops separate a button click from a container action. Each has a different
threat model and a different mechanism.

```
  React webview  ──(1) Tauri IPC──▶  Desktop Rust core  ──(2) HTTPS/WSS──▶  Agent
                                                                              │
                                                                        (3) Docker
```

Hop 3 belongs to `docs/docker.md`. This document covers 1 and 2.

---

## 1. Why not let the webview call the agent directly

It would be less code: point `fetch` at `https://127.0.0.1:8787` and delete the
middle hop. It is rejected for three reasons.

- **The token would have to live in the webview.** Anywhere it could be stored —
  `localStorage`, memory, a cookie — is reachable by injected script. The
  specification forbids tokens in `localStorage`; putting the token one layer
  down removes the entire class of problem instead of relocating it.
- **Certificate pinning is not available to a webview.** The agent uses a
  self-signed certificate. A webview would either refuse it or need a permanent
  exception, which is precisely the posture that makes interception easy. A Rust
  client pins the exact key and rejects everything else.
- **A compromised renderer would inherit the agent's full API.** With the core
  in between, the renderer's reach is the IPC command list — a closed set that
  can be reasoned about.

The cost is one serialisation hop. Measured against the alternative, it is
cheap.

---

## 2. Hop 1 — Tauri IPC

Typed commands from webview to Rust core. Not a generic proxy: there is no
`invoke("request", { url })`. Each command is an explicit, named capability.

```rust
#[tauri::command]
async fn project_list(
    state: tauri::State<'_, AppState>,
    connection: ConnectionId,
    page: PageRequest,
) -> Result<Page<ProjectSummary>, IpcError>;

#[tauri::command]
async fn project_start(
    state: tauri::State<'_, AppState>,
    connection: ConnectionId,
    project: ProjectId,
) -> Result<OperationHandle, IpcError>;
```

Rules:

- **`ConnectionId` is an opaque handle**, not an address. The webview names
  _which_ server, never _where_ it is. It cannot be pointed at an arbitrary host.
- **No command returns a credential.** Session tokens, device private keys and
  decrypted secret values have no path upward. `env_var_list` returns values for
  non-secret variables and `null` plus `is_set: true` for secrets.
- **Every command validates its arguments in Rust** before touching the network,
  using the same generated schema the agent will apply.
- **The CSP forbids outbound connections** — `connect-src 'self' ipc:` — so even
  injected script cannot open a socket.

Streams (logs, metrics, events) go the other way as Tauri events. The core
subscribes to the agent's WebSocket, decodes, and re-emits on a channel the
webview listens to. Backpressure and reconnection live in the core; the webview
sees a clean stream and a connection-state enum.

---

## 3. Hop 2 — the agent's local API

### 3.1 Transport

**HTTPS and WSS, always — including on loopback.**

Plain HTTP on loopback would be defensible, and it was considered. It is
rejected because it forks the code: loopback would need one auth path and LAN
another, and the LAN path — the one that actually faces a network — would be
the less-exercised of the two. One transport, exercised constantly by every
local user, is the safer arrangement. TLS on loopback costs a handshake per
connection and nothing measurable thereafter.

- TLS 1.3 only, `rustls`, no downgrade.
- On first start the agent generates a self-signed certificate (ECDSA P-256,
  10-year validity, SAN covering `localhost`, `127.0.0.1`, `::1` and the host
  name). The private key is stored at `config/agent.key`, mode `0600` on Linux
  and SYSTEM-only ACL on Windows.
- Clients **pin the SPKI SHA-256 fingerprint**, not the certificate chain. No CA,
  no expiry surprise, no trust in the system store.
- Default bind `127.0.0.1:8787`. LAN binding is opt-in and separately audited.
- HTTP/1.1 with keep-alive; WebSocket upgrade on the same port.

### 3.2 Local trust bootstrap

A local desktop client must prove it is entitled to talk to the agent, without
the user copying a token by hand on every launch.

1. The agent writes a **bootstrap file** at start into a directory only
   administrators can read: `config/local-bootstrap.json`, containing the
   certificate fingerprint and a rotating 32-byte local token.
2. A desktop client running as an administrator on the same machine reads it,
   connects, pins the fingerprint, and exchanges the local token for a session.
3. The local token rotates on every agent start, so a stale copy is useless.

The file's permissions carry the security here: a user who can read it is an
administrator on the machine and could reach Docker anyway. This is stated
rather than assumed — it is the reason the ACL work in
`docs/platform-support.md` §2.1 is load-bearing and not decoration.

Password login remains available and is required for remote clients (§4) and
for any action marked sensitive.

### 3.3 Authentication and sessions

- First run has no administrator. The agent enters **setup mode** and serves
  only `/api/v1/setup/*` until an administrator exists. Setup requires the
  bootstrap token, so a LAN-exposed agent cannot be claimed by a stranger.
- Passwords are hashed with **Argon2id** (19 MiB, t=2, p=1, per-hash salt).
- Login returns an opaque 256-bit session token. Only its SHA-256 is stored.
- Tokens are sent as `Authorization: Bearer`. Not cookies — there is no browser
  and no ambient authority, which removes CSRF from the model entirely.
- Sessions carry absolute expiry (default 30 days) and idle expiry (default 7
  days), are bound to a client identity, and are individually revocable.
- Failed logins are rate limited per account and per source: 5 attempts, then
  exponential lockout to a 15-minute ceiling. Lockout is recorded in the audit
  log. The response is identical for unknown account and wrong password, and
  takes the same time.

### 3.4 Request and response envelope

Every response carries correlation identifiers. Every mutating request may carry
an idempotency key.

```http
POST /api/v1/projects/{id}/restart HTTP/1.1
Authorization: Bearer <token>
Content-Type: application/json
X-Request-Id: 6c1f…            (client-generated, echoed)
X-Idempotency-Key: 9ab3…       (optional; required for create/restore/delete)
```

```json
{
  "ok": true,
  "data": { "operation_id": "op_8f2c…", "state": "RUNNING" },
  "meta": { "request_id": "6c1f…", "server_time": "2026-07-29T00:11:04Z" }
}
```

Errors are the same shape, and never contain a stack trace:

```json
{
  "ok": false,
  "error": {
    "code": "PROJECT_LOCKED",
    "message": "This project is being restored. Try again when it finishes.",
    "details": { "held_by": "RESTORE", "operation_id": "op_71aa…" },
    "request_id": "6c1f…"
  }
}
```

`code` is a stable machine identifier the UI switches on. `message` is written
for a person. Technical detail — the underlying Docker error, the failing path —
goes to the agent log, keyed by `request_id`, and never to the client.

**Idempotency.** Keys are stored with their response for 24 hours. A repeat of a
completed key returns the stored response without re-executing; a repeat of an
in-flight key returns `409 OPERATION_IN_PROGRESS`. This is what makes a retry
after a dropped connection safe for "create project" and "restore backup".

### 3.5 Long operations

Builds, imports, backups and restores return immediately with an
`operation_id`. Progress arrives over the event stream; the operation is also
pollable at `/api/v1/operations/{id}` for a client that reconnects. Operations
are cancellable where cancellation is safe, and cancellation is honoured at
defined checkpoints rather than by killing a task mid-write.

---

## 4. Remote clients

The transport is identical; the authentication is stronger. A remote client
proves possession of a device key rather than presenting a bearer token obtained
from a file it could not have read.

1. **Pairing.** The agent displays a 8-character code (from the desktop UI on
   the host, or `project-host-ctl pair --new`), valid 10 minutes, single use.
2. The client generates an **Ed25519 device keypair**, keeps the private key in
   the OS keychain, and sends the public key with the pairing code and a device
   label.
3. The agent verifies the code, stores a `TrustedClient` row, and returns its
   certificate fingerprint for pinning.
4. **Every subsequent session** starts with a challenge: the agent sends a
   nonce, the client signs it, the agent verifies against the stored public key
   and issues a session token.

Revoking a client deletes the public key and kills its sessions immediately.
Full treatment in `docs/remote-management.md`.

---

## 5. Streaming

One multiplexed WebSocket per connection at `/api/v1/stream`, authenticated by
the same bearer token during the upgrade — the token is sent in a header, never
in the query string, where it would land in logs.

Client subscribes to topics; the server sends only what is subscribed:

```json
{
  "op": "subscribe",
  "topic": "project.logs",
  "project_id": "prj_…",
  "params": { "tail": 200, "follow": true }
}
```

| Topic                | Payload                                                  |
| -------------------- | -------------------------------------------------------- |
| `project.logs`       | stdout/stderr lines with timestamp and stream type       |
| `project.status`     | state transitions, health, exit codes                    |
| `project.metrics`    | CPU, RAM, network, disk per container                    |
| `host.metrics`       | host-level sampling                                      |
| `operation.progress` | build and backup progress                                |
| `agent.events`       | Docker availability, connectivity changes, notifications |

Discipline that keeps this from leaking:

- **One follower per container**, reference-counted across subscribers. Two
  clients watching the same project share a single Docker log stream.
- **`tail` is bounded** (max 5000). History is fetched once at subscribe; after
  that only new lines are sent. The full history is never re-sent on update.
- **Per-topic ring buffers** with drop-oldest and an explicit `dropped: n`
  notice, so a slow client degrades visibly instead of ballooning agent memory.
- **Unsubscribe on close, always** — the follower is tied to the subscription's
  lifetime via a guard, so a dropped connection cannot leak a Docker stream.
- **Container stop detaches cleanly; container restart re-attaches** with a
  fresh stream and a `stream.restarted` marker so the UI can show the seam.
- Heartbeat ping every 20s; a client that misses two is disconnected.

---

## 6. Contract generation

Rust is the single source of truth. TypeScript is generated. Nothing is written
twice.

```
crates/api-types/src/**.rs          ← authoritative
  │  #[derive(Serialize, Deserialize, JsonSchema)]
  ▼
cargo run -p api-types --bin emit-schema
  │
  ▼
contracts/openapi.json + contracts/schemas/*.json
  │
  ├─▶ packages/shared-types/src/generated.ts    (TS interfaces)
  ├─▶ packages/api-contracts/src/generated.ts   (Zod schemas + client signatures)
  └─▶ docs/api-reference.md                     (generated reference)
```

Generated files carry a header banning hand edits and are committed, so a
checkout builds without running Rust codegen first. CI regenerates and fails on
any diff — a Rust type change that skips regeneration cannot merge.

The same JSON Schema validates on both sides: Zod in the client for immediate
form feedback, and the Rust validator in the agent as the authority. The client
check is a convenience; the agent never trusts it.

---

## 7. Failure and reconnection

| Failure               | Detection             | Client behaviour                                                                                         |
| --------------------- | --------------------- | -------------------------------------------------------------------------------------------------------- |
| Agent stopped         | connection refused    | Offline state, last-known data marked stale, retry with backoff                                          |
| Agent restarting      | refused then accepted | Auto-reconnect, resubscribe, resync                                                                      |
| Token expired         | `401 SESSION_EXPIRED` | Silent re-auth via device key or bootstrap; password prompt only if that fails                           |
| Certificate changed   | pin mismatch          | **Hard stop.** Fingerprints shown side by side, explicit user confirmation required. Never auto-trusted. |
| Network lost (remote) | heartbeat timeout     | Offline banner, backoff reconnect, projects keep running                                                 |
| Docker stopped        | agent event           | Docker-unavailable banner; project actions disabled with explanation; agent stays up                     |

Reconnection backoff is 1s, 2s, 4s … capped at 30s, with jitter. The client
resubscribes to its previous topics and reconciles by fetching current state
rather than assuming the stream filled the gap.

The certificate-change case is deliberately hostile to click-through: it is the
one signal that distinguishes a reinstalled agent from an interception attempt,
and treating it as routine would waste it.
