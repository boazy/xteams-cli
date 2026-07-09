//! Seed other CLIs' credential stores from xteams' FOCI-minted tokens, so tools
//! like the m365 CLI can call Microsoft Graph without their own sign-in.

mod connection;
mod store;

use eyre::Result;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::auth::{self, Authenticator};
use crate::error::SeedError;
use crate::model::SeedResult;

const GRAPH: &str = "https://graph.microsoft.com";
const M365_FOCI_CLIENT: &str = "04b07795-8ddb-461a-bbee-02f9e1bf7b46";
const M365_TENANT: &str = "common";

pub async fn seed_m365_access(authenticator: &Authenticator) -> Result<SeedResult> {
    let token = authenticator.token_for(GRAPH).await?;
    let id = auth::graph_identity(&token);
    let oid = id.oid.clone().ok_or(SeedError::NoIdentity)?;
    let tid = id.tid.clone().ok_or(SeedError::NoIdentity)?;
    let (_, ttl) = auth::jwt_audience_and_ttl(&token);
    let expires_on = expires_on_rfc3339(ttl);
    let conn = connection::build_connection(
        &token,
        &expires_on,
        id.upn.as_deref(),
        &oid,
        &tid,
        M365_FOCI_CLIENT,
        M365_TENANT,
    );
    let wrote = store::write_connection(&conn)?;
    Ok(SeedResult {
        target: "m365",
        token_type: "access",
        resource: GRAPH,
        identity: id.upn,
        expires_in_min: ttl.map(|secs| secs / 60),
        wrote: wrote.into_iter().map(|p| p.display().to_string()).collect(),
    })
}

fn expires_on_rfc3339(ttl_secs: Option<i64>) -> String {
    let secs = ttl_secs.unwrap_or(3600);
    (OffsetDateTime::now_utc() + time::Duration::seconds(secs)).format(&Rfc3339).unwrap_or_default()
}
