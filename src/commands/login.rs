//! `login` / `logout` — device-code sign-in that unlocks the bearer-token features
//! (team, user, calendar). The refresh token is stored in the macOS Keychain.

use eyre::Result;

use crate::auth;
use crate::model::AuthAction;
use crate::output::render;

pub async fn login(json: bool) -> Result<()> {
    auth::login_authenticator().await?;
    render(&AuthAction { action: "login", signed_in: true }, json)
}

pub async fn logout(json: bool) -> Result<()> {
    auth::logout()?;
    render(&AuthAction { action: "logout", signed_in: false }, json)
}
