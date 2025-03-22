use actix_session::Session;
use actix_web::{delete, post, web, HttpResponse, Responder, Scope};
use log::error;
use utoipa::OpenApi;

use crate::{
    api::v1::event::UserInfo,
    app::{ApiError, AppState, RedisJob, SimpleRiderChange},
    auth::SessionAuth,
    db::car::Car,
};

#[derive(OpenApi)]
#[openapi(paths(create_rider, delete_rider))]
pub struct ApiDoc;

#[utoipa::path(
    params(
        ("event_id" = i32, Path, description = "ID of the Event this Rider Applies To"),
        ("car_id" = i32, Path, description = "ID of the Car this Rider Applies To")
    ),
    responses(
        (status = 200, description = "Add a rider to a car."),
        (status = 400, body = ApiError),
        (status = 401, body = ApiError),
        (status = 500, body = ApiError)
    )
)]
#[post("/", wrap = "SessionAuth")]
async fn create_rider(
    data: web::Data<AppState>,
    session: Session,
    path: web::Path<(i32, i32)>,
) -> impl Responder {
    let (event_id, car_id) = path.into_inner();
    let user_id = match session.get::<UserInfo>("userinfo").ok().flatten() {
        Some(user) => user.id,
        None => {
            return HttpResponse::Unauthorized().json(ApiError::from(
                "Failed to get user data from session".to_string(),
            ))
        }
    };

    match Car::select_one(event_id, car_id, &data.db).await {
        Ok(Some(car)) => {
            if car.max_capacity <= car.riders.map(|riders| riders.len()).unwrap_or(0) as i32 {
                return HttpResponse::BadRequest().json(ApiError::from("Car is full.".to_string()));
            }
        }
        Ok(None) => {
            return HttpResponse::BadRequest()
                .json(ApiError::from("Car does not exist.".to_string()))
        }
        Err(err) => {
            error!("{}", err);
            return HttpResponse::InternalServerError()
                .json(ApiError::from("Failed to check car capacity".to_string()));
        }
    }

    match Car::user_in_car(event_id, &user_id, &data.db).await {
        Ok(false) => {}
        Ok(true) => {
            return HttpResponse::BadRequest()
                .json(ApiError::from("User is already in a car.".to_string()))
        }
        Err(err) => {
            error!("{}", err);
            return HttpResponse::InternalServerError().json(ApiError::from(
                "Failed to check user's occupancy in other cars".to_string(),
            ));
        }
    }

    if let Err(err) = sqlx::query!(
        r#"
        INSERT INTO rider (car_id, rider) VALUES ($1, $2)
        "#,
        car_id,
        user_id
    )
    .execute(&data.db)
    .await
    {
        error!("{}", err);
        return HttpResponse::InternalServerError()
            .json(ApiError::from("Failed to join ride".to_string()));
    };

    match data.redis.lock().map(|mut mutex| async move {
        mutex
            .insert_job(RedisJob::Join(SimpleRiderChange {
                event_id,
                car_id,
                rider_id: user_id,
            }))
            .await
    }) {
        Ok(res) => {
            if let Err(err) = res.await {
                error!("{}", err);
            }
        }
        Err(err) => error!("{}", err),
    }
    HttpResponse::Ok().body("Joined Car")
}

#[utoipa::path(
    params(
        ("event_id" = i32, Path, description = "ID of the Event this Rider Applies To"),
        ("car_id" = i32, Path, description = "ID of the Car this Rider Applies To")
    ),
    responses(
        (status = 200, description = "Remove other rider from car. Must be done by driver."),
        (status = 401, body = ApiError),
        (status = 500, body = ApiError)
    )
)]
#[delete("/", wrap = "SessionAuth")]
async fn delete_rider(
    data: web::Data<AppState>,
    session: Session,
    path: web::Path<(i32, i32)>,
) -> impl Responder {
    let (event_id, car_id) = path.into_inner();
    let user_id = match session.get::<UserInfo>("userinfo").ok().flatten() {
        Some(user) => user.id,
        None => {
            return HttpResponse::Unauthorized().json(ApiError::from(
                "Failed to get user data from session".to_string(),
            ))
        }
    };

    if let Err(err) = sqlx::query!(
        "DELETE FROM rider WHERE car_id = $1 AND rider = $2",
        car_id,
        user_id
    )
    .execute(&data.db)
    .await
    {
        error!("{}", err);
        return HttpResponse::InternalServerError()
            .json(ApiError::from("Failed to delete rider".to_string()));
    }

    match data.redis.lock().map(|mut mutex| async move {
        mutex
            .insert_job(RedisJob::Leave(SimpleRiderChange {
                event_id,
                car_id,
                rider_id: user_id,
            }))
            .await
    }) {
        Ok(res) => {
            if let Err(err) = res.await {
                error!("{}", err);
            }
        }
        Err(err) => error!("{}", err),
    }

    HttpResponse::Ok().body("Rider deleted")
}

pub fn scope() -> Scope {
    web::scope("/{car_id}/rider")
        .service(create_rider)
        .service(delete_rider)
}
