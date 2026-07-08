//! `team` commands. `list`/`search` enumerate your teams via the chatsvcagg (CSA)
//! aggregator using a device-code bearer token; run `xteams auth login` first.

use eyre::{Result, bail};

use crate::api::csa;
use crate::auth;
use crate::cli::TeamVerb;
use crate::model::Team;
use crate::output::render;

pub async fn dispatch(verb: TeamVerb, json: bool) -> Result<()> {
    match verb {
        TeamVerb::List => {
            let authenticator = auth::load_authenticator().await?;
            render(&csa::list_teams(&authenticator).await?, json)
        }
        TeamVerb::Search(args) => {
            let authenticator = auth::load_authenticator().await?;
            let teams: Vec<Team> = csa::list_teams(&authenticator)
                .await?
                .into_iter()
                .filter(|team| team.matches(&args.query))
                .collect();
            render(&teams, json)
        }
        TeamVerb::Join(_) => {
            bail!(
                "`team join` is not implemented: the CSA join endpoint is unverified and joining \
                 is a write operation. `team list` and `team search` work."
            )
        }
    }
}
