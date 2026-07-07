//! Command dispatch (two-tier: noun -> verb). Handlers return data values;
//! `output::render` is the sole writer.

mod auth;
mod channel;
mod chat;
mod message;
mod team;
mod thread;
mod user;

use eyre::Result;

use crate::cli::{Cli, Command};
use crate::output::render;

pub async fn dispatch(cli: Cli) -> Result<()> {
    let cookies_owned = cli.cookies.clone();
    let cookies = cookies_owned.as_deref();
    let json = cli.json;
    match cli.command {
        Command::Auth => render(&auth::status(cookies).await?, json),
        Command::Chat { verb } => chat::dispatch(verb, cookies, json).await,
        Command::Team { verb } => team::dispatch(verb, cookies, json).await,
        Command::Channel { verb } => channel::dispatch(verb, cookies, json).await,
        Command::Thread { verb } => thread::dispatch(verb, cookies, json).await,
        Command::Message { verb } => message::dispatch(verb, cookies, json).await,
        Command::User { verb } => user::dispatch(verb, cookies, json).await,
    }
}
