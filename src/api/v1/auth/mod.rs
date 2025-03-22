use actix_session::Session;
use actix_web::{get, http::header, post, web, HttpResponse, Responder, Scope};
use log::error;
use utoipa::OpenApi;

use crate::{api::v1::auth::models::UserInfo, app::ApiError, auth::SessionAuth};

mod common;
mod csh;
mod google;
pub mod models;

#[utoipa::path(
    responses(
        (status = 302, description = "Logged out")
    )
)]
#[post("/logout")]
async fn logout(session: Session) -> impl Responder {
    session.remove("login");
    session.remove("userinfo");
    HttpResponse::Found()
        .append_header((header::LOCATION, "/"))
        .finish()
}

#[utoipa::path(
    responses(
        (status = 200, description = "Get current user information"),
        (status = 500, body = ApiError)
    )
)]
#[get("/", wrap = "SessionAuth")]
async fn get_user_data(session: Session) -> impl Responder {
    match session.get::<UserInfo>("userinfo") {
        Ok(data) => HttpResponse::Ok().json(data),
        Err(err) => {
            error!("{}", err);
            HttpResponse::InternalServerError()
                .json(ApiError::from("Failed to get Session Data".to_string()))
        }
    }
}

#[derive(OpenApi)]
#[openapi(
    paths(
        logout,
        get_user_data,
    ),
    nest(
        (path = "/csh", api = csh::ApiDoc),
        (path = "/google", api = google::ApiDoc)
    ),
)]
pub(super) struct ApiDoc;

pub fn scope() -> Scope {
    web::scope("/auth")
        .service(logout)
        .service(get_user_data)
        .service(csh::scope())
        .service(google::scope())
}
