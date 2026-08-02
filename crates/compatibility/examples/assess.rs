//! Report what this machine is, what tier it lands in, and which Docker
//! artefact it would be offered.
//!
//! `cargo run -p project-host-compatibility --example assess`
//!
//! The counterpart to `project-host-platform`'s `scan` example: that one shows
//! what was measured, this one shows what was concluded from it. Between them a
//! support request can carry both halves.

fn main() {
    use project_host_platform::SystemProbe;

    let snapshot = project_host_platform::SystemScanner.snapshot();
    let assessment = project_host_compatibility::assess(&snapshot);

    println!("tier             {}", assessment.tier.as_str());
    println!("memory_limit_mb  {}", assessment.defaults.memory_limit_mb);
    println!("cpu_limit_cores  {}", assessment.defaults.cpu_limit_cores);
    println!("process_limit    {}", assessment.defaults.process_limit);

    match project_host_compatibility::select(&snapshot) {
        project_host_compatibility::Selection::Artifact(artifact) => {
            println!("\nwould install    {}", artifact.product.display_name());
            println!("artefact         {}", artifact.id);
        }
        project_host_compatibility::Selection::Blocked(blockers) => {
            println!("\nno artefact applies to this machine:");
            for blocker in &blockers {
                println!("  - {blocker}");
            }
        }
    }
}
