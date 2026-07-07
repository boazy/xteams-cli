//! `auth` — signed-in account and token status.

use std::path::Path;

use eyre::Result;

use crate::auth;
use crate::model::AuthStatus;

pub async fn status(cookies: Option<&Path>) -> Result<AuthStatus> {
    let (_client, session) = auth::connect(cookies).await?;
    let id = session.identity;
    Ok(AuthStatus {
        user: id.upn,
        name: id.name,
        tenant: id.tenant,
        audience: id.audience,
        region: session.region,
        chat_service: session.chat_service,
        token_ttl_min: id.expires_in_secs.map(|secs| secs / 60),
        services: session.gtms.len(),
    })
}
