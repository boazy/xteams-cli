//! `auth` — account/token status, device-code sign-in, and sign-out.

use std::path::Path;

use eyre::Result;

use crate::auth::{self, Authenticator};
use crate::cli::AuthVerb;
use crate::model::{AuthAction, AuthStatus, TokenInfo};
use crate::output::render;

const DEVICE_CODE_AUDIENCES: [(&str, &str); 3] = [
    ("chatsvcagg", "https://chatsvcagg.teams.microsoft.com"),
    ("substrate", "https://substrate.office.com"),
    ("graph (calendar)", "https://graph.microsoft.com"),
];

pub async fn dispatch(verb: AuthVerb, cookies: Option<&Path>, json: bool) -> Result<()> {
    match verb {
        AuthVerb::Status(args) => render(&status(cookies, args.include_tokens).await?, json),
        AuthVerb::Login => {
            auth::login_authenticator().await?;
            render(&AuthAction { action: "login", signed_in: true }, json)
        }
        AuthVerb::Logout => {
            auth::logout()?;
            render(&AuthAction { action: "logout", signed_in: false }, json)
        }
    }
}

async fn status(cookies: Option<&Path>, include_tokens: bool) -> Result<AuthStatus> {
    let (_client, session) = auth::connect(cookies).await?;
    let mut tokens = vec![
        token_info("aad (cookie)", &session.aad_bearer, include_tokens),
        token_info("skypetoken (cookie)", &session.skype_token, include_tokens),
    ];
    if let Ok(authenticator) = auth::load_authenticator().await {
        append_device_code_tokens(&authenticator, include_tokens, &mut tokens).await;
    }
    let id = session.identity;
    Ok(AuthStatus {
        user: id.upn,
        name: id.name,
        tenant: id.tenant,
        region: session.region,
        chat_service: session.chat_service,
        services: session.gtms.len(),
        tokens,
    })
}

async fn append_device_code_tokens(
    authenticator: &Authenticator,
    include_tokens: bool,
    tokens: &mut Vec<TokenInfo>,
) {
    if let Ok(refresh) = authenticator.refresh_token() {
        tokens.push(TokenInfo {
            name: "refresh (FOCI)".to_owned(),
            audience: None,
            expires_in_min: None,
            value: include_tokens.then_some(refresh),
        });
    }
    for (name, resource) in DEVICE_CODE_AUDIENCES {
        if let Ok(token) = authenticator.token_for(resource).await {
            tokens.push(token_info(name, &token, include_tokens));
        }
    }
}

fn token_info(name: &str, token: &str, include_tokens: bool) -> TokenInfo {
    let (audience, ttl_secs) = auth::jwt_audience_and_ttl(token);
    TokenInfo {
        name: name.to_owned(),
        audience,
        expires_in_min: ttl_secs.map(|secs| secs / 60),
        value: include_tokens.then(|| token.to_owned()),
    }
}
