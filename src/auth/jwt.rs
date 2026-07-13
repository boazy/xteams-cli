//! Pure JWT claim decoding — no signature verification (display/metadata only).
//! Pulls identity (upn/name/tenant), audience, and expiry out of AAD tokens,
//! whether they come from the desktop `authtoken` cookie or an FRT-minted token.

use base64::Engine as _;

/// Identity decoded from an AAD bearer's JWT claims (display/metadata only).
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct Identity {
    pub upn: Option<String>,
    pub name: Option<String>,
    pub tenant: Option<String>,
}

/// Identity claims pulled from a Graph access-token JWT, used to seed other CLIs'
/// credential stores (they key on `oid`/`tid`, which `Identity` does not carry).
#[derive(Debug, Clone, Default)]
pub struct GraphIdentity {
    pub oid: Option<String>,
    pub upn: Option<String>,
    pub tid: Option<String>,
}

/// Decode upn/name/tenant from an AAD bearer JWT (falls back to `preferred_username`).
pub fn identity_from_jwt(jwt: &str) -> Identity {
    let Some(claims) = decode_claims(jwt) else {
        return Identity::default();
    };
    let get = |key: &str| claims.get(key).and_then(|v| v.as_str()).map(str::to_owned);
    Identity {
        upn: get("upn").or_else(|| get("preferred_username")),
        name: get("name"),
        tenant: get("tid"),
    }
}

/// Decode a JWT's audience and remaining lifetime in seconds; `(None, None)` when the
/// token is not a decodable JWT (e.g. an opaque refresh token).
pub fn jwt_audience_and_ttl(jwt: &str) -> (Option<String>, Option<i64>) {
    let Some(claims) = decode_claims(jwt) else {
        return (None, None);
    };
    let audience = claims
        .get("aud")
        .and_then(|v| v.as_str())
        .map(str::to_owned);
    let ttl = claims
        .get("exp")
        .and_then(serde_json::Value::as_i64)
        .map(|exp| exp - now_unix());
    (audience, ttl)
}

/// The absolute `exp` (unix seconds) of a JWT, if it is a decodable JWT with `exp`.
pub fn jwt_expiry(jwt: &str) -> Option<i64> {
    decode_claims(jwt)?
        .get("exp")
        .and_then(serde_json::Value::as_i64)
}

/// Extract the seed-relevant identity claims (`oid`, `upn`, `tid`) from a Graph
/// access-token JWT. Returns defaults for a non-decodable token.
pub fn graph_identity(jwt: &str) -> GraphIdentity {
    let Some(claims) = decode_claims(jwt) else {
        return GraphIdentity::default();
    };
    let get = |key: &str| claims.get(key).and_then(|v| v.as_str()).map(str::to_owned);
    GraphIdentity {
        oid: get("oid"),
        upn: get("upn").or_else(|| get("preferred_username")),
        tid: get("tid"),
    }
}

fn decode_claims(jwt: &str) -> Option<serde_json::Map<String, serde_json::Value>> {
    let payload = jwt.split('.').nth(1)?;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload)
        .ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// Current unix time in seconds (saturating). Shared clock for token/expiry math.
pub(crate) fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    fn fake_jwt(aud: &str, exp: i64) -> String {
        let payload = serde_json::json!({ "aud": aud, "exp": exp }).to_string();
        let encoded = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(payload);
        format!("eyJhbGciOiJub25lIn0.{encoded}.sig")
    }

    fn fake_jwt_claims(claims: serde_json::Value) -> String {
        let encoded = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(claims.to_string());
        format!("eyJhbGciOiJub25lIn0.{encoded}.sig")
    }

    #[test]
    fn jwt_audience_and_ttl_reads_aud_and_remaining_minutes() {
        let (aud, ttl) =
            jwt_audience_and_ttl(&fake_jwt("https://graph.microsoft.com", now_unix() + 3600));
        assert_eq!(aud.as_deref(), Some("https://graph.microsoft.com"));
        let ttl = ttl.expect("ttl should decode");
        assert!(ttl > 3500 && ttl <= 3600, "expected ~1h ttl, got {ttl}");
    }

    #[test]
    fn jwt_audience_and_ttl_on_opaque_refresh_token_is_none() {
        let (aud, ttl) = jwt_audience_and_ttl("1.AcoOpaqueRefreshTokenNotAJwt.q0qw");
        assert_eq!(aud, None);
        assert_eq!(ttl, None);
    }

    #[test]
    fn jwt_expiry_reads_exp_or_none() {
        let exp = now_unix() + 1200;
        assert_eq!(jwt_expiry(&fake_jwt("aud", exp)), Some(exp));
        assert_eq!(jwt_expiry("not-a-jwt"), None);
    }

    #[test]
    fn identity_from_jwt_prefers_upn_then_preferred_username() {
        let jwt =
            fake_jwt_claims(serde_json::json!({ "preferred_username": "p@c.com", "tid": "t" }));
        let id = identity_from_jwt(&jwt);
        assert_eq!(id.upn.as_deref(), Some("p@c.com"));
        assert_eq!(id.tenant.as_deref(), Some("t"));
    }

    #[test]
    fn graph_identity_extracts_oid_upn_tid() {
        let jwt = fake_jwt_claims(serde_json::json!({
            "oid": "11111111-1111-1111-1111-111111111111",
            "upn": "user@contoso.com",
            "tid": "22222222-2222-2222-2222-222222222222",
        }));
        let id = graph_identity(&jwt);
        assert_eq!(
            id.oid.as_deref(),
            Some("11111111-1111-1111-1111-111111111111")
        );
        assert_eq!(id.upn.as_deref(), Some("user@contoso.com"));
        assert_eq!(
            id.tid.as_deref(),
            Some("22222222-2222-2222-2222-222222222222")
        );
    }

    #[test]
    fn graph_identity_falls_back_to_preferred_username() {
        let jwt = fake_jwt_claims(serde_json::json!({
            "oid": "abc",
            "preferred_username": "pref@contoso.com",
            "tid": "def",
        }));
        let id = graph_identity(&jwt);
        assert_eq!(id.upn.as_deref(), Some("pref@contoso.com"));
        assert_eq!(id.oid.as_deref(), Some("abc"));
    }
}
