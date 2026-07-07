//! xteams — unofficial Microsoft Teams CLI using the local desktop app's
//! credentials (no Microsoft Graph, no Azure app registration required).

mod api;
mod auth;
mod cli;
mod commands;
mod creds;
mod error;
mod model;
mod output;

use clap::Parser;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = cli::Cli::parse();
    commands::dispatch(cli).await
}
