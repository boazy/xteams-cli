//! Session orchestration: FRT-first (mint a spaces token → authz → skypetoken), with
//! a desktop-cookie fallback. Submodules own the pieces; this root wires them together
//! and exposes the public surface.

use std::path::Path;

use eyre::Result;

use crate::creds;

mod authenticator;
mod credential;
mod device_code;
mod jwt;
mod lock;
mod oauth;
mod session;
mod token_cache;
mod token_cache_io;

pub use authenticator::{Authenticator, invalidate_credential, logout};
pub use credential::{CachedCredential, SessionCredential, credential_to_invalidate};
pub use jwt::{graph_identity, jwt_audience_and_ttl};
pub use lock::AuthInteraction;
pub use oauth::FOCI_CLIENT;
pub use session::Session;

/// The `api.spaces.skype.com` audience: the AAD token authz accepts to mint a
/// skypetoken, and the FRT-minted token whose rejection also poisons the skype session.
pub const SPACES_RESOURCE: &str = "https://api.spaces.skype.com";

const DEFAULT_TENANT: &str = "organizations";
const USER_AGENT: &str = "xteams-cli/0.1 (Teams-compatible)";

/// Establish a session: FRT-first when signed in (no cookies or Teams app needed),
/// else the desktop-cookie path (silent, but limited to the chat service).
pub async fn connect(
    cookies: Option<&Path>,
    interaction: AuthInteraction,
) -> Result<(reqwest::Client, Session)> {
    let client = build_client()?;
    if let Some(auth) = Authenticator::try_load(client.clone(), DEFAULT_TENANT, interaction)? {
        let skype = auth.skype_session().await?;
        let session = Session::from_skype_session(&skype, auth.identity()?);
        return Ok((client, session));
    }
    let path = match cookies {
        Some(p) => p.to_path_buf(),
        None => creds::default_cookies_path()?,
    };
    let cookies = creds::load_cookies(&path)?;
    let session = Session::establish(&client, &cookies).await?;
    Ok((client, session))
}

pub fn build_client() -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .timeout(std::time::Duration::from_secs(30))
        .build()?)
}

pub fn load_authenticator(interaction: AuthInteraction) -> Result<Authenticator> {
    Authenticator::load(build_client()?, DEFAULT_TENANT, interaction)
}

pub async fn login_authenticator(interaction: AuthInteraction) -> Result<Authenticator> {
    Authenticator::login(build_client()?, DEFAULT_TENANT, interaction).await
}
