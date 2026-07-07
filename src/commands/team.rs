//! `team` commands (Phase 3 — pending a chatsvcagg-audience token).

use std::path::Path;

use eyre::{Result, bail};

use crate::cli::TeamVerb;

pub async fn dispatch(verb: TeamVerb, _cookies: Option<&Path>, _json: bool) -> Result<()> {
    match verb {
        TeamVerb::List | TeamVerb::Join(_) | TeamVerb::Search(_) => {
            bail!("team list/join/search are deferred (need a chatsvcagg-audience token via OneAuth minting)")
        }
    }
}
