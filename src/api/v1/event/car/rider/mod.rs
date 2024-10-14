use crate::app::{AppState, SimpleRiderChange};
use crate::auth::SessionAuth;
use crate::db::car::Car;
use crate::{api::v1::event::UserInfo, app::RedisJob};
use actix_session::Session;
use actix_web::{
    delete, post,
    web::{self},
    HttpResponse, Responder, Scope,
};
use log::error;
use redis_work_queue::{Item, WorkQueue};
use serde_json::json;
use sqlx::query;
use utoipa::OpenApi;

#[derive(OpenApi)]
#[openapi(paths(create_rider, delete_rider))]
pub struct ApiDoc;

#[utoipa::path(
    params(
        ("event_id" = i32, Path, description = "ID of the Event this Rider Applies To"),
        ("car_id" = i32, Path, description = "ID of the Car this Rider Applies To")
    ),
    responses(
        (status = 200, description = "Add a rider to a car.")
    )
)]
#[post("/", wrap = "SessionAuth")]
async fn create_rider(
    data: web::Data<AppState>,
    session: Session,
    path: web::Path<(i32, i32)>,
) -> impl Responder {
    let (event_id, car_id) = path.into_inner();
    let user_id = session.get::<UserInfo>("userinfo").unwrap().unwrap().id;

    match Car::select_one(event_id, car_id, &data.db).await {
        Ok(Some(car)) => {
            if car.max_capacity <= car.riders.unwrap().len() as i32 {
                return HttpResponse::BadRequest().json(json!({
                    "error": "Car is full."
                }));
            }
        }
        Ok(None) => {
            return HttpResponse::BadRequest().json(json!({
                "error": "Car does not exist."
            }))
        }
        Err(err) => {
            error!("{}", err);
            return HttpResponse::InternalServerError().body("Unable to Join Car");
        }
    }

    let user_in_car = query!(
        r#"
        SELECT COUNT(*) as count
        FROM (
            SELECT id FROM car
            WHERE event_id = $1 AND driver = $2 
            UNION
            SELECT rider.car_id 
            FROM rider 
            JOIN car ON rider.car_id = car.id 
            WHERE car.event_id = $1 AND rider.rider = $2
        ) AS data"#,
        event_id,
        user_id
    )
    .fetch_one(&data.db)
    .await
    .unwrap();

    if user_in_car.count.unwrap() > 0 {
        return HttpResponse::BadRequest().body("User is already in a car.");
    }

    let result = sqlx::query!(
        r#"
        INSERT INTO rider (car_id, rider) VALUES ($1, $2)
        "#,
        car_id,
        user_id
    )
    .execute(&data.db)
    .await;

    match result {
        Ok(_) => {
            let work_queue = WorkQueue::new(data.work_queue_key.clone());
            let item = Item::from_json_data(&RedisJob::Join(SimpleRiderChange {
                event_id,
                car_id,
                rider_id: user_id,
            }))
            .unwrap();
            let mut redis = data.redis.lock().unwrap().clone();
            work_queue
                .add_item(&mut redis, &item)
                .await
                .expect("failed to add item to work queue");
            HttpResponse::Ok().body("Joined Car")
        }
        Err(e) => {
            error!("Failed to Add Rider: {}", e);
            HttpResponse::InternalServerError().body("Failed to create car")
        }
    }
}

#[utoipa::path(
    params(
        ("event_id" = i32, Path, description = "ID of the Event this Rider Applies To"),
        ("car_id" = i32, Path, description = "ID of the Car this Rider Applies To")
    ),
    responses(
        (status = 200, description = "Remove other rider from car. Must be done by driver.")
    )
)]
#[delete("/", wrap = "SessionAuth")]
async fn delete_rider(
    data: web::Data<AppState>,
    session: Session,
    path: web::Path<(i32, i32)>,
) -> impl Responder {
    let (event_id, car_id) = path.into_inner();
    let user_id = session.get::<UserInfo>("userinfo").unwrap().unwrap().id;

    let deleted = sqlx::query!(
        "DELETE FROM rider WHERE car_id = $1 AND rider = $2",
        car_id,
        user_id
    )
    .execute(&data.db)
    .await;

    match deleted {
        Ok(_) => {
            let work_queue = WorkQueue::new(data.work_queue_key.clone());
            let item = Item::from_json_data(&RedisJob::Leave(SimpleRiderChange {
                event_id,
                car_id,
                rider_id: user_id,
            }))
            .unwrap();
            let mut redis = data.redis.lock().unwrap().clone();
            work_queue
                .add_item(&mut redis, &item)
                .await
                .expect("failed to add item to work queue");
            HttpResponse::Ok().body("Rider deleted")
        }
        Err(err) => {
            error!("{}", err);
            HttpResponse::InternalServerError().body("Failed to delete rider")
        }
    }
}

pub fn scope() -> Scope {
    web::scope("/{car_id}/rider")
        .service(create_rider)
        .service(delete_rider)
}
