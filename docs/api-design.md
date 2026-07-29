# API Design

The agent's local API. Consumed by the desktop client's Rust core over HTTPS on
`127.0.0.1:8787` by default, or over the LAN when explicitly enabled. Never
public. Envelope, authentication and transport are specified in
`docs/agent-desktop-communication.md`; this document is the surface itself.

Base path `/api/v1`. Versioned from the first release so that a client and agent
at different versions can negotiate rather than fail obscurely.

---

## 1. Conventions

**Naming.** Plural resource nouns, verbs only for actions that are genuinely not
CRUD (`/start`, `/restart`, `/rebuild`). Actions are `POST` and are never
idempotent by accident — they take an idempotency key where a repeat would be
harmful.

**Pagination.** Cursor-based everywhere. UUIDv7 keys sort by time, so the cursor
is the last id seen. Offsets are not offered: they skip and duplicate rows when
the underlying data changes between pages, which for an audit log is a
correctness bug.

```
GET /api/v1/projects?limit=50&cursor=prj_01J…&sort=created_at:desc
```

```json
{ "ok": true,
  "data": { "items": [ … ], "next_cursor": "prj_01J…", "has_more": true },
  "meta": { "request_id": "…", "total_estimate": 137 } }
```

`limit` defaults to 50, caps at 200. `total_estimate` is explicitly an estimate;
an exact count over a large audit table is an expensive answer to a question
nobody needs precisely.

**Validation.** Every request body is validated against the generated JSON
Schema before a handler runs. Failures return `422` with a per-field list.

**Permissions.** Every route declares a required capability. Version one has one
role, so the check is uniform — but it is a real check in a middleware, not an
assumption, so adding roles later does not mean auditing every handler.

**Rate limits.** Applied per session and per source address. Auth routes are
strict (5/min); read routes generous (600/min); mutating routes moderate
(60/min); streaming subscriptions capped at 20 concurrent topics per session.

---

## 2. Errors

Stable machine-readable codes. The UI switches on `code` and never parses
`message`.

| Code                      | HTTP | Meaning                                     |
| ------------------------- | ---- | ------------------------------------------- |
| `VALIDATION_FAILED`       | 422  | Request body failed schema validation       |
| `UNAUTHENTICATED`         | 401  | Missing or invalid credentials              |
| `SESSION_EXPIRED`         | 401  | Valid token, past expiry                    |
| `FORBIDDEN`               | 403  | Authenticated, not permitted                |
| `NOT_FOUND`               | 404  | No such resource                            |
| `CONFLICT`                | 409  | Violates a uniqueness or state constraint   |
| `PROJECT_LOCKED`          | 409  | Another operation holds the project lock    |
| `OPERATION_IN_PROGRESS`   | 409  | Idempotency key still executing             |
| `PRECONDITION_FAILED`     | 412  | e.g. restore attempted on a running project |
| `PAYLOAD_TOO_LARGE`       | 413  | Upload beyond the configured limit          |
| `RATE_LIMITED`            | 429  | Too many requests; `Retry-After` set        |
| `DOCKER_UNAVAILABLE`      | 503  | Daemon unreachable                          |
| `DOCKER_OPERATION_FAILED` | 502  | Daemon reachable, operation failed          |
| `PORT_UNAVAILABLE`        | 409  | Requested host port taken or out of range   |
| `RESOURCE_LIMIT_EXCEEDED` | 409  | Project count, disk or memory ceiling       |
| `ARCHIVE_REJECTED`        | 422  | ZIP failed a security check                 |
| `PATH_REJECTED`           | 422  | Path escaped the project root               |
| `INTEGRITY_CHECK_FAILED`  | 422  | Backup checksum mismatch                    |
| `SETUP_REQUIRED`          | 428  | No administrator exists yet                 |
| `AGENT_STARTING`          | 503  | Migrations or reconciliation still running  |
| `INTERNAL`                | 500  | Unexpected; details in the agent log only   |

`ARCHIVE_REJECTED` and `PATH_REJECTED` deliberately do not report _which_ rule
tripped in the message shown to a remote caller — the detail goes to the audit
log and the agent log. Telling an attacker precisely which check caught them is
free reconnaissance.

---

## 3. Routes

### Setup and authentication

| Method   | Path                   | Notes                                                                          |
| -------- | ---------------------- | ------------------------------------------------------------------------------ |
| `GET`    | `/setup/status`        | Unauthenticated. Whether an administrator exists                               |
| `POST`   | `/setup/administrator` | Bootstrap token required. Creates first admin, returns recovery codes **once** |
| `POST`   | `/auth/login`          | Email + password → session                                                     |
| `POST`   | `/auth/local-token`    | Exchanges the rotating local bootstrap token for a session                     |
| `POST`   | `/auth/challenge`      | Returns a nonce for device-key authentication                                  |
| `POST`   | `/auth/verify`         | Signed nonce → session                                                         |
| `POST`   | `/auth/logout`         | Revokes the current session                                                    |
| `POST`   | `/auth/logout-all`     | Revokes every session for the user                                             |
| `GET`    | `/auth/session`        | Current session and user                                                       |
| `POST`   | `/auth/password`       | Change password; revokes other sessions                                        |
| `POST`   | `/auth/recovery`       | Consume a recovery code to reset the password                                  |
| `GET`    | `/auth/sessions`       | List active sessions                                                           |
| `DELETE` | `/auth/sessions/{id}`  | Revoke one session                                                             |

Recovery codes are shown exactly once, at generation. There is no route that
returns them again, because storing them retrievably would defeat them.

### Server and system

| Method | Path                      | Notes                                                            |
| ------ | ------------------------- | ---------------------------------------------------------------- |
| `GET`  | `/server/info`            | Agent version, schema version, OS, platform capabilities, uptime |
| `GET`  | `/server/health`          | Liveness; unauthenticated, minimal detail                        |
| `GET`  | `/server/connectivity`    | The five states from `docs/architecture.md` §8                   |
| `GET`  | `/system/metrics`         | Current host metrics snapshot                                    |
| `GET`  | `/system/metrics/history` | Downsampled range query                                          |
| `GET`  | `/docker/status`          | Availability, version, endpoint kind, install hint when absent   |
| `GET`  | `/docker/info`            | Storage driver, container counts, disk usage                     |

`/server/health` is intentionally thin: it answers "is the agent alive" for a
service watchdog and reveals nothing about configuration.

### Projects

| Method   | Path                                   | Notes                                                  |
| -------- | -------------------------------------- | ------------------------------------------------------ |
| `GET`    | `/projects`                            | Paginated; filter by status, type, search              |
| `POST`   | `/projects`                            | Create. Idempotency key required                       |
| `GET`    | `/projects/{id}`                       | Full detail                                            |
| `PATCH`  | `/projects/{id}`                       | Update settings; reports whether a restart is required |
| `DELETE` | `/projects/{id}`                       | Idempotency key + `confirm_name` required              |
| `POST`   | `/projects/{id}/start`                 |                                                        |
| `POST`   | `/projects/{id}/stop`                  | Graceful, with timeout                                 |
| `POST`   | `/projects/{id}/force-stop`            | SIGKILL equivalent                                     |
| `POST`   | `/projects/{id}/restart`               |                                                        |
| `POST`   | `/projects/{id}/rebuild`               | Rebuilds the image, then recreates                     |
| `POST`   | `/projects/{id}/archive`               | Stops and marks archived                               |
| `POST`   | `/projects/{id}/unarchive`             |                                                        |
| `POST`   | `/projects/{id}/duplicate`             | Copies files and config; never secrets                 |
| `GET`    | `/projects/{id}/status`                | Lightweight polling fallback for the stream            |
| `GET`    | `/projects/{id}/container`             | Container inspection, sanitised                        |
| `GET`    | `/projects/{id}/metrics`               | Current                                                |
| `GET`    | `/projects/{id}/metrics/history`       | Range                                                  |
| `GET`    | `/projects/{id}/deployments`           | Paginated history                                      |
| `GET`    | `/projects/{id}/deployments/{did}/log` | Build log, streamed                                    |
| `GET`    | `/projects/{id}/events`                | Container event history                                |
| `POST`   | `/projects/{id}/export`                | Produces a portable archive                            |
| `POST`   | `/projects/import`                     | Imports an exported archive                            |

`DELETE` requiring `confirm_name` — the client must echo the project's display
name — is the confirmation requirement for destructive actions made structural
rather than left to a dialog that could be bypassed by a direct API call.

`duplicate` copying everything except secret values is deliberate: silently
cloning credentials into a second project is a leak that looks like a feature.

### Project creation and detection

| Method | Path                     | Notes                                                                    |
| ------ | ------------------------ | ------------------------------------------------------------------------ |
| `POST` | `/projects/detect`       | Inspects an upload or folder, proposes runtime, version, commands, ports |
| `GET`  | `/runtimes`              | Approved templates and supported versions                                |
| `POST` | `/uploads`               | Chunked upload session for a ZIP; returns an upload id                   |
| `PUT`  | `/uploads/{id}`          | Chunk append                                                             |
| `POST` | `/uploads/{id}/finalize` | Validates the archive; does **not** extract yet                          |

Detection is a separate call from creation so the wizard can show what was found
and let the user correct it before anything is written. Finalize validates
structure and rejects unsafe archives before a single byte is extracted to the
project directory.

### Files

| Method   | Path                             | Notes                                            |
| -------- | -------------------------------- | ------------------------------------------------ |
| `GET`    | `/projects/{id}/files`           | Directory listing; `path` query, always relative |
| `GET`    | `/projects/{id}/files/content`   | Text content; refuses binary and oversized       |
| `PUT`    | `/projects/{id}/files/content`   | Write; atomic via temp + rename                  |
| `POST`   | `/projects/{id}/files/directory` | Create directory                                 |
| `POST`   | `/projects/{id}/files/move`      | Move or rename                                   |
| `POST`   | `/projects/{id}/files/copy`      |                                                  |
| `DELETE` | `/projects/{id}/files`           | Delete; confirmation required for directories    |
| `POST`   | `/projects/{id}/files/upload`    | Multipart, size-limited                          |
| `GET`    | `/projects/{id}/files/download`  | Single file or directory as an archive           |
| `GET`    | `/projects/{id}/files/search`    | Name and content search, bounded                 |

Every path parameter is relative to the project root and is canonicalised and
containment-checked server-side. An absolute path, a drive letter, a UNC prefix
or any `..` segment is rejected outright rather than normalised — normalising
attacker input is how traversal bugs survive review.

### Environment variables

| Method   | Path                                | Notes                                                 |
| -------- | ----------------------------------- | ----------------------------------------------------- |
| `GET`    | `/projects/{id}/env`                | Values for non-secrets; `null` + `is_set` for secrets |
| `POST`   | `/projects/{id}/env`                | Create                                                |
| `PATCH`  | `/projects/{id}/env/{key}`          | Update                                                |
| `DELETE` | `/projects/{id}/env/{key}`          |                                                       |
| `POST`   | `/projects/{id}/env/bulk`           | Transactional multi-edit                              |
| `POST`   | `/projects/{id}/env/import`         | Parse `.env`; reports duplicates and conflicts        |
| `GET`    | `/projects/{id}/env/export-example` | `.env.example` with keys only                         |

There is no route that returns a decrypted secret. Once written, a secret leaves
the agent only as an environment variable inside its own container.

### Backups

| Method   | Path                            | Notes                                         |
| -------- | ------------------------------- | --------------------------------------------- |
| `GET`    | `/projects/{id}/backups`        |                                               |
| `POST`   | `/projects/{id}/backups`        | Create; idempotency key                       |
| `GET`    | `/backups/{bid}`                |                                               |
| `DELETE` | `/backups/{bid}`                | Confirmation required                         |
| `POST`   | `/backups/{bid}/restore`        | Requires the project stopped; idempotency key |
| `POST`   | `/backups/{bid}/verify`         | Checksum and archive integrity                |
| `GET`    | `/backups/{bid}/download`       | Export                                        |
| `POST`   | `/projects/{id}/backups/import` | Import an exported archive                    |
| `GET`    | `/operations/{id}`              | State of any long operation                   |
| `POST`   | `/operations/{id}/cancel`       | Where cancellation is safe                    |

### Audit, notifications, settings

| Method  | Path                       | Notes                                             |
| ------- | -------------------------- | ------------------------------------------------- |
| `GET`   | `/audit`                   | Paginated; filter by action, target, result, time |
| `GET`   | `/audit/export`            | CSV/JSON export of a filtered range               |
| `GET`   | `/notifications`           |                                                   |
| `POST`  | `/notifications/{id}/read` |                                                   |
| `POST`  | `/notifications/read-all`  |                                                   |
| `GET`   | `/settings`                | System settings                                   |
| `PATCH` | `/settings`                | Audited; some keys require confirmation           |
| `GET`   | `/settings/network`        | Bind address, LAN state, firewall rule state      |
| `POST`  | `/settings/network/lan`    | Enable/disable LAN exposure. Heavily audited      |

### Trusted clients

| Method   | Path                    | Notes                                            |
| -------- | ----------------------- | ------------------------------------------------ |
| `GET`    | `/clients`              | Paired clients, last seen, fingerprints          |
| `POST`   | `/clients/pairing-code` | Generate a single-use code                       |
| `POST`   | `/clients/pair`         | Consume a code, register a public key            |
| `DELETE` | `/clients/{id}`         | Revoke; kills its sessions immediately           |
| `POST`   | `/clients/test`         | Connectivity check from the client's perspective |

### Streaming

| Path                               | Notes                                                                  |
| ---------------------------------- | ---------------------------------------------------------------------- |
| `GET /stream`                      | WebSocket upgrade. Topics per `docs/agent-desktop-communication.md` §5 |
| `GET /projects/{id}/logs`          | Bounded historical fetch; the stream handles live tail                 |
| `GET /projects/{id}/logs/download` | Full retained log as a file                                            |

---

## 4. Documentation generation

The route table above is the human-readable view. The machine-readable contract
is generated from the Rust handler and type definitions into
`contracts/openapi.json`, and rendered to `docs/api-reference.md`. CI fails if
the generated output differs from what is committed, so a route added without
regenerating cannot merge — which is what keeps acceptance criterion 30
("documentation matches the actual implementation") true for the API surface
without relying on anybody remembering.
