//! `auth` — account/token status, device-code sign-in, and sign-out.

use std::path::Path;

use eyre::Result;

use crate::auth::{self, AuthInteraction, Authenticator, SessionCredential};
use crate::cli::{AuthVerb, SeedTarget};
use crate::model::{AuthAction, AuthStatus, TokenInfo};
use crate::output::render;

const DEVICE_CODE_AUDIENCES: [(&str, &str); 3] = [
    ("chatsvcagg", "https://chatsvcagg.teams.microsoft.com"),
    ("substrate", "https://substrate.office.com"),
    ("graph (calendar)", "https://graph.microsoft.com"),
];

pub async fn dispatch(verb: AuthVerb, cookies: Option<&Path>, json: bool) -> Result<()> {
    let interaction = AuthInteraction::from_json(json);
    match verb {
        AuthVerb::Status(args) => render(
            &status(cookies, args.include_tokens, interaction).await?,
            json,
        ),
        AuthVerb::Login => {
            auth::login_authenticator(interaction).await?;
            render(
                &AuthAction {
                    action: "login",
                    signed_in: true,
                },
                json,
            )
        }
        AuthVerb::Logout => {
            auth::logout()?;
            render(
                &AuthAction {
                    action: "logout",
                    signed_in: false,
                },
                json,
            )
        }
        AuthVerb::Seed { target } => match target {
            SeedTarget::M365(args) => {
                let authenticator = auth::load_authenticator(interaction)?;
                render(
                    &crate::seed::seed_m365(args.token_type, &authenticator).await?,
                    json,
                )
            }
        },
    }
}

async fn status(
    cookies: Option<&Path>,
    include_tokens: bool,
    interaction: AuthInteraction,
) -> Result<AuthStatus> {
    let (_client, session) = auth::connect(cookies, interaction).await?;
    let label = match &session.credential {
        SessionCredential::CachedSkype => "skypetoken (FRT)",
        SessionCredential::Cookie => "skypetoken (cookie)",
    };
    let mut tokens = vec![token_info(label, &session.skype_token, include_tokens)];
    if let Ok(authenticator) = auth::load_authenticator(interaction) {
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
    let mut minted = Vec::new();
    for (name, resource) in DEVICE_CODE_AUDIENCES {
        match authenticator.token_for(resource).await {
            Ok(token) => minted.push(token_info(name, &token, include_tokens)),
            // A stale/revoked refresh token was just cleared by `token_for`; omit the
            // device-code tokens (a feature command surfaces the "sign in again" error).
            Err(_) => return,
        }
    }
    if let Ok(refresh) = authenticator.refresh_token() {
        tokens.push(TokenInfo {
            name: "refresh (FOCI)".to_owned(),
            audience: None,
            expires_in_min: None,
            value: include_tokens.then_some(refresh),
        });
    }
    tokens.extend(minted);
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
