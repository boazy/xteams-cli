//! chatsvcagg (CSA) team/channel operations. Uses `Authorization: Bearer` with a
//! `chatsvcagg.teams.microsoft.com`-audience token; the region is discovered from
//! `authz` (never hardcoded) and filled into the CSA path.

use eyre::Result;
use reqwest::Method;

use super::send_ok;
use crate::auth::Authenticator;
use crate::model::{CsaUpdates, Team};

const CHATSVCAGG: &str = "https://chatsvcagg.teams.microsoft.com";
const CLIENT_VERSION: &str = "1415/25000000000";

pub async fn list_teams(auth: &Authenticator) -> Result<Vec<Team>> {
    let region = auth.region().await?;
    let url = format!("https://teams.microsoft.com/api/csa/{region}/api/v1/teams/users/me/updates");
    let request = auth
        .authed(CHATSVCAGG, Method::GET, &url)
        .await?
        .header("x-ms-client-version", CLIENT_VERSION);
    let resp = send_ok(request, "GET csa teams/updates").await?;
    Ok(resp.json::<CsaUpdates>().await?.teams)
}
