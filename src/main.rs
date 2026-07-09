//! xteams — unofficial Microsoft Teams CLI using the local desktop app's
//! credentials (no Microsoft Graph, no Azure app registration required).

mod api;
mod auth;
mod cli;
mod commands;
mod creds;
mod error;
mod link;
mod model;
mod output;
mod seed;

use clap::Parser;

#[tokio::main]
async fn main() -> eyre::Result<()> {
    color_eyre::install()?;
    let cli = cli::Cli::parse();
    commands::dispatch(cli).await
}
