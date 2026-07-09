//! substrate.office.com people search (SubstrateSearch "suggestions"). Uses
//! `Authorization: Bearer` with a `substrate.office.com`-audience token. The
//! `scenario=powerbar` query param and the `cvid`/`logicalId` correlation ids are
//! required — the endpoint returns HTTP 400 without them.

use eyre::Result;
use reqwest::Method;
use serde_json::json;
use uuid::Uuid;

use super::send_ok;
use crate::auth::{Authenticator, CachedCredential};
use crate::model::{Person, SubstrateSuggestions};

const SUBSTRATE: &str = "https://substrate.office.com";
const TEAMS_ORIGIN: &str = "https://teams.microsoft.com";

pub async fn search_people(auth: &Authenticator, query: &str, limit: u32) -> Result<Vec<Person>> {
    let url = format!("{SUBSTRATE}/search/api/v1/suggestions?scenario=powerbar");
    let body = json!({
        "EntityRequests": [{
            "Query": { "QueryString": query, "DisplayQueryString": query },
            "EntityType": "People",
            "Size": limit,
            "Fields": ["Id", "MRI", "DisplayName", "EmailAddresses", "JobTitle", "Department"],
        }],
        "cvid": Uuid::new_v4().to_string(),
        "logicalId": Uuid::new_v4().to_string(),
    });
    let request = auth
        .authed(SUBSTRATE, Method::POST, &url)
        .await?
        .header("Accept", "application/json")
        .header("Origin", TEAMS_ORIGIN)
        .header("Referer", format!("{TEAMS_ORIGIN}/"))
        .json(&body);
    let resp = send_ok(request, "POST substrate suggestions", Some(CachedCredential::access(SUBSTRATE))).await?;
    Ok(resp.json::<SubstrateSuggestions>().await?.people())
}
