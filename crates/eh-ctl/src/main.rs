//! # eh-ctl
//!
//! Command-line tool for talking to a running EventHorizon gateway. Speaks
//! to the gateway's public REST surface — never to backend databases
//! directly (zero-trust §15).
//!
//! Phase 1 subcommands:
//!   * `eh-ctl health [--gateway URL]` — `GET /healthz`.
//!   * `eh-ctl intent send <file|-> [--token TOKEN] [--gateway URL]` —
//!     POST a JSON intent body and print the response.
//!
//! `--gateway` and `--token` accept env defaults (`EH_GATEWAY_URL` and
//! `EH_AGENT_TOKEN`). No hardcoded values.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

use std::io::Read;
use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{anyhow, Context, Result};
use clap::{Parser, Subcommand};
use eh_core::Intent;
use eh_protocol::{IntentEnvelope, ResponseEnvelope};

const DEFAULT_GATEWAY: &str = "http://127.0.0.1:8080";

#[derive(Debug, Parser)]
#[command(
    name = "eh-ctl",
    about = "EventHorizon command-line client (talks to a running gateway over REST)",
    version
)]
struct Cli {
    /// Gateway base URL. Falls back to `EH_GATEWAY_URL` env, then localhost.
    #[arg(long, env = "EH_GATEWAY_URL", default_value = DEFAULT_GATEWAY, global = true)]
    gateway: String,

    /// Agent bearer token. Falls back to `EH_AGENT_TOKEN` env.
    #[arg(long, env = "EH_AGENT_TOKEN", default_value = "", global = true)]
    token: String,

    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Debug, Subcommand)]
enum Cmd {
    /// Probe the gateway's `GET /healthz`.
    Health,
    /// `intent` family of subcommands.
    Intent {
        #[command(subcommand)]
        cmd: IntentCmd,
    },
}

#[derive(Debug, Subcommand)]
enum IntentCmd {
    /// POST an intent JSON file (or stdin via `-`) to `/v1/intent`.
    Send {
        /// Path to a JSON file with the `Intent` body. `-` reads stdin.
        file: PathBuf,
    },
}

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(cli).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("eh-ctl: {e:#}");
            ExitCode::from(2)
        }
    }
}

async fn run(cli: Cli) -> Result<()> {
    match cli.cmd {
        Cmd::Health => health(&cli.gateway).await,
        Cmd::Intent {
            cmd: IntentCmd::Send { file },
        } => intent_send(&cli.gateway, &cli.token, &file).await,
    }
}

async fn health(gateway: &str) -> Result<()> {
    let url = format!("{}/healthz", gateway.trim_end_matches('/'));
    let resp = reqwest::Client::new()
        .get(&url)
        .send()
        .await
        .with_context(|| format!("GET {url}"))?;
    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    println!("{status} {body}");
    if status.is_success() {
        Ok(())
    } else {
        Err(anyhow!("gateway unhealthy (HTTP {status})"))
    }
}

async fn intent_send(gateway: &str, token: &str, file: &PathBuf) -> Result<()> {
    if token.is_empty() {
        return Err(anyhow!(
            "no agent token; pass --token or set EH_AGENT_TOKEN"
        ));
    }

    let intent: Intent = read_intent(file)?;
    let envelope = IntentEnvelope {
        agent_token: token.to_string(),
        intent,
    };

    let url = format!("{}/v1/intent", gateway.trim_end_matches('/'));
    let resp = reqwest::Client::new()
        .post(&url)
        .json(&envelope)
        .send()
        .await
        .with_context(|| format!("POST {url}"))?;

    let status = resp.status();
    let response: ResponseEnvelope = resp
        .json()
        .await
        .context("decoding gateway response as ResponseEnvelope JSON")?;

    println!("{}", serde_json::to_string_pretty(&response)?);

    if status.is_success() && response.is_success() {
        Ok(())
    } else {
        Err(anyhow!("intent failed (HTTP {status})"))
    }
}

fn read_intent(file: &PathBuf) -> Result<Intent> {
    let raw = if file.to_str() == Some("-") {
        let mut buf = String::new();
        std::io::stdin()
            .read_to_string(&mut buf)
            .context("read intent JSON from stdin")?;
        buf
    } else {
        std::fs::read_to_string(file).with_context(|| format!("read intent file {file:?}"))?
    };
    let intent: Intent = serde_json::from_str(&raw).context("parse intent JSON")?;
    Ok(intent)
}
