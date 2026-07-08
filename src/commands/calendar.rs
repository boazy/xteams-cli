//! `calendar` commands. Lists upcoming events from Microsoft Graph using a
//! device-code bearer token; run `xteams login` first.

use eyre::Result;

use crate::api::calendar;
use crate::auth;
use crate::cli::CalendarVerb;
use crate::output::render;

pub async fn dispatch(verb: CalendarVerb, json: bool) -> Result<()> {
    let authenticator = auth::load_authenticator().await?;
    match verb {
        CalendarVerb::List(args) => {
            render(&calendar::calendar_view(&authenticator, args.days).await?, json)
        }
    }
}
