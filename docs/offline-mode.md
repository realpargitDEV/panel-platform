# Offline-First Operation

Project Host has no cloud component. There is no account to create, no licence
to check, no telemetry endpoint, and no external API the product needs in order
to function. Offline is not a degraded mode that was added later — it is the
normal mode, and internet access is an optional capability that some _projects_
happen to want.

---

## 1. Five connectivity states

Collapsing these into one "online" flag would be the central design error. An
unplugged cable, a stopped Docker daemon and a crashed agent produce completely
different symptoms and need completely different responses. Each is tracked and
displayed separately.

| State                  | How it is determined                                 | Refresh                      |
| ---------------------- | ---------------------------------------------------- | ---------------------------- |
| **Agent reachable**    | Client's transport to the agent                      | continuous (heartbeat)       |
| **Docker reachable**   | Agent pings the daemon                               | 5s, plus event-stream health |
| **LAN available**      | Host has a non-loopback address with a default route | 15s                          |
| **Internet available** | TCP connect to a small set of well-known addresses   | 30s, backoff on failure      |
| **External service**   | Per-project, from that project's own health signal   | per project                  |

The internet check uses plain TCP connects (for example `1.1.1.1:443`,
`8.8.8.8:53`), not an HTTP request to a vendor endpoint. It is a reachability
probe, not a callback, and it sends nothing about the installation. If a user
disables it in settings, the state becomes `UNKNOWN` and the UI says so instead
of guessing.

---

## 2. What works with no internet

Everything the product itself does:

| Capability                        | Works offline | Why                            |
| --------------------------------- | ------------- | ------------------------------ |
| Agent starts, runs, recovers      | ✅            | No external dependency at boot |
| Desktop client opens and connects | ✅            | Loopback or LAN                |
| Login, sessions, recovery codes   | ✅            | Local Argon2id, local database |
| Create project from ZIP or folder | ✅            | Local extraction               |
| Start / stop / restart / delete   | ✅            | Local Docker                   |
| Live logs and metrics             | ✅            | Local streams                  |
| File management and editing       | ✅            | Local filesystem               |
| Environment variables and secrets | ✅            | Local keychain and database    |
| Backups and restores              | ✅            | Local archives                 |
| Audit log, history, settings      | ✅            | Local database                 |
| LAN remote management             | ✅            | Local network only             |

What genuinely requires the internet, stated honestly:

| Capability                                                | Why it cannot work offline                     |
| --------------------------------------------------------- | ---------------------------------------------- |
| **Building an image that pulls a base image or packages** | Docker must fetch from a registry / npm / PyPI |
| Update checks and downloads                               | Fetches a signed artefact                      |
| A project's own outbound traffic                          | The project's business, not the product's      |

The build case is the one that catches people. A project that has already been
built keeps starting and running offline forever; only a _first_ build or a
_rebuild_ needs the network. The UI states this before a rebuild rather than
failing halfway with a registry error.

---

## 3. Discord bots and reconnection

The specification calls this out, and it is the clearest illustration of why the
five states are separate.

When the internet drops:

- The bot's container **keeps running**. Nothing stops it. The agent does not
  restart it, because the container is healthy — its dependency is not.
- The bot's own library sees a disconnected gateway and retries with its own
  backoff. That is the library's job, and Project Host does not interfere.
- The UI shows the project as running, with internet unavailable, and — where
  the project reports it — an external-service state of disconnected.
- When connectivity returns, the bot reconnects on its own. No container
  restart, no agent restart, no reboot.

The important negative: **the agent must not treat an internet outage as a
project failure.** Restarting a Discord bot because the network blipped turns a
30-second recovery into a cold start, loses in-memory state, and can trip
Discord's own reconnect limits. Restart policy responds to process exit, never
to connectivity.

Other local projects — a static site on the LAN, an internal API — are
unaffected throughout.

---

## 4. Client behaviour when the agent is unreachable

The desktop client stays useful. It does not blank the screen or refuse to open.

- Last-known project list and status are cached in the client's own database and
  shown **explicitly marked stale**, with the timestamp of the last successful
  sync. Stale data presented as live is worse than no data.
- Actions that require the agent are disabled with an explanation, not hidden.
- Reconnection is automatic with jittered backoff (1s → 30s).
- On reconnect the client resyncs by fetching current state rather than assuming
  its cache is still correct.
- **Projects keep running the entire time.** The agent is a separate process; a
  closed or crashed client has no effect on it.

---

## 5. Agent behaviour when Docker is unavailable

The agent starts and serves regardless. Docker being down is a reported
condition, never a startup failure — an agent that refuses to start without
Docker cannot tell the user why Docker is missing.

Available: files, backups, environment variables, settings, audit, history,
metrics for the host. Unavailable, with a clear reason: start, stop, restart,
rebuild, create, and container metrics. On reconnect the agent re-establishes
the event stream and reconciles every project.

---

## 6. Time and clocks

No network time dependency. Timestamps are the host clock in UTC. A clock jump —
common on a machine that has been suspended — is handled by using monotonic
clocks for durations, timeouts and rate limiting, and wall-clock time only for
display and storage. Session expiry uses stored absolute timestamps, so a clock
moving backwards cannot extend a session indefinitely.

---

## 7. Testing offline behaviour

| Test                                | Method                                             | Host required       |
| ----------------------------------- | -------------------------------------------------- | ------------------- |
| Agent starts with no network        | Fake `MetricsProvider`/probe returning unreachable | any                 |
| Connectivity states are independent | Unit tests over the state machine                  | any                 |
| Client shows stale data correctly   | Frontend tests with a stubbed transport            | any                 |
| Reconnect and resubscribe           | Integration test killing and restarting the agent  | any                 |
| Containers survive agent restart    | Integration test                                   | Docker              |
| Bot survives internet loss          | Manual: disable the adapter, watch the container   | Docker + a real bot |
| Cached build works offline          | Build, disconnect, restart the project             | Docker              |

The last three cannot run on the current development machine. They are Phase 12
items against real hardware and will be reported as unverified until then.
