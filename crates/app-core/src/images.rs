//! The `Dockerfile` a project gets when it does not bring its own.
//!
//! One function per concern: [`dockerfile_for`] produces the text,
//! [`starter_files`] produces the "hello world" a genuinely empty project needs.
//! Neither touches the disk — `lifecycle::scaffold` does that, and never
//! overwrites, because once a project exists its files belong to the user.
//!
//! ## Why these are generated rather than read from `docker/templates/`
//!
//! The manifests there are the reviewed, allow-listed description of what a
//! project may ask for, and they are the right long-term home. They are also not
//! deployed alongside the binary yet. Until the installer ships them, a project
//! needs *a* Dockerfile, and one generated from values this application already
//! validated is better than a path that does not resolve at runtime.
//!
//! ## What every image here has in common
//!
//! - A **pinned base image**. `node:22-slim` being republished must not silently
//!   change what a project runs.
//! - **uid 10001**, non-root, identical across every runtime, so files a
//!   container writes are predictable on the host.
//! - A **read-only root filesystem** at run time, which is why nothing here may
//!   expect to write outside `/app` or `/tmp`.
//! - `CMD ["sh", "-c", "exec …"]`. The `exec` matters: without it the shell stays
//!   as PID 1, swallows `SIGTERM`, and every stop waits for the kill timeout.
//!
//! ## What has not been checked
//!
//! **None of these images has ever been built.** There is no Docker daemon on
//! the machine this was written on. They are written from the base images'
//! documented conventions, and they are unverified — see the README's
//! verification table.

/// The values a generated `Dockerfile` needs. All of them have already been
/// through `runtime_plan`, so none is free text from a project's own files.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImageSpec<'a> {
    pub runtime: &'a str,
    pub install_command: Option<&'a str>,
    pub build_command: Option<&'a str>,
    pub start_command: &'a str,
    /// Where a static site's files are, relative to the project.
    pub publish_dir: Option<&'a str>,
}

/// Quote a command for a JSON string inside `CMD`.
///
/// Backslashes first, then quotes: reversing them would double-escape.
fn json(value: &str) -> String {
    let escaped = value.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{escaped}\"")
}

/// `CMD ["sh", "-c", "exec <command>"]`.
fn cmd(start: &str) -> String {
    format!("CMD [\"sh\", \"-c\", {}]\n", json(&format!("exec {start}")))
}

/// A `RUN` line, or nothing when there is no command.
///
/// `|| true` is deliberately *not* used. The existing Node and Python templates
/// had it; it turns a failed dependency install into a container that starts and
/// then fails mysteriously at runtime, which is worse than a build that stops and
/// says why.
fn run(command: Option<&str>) -> String {
    match command {
        Some(command) => format!("RUN {command}\n"),
        None => String::new(),
    }
}

const NON_ROOT: &str = "\
# A fixed non-root uid, identical across every runtime, so files written by a\n\
# container are predictable on the host.\n\
RUN (getent group 10001 || groupadd --gid 10001 app) \\\n\
 && (getent passwd 10001 || useradd --uid 10001 --gid 10001 --no-create-home --shell /usr/sbin/nologin app)\n\
USER 10001:10001\n";

/// The same, for Alpine-based images, whose BusyBox tools are `addgroup` and
/// `adduser`.
///
/// No `|| true`. An earlier version had it, which would have turned "the user
/// could not be created" into an image that silently runs as root — and because
/// the base images are pinned, we know uid 10001 is free.
const NON_ROOT_ALPINE: &str = "\
RUN addgroup -g 10001 app \\\n\
 && adduser -D -u 10001 -G app app\n\
USER 10001:10001\n";

const HEADER: &str =
    "# Managed by Panel Platform. Safe to edit — it is written only when absent.\n";

/// The `Dockerfile` for one project.
pub fn dockerfile_for(spec: &ImageSpec<'_>) -> String {
    match spec.runtime {
        "NODEJS" => interpreted(spec, "node:22.14.0-bookworm-slim", &["package*.json"]),
        "TYPESCRIPT" => compiled_node(spec),
        "BUN" => interpreted(spec, "oven/bun:1.1.42-slim", &["package.json", "bun.lock*"]),
        "DENO" => interpreted(spec, "denoland/deno:bin-2.1.4", &[]),
        "PYTHON" => interpreted(spec, "python:3.12.8-slim-bookworm", &["requirements.txt"]),
        "PHP" => interpreted(spec, "php:8.3.15-cli-bookworm", &["composer.*"]),
        "RUBY" => interpreted(spec, "ruby:3.3.6-slim-bookworm", &["Gemfile*"]),
        "GO" => compiled(
            spec,
            "golang:1.23.4-bookworm",
            "gcr.io/distroless/base-debian12:nonroot",
            "/app/server",
        ),
        "RUST" => compiled_rust(spec),
        "JAVA" => compiled(
            spec,
            "maven:3.9.9-eclipse-temurin-21",
            "eclipse-temurin:21.0.5_11-jre-jammy",
            "/app/app.jar",
        ),
        "DOTNET" => compiled(
            spec,
            "mcr.microsoft.com/dotnet/sdk:8.0",
            "mcr.microsoft.com/dotnet/aspnet:8.0",
            "/app/publish",
        ),
        "STATIC" => static_site(spec),
        "POLYGLOT" => polyglot(spec),
        // An unknown runtime cannot reach here through `runtime_plan`, and
        // guessing would produce an image that fails obscurely. Node is the
        // safest fallback and the comment says what happened.
        other => format!(
            "{HEADER}# Unrecognised runtime `{other}`; built as Node.js.\n{}",
            interpreted(spec, "node:22.14.0-bookworm-slim", &["package*.json"])
                .trim_start_matches(HEADER)
        ),
    }
}

/// One stage: install, copy, run. For runtimes with no build artefact to leave
/// behind.
fn interpreted(spec: &ImageSpec<'_>, base: &str, manifests: &[&str]) -> String {
    let mut text = String::from(HEADER);
    text.push_str(&format!("FROM {base}\nWORKDIR /app\n\n"));

    if !manifests.is_empty() {
        text.push_str(
            "# Manifests first, so a dependency install is cached independently of source.\n",
        );
        text.push_str(&format!("COPY {} ./\n", manifests.join(" ")));
    }
    text.push_str(&run(spec.install_command));
    text.push_str("\nCOPY . .\n");
    text.push_str(&run(spec.build_command));

    text.push('\n');
    // Keyed on the distribution, not on the image's name: `oven/bun:…-slim` is
    // Debian-based despite what "bun" suggests, and picking the BusyBox commands
    // for it would produce an image that fails to build.
    text.push_str(if base.contains("alpine") {
        NON_ROOT_ALPINE
    } else {
        NON_ROOT
    });
    text.push('\n');
    text.push_str(&cmd(spec.start_command));
    text
}

/// Two stages, so the toolchain never reaches the runtime image: a smaller image
/// is a smaller attack surface, and shipping a compiler with a service is how a
/// container ends up able to build an exploit in place.
fn compiled(spec: &ImageSpec<'_>, builder: &str, runtime: &str, artefact: &str) -> String {
    let mut text = String::from(HEADER);
    text.push_str(&format!("FROM {builder} AS build\nWORKDIR /src\n\n"));
    text.push_str("COPY . .\n");
    text.push_str(&run(spec.install_command));
    text.push_str(&run(spec.build_command));

    text.push_str(&format!("\nFROM {runtime}\nWORKDIR /app\n\n"));
    text.push_str(&format!(
        "# Only the built artefact crosses the stage boundary.\nCOPY --from=build {artefact} {artefact}\n"
    ));

    // The distroless and Microsoft runtime images already run unprivileged and
    // have no shell to add a user with.
    if runtime.contains("distroless") {
        text.push_str("\nUSER nonroot\n");
        // No shell in a distroless image, so the exec-form wrapper cannot be used.
        text.push_str(&format!("\nCMD [{}]\n", json(spec.start_command)));
        return text;
    }

    text.push('\n');
    text.push_str(NON_ROOT);
    text.push('\n');
    text.push_str(&cmd(spec.start_command));
    text
}

/// Rust needs its artefact copied from Cargo's target directory, whose path
/// depends on the profile rather than on the command.
fn compiled_rust(spec: &ImageSpec<'_>) -> String {
    let mut text = String::from(HEADER);
    text.push_str("FROM rust:1.83.0-bookworm AS build\nWORKDIR /src\n\n");
    text.push_str("COPY . .\n");
    text.push_str(&run(spec.build_command));
    text.push_str(
        "# The binary's name comes from Cargo.toml, so it is found rather than assumed.\n\
         RUN mkdir -p /out && find target/release -maxdepth 1 -type f -executable \\\n\
             -exec cp {} /out/server \\; -quit\n",
    );

    text.push_str("\nFROM debian:bookworm-20241202-slim\nWORKDIR /app\n\n");
    text.push_str("COPY --from=build /out/server /app/server\n");
    text.push('\n');
    text.push_str(NON_ROOT);
    text.push('\n');
    text.push_str(&cmd(spec.start_command));
    text
}

/// TypeScript: build with dev dependencies present, then ship without them.
fn compiled_node(spec: &ImageSpec<'_>) -> String {
    let mut text = String::from(HEADER);
    text.push_str("FROM node:22.14.0-bookworm-slim AS build\nWORKDIR /app\n\n");
    text.push_str("COPY package*.json ./\n");
    text.push_str(&run(spec.install_command));
    text.push_str("COPY . .\n");
    text.push_str(&run(spec.build_command));

    text.push_str("\nFROM node:22.14.0-bookworm-slim\nWORKDIR /app\nENV NODE_ENV=production\n\n");
    text.push_str("COPY package*.json ./\n");
    text.push_str("# Production dependencies only: the compiler stays in the build stage.\n");
    text.push_str("RUN npm ci --omit=dev || npm install --omit=dev\n");
    let published = spec.publish_dir.unwrap_or("dist");
    text.push_str(&format!(
        "COPY --from=build /app/{published} ./{published}\n"
    ));
    text.push('\n');
    text.push_str(NON_ROOT);
    text.push('\n');
    text.push_str(&cmd(spec.start_command));
    text
}

/// A static site is served, not run.
fn static_site(spec: &ImageSpec<'_>) -> String {
    let published = spec.publish_dir.unwrap_or("public");
    format!(
        "{HEADER}FROM nginx:1.27.3-alpine\n\n\
         # nginx's own image runs its workers unprivileged; the master needs root\n\
         # to bind port 80 inside the container's own namespace.\n\
         COPY {published}/ /usr/share/nginx/html/\n\
         EXPOSE 80\n"
    )
}

/// Several toolchains in one image.
///
/// Large and slow to build, which is the honest cost of "this project needs Node
/// and Python". Chosen only when a tree shows evidence of more than one language,
/// and the interface says so before it is used.
fn polyglot(spec: &ImageSpec<'_>) -> String {
    let mut text = String::from(HEADER);
    text.push_str(
        "# Several languages were detected in this project, so this image carries\n\
         # more than one toolchain. It is correspondingly large. If only one of\n\
         # them actually runs, choose that runtime instead and rebuild.\n",
    );
    text.push_str("FROM debian:bookworm-20241202-slim\nWORKDIR /app\n\n");
    text.push_str(
        "ENV DEBIAN_FRONTEND=noninteractive\n\
         RUN apt-get update \\\n\
          && apt-get install -y --no-install-recommends \\\n\
             ca-certificates curl git \\\n\
             nodejs npm \\\n\
             python3 python3-pip python3-venv \\\n\
             golang-go \\\n\
             default-jre-headless \\\n\
             php-cli \\\n\
             ruby-full \\\n\
          && rm -rf /var/lib/apt/lists/*\n\n",
    );
    text.push_str("COPY . .\n");
    text.push_str(&run(spec.install_command));
    text.push_str(&run(spec.build_command));
    text.push('\n');
    text.push_str(NON_ROOT);
    text.push('\n');
    text.push_str(&cmd(spec.start_command));
    text
}

/// The "hello world" a genuinely empty project needs, as `(path, contents)`.
///
/// Only ever written when absent, so a fetched repository never receives any of
/// it — its own files are already there.
pub fn starter_files(runtime: &str) -> Vec<(&'static str, &'static str)> {
    match runtime {
        "NODEJS" => vec![
            ("index.js", "console.log('Hello from Panel Platform');\n"),
            (
                "package.json",
                "{\n  \"name\": \"project\",\n  \"private\": true,\n  \"version\": \"1.0.0\",\n  \"main\": \"index.js\"\n}\n",
            ),
        ],
        "TYPESCRIPT" => vec![
            (
                "src/index.ts",
                "console.log('Hello from Panel Platform');\n",
            ),
            (
                "package.json",
                "{\n  \"name\": \"project\",\n  \"private\": true,\n  \"version\": \"1.0.0\",\n  \"scripts\": {\n    \"build\": \"tsc\",\n    \"start\": \"node dist/index.js\"\n  }\n}\n",
            ),
            (
                "tsconfig.json",
                "{\n  \"compilerOptions\": {\n    \"target\": \"ES2022\",\n    \"module\": \"NodeNext\",\n    \"outDir\": \"dist\",\n    \"strict\": true\n  },\n  \"include\": [\"src\"]\n}\n",
            ),
        ],
        "BUN" => vec![
            ("index.ts", "console.log('Hello from Panel Platform');\n"),
            // `bunfig.toml` is what makes this a Bun project rather than a Node
            // one, and detection reads it back to decide the runtime.
            ("bunfig.toml", "# Bun configuration.\n"),
            (
                "package.json",
                "{\n  \"name\": \"project\",\n  \"private\": true,\n  \"version\": \"1.0.0\"\n}\n",
            ),
        ],
        "DENO" => vec![
            ("main.ts", "console.log('Hello from Panel Platform');\n"),
            ("deno.json", "{\n  \"tasks\": {}\n}\n"),
        ],
        "PYTHON" => vec![
            ("main.py", "print('Hello from Panel Platform', flush=True)\n"),
            ("requirements.txt", ""),
        ],
        "GO" => vec![
            (
                "main.go",
                "package main\n\nimport \"fmt\"\n\nfunc main() {\n\tfmt.Println(\"Hello from Panel Platform\")\n}\n",
            ),
            ("go.mod", "module project\n\ngo 1.23\n"),
        ],
        "RUST" => vec![
            (
                "src/main.rs",
                "fn main() {\n    println!(\"Hello from Panel Platform\");\n}\n",
            ),
            (
                "Cargo.toml",
                "[package]\nname = \"project\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
            ),
        ],
        "JAVA" => vec![
            (
                "src/main/java/Main.java",
                "public class Main {\n    public static void main(String[] args) {\n        System.out.println(\"Hello from Panel Platform\");\n    }\n}\n",
            ),
            // Without a build file this is a directory of Java source, and
            // detection would not call it a Java project at all.
            (
                "pom.xml",
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<project xmlns=\"http://maven.apache.org/POM/4.0.0\">\n  <modelVersion>4.0.0</modelVersion>\n  <groupId>platform.panel</groupId>\n  <artifactId>project</artifactId>\n  <version>1.0.0</version>\n  <properties>\n    <maven.compiler.release>21</maven.compiler.release>\n  </properties>\n</project>\n",
            ),
        ],
        "PHP" => vec![(
            "index.php",
            "<?php\n\necho \"Hello from Panel Platform\\n\";\n",
        )],
        "RUBY" => vec![
            ("app.rb", "puts 'Hello from Panel Platform'\n"),
            ("Gemfile", "source 'https://rubygems.org'\n"),
        ],
        "DOTNET" => vec![
            (
                "Program.cs",
                "System.Console.WriteLine(\"Hello from Panel Platform\");\n",
            ),
            // The project file is the marker, and its stem becomes the assembly
            // name the start command expects.
            (
                "app.csproj",
                "<Project Sdk=\"Microsoft.NET.Sdk\">\n  <PropertyGroup>\n    <OutputType>Exe</OutputType>\n    <TargetFramework>net8.0</TargetFramework>\n  </PropertyGroup>\n</Project>\n",
            ),
        ],
        "STATIC" => vec![(
            "public/index.html",
            "<!doctype html>\n<html lang=\"en\">\n  <head>\n    <meta charset=\"utf-8\" />\n    <title>Panel Platform</title>\n  </head>\n  <body>\n    <h1>It works</h1>\n  </body>\n</html>\n",
        )],
        // Nothing sensible to scaffold: a polyglot project is by definition one
        // that already has files in more than one language.
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use project_host_project_manager::detection::Runtime;

    fn spec(runtime: &str) -> ImageSpec<'_> {
        ImageSpec {
            runtime,
            install_command: Some("install-things"),
            build_command: Some("build-things"),
            start_command: "start-things",
            publish_dir: None,
        }
    }

    #[test]
    fn every_runtime_produces_a_dockerfile() {
        for runtime in Runtime::ALL {
            let text = dockerfile_for(&spec(runtime.as_str()));
            assert!(
                text.starts_with(HEADER),
                "{} has no header",
                runtime.as_str()
            );
            assert!(
                text.contains("FROM "),
                "{} has no base image",
                runtime.as_str()
            );
            // A static site is the one runtime with no CMD of its own: nginx's
            // own image already has the right one, and overriding it would mean
            // reimplementing its signal handling and worker startup.
            assert!(
                text.contains("CMD ") || runtime.as_str() == "STATIC",
                "{} never starts anything",
                runtime.as_str()
            );
        }
    }

    #[test]
    fn every_base_image_is_pinned_to_a_version() {
        // `node:22-slim` being republished must not change what a project runs.
        for runtime in Runtime::ALL {
            let text = dockerfile_for(&spec(runtime.as_str()));
            for line in text.lines().filter(|line| line.starts_with("FROM ")) {
                let image = line
                    .trim_start_matches("FROM ")
                    .split(' ')
                    .next()
                    .unwrap_or("");
                assert!(
                    image.contains(':'),
                    "{} uses an untagged image: {line}",
                    runtime.as_str()
                );
                assert!(
                    !image.ends_with(":latest"),
                    "{} uses :latest: {line}",
                    runtime.as_str()
                );
            }
        }
    }

    #[test]
    fn no_image_runs_as_root() {
        // Every runtime either adds uid 10001 or uses a base image that is
        // already unprivileged. nginx is the exception and says why in a comment.
        for runtime in Runtime::ALL {
            if runtime.as_str() == "STATIC" {
                continue;
            }
            let text = dockerfile_for(&spec(runtime.as_str()));
            assert!(
                text.contains("USER 10001:10001") || text.contains("USER nonroot"),
                "{} runs as root",
                runtime.as_str()
            );
        }
    }

    #[test]
    fn the_start_command_replaces_the_shell_rather_than_being_wrapped_by_it() {
        // Without `exec`, the shell stays PID 1, swallows SIGTERM, and every stop
        // waits out the kill timeout.
        let text = dockerfile_for(&spec("NODEJS"));
        assert!(
            text.contains(r#"CMD ["sh", "-c", "exec start-things"]"#),
            "{text}"
        );
    }

    #[test]
    fn a_start_command_containing_quotes_stays_valid_json() {
        let text = dockerfile_for(&ImageSpec {
            start_command: r#"node -e "console.log('hi')""#,
            ..spec("NODEJS")
        });
        let cmd_line = text
            .lines()
            .find(|line| line.starts_with("CMD "))
            .expect("a CMD line");
        let payload = cmd_line.trim_start_matches("CMD ");
        serde_json::from_str::<Vec<String>>(payload)
            .unwrap_or_else(|error| panic!("{cmd_line} is not valid JSON: {error}"));
    }

    #[test]
    fn a_failed_dependency_install_stops_the_build() {
        // The old templates ended their install with `|| true`, which turns a
        // failed install into a container that starts and then fails obscurely.
        for runtime in Runtime::ALL {
            let text = dockerfile_for(&spec(runtime.as_str()));
            assert!(
                !text.contains("|| true"),
                "{} swallows a failed build step",
                runtime.as_str()
            );
        }
    }

    #[test]
    fn compiled_runtimes_do_not_ship_their_toolchain() {
        // Two stages, and the runtime stage is not the builder.
        for runtime in ["GO", "RUST", "JAVA", "DOTNET", "TYPESCRIPT"] {
            let text = dockerfile_for(&spec(runtime));
            let froms: Vec<&str> = text
                .lines()
                .filter(|line| line.starts_with("FROM "))
                .collect();
            assert!(
                froms.len() >= 2,
                "{runtime} builds in a single stage: {froms:?}"
            );
            assert!(
                text.contains("COPY --from=build"),
                "{runtime} does not copy an artefact out of its build stage"
            );
        }
    }

    #[test]
    fn a_static_site_serves_its_publish_directory() {
        let text = dockerfile_for(&ImageSpec {
            publish_dir: Some("web"),
            ..spec("STATIC")
        });
        assert!(text.contains("COPY web/ /usr/share/nginx/html/"), "{text}");
    }

    #[test]
    fn an_unknown_runtime_says_what_it_did() {
        // Reachable only if `runtime_plan` and this module disagree, which is
        // worth leaving a trace of in the file itself.
        let text = dockerfile_for(&spec("COBOL"));
        assert!(text.contains("Unrecognised runtime `COBOL`"), "{text}");
        assert!(text.contains("FROM node:"), "{text}");
    }

    #[test]
    fn a_starter_file_set_matches_what_detection_would_then_find() {
        // The round trip that matters for an empty project: scaffold it, and the
        // detector must recognise what was written as the runtime it was written
        // for. Otherwise a user creates an empty Go project and the next
        // detection calls it something else.
        for runtime in Runtime::ALL {
            let files = starter_files(runtime.as_str());
            if files.is_empty() {
                continue;
            }

            let directory = tempfile::tempdir().expect("temp dir");
            for (path, contents) in &files {
                let full = directory.path().join(path);
                if let Some(parent) = full.parent() {
                    std::fs::create_dir_all(parent).expect("parent");
                }
                std::fs::write(full, contents).expect("write");
            }

            let found = project_host_project_manager::detection::signals(directory.path());
            assert!(
                found.contains(&runtime),
                "the starter files for {} are detected as {:?}",
                runtime.as_str(),
                found
            );
        }
    }
}
