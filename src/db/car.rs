use anyhow::{anyhow, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{query, query_as, Executor, Postgres};
use utoipa::ToSchema;

use crate::db::user::UserData;

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CarData {
    pub max_capacity: i32,
    pub departure_time: DateTime<Utc>,
    pub return_time: DateTime<Utc>,
    pub comment: String,
    pub riders: Vec<String>,
}

impl CarData {
    pub fn validate(&self, user: &String, other_cars: Vec<Car>) -> Result<(), Vec<String>> {
        let mut errs = Vec::new();
        if self.return_time < self.departure_time {
            errs.push("Return time cannot be before departure.".to_string())
        }
        if self.departure_time < Utc::now() {
            errs.push("Car cannot leave in the past.".to_string());
        }
        if self.max_capacity < 0 {
            errs.push("Capacity must be greater than or equal to 0".to_string());
        }
        if self.riders.len() > (self.max_capacity as usize) {
            errs.push("You have too many riders for your capacity.".to_string());
        }
        if self.riders.contains(user) {
            errs.push("You cannot be a rider in your own car.".to_string());
        }
        let other_car_members: Vec<String> = other_cars
            .iter()
            .filter_map(|car| {
                car.riders
                    .clone()
                    .map(|riders| (riders, car.driver.clone()))
            })
            .flat_map(|(riders, driver)| {
                let mut out = riders;
                out.push(driver);
                out
            })
            .map(|user| user.id)
            .collect();
        for rider in self.riders.iter() {
            if other_car_members.contains(rider) {
                errs.push(format!(
                    "{} is already in another car or is a driver.",
                    rider
                ))
            }
        }
        if !errs.is_empty() {
            return Err(errs);
        }
        Ok(())
    }
}

#[derive(Serialize, Deserialize, sqlx::FromRow, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Car {
    pub id: i32,
    pub event_id: Option<i32>,
    pub driver: UserData,
    pub riders: Option<Vec<UserData>>,
    pub max_capacity: i32,
    pub departure_time: DateTime<Utc>,
    pub return_time: DateTime<Utc>,
    pub comment: String,
}

impl Car {
    pub async fn insert_new<'c, C>(
        event_id: i32,
        driver_id: String,
        data: &CarData,
        conn: C,
    ) -> Result<Self>
    where
        C: Executor<'c, Database = Postgres>,
    {
        query_as!(
            Car,
            r#"
            WITH new_car AS (
                INSERT INTO car (event_id, driver, max_capacity, departure_time, return_time, comment)
                VALUES ($1, $2, $3, $4, $5, $6) RETURNING *
            )
            SELECT car.id, car.event_id, car.max_capacity, car.departure_time, car.return_time, car.comment,
            ROW(driverUser.*)::users AS "driver!: UserData",
            ARRAY_REMOVE(ARRAY_AGG(
                CASE WHEN riderUser.id IS NOT NULL
                THEN ROW(riderUser.*)::users
                END
            ), NULL) as "riders!: Vec<UserData>"
            FROM car
            JOIN users driverUser ON car.driver = driverUser.id
            LEFT JOIN rider on car.id = rider.car_id
            LEFT JOIN users riderUser ON rider.rider = riderUser.id
            GROUP BY car.id, driverUser.id
            "#,
            event_id,
            driver_id,
            data.max_capacity,
            data.departure_time,
            data.return_time,
            data.comment
        )
        .fetch_one(conn)
        .await.map_err(|err| anyhow!("Failed to Create Car: {}", err))
    }
    pub async fn update<'c, C>(
        id: i32,
        event_id: i32,
        driver_id: String,
        data: &CarData,
        conn: C,
    ) -> Result<Option<Self>>
    where
        C: Executor<'c, Database = Postgres>,
    {
        sqlx::query_as!(
            Car,
            r#"
            WITH new_car AS (
                UPDATE car SET
                max_capacity = COALESCE($1, max_capacity),
                departure_time = COALESCE($2, departure_time),
                return_time = COALESCE($3, return_time),
                comment = COALESCE($4, comment)
                WHERE event_id = $5 AND id = $6 AND driver = $7 RETURNING *
            )
            SELECT new_car.id, new_car.event_id, new_car.max_capacity, new_car.departure_time, new_car.return_time, new_car.comment,
            ROW(driverUser.*)::users AS "driver!: UserData",
            ARRAY_REMOVE(ARRAY_AGG(
                CASE WHEN riderUser.id IS NOT NULL
                THEN ROW(riderUser.*)::users
                END
            ), NULL) as "riders!: Vec<UserData>"
            FROM new_car
            JOIN users driverUser ON new_car.driver = driverUser.id
            LEFT JOIN rider on new_car.id = rider.car_id
            LEFT JOIN users riderUser ON rider.rider = riderUser.id
            GROUP BY new_car.id, new_car.event_id, new_car.max_capacity, new_car.departure_time, new_car.return_time, new_car.comment, driverUser.id
            "#,
            data.max_capacity,
            data.departure_time,
            data.return_time,
            data.comment,
            event_id,
            id,
            driver_id
        )
        .fetch_optional(conn)
        .await.map_err(|err| anyhow!("Failed to update Car: {}", err))
    }
    pub async fn select_all<'c, C>(event_id: i32, conn: C) -> Result<Vec<Self>>
    where
        C: Executor<'c, Database = Postgres>,
    {
        query_as!(
            Car,
            r#"SELECT car.id, car.event_id, car.max_capacity, car.departure_time, car.return_time, car.comment,
            ROW(driverUser.*)::users AS "driver!: UserData",
            ARRAY_REMOVE(ARRAY_AGG(
                CASE WHEN riderUser.id IS NOT NULL
                THEN ROW(riderUser.*)::users
                END
            ), NULL) as "riders!: Vec<UserData>"
            FROM car
            JOIN users driverUser ON car.driver = driverUser.id
            LEFT JOIN rider on car.id = rider.car_id
            LEFT JOIN users riderUser ON rider.rider = riderUser.id
            WHERE event_id = $1 GROUP BY car.id, driverUser.id"#,
            event_id)
            .fetch_all(conn)
            .await.map_err(|err| anyhow!("Failed to get cars: {}", err))
    }
    pub async fn select_one<'c, C>(event_id: i32, car_id: i32, conn: C) -> Result<Option<Self>>
    where
        C: Executor<'c, Database = Postgres>,
    {
        query_as!(
            Car,
            r#"SELECT car.id, car.event_id, car.max_capacity, car.departure_time, car.return_time, car.comment,
            ROW(driverUser.*)::users AS "driver!: UserData",
            ARRAY_REMOVE(ARRAY_AGG(
                CASE WHEN riderUser.id IS NOT NULL
                THEN ROW(riderUser.*)::users
                END
            ), NULL) as "riders!: Vec<UserData>"
            FROM car
            JOIN users driverUser ON car.driver = driverUser.id
            LEFT JOIN rider on car.id = rider.car_id
            LEFT JOIN users riderUser ON rider.rider = riderUser.id
            WHERE event_id = $1 AND car.id = $2 GROUP BY car.id, driverUser.id"#,
            event_id,
            car_id
        )
        .fetch_optional(conn)
        .await.map_err(|err| anyhow!("Failed to get car: {}", err))
    }
    pub async fn user_in_car<'c, C>(event_id: i32, user_id: &String, conn: C) -> Result<bool>
    where
        C: Executor<'c, Database = Postgres>,
    {
        match query!(
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
        .fetch_one(conn)
        .await
        .map(|record| record.count)
        {
            Ok(Some(count)) => Ok(count > 0),
            Ok(None) => Err(anyhow!("Failed to get Car Data")),
            Err(err) => Err(anyhow!("Failed to get Car Data: {}", err)),
        }
    }
    pub async fn delete<'c, C>(
        id: i32,
        event_id: i32,
        driver_id: String,
        conn: C,
    ) -> Result<Option<i32>>
    where
        C: Executor<'c, Database = Postgres>,
    {
        query!(
            "DELETE FROM car WHERE event_id = $1 AND id = $2 AND driver = $3 RETURNING id",
            event_id,
            id,
            driver_id
        )
        .fetch_optional(conn)
        .await
        .map(|res| res.map(|rec| rec.id))
        .map_err(|err| anyhow!("Failed to Delete Car: {}", err))
    }
}
