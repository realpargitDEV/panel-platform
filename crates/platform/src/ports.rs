//! Who is holding a TCP port.
//!
//! Answering "is this port free" is easy and portable: bind it. Answering "then
//! who has it" is neither, and it is the half that matters — a start refused
//! with "port 20001 is in use" tells a user nothing they could not already see,
//! where "port 20001 is held by node.exe (pid 24180)" tells them exactly which
//! window to close.
//!
//! There is no portable API for the second question, so each platform is asked
//! in the way it can answer:
//!
//! | Platform | Asked via |
//! | -------- | --------- |
//! | Windows  | `netstat -ano`, then the pid resolved to a name |
//! | macOS    | `lsof -nP -iTCP:<port> -sTCP:LISTEN` |
//! | Linux    | `ss -lptnH`, falling back to `lsof` |
//!
//! Every one of those is a subprocess, which is why this is only ever called on
//! the failure path. A start that succeeds does not pay for it.
//!
//! **Best effort by design.** `None` means "could not find out", never "nobody
//! has it" — the tool may be absent, the owning process may belong to another
//! user, or the output format may have changed. A caller must treat a `None`
//! as an unexplained conflict rather than as an absence of one, which is why
//! [`owner_of`] is separate from the bind test rather than replacing it.
//!
//! **Verified on Windows.** The macOS and Linux branches are written from the
//! documented output of the respective tools and have not been run against
//! them; the workspace has no machine of either kind.

use std::net::TcpListener;
use std::process::Command;

/// What is holding a port.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortOwner {
    pub pid: u32,
    /// The executable's name, when it could be resolved. `None` means the pid
    /// was found and the name was not, which is still worth reporting.
    pub program: Option<String>,
}

impl PortOwner {
    /// How to name this owner in a sentence shown to a person.
    pub fn describe(&self) -> String {
        match &self.program {
            Some(program) => format!("{program} (pid {})", self.pid),
            None => format!("process {}", self.pid),
        }
    }
}

/// Whether this machine will let a listener bind `port` on loopback right now.
///
/// The same test `project-manager`'s allocator uses, repeated here because a
/// port that was free when the project was created may not be free when it is
/// started — which is the entire reason a start-time check exists.
pub fn is_free(port: u16) -> bool {
    TcpListener::bind(("127.0.0.1", port)).is_ok()
}

/// Find what is listening on `port`, if it can be found out.
pub fn owner_of(port: u16) -> Option<PortOwner> {
    let pid = listening_pid(port)?;
    Some(PortOwner {
        pid,
        program: program_name(pid),
    })
}

/// The pid listening on `port`, asked in whichever way this platform answers.
fn listening_pid(port: u16) -> Option<u32> {
    #[cfg(windows)]
    {
        windows_listening_pid(port)
    }
    #[cfg(not(windows))]
    {
        unix_listening_pid(port)
    }
}

/// Parse `netstat -ano` for a LISTENING row on this port.
///
/// `-ano` rather than `-anob`: the `b` flag names the owning executable but
/// needs administrator rights, and a diagnostic that only works when elevated
/// is a diagnostic that does not work. The pid is enough — the name comes from
/// the process table afterwards.
#[cfg(windows)]
fn windows_listening_pid(port: u16) -> Option<u32> {
    let output = Command::new("netstat")
        .args(["-ano", "-p", "TCP"])
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&output.stdout);
    parse_netstat(&text, port)
}

/// Pull the pid out of netstat's listening rows.
///
/// Split out from the command so it can be tested against real output on any
/// platform, which is the only part of the Windows branch that has any logic in
/// it.
#[cfg(any(windows, test))]
fn parse_netstat(text: &str, port: u16) -> Option<u32> {
    let suffix = format!(":{port}");
    text.lines()
        .filter(|line| line.contains("LISTENING"))
        .find_map(|line| {
            let mut fields = line.split_whitespace();
            // Proto, Local Address, Foreign Address, State, PID
            let _proto = fields.next()?;
            let local = fields.next()?;
            // Endswith rather than contains: `:8080` must not match `:18080`,
            // and a local address is always `address:port`.
            if !local.ends_with(&suffix) {
                return None;
            }
            fields.last()?.parse().ok()
        })
}

/// Ask `ss` and then `lsof`, taking the first that answers.
///
/// Two rather than one because neither is guaranteed: `ss` is standard on
/// modern Linux and absent on macOS, `lsof` is usual on macOS and often not
/// installed in a minimal container.
#[cfg(not(windows))]
fn unix_listening_pid(port: u16) -> Option<u32> {
    ss_listening_pid(port).or_else(|| lsof_listening_pid(port))
}

/// `ss -lptnH` prints one listener per line, ending in `users:(("node",pid=123,fd=20))`.
#[cfg(not(windows))]
fn ss_listening_pid(port: u16) -> Option<u32> {
    let output = Command::new("ss")
        .args(["-lptnH", "sport", "=", &format!(":{port}")])
        .output()
        .ok()?;
    parse_ss(&String::from_utf8_lossy(&output.stdout))
}

#[cfg(any(not(windows), test))]
fn parse_ss(text: &str) -> Option<u32> {
    let marker = "pid=";
    let start = text.find(marker)? + marker.len();
    let rest = text.get(start..)?;
    let end = rest
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(rest.len());
    rest.get(..end)?.parse().ok()
}

/// `lsof -nP -iTCP:<port> -sTCP:LISTEN -Fp` prints `p<pid>` on its own line.
///
/// The `-F` field output is used rather than the human table precisely because
/// it is the format lsof promises not to change.
#[cfg(not(windows))]
fn lsof_listening_pid(port: u16) -> Option<u32> {
    let output = Command::new("lsof")
        .args(["-nP", &format!("-iTCP:{port}"), "-sTCP:LISTEN", "-Fp"])
        .output()
        .ok()?;
    parse_lsof(&String::from_utf8_lossy(&output.stdout))
}

#[cfg(any(not(windows), test))]
fn parse_lsof(text: &str) -> Option<u32> {
    text.lines()
        .find_map(|line| line.strip_prefix('p')?.trim().parse().ok())
}

/// The executable name for a pid, from the process table.
///
/// `sysinfo` rather than another subprocess: the process list is already a
/// dependency, and refreshing one pid is cheap next to spawning `tasklist`.
fn program_name(pid: u32) -> Option<String> {
    use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};

    let mut system = System::new();
    let pid = Pid::from_u32(pid);
    system.refresh_processes_specifics(
        ProcessesToUpdate::Some(&[pid]),
        true,
        ProcessRefreshKind::nothing(),
    );
    system
        .process(pid)
        .map(|process| process.name().to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A port nothing holds is free, and one this test holds is not. The bind
    /// test is the load-bearing half — the owner lookup only ever decorates it.
    #[test]
    fn a_bound_port_is_not_free_and_a_released_one_is() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();

        assert!(!is_free(port), "a port this test is holding read as free");

        drop(listener);
        assert!(is_free(port), "a released port read as still held");
    }

    /// The owner of a port this very process holds is this very process. The
    /// one case where the expected answer is knowable on any machine.
    #[test]
    fn the_owner_of_a_port_this_process_holds_is_this_process() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();

        // Best effort: the tool may not be installed, and that is a documented
        // `None` rather than a failure. When it does answer, it has to be right.
        if let Some(owner) = owner_of(port) {
            assert_eq!(
                owner.pid,
                std::process::id(),
                "the lookup named a process other than the one holding the port"
            );
        }

        drop(listener);
    }

    /// The parse is the only logic in the Windows branch, so it is pinned
    /// against real output rather than against the command being run.
    #[test]
    fn a_netstat_listing_yields_the_listening_pid() {
        let text = "\
Active Connections

  Proto  Local Address          Foreign Address        State           PID
  TCP    0.0.0.0:135            0.0.0.0:0              LISTENING       1160
  TCP    127.0.0.1:20001        0.0.0.0:0              LISTENING       24180
  TCP    127.0.0.1:20002        0.0.0.0:0              LISTENING       9002
  TCP    127.0.0.1:52001        127.0.0.1:20001        ESTABLISHED     7777
";
        assert_eq!(parse_netstat(text, 20001), Some(24180));
        assert_eq!(parse_netstat(text, 20002), Some(9002));
        assert_eq!(parse_netstat(text, 30000), None);
    }

    /// `:8080` must not match `:18080`. Substring matching here would name the
    /// wrong process, which is worse than naming none.
    #[test]
    fn a_port_is_not_matched_by_the_end_of_a_longer_one() {
        let text = "  TCP    127.0.0.1:18080        0.0.0.0:0              LISTENING       1234\n";
        assert_eq!(parse_netstat(text, 8080), None);
        assert_eq!(parse_netstat(text, 18080), Some(1234));
    }

    /// An established connection to the port is not a listener on it. Matching
    /// one would blame whoever was talking to the project rather than the
    /// project.
    #[test]
    fn a_connection_to_the_port_is_not_its_owner() {
        let text = "  TCP    127.0.0.1:52001        127.0.0.1:20001        ESTABLISHED     7777\n";
        assert_eq!(parse_netstat(text, 20001), None);
    }

    #[test]
    fn an_ss_listing_yields_the_listening_pid() {
        let text = "LISTEN 0 511 127.0.0.1:20001 0.0.0.0:* users:((\"node\",pid=24180,fd=20))\n";
        assert_eq!(parse_ss(text), Some(24180));
        assert_eq!(parse_ss("LISTEN 0 511 127.0.0.1:20001 0.0.0.0:*\n"), None);
    }

    #[test]
    fn an_lsof_field_listing_yields_the_listening_pid() {
        assert_eq!(parse_lsof("p24180\nf20\n"), Some(24180));
        assert_eq!(parse_lsof(""), None);
    }

    /// The describe string is what a user reads, so both shapes are pinned.
    #[test]
    fn an_owner_names_itself_in_a_sentence() {
        assert_eq!(
            PortOwner {
                pid: 24180,
                program: Some("node.exe".to_string())
            }
            .describe(),
            "node.exe (pid 24180)"
        );
        assert_eq!(
            PortOwner {
                pid: 24180,
                program: None
            }
            .describe(),
            "process 24180"
        );
    }
}
