//! `user` commands. `search` finds people via substrate people-search using a
//! device-code bearer token; run `xteams auth login` first.

use eyre::Result;

use crate::api::substrate;
use crate::auth;
use crate::cli::UserVerb;
use crate::output::render;

const SEARCH_LIMIT: u32 = 15;

pub async fn dispatch(verb: UserVerb, json: bool) -> Result<()> {
    let authenticator = auth::load_authenticator().await?;
    match verb {
        UserVerb::Search(args) => {
            render(&substrate::search_people(&authenticator, &args.query, SEARCH_LIMIT).await?, json)
        }
    }
}
