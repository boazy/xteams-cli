//! Command dispatch (two-tier: noun -> verb). Handlers return data values;
//! `output::render` is the sole writer. A thin top-level wrapper catches an
//! authentication 401 from any command, evicts exactly the cached credential that was
//! rejected, and asks the user to re-run (the next run re-mints it from the FRT).

mod auth;
mod calendar;
mod channel;
mod chat;
mod message;
mod team;
mod thread;
mod user;

use eyre::Result;

use crate::cli::{Cli, Command};

pub async fn dispatch(cli: Cli) -> Result<()> {
    let interaction = crate::auth::AuthInteraction::from_json(cli.json);
    match dispatch_inner(cli).await {
        Ok(()) => Ok(()),
        Err(err) => match crate::auth::credential_to_invalidate(&err) {
            Some(credential) => {
                crate::auth::invalidate_credential(&credential, interaction).await?;
                Err(err.wrap_err(format!(
                    "cached credential {credential:?} was rejected and has been cleared — \
                     re-run the command to refresh it"
                )))
            }
            None => Err(err),
        },
    }
}

async fn dispatch_inner(cli: Cli) -> Result<()> {
    let cookies_owned = cli.cookies.clone();
    let cookies = cookies_owned.as_deref();
    let json = cli.json;
    match cli.command {
        Command::Auth { verb } => auth::dispatch(verb, cookies, json).await,
        Command::Chat { verb } => chat::dispatch(verb, cookies, json).await,
        Command::Team { verb } => team::dispatch(verb, json).await,
        Command::Channel { verb } => channel::dispatch(verb, cookies, json).await,
        Command::Thread { verb } => thread::dispatch(verb, cookies, json).await,
        Command::Message { verb } => message::dispatch(verb, cookies, json).await,
        Command::User { verb } => user::dispatch(verb, json).await,
        Command::Calendar { verb } => calendar::dispatch(verb, json).await,
    }
}
