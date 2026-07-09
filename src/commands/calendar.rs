//! `calendar` commands. Lists upcoming events from Microsoft Graph using a
//! device-code bearer token; run `xteams auth login` first.

use eyre::Result;

use crate::api::calendar;
use crate::auth::{self, AuthInteraction};
use crate::cli::CalendarVerb;
use crate::output::render;

pub async fn dispatch(verb: CalendarVerb, json: bool) -> Result<()> {
    let authenticator = auth::load_authenticator(AuthInteraction::from_json(json))?;
    match verb {
        CalendarVerb::Upcoming(args) => {
            render(&calendar::calendar_view(&authenticator, args.days).await?, json)
        }
    }
}
