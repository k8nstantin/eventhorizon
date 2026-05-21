//! # eh-ctl
//!
//! EventHorizon CLI. Subcommands (`start`, `intent send`, `config validate`,
//! `health`) land in Phase 1; this Phase 0 stub exists so the binary builds
//! and the deployment topology is in place.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

fn main() -> std::process::ExitCode {
    eprintln!("eh-ctl: subcommands arrive in Phase 1");
    eprintln!("       see: https://github.com/k8nstantin/eventhorizon/issues/2");
    std::process::ExitCode::from(2)
}
