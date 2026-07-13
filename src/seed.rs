//! Seed other CLIs' credential stores from xteams' FOCI-minted tokens, so tools
//! like the m365 CLI can call Microsoft Graph without their own sign-in.

mod connection;
mod msal_cache;
mod store;

use eyre::Result;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::auth::{self, Authenticator};
use crate::cli::TokenType;
use crate::error::SeedError;
use crate::model::SeedResult;

const GRAPH: &str = "https://graph.microsoft.com";
// m365 must use the same client our tokens were issued by (the Teams FOCI client):
// AAD refuses to redeem the refresh token for any other client id (AADSTS700007),
// even within the FOCI family, so a different client here silently breaks renewal.
const M365_CLIENT: &str = auth::FOCI_CLIENT;
const M365_TENANT: &str = "organizations";

pub async fn seed_m365(token_type: TokenType, authenticator: &Authenticator) -> Result<SeedResult> {
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
        M365_CLIENT,
        M365_TENANT,
    );
    let mut wrote = store::write_connection(&conn)?;

    if matches!(token_type, TokenType::Refresh) {
        let refresh = authenticator.refresh_token()?;
        let username = id.upn.clone().unwrap_or_else(|| oid.clone());
        let cache = msal_cache::build_cache(&oid, &tid, &username, &refresh, M365_CLIENT);
        wrote.push(store::write_msal_cache(&cache)?);
    }

    Ok(SeedResult {
        target: "m365",
        token_type: token_type_label(token_type),
        resource: GRAPH,
        identity: id.upn,
        expires_in_min: ttl.map(|secs| secs / 60),
        wrote: wrote.into_iter().map(|p| p.display().to_string()).collect(),
    })
}

fn token_type_label(token_type: TokenType) -> &'static str {
    match token_type {
        TokenType::Refresh => "refresh",
        TokenType::Access => "access",
    }
}

fn expires_on_rfc3339(ttl_secs: Option<i64>) -> String {
    let secs = ttl_secs.unwrap_or(3600);
    (OffsetDateTime::now_utc() + time::Duration::seconds(secs))
        .format(&Rfc3339)
        .unwrap_or_default()
}
