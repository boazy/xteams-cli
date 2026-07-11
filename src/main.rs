//! xteams — unofficial Microsoft Teams CLI using the local desktop app's
//! credentials (no Microsoft Graph, no Azure app registration required).

mod api;
mod auth;
mod cli;
mod commands;
mod content;
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
    let (args, pandoc_args) = content::split_pandoc_args(std::env::args().collect());
    let mut cli = cli::Cli::parse_from(args);
    cli.pandoc_args = pandoc_args;
    commands::dispatch(cli).await
}
