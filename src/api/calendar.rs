//! Microsoft Graph calendar view. Uses `Authorization: Bearer` with a
//! `graph.microsoft.com`-audience token (`.default` scope; the Teams FOCI client is
//! pre-consented for the calendar scopes).

use eyre::Result;
use reqwest::Method;
use time::format_description::well_known::Rfc3339;
use time::{Duration, OffsetDateTime};

use super::send_ok;
use crate::auth::{Authenticator, CachedCredential};
use crate::model::{CalendarEvent, CalendarView};

const GRAPH: &str = "https://graph.microsoft.com";

pub async fn calendar_view(auth: &Authenticator, days: i64) -> Result<Vec<CalendarEvent>> {
    let now = OffsetDateTime::now_utc();
    let end = now + Duration::days(days.max(1));
    let url = format!("{GRAPH}/v1.0/me/calendarView");
    let request = auth
        .authed(GRAPH, Method::GET, &url)
        .await?
        .header("Prefer", "outlook.timezone=\"UTC\"")
        .query(&[
            ("startDateTime", now.format(&Rfc3339)?),
            ("endDateTime", end.format(&Rfc3339)?),
            ("$orderby", "start/dateTime".to_owned()),
            ("$top", "50".to_owned()),
        ]);
    let resp = send_ok(request, "GET graph calendarView", Some(CachedCredential::access(GRAPH))).await?;
    Ok(resp.json::<CalendarView>().await?.value)
}
