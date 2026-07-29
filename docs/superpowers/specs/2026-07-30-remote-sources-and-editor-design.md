# Remote project sources and in-app editing

Date: 2026-07-30
Status: approved, implementation in progress

Two additions to Panel Platform:

1. A project's files may come from a **GitHub repository or any HTTPS git
   remote**, or from an **HTTPS archive URL**, not only from a local folder, a
   ZIP upload, another project, or nothing.
2. A project's files may be **edited in the application**, in a Monaco editor
   with a file tree and tabs — the VS Code editing experience, without the rest
   of VS Code.

Both are host-side work. Unlike phases 6, 7 and 12, everything specified here is
verifiable on a machine with no Docker daemon, no Linux host and no Discord bot,
and so none of it may be marked complete without tests that actually ran.

---

## 1. Sources

### 1.1 Contract

`SourceType` becomes:

```
EMPTY | ZIP_UPLOAD | LOCAL_FOLDER | DUPLICATE | IMPORT_ARCHIVE | GIT_CLONE | REMOTE_ARCHIVE
```

`ProjectSource` gains four fields:

| Field          | Applies to                    | Meaning                                              |
| -------------- | ----------------------------- | ---------------------------------------------------- |
| `repo_url`     | `GIT_CLONE`, `REMOTE_ARCHIVE` | Absolute `https://` URL. Validated before any I/O.   |
| `git_ref`      | `GIT_CLONE`                   | Branch, tag or full commit id. Omitted = remote HEAD |
| `subdirectory` | `GIT_CLONE`, `REMOTE_ARCHIVE` | Relative path within the fetched tree to promote     |
| `credential`   | `GIT_CLONE`, `REMOTE_ARCHIVE` | Write-only access token. Never returned by any call. |

The comment at `crates/api-types/src/dto.rs:276` recording git as deliberately
absent is removed, not left contradicting the code.

Responses expose `has_credential: bool`. There is no read path for the token.

### 1.2 URL validation — `file-manager/src/remote_url.rs`

A URL is rejected unless every one of these holds:

- Scheme is exactly `https`. `http`, `file`, `git`, `ssh`, `data` and anything
  else are refused. Downgrade to `http` on redirect is refused.
- No userinfo component. Credentials travel in the `credential` field so that a
  token cannot reach a log line, an error message, or the provenance column.
- Host resolves to no address in: loopback, link-local (which covers the cloud
  metadata address `169.254.169.254`), unique-local, private v4 ranges,
  unspecified, or multicast. This is an SSRF control: the process runs as the
  user and can reach their LAN and their own loopback services.
- Every redirect target is re-validated by the same rules. Redirects are capped
  at 5.

Validation happens before the first connection and again per redirect, because a
DNS answer that passed once may change (rebinding).

### 1.3 Git clone — `file-manager/src/git_clone.rs`

`gix`, in-process. No `git` binary is required on the host, which preserves the
product's rule that nothing it needs runs on the host outside a container.

- Depth-1 clone of the requested ref.
- **Submodules are not initialised.** Recursive submodules are an
  arbitrary-remote-fetch primitive and would let a repository reach a URL that
  never passed §1.2.
- **No hooks run.** `gix` does not execute them at all; this is the decisive
  reason not to shell out to `git`, where a `post-checkout` hook in a cloned
  repository would run on the host as the user.
- Byte cap, entry-count cap and wall-clock timeout applied during fetch, not
  after, so a hostile remote cannot fill the disk before the check.
- `.git` is kept, so a later "update from remote" is possible, and counts
  against the byte cap.

### 1.4 HTTPS archive — `file-manager/src/http_archive.rs`

Download to the staging directory under a byte cap and timeout, then hand the
file to the existing archive path so that it meets the same entry rules as an
uploaded ZIP.

### 1.5 The hostility rule

**A fetched tree is exactly as hostile as an uploaded ZIP.** It goes through the
same entry validation, the same path rules, the same symlink refusal, the same
decompression-ratio cap, into the same UUID-named staging directory, and is
renamed into place only on complete success. A failure removes the staging
directory and leaves no project.

### 1.6 What "install a CLI from GitHub" means here

After the tree is promoted, the existing runtime detection in
`project-manager/src/detection.rs` chooses Node, Python or static and the
matching install command. That command runs **inside the project's container**
during build, exactly as it does for a ZIP-sourced project.

Nothing from a fetched repository is executed on the host. Not an install
script, not a hook, not a `package.json` lifecycle script.

### 1.7 Credentials

A supplied token is encrypted with XChaCha20-Poly1305 through the existing
`security` key management and stored in a new table:

```sql
CREATE TABLE project_source_credentials (
    project_id   TEXT PRIMARY KEY REFERENCES projects(id) ON DELETE CASCADE,
    ciphertext   BLOB NOT NULL,
    nonce        BLOB NOT NULL,
    created_at   TEXT NOT NULL,
    CHECK (length(ciphertext) > 0 AND length(nonce) = 24)
);
```

Same shape as the Discord bot token and secret environment variables: the
repository layer never sees a key, and the schema itself refuses a row that
could be plaintext. The existing secret-masking helpers mask the value before
any log line leaves.

### 1.8 Provenance and schema version

Schema goes to **version 3**. `projects` gains `source_url`, `source_ref` and
`source_commit` (all nullable), recording where a project came from and the
exact commit that was checked out.

---

## 2. In-app editing

### 2.1 Shape

A `ProjectFiles` view: file tree on the left, tabbed Monaco on the right.

Monaco and its web workers are **bundled into the application**, not fetched
from a CDN, because offline operation is a documented property of this product
(`docs/offline-mode.md`).

### 2.2 The path invariant

New Tauri commands wrap the file operations built in Phase 5: list, read, write,
create file, create directory, rename, delete, search.

Every command takes a **project id and a path relative to that project's root**.
`SafePath` is constructed in Rust from those two things. The frontend cannot
express an absolute path, a UNC path, or a traversal. The editor is the first
feature with a real reason to want to pass a full path, and it does not get to.

Symlinks and Windows junctions remain listable and deletable, and refused as the
target of every other operation.

### 2.3 Behaviour

In scope:

- Syntax highlighting selected by file extension.
- Tabs, dirty markers, `Ctrl+S`, and a confirmation before closing a dirty tab
  or navigating away from the view with unsaved buffers.
- Find and replace within the open file.
- Atomic save: write a temporary file in the same directory, then rename over
  the target.
- Refusal with an explanatory placeholder, not a hang or a garbled buffer, for
  files over 2 MiB and for binary files (a NUL byte in the first 8 KiB).
- Read-only while the project's status is `BUILDING` or `DELETING`.

Out of scope, deliberately: language servers, extensions, an integrated
terminal, and any git user interface.

Editing a running project's files does not restart it. The view states that a
restart is needed rather than bouncing a container behind the user's back.

---

## 3. Testing

Rust:

- `remote_url`: a table of hostile URLs — schemes, userinfo, loopback,
  `169.254.169.254`, private ranges, redirect chains ending in a private
  address, `https`→`http` downgrade.
- `git_clone`: clones against local bare repositories; ref selection; a
  repository with a submodule asserted to produce no submodule content; a
  repository with a hook asserted not to have run it; byte cap abort leaving no
  project and no staging directory.
- `http_archive`: an in-process test server serving a valid archive, an
  oversized body, a redirect into a private address, and a slow body that hits
  the timeout.
- Credentials: the `CHECK` asserted directly against SQLite with raw SQL; a
  round trip through encryption; a token asserted absent from a rendered log
  line and from every response type.
- Migration v2 → v3, and the schema-version guard.

TypeScript (Vitest):

- Tab and dirty-state reducer: open, switch, edit, save, close-while-dirty.
- Extension → language mapping.
- Binary and size detection at the boundary values.

Nothing here is gated on Docker, Linux or Discord. Any item that cannot be shown
to pass is reported as unverified, not as done.

---

## 4. Implementation order

1. Contracts (`api-types`) and generated TypeScript.
2. Migration to schema version 3.
3. `remote_url` validation, test-first.
4. `git_clone` and `http_archive` into staging with the existing entry rules.
5. Credential encryption and storage.
6. Tauri commands and the creation dialog's source picker.
7. Tauri file commands.
8. The Monaco editor view.
9. Documentation: `api-design.md`, `database-schema.md`, `architecture.md`,
   `offline-mode.md`, `testing-strategy.md`, `file-tree.md`, and the README's
   phase and verification tables.
