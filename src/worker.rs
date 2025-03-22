use anyhow::{anyhow, Result};
use log::error;
use redis::{aio::MultiplexedConnection, RedisResult};
use redis_work_queue::{Item, KeyPrefix, WorkQueue};
use sqlx::{postgres::PgPoolOptions, query, query_as, Pool, Postgres};
use std::{collections::HashSet, env, time::Duration};

use crate::{
    app::{RedisJob, SimpleRiderChange},
    db::user::{UserData, UserRealm},
    pings::PingClient,
};

struct RedisError {
    pub msg: String,
    pub should_retry: bool,
}

pub async fn main() -> Result<()> {
    let db = redis::Client::open(env::var("REDIS_URL").expect("REDIS_URL must be set"))?
        .get_multiplexed_async_connection()
        .await?;

    let work_queue = WorkQueue::new(KeyPrefix::from("rideboard"));

    let db_pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&env::var("DATABASE_URL").expect("DATABASE_URL must be set"))
        .await?;

    let pings = PingClient::new(
        env::var("PINGS_TOKEN").expect("PINGS_TOKEN must be set"),
        env::var("PINGS_JOIN_ROUTE").expect("PINGS_JOIN_ROUTE must be set"),
        env::var("PINGS_LEAVE_ROUTE").expect("PINGS_LEAVE_ROUTE must be set"),
        env::var("PINGS_ADD_ROUTE").expect("PINGS_ADD_ROUTE must be set"),
        env::var("PINGS_REMOVE_ROUTE").expect("PINGS_REMOVE_ROUTE must be set"),
    )?;

    work_loop(db, work_queue, db_pool, pings).await?;
    Ok(())
}

async fn get_event_name(event_id: i32, db_pool: &Pool<Postgres>) -> Result<String> {
    match query!(r#"SELECT name FROM event WHERE id = $1"#, event_id)
        .fetch_one(db_pool)
        .await
    {
        Ok(event) => Ok(event.name),
        Err(err) => Err(anyhow!("Failed to get event name: {}", err)),
    }
}

async fn get_driver(car_id: i32, db_pool: &Pool<Postgres>) -> Result<UserData> {
    query_as!(
        UserData,
        r#"
        SELECT users.id AS "id!", users.realm AS "realm!: UserRealm", users.name AS "name!", users.email AS "email!"
        FROM car JOIN users ON car.driver = users.id WHERE car.id = $1;
        "#, car_id
    ).fetch_one(db_pool).await.map_err(|err| anyhow!("Failed to get driver: {}", err))
}

async fn get_simple_data(
    data: SimpleRiderChange,
    db_pool: &Pool<Postgres>,
) -> Result<(String, UserData, UserData)> {
    let rider = UserData::select_one(data.rider_id, db_pool)
        .await?
        .ok_or(anyhow!("Rider does not exist"))?;
    Ok((
        get_event_name(data.event_id, db_pool).await?,
        get_driver(data.car_id, db_pool).await?,
        rider,
    ))
}

async fn work(job: &Item, db_pool: &Pool<Postgres>, pings: &PingClient) -> Result<(), RedisError> {
    let job_data: RedisJob = job.data_json().map_err(|_err| RedisError {
        msg: "Failed to Parse into Job".to_string(),
        should_retry: false,
    })?;
    match job_data {
        RedisJob::Join(data) => {
            let (event_name, driver, rider) =
                get_simple_data(data, db_pool)
                    .await
                    .map_err(|err| RedisError {
                        msg: err.to_string(),
                        should_retry: false,
                    })?;
            if driver.realm != UserRealm::Csh {
                return Ok(());
            }
            pings
                .send_join(
                    driver.email.trim_end_matches("@csh.rit.edu"),
                    &rider.name,
                    &event_name,
                )
                .await
                .map_err(|err| RedisError {
                    msg: err.to_string(),
                    should_retry: true,
                })?
        }
        RedisJob::Leave(data) => {
            let (event_name, driver, rider) =
                get_simple_data(data, db_pool)
                    .await
                    .map_err(|err| RedisError {
                        msg: err.to_string(),
                        should_retry: false,
                    })?;
            if driver.realm != UserRealm::Csh {
                return Ok(());
            }
            pings
                .send_leave(
                    driver.email.trim_end_matches("@csh.rit.edu"),
                    &rider.name,
                    &event_name,
                )
                .await
                .map_err(|err| RedisError {
                    msg: err.to_string(),
                    should_retry: true,
                })?;
        }
        RedisJob::RiderUpdate(data) => {
            let event_name = get_event_name(data.event_id, db_pool)
                .await
                .map_err(|err| RedisError {
                    msg: err.to_string(),
                    should_retry: true,
                })?;
            let driver = get_driver(data.car_id, db_pool)
                .await
                .map_err(|err| RedisError {
                    msg: err.to_string(),
                    should_retry: true,
                })?;
            let old_set: HashSet<String> = HashSet::from_iter(data.old_riders);
            let new_set: HashSet<String> = HashSet::from_iter(data.new_riders);
            let user_map = UserData::select_map(
                old_set
                    .difference(&new_set)
                    .chain(new_set.difference(&old_set))
                    .map(|s| s.to_string())
                    .collect(),
                db_pool,
            )
            .await
            .map_err(|err| RedisError {
                msg: err.to_string(),
                should_retry: true,
            })?;
            for removed in old_set.difference(&new_set) {
                let user = user_map.get(removed).ok_or(RedisError {
                    msg: "User was missing from map.".to_string(),
                    should_retry: false,
                })?;
                if user.realm != UserRealm::Csh {
                    continue;
                }
                pings
                    .send_remove(
                        user.email.trim_end_matches("@csh.rit.edu"),
                        &driver.name,
                        &event_name,
                    )
                    .await
                    .map_err(|err| RedisError {
                        msg: format!("Failed to send message: {}", err),
                        should_retry: true,
                    })?;
            }
            for added in new_set.difference(&old_set) {
                let user = user_map.get(added).ok_or(RedisError {
                    msg: "User was missing from map.".to_string(),
                    should_retry: false,
                })?;
                if user.realm != UserRealm::Csh {
                    continue;
                }
                pings
                    .send_add(
                        user.email.trim_end_matches("@csh.rit.edu"),
                        &driver.name,
                        &event_name,
                    )
                    .await
                    .map_err(|err| RedisError {
                        msg: format!("Failed to send message: {}", err),
                        should_retry: true,
                    })?;
            }
        }
    }
    Ok(())
}

pub async fn work_loop(
    mut db: MultiplexedConnection,
    work_queue: WorkQueue,
    db_pool: Pool<Postgres>,
    pings: PingClient,
) -> RedisResult<()> {
    loop {
        // Wait for a job with no timeout and a lease time of 5 seconds.
        let job: Item = match work_queue
            .lease(&mut db, None, Duration::from_secs(5))
            .await?
        {
            Some(job) => job,
            None => {
                error!("Failed to get job.");
                continue;
            }
        };
        match work(&job, &db_pool, &pings).await {
            // Mark successful jobs as complete
            Ok(()) => {
                work_queue.complete(&mut db, &job).await?;
            }
            // Drop a job that should be retried - it will be returned to the work queue after
            // the (5 second) lease expires.
            Err(err) if err.should_retry => error!("{}", err.msg),
            // Errors that shouldn't cause a retry should mark the job as complete so it isn't
            // tried again.
            Err(err) => {
                error!("{}", err.msg);
                work_queue.complete(&mut db, &job).await?;
            }
        }
    }
}
