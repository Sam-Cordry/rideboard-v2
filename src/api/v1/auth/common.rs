use actix_session::Session;
use actix_web::{HttpResponse, Responder};
use anyhow::Result;
use oauth2::{basic::BasicClient, CsrfToken, Scope as OAuthScope};

use super::models::UserInfo;

pub async fn login(client: &BasicClient, scopes: Vec<String>) -> impl Responder {
    //let (pkce_code_challenge, _pkce_code_verifier) = PkceCodeChallenge::new_random_sha256();

    // Generate the authorization URL to which we'll redirect the user.
    let mut configured_client = client.authorize_url(CsrfToken::new_random);

    for scope in scopes {
        configured_client = configured_client.add_scope(OAuthScope::new(scope));
    }

    let (authorize_url, _csrf_state) = configured_client
        //.set_pkce_challenge(pkce_code_challenge)
        .url();

    HttpResponse::Ok().body(authorize_url.to_string())
}

pub fn login_session(session: &Session, user_info: UserInfo) -> Result<()> {
    session.insert("login", true)?;
    session.insert("userinfo", user_info)?;
    Ok(())
}
