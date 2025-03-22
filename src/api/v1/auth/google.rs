use actix_session::Session;
use actix_web::{get, http::header, web, HttpResponse, Responder, Scope};
use log::error;
use oauth2::{reqwest::async_http_client, AuthorizationCode, TokenResponse};
use reqwest::Client;
use serde::Deserialize;
use utoipa::{OpenApi, ToSchema};

use crate::{
    api::v1::auth::{
        common::{self, login_session},
        models::{GoogleUserInfo, UserInfo},
    },
    app::{ApiError, AppState},
    db::user::{UserData, UserRealm},
};

#[derive(OpenApi)]
#[openapi(paths(login, auth,), components(schemas(AuthRequest)))]
pub(super) struct ApiDoc;

#[utoipa::path(
    responses(
        (status = 200, description = "Redirect to OAuth2 Link to Log In")
    )
)]
#[get("/")]
async fn login(data: web::Data<AppState>) -> impl Responder {
    common::login(
        &data.google_oauth,
        Vec::from([
            "openid".to_string(),
            "profile".to_string(),
            "email".to_string(),
        ]),
    )
    .await
}

#[derive(Deserialize, ToSchema)]
pub struct AuthRequest {
    code: String,
    #[allow(dead_code)]
    state: String,
}

#[utoipa::path(
    responses(
        (status = 302, description = "Successful login, Redirect to home page."),
        (status = 500, body = ApiError)
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
    let token = match &data
        .google_oauth
        .exchange_code(code)
        .request_async(async_http_client)
        .await
    {
        Ok(token) => token.access_token().secret().clone(),
        Err(err) => {
            error!("{}", err);
            return HttpResponse::InternalServerError()
                .json(ApiError::from("Failed to get OAuth Token".to_string()));
        }
    };

    let client = Client::new();

    let user_info: GoogleUserInfo = match client
        .get(&data.google_userinfo_url)
        .bearer_auth(token)
        .send()
        .await
    {
        Ok(res) => match res.json().await {
            Ok(out) => out,
            Err(err) => {
                error!("{}", err);
                return HttpResponse::InternalServerError().json(ApiError::from(
                    "Failed to deserialize UserInfo token".to_string(),
                ));
            }
        },
        Err(err) => {
            error!("{}", err);
            return HttpResponse::InternalServerError()
                .json(ApiError::from("Failed to get UserInfo Token".to_string()));
        }
    };

    if let Err(err) = UserData::insert_new(
        user_info.sub.clone(),
        UserRealm::Google,
        format!("{} {}", user_info.given_name, user_info.family_name),
        user_info.email.clone(),
        &data.db,
    )
    .await
    {
        error!("{}", err);
        return HttpResponse::InternalServerError()
            .json(ApiError::from("Failed to add user to database".to_string()));
    }

    if let Err(err) = login_session(&session, UserInfo::from(user_info)) {
        error!("{}", err);
        return HttpResponse::InternalServerError()
            .json(ApiError::from("Failed to Authorize Session".to_string()));
    }

    HttpResponse::Found()
        .append_header((header::LOCATION, "/"))
        .finish()
}

pub fn scope() -> Scope {
    web::scope("/google").service(login).service(auth)
}
