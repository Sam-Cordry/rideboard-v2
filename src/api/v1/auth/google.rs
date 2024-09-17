use actix_session::Session;
use actix_web::http::header;
use actix_web::{HttpResponse, Responder};
use oauth2::reqwest::async_http_client;
use oauth2::{AuthorizationCode, TokenResponse};
use reqwest::Client;
use utoipa::{OpenApi, ToSchema};

use crate::api::v1::auth::common;
use crate::api::v1::auth::models::{AuthType, GoogleUserInfo};
use crate::AppState;
use actix_web::{get, web, Scope};
use serde::Deserialize;

#[derive(OpenApi)]
#[openapi(
    paths(
        login,
        auth,
    ),
    components(schemas(AuthRequest))
)]
pub(super) struct ApiDoc;

#[utoipa::path(
    responses(
        (status = 200, description = "List current todo items")
    )
)]
#[get("/")]
async fn login(data: web::Data<AppState>) -> impl Responder {
    common::login(&data.google_oauth, Vec::from(["openid".to_string(), "profile".to_string(), "email".to_string()])).await
}

#[derive(Deserialize, ToSchema)]
pub struct AuthRequest {
    code: String,
    state: String,
}

#[utoipa::path(
    responses(
        (status = 200, description = "List current todo items")
    )
)]
#[get("/redirect")]
async fn auth(
    session: Session,
    data: web::Data<AppState>,
    params: web::Query<AuthRequest>,
) -> impl Responder {
    let code = AuthorizationCode::new(params.code.clone());
    //let _scope = params.scope.clone();

    // Exchange the code with a token.
    let token = &data
        .google_oauth
        .exchange_code(code)
        .request_async(async_http_client)
        .await
        .unwrap();

    let client = Client::new();

    let user_info: GoogleUserInfo = client
        .get(&data.google_userinfo_url)
        .bearer_auth(token.access_token().secret())
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();

    session.insert("login", true).unwrap();
    session.insert("userinfo", AuthType::GOOGLE(user_info)).unwrap();

    HttpResponse::Found().append_header((header::LOCATION, "/")).finish()
}

pub fn scope() -> Scope {
    web::scope("/google").service(login).service(auth)
}
