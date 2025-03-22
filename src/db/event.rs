use anyhow::{anyhow, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{query_as, Executor, Postgres};
use utoipa::ToSchema;

use crate::db::user::UserData;

#[derive(Serialize, Deserialize, sqlx::FromRow, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Event {
    pub id: i32,
    pub name: String,
    pub location: String,
    pub start_time: DateTime<Utc>,
    pub end_time: DateTime<Utc>,
    pub creator: UserData,
}

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct EventData {
    pub name: String,
    pub location: String,
    pub start_time: DateTime<Utc>,
    pub end_time: DateTime<Utc>,
}

impl EventData {
    pub fn validate(&self) -> Result<(), Vec<String>> {
        let mut errs = Vec::new();
        if self.name.is_empty() {
            errs.push("Missing Name.".to_string());
        }
        if self.location.is_empty() {
            errs.push("Missing Location.".to_string());
        }
        if self.start_time > self.end_time {
            errs.push("Start date cannot be after end date.".to_string());
        }
        if self.end_time < Utc::now() {
            errs.push("Event cannot be in the past.".to_string())
        }
        if !errs.is_empty() {
            return Err(errs);
        }
        Ok(())
    }
}

impl Event {
    pub async fn insert_new<'c, C>(data: &EventData, creator_id: String, conn: C) -> Result<Self>
    where
        C: Executor<'c, Database = Postgres>,
    {
        query_as!(
            Event,
            r#"
            WITH new_event AS (
                INSERT INTO event (name, location, start_time, end_time, creator) VALUES ($1, $2, $3, $4, $5) RETURNING *
            )
            SELECT new_event.id, new_event.name, new_event.location, new_event.start_time, new_event.end_time,
            (users.id, users.realm, users.name, users.email)::users AS "creator!: UserData"
            FROM new_event LEFT JOIN users ON new_event.creator = users.id
            "#,
            data.name, data.location, data.start_time, data.end_time, creator_id
        )
        .fetch_one(conn)
        .await.map_err(|err| anyhow!("Failed to Create Event: {}", err))
    }
    pub async fn update<'c, C>(
        id: i32,
        creator_id: String,
        data: &EventData,
        conn: C,
    ) -> Result<Option<Self>>
    where
        C: Executor<'c, Database = Postgres>,
    {
        sqlx::query_as!(
            Event,
            r#"
            WITH new_event AS (
                UPDATE event SET
                name = COALESCE($1, name),
                location = COALESCE($2, location),
                start_time = COALESCE($3, start_time),
                end_time = COALESCE($4, end_time)
                WHERE id = $5 AND creator = $6
                RETURNING *
            )
            SELECT new_event.id, new_event.name, new_event.location, new_event.start_time, new_event.end_time,
            ROW(users.*)::users AS "creator!: UserData"
            FROM new_event JOIN users ON new_event.creator = users.id
            "#,
            data.name,
            data.location,
            data.start_time,
            data.end_time,
            id,
            creator_id
        )
        .fetch_optional(conn)
        .await.map_err(|err| anyhow!("Failed to update Event: {}", err))
    }
    pub async fn select_all<'c, C>(past: bool, conn: C) -> Result<Vec<Self>>
    where
        C: Executor<'c, Database = Postgres>,
    {
        query_as!(
            Event,
            r#"
            SELECT
            event.id, event.name, event.location, event.start_time, event.end_time,
            ROW(users.*)::users AS "creator!: UserData"
            FROM event
            JOIN users ON users.id = event.creator
            WHERE (end_time >= NOW() AND $1 = False) OR (end_time < NOW() AND $1)
            ORDER BY start_time ASC
            "#,
            past
        )
        .fetch_all(conn)
        .await
        .map_err(|err| anyhow!("Failed to Get Events: {}", err))
    }
    pub async fn select_one<'c, C>(id: i32, conn: C) -> Result<Option<Self>>
    where
        C: Executor<'c, Database = Postgres>,
    {
        query_as!(
            Event,
            r#"
            SELECT
            event.id, event.name, event.location, event.start_time, event.end_time,
            ROW(users.*)::users AS "creator!: UserData"
            FROM event
            JOIN users ON users.id = event.creator
            WHERE event.id = $1
            "#,
            id
        )
        .fetch_optional(conn)
        .await
        .map_err(|err| anyhow!("Failed to Get Events: {}", err))
    }
    pub async fn delete<'c, C>(id: i32, creator_id: String, conn: C) -> Result<Option<i32>>
    where
        C: Executor<'c, Database = Postgres>,
    {
        sqlx::query!(
            "DELETE FROM event WHERE id = $1 AND creator = $2 RETURNING id",
            id,
            creator_id
        )
        .fetch_optional(conn)
        .await
        .map(|res| res.map(|rec| rec.id))
        .map_err(|err| anyhow!("Failed to Delete Event: {}", err))
    }
}
