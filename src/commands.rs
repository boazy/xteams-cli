//! Command dispatch (two-tier: noun -> verb). Handlers return data values;
//! `output::render` is the sole writer.

mod auth;
mod calendar;
mod channel;
mod chat;
mod login;
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
        Command::Login => login::login(json).await,
        Command::Logout => login::logout(json).await,
        Command::Chat { verb } => chat::dispatch(verb, cookies, json).await,
        Command::Team { verb } => team::dispatch(verb, json).await,
        Command::Channel { verb } => channel::dispatch(verb, cookies, json).await,
        Command::Thread { verb } => thread::dispatch(verb, cookies, json).await,
        Command::Message { verb } => message::dispatch(verb, cookies, json).await,
        Command::User { verb } => user::dispatch(verb, json).await,
        Command::Calendar { verb } => calendar::dispatch(verb, json).await,
    }
}
