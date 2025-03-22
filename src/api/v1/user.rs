use actix_web::{get, web, HttpResponse, Responder, Scope};
use serde::Deserialize;
use utoipa::OpenApi;

use crate::{
    app::{ApiError, AppState},
    auth::SessionAuth,
    db::user::UserData,
};

#[derive(OpenApi)]
#[openapi(paths(user_search), components(schemas(UserData)))]
pub struct ApiDoc;

#[derive(Deserialize)]
struct UserSearchParams {
    query: String,
}

#[utoipa::path(
    responses(
        (status = 200, description = "Get all users matching search", body = [UserData]),
        (status = 500, body = ApiError)
    )
)]
#[get("/", wrap = "SessionAuth")]
async fn user_search(
    data: web::Data<AppState>,
    params: web::Query<UserSearchParams>,
) -> impl Responder {
    let query = format!("%{}%", params.query.to_lowercase());

    let result = UserData::select_search(query, &data.db).await;

    match result {
        Ok(users) => HttpResponse::Ok().json(users),
        Err(_) => HttpResponse::InternalServerError()
            .json(ApiError::from("Failed to get events".to_string())),
    }
}

pub fn scope() -> Scope {
    web::scope("/user").service(user_search)
}
