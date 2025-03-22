use actix_session::Session;
use actix_web::{
    delete, get, post, put,
    web::{self},
    HttpResponse, Responder, Scope,
};
use log::error;
use sqlx::query;
use utoipa::OpenApi;

use crate::{
    api::v1::auth::models::UserInfo,
    app::{ApiError, AppState, MultipleRiderChange, RedisJob},
    auth::SessionAuth,
    db::{
        car::{Car, CarData},
        user::UserData,
    },
};

mod rider;

#[derive(OpenApi)]
#[openapi(
    nest(
        (path = "/{car_id}/rider", api = rider::ApiDoc),
    ),
    paths(
        create_car,
        get_car,
        get_all_cars,
        update_car,
        delete_car
    ),
    components(schemas(Car, CarData, UserData))
)]
pub struct ApiDoc;

#[utoipa::path(
    params(
        ("event_id" = i32, Path, description = "ID of the Event this Car Applies To")
    ),
    responses(
        (status = 200, description = "Create new Car for Event.", body = i32),
        (status = 400, body = ApiError),
        (status = 401, body = ApiError),
        (status = 500, body = ApiError),
    )
)]
#[post("/", wrap = "SessionAuth")]
async fn create_car(
    data: web::Data<AppState>,
    session: Session,
    car: web::Json<CarData>,
    path: web::Path<i32>,
) -> impl Responder {
    let event_id: i32 = path.into_inner();
    let user_id = match session.get::<UserInfo>("userinfo").ok().flatten() {
        Some(user) => user.id,
        None => {
            return HttpResponse::Unauthorized().json(ApiError::from(
                "Failed to get user data from session".to_string(),
            ))
        }
    };

    let other_cars = match Car::select_all(event_id, &data.db).await {
        Ok(cars) => cars,
        Err(err) => {
            error!("{}", err);
            return HttpResponse::InternalServerError().json(ApiError::from(
                "Failed to get other cars for data validation".to_string(),
            ));
        }
    };
    if let Err(errs) = car.validate(&user_id, other_cars) {
        return HttpResponse::BadRequest().json(ApiError::from(errs));
    }

    let mut tx = match data.db.begin().await {
        Ok(tx) => tx,
        Err(err) => {
            error!("{}", err);
            return HttpResponse::InternalServerError()
                .json(ApiError::from("Failed to make SQL Transaction".to_string()));
        }
    };

    let record = match Car::insert_new(event_id, user_id, &car, &mut *tx).await {
        Ok(car) => car,
        Err(err) => {
            error!("{}", err);
            tx.rollback().await.unwrap();
            return HttpResponse::InternalServerError().json(ApiError::from(
                "Failed to create new car in database".to_string(),
            ));
        }
    };

    if let Err(err) = query!(
        r#"
        INSERT INTO rider (car_id, rider) SELECT $1, * FROM UNNEST($2::VARCHAR[])
        "#,
        record.id,
        &car.riders
    )
    .execute(&mut *tx)
    .await
    {
        error!("{}", err);
        tx.rollback().await.unwrap();
        return HttpResponse::InternalServerError()
            .json(ApiError::from("Failed to add riders to car".to_string()));
    }
    if let Err(err) = tx.commit().await {
        error!("{}", err);
        return HttpResponse::InternalServerError()
            .json(ApiError::from("Failed to commit transaction".to_string()));
    }
    match data.redis.lock().map(|mut mutex| async move {
        mutex
            .insert_job(RedisJob::RiderUpdate(MultipleRiderChange {
                event_id,
                car_id: record.id,
                old_riders: Vec::new(),
                new_riders: car.riders.clone(),
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
    HttpResponse::Ok().json(record.id)
}

#[utoipa::path(
    params(
        ("event_id" = i32, Path, description = "ID of the Event this Car Applies To")
    ),
    responses(
        (status = 200, description = "Get car by ID", body = CarData),
        (status = 404, body = ApiError),
        (status = 500, body = ApiError)
    )
)]
#[get("/{car_id}", wrap = "SessionAuth")]
async fn get_car(data: web::Data<AppState>, path: web::Path<(i32, i32)>) -> impl Responder {
    let (event_id, car_id) = path.into_inner();
    let result = Car::select_one(event_id, car_id, &data.db).await;

    match result {
        Ok(Some(car)) => HttpResponse::Ok().json(car),
        Ok(None) => HttpResponse::NotFound().json(ApiError::from("Car not found".to_string())),
        Err(_) => HttpResponse::InternalServerError()
            .json(ApiError::from("Failed to get Car".to_string())),
    }
}

#[utoipa::path(
    params(
        ("event_id" = i32, Path, description = "ID of the Event this Car Applies To")
    ),
    responses(
        (status = 200, description = "Get all cars for event", body = [CarData]),
        (status = 500, body = ApiError)
    )
)]
#[get("/", wrap = "SessionAuth")]
async fn get_all_cars(data: web::Data<AppState>, path: web::Path<i32>) -> impl Responder {
    let event_id: i32 = path.into_inner();
    let result = Car::select_all(event_id, &data.db).await;

    match result {
        Ok(cars) => HttpResponse::Ok().json(cars),
        Err(e) => {
            error!("{}", e);
            HttpResponse::InternalServerError()
                .json(ApiError::from("Failed to get cars".to_string()))
        }
    }
}

#[utoipa::path(
    params(
        ("event_id" = i32, Path, description = "ID of the Event this Car Applies To")
    ),
    responses(
        (status = 200, description = "Update Car"),
        (status = 400, body = ApiError),
        (status = 401, body = ApiError),
        (status = 404, body = ApiError),
        (status = 500, body = ApiError)
    )
)]
#[put("/{car_id}", wrap = "SessionAuth")]
async fn update_car(
    data: web::Data<AppState>,
    session: Session,
    path: web::Path<(i32, i32)>,
    car: web::Json<CarData>,
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

    let other_cars = match Car::select_all(event_id, &data.db).await {
        Ok(cars) => cars.into_iter().filter(|car| car.id != car_id).collect(),
        Err(err) => {
            error!("{}", err);
            return HttpResponse::InternalServerError().json(ApiError::from(
                "Failed to get other cars for data validation".to_string(),
            ));
        }
    };
    if let Err(errs) = car.validate(&user_id, other_cars) {
        return HttpResponse::BadRequest().json(ApiError::from(errs));
    }

    let mut tx = match data.db.begin().await {
        Ok(tx) => tx,
        Err(err) => {
            error!("{}", err);
            return HttpResponse::InternalServerError()
                .json(ApiError::from("Failed to make SQL Transaction".to_string()));
        }
    };

    let updated = Car::update(car_id, event_id, user_id, &car, &mut *tx).await;

    match updated {
        Ok(Some(_)) => {}
        Ok(None) => {
            tx.rollback().await.unwrap();
            return HttpResponse::NotFound().json(ApiError::from(
                "Car not found or you are not the driver.".to_string(),
            ));
        }
        Err(err) => {
            tx.rollback().await.unwrap();
            error!("{}", err);
            return HttpResponse::InternalServerError()
                .json(ApiError::from("Failed to update car".to_string()));
        }
    }

    // Used for sending pings
    let current_riders: Vec<String> = match query!(
        r#"DELETE FROM rider WHERE car_id = $1 RETURNING rider"#,
        car_id
    )
    .fetch_all(&mut *tx)
    .await
    {
        Ok(riders) => riders.iter().map(|record| record.rider.clone()).collect(),
        Err(err) => {
            error!("{}", err);
            tx.rollback().await.unwrap();
            return HttpResponse::InternalServerError()
                .json(ApiError::from("Failed to remove old riders".to_string()));
        }
    };

    if let Err(err) = query!(
        r#"
        INSERT INTO rider (car_id, rider) SELECT $1, * FROM UNNEST($2::VARCHAR[])
        "#,
        car_id,
        &car.riders
    )
    .execute(&mut *tx)
    .await
    {
        error!("{}", err);
        tx.rollback().await.unwrap();
        return HttpResponse::InternalServerError()
            .json(ApiError::from("Failed to add new riders".to_string()));
    }
    if let Err(err) = tx.commit().await {
        error!("{}", err);
        return HttpResponse::InternalServerError()
            .json(ApiError::from("Failed to commit transaction".to_string()));
    }

    match data.redis.lock().map(|mut mutex| async move {
        mutex
            .insert_job(RedisJob::RiderUpdate(MultipleRiderChange {
                event_id,
                car_id,
                old_riders: current_riders,
                new_riders: car.riders.clone(),
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
    HttpResponse::Ok().body("Car updated successfully")
}

#[utoipa::path(
    params(
        ("event_id" = i32, Path, description = "ID of the Event this Car Applies To")
    ),
    responses(
        (status = 200, description = "Delete Car"),
        (status = 401, body = ApiError),
        (status = 404, body = ApiError),
        (status = 500, body = ApiError),
    )
)]
#[delete("/{car_id}", wrap = "SessionAuth")]
async fn delete_car(
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

    let deleted = Car::delete(car_id, event_id, user_id, &data.db).await;

    match deleted {
        Ok(Some(_)) => HttpResponse::Ok().json("Car deleted"),
        Ok(None) => HttpResponse::NotFound().json(ApiError::from(
            "Car not found or you are not the driver.".to_string(),
        )),
        Err(err) => {
            error!("{}", err);
            HttpResponse::InternalServerError()
                .json(ApiError::from("Failed to delete car".to_string()))
        }
    }
}

pub fn scope() -> Scope {
    web::scope("/{event_id}/car")
        .service(create_car)
        .service(get_car)
        .service(get_all_cars)
        .service(update_car)
        .service(delete_car)
        .service(rider::scope())
}
