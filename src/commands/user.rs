//! `user` commands (Phase 3 — pending a substrate-audience token).

use std::path::Path;

use eyre::{Result, bail};

use crate::cli::UserVerb;

pub async fn dispatch(verb: UserVerb, _cookies: Option<&Path>, _json: bool) -> Result<()> {
    match verb {
        UserVerb::Search(_) => {
            bail!("user search is deferred (needs a substrate-audience token via OneAuth minting)")
        }
    }
}
