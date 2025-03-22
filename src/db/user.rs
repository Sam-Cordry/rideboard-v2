use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use sqlx::{query_as, Executor, Postgres};
use std::collections::HashMap;
use utoipa::ToSchema;

#[derive(Serialize, Deserialize, sqlx::Type, Clone, PartialEq)]
#[sqlx(type_name = "user_realm", rename_all = "lowercase")]
pub enum UserRealm {
    Csh,
    Google,
}

#[derive(Serialize, Deserialize, sqlx::Type, ToSchema, Clone)]
#[serde(rename_all = "camelCase")]
pub struct UserData {
    pub id: String,
    pub realm: UserRealm,
    pub name: String,
    pub email: String,
}

impl UserData {
    pub async fn insert_new<'c, C>(
        id: String,
        realm: UserRealm,
        name: String,
        email: String,
        conn: C,
    ) -> Result<Self>
    where
        C: Executor<'c, Database = Postgres>,
    {
        query_as!(
            UserData,
            r#"INSERT INTO users (id, realm, name, email)
            VALUES ($1, $2, $3, $4)
            ON CONFLICT (id) DO UPDATE SET realm = EXCLUDED.realm, name = EXCLUDED.name, email = EXCLUDED.email
            RETURNING id AS "id!", realm AS "realm!: UserRealm", name AS "name!", email AS "email!";"#,
            id,
            realm as _,
            name,
            email
        )
        .fetch_one(conn)
        .await.map_err(|err| anyhow!("Failed to insert/update user: {}", err))
    }
    pub async fn select_search<'c, C>(query: String, conn: C) -> Result<Vec<Self>>
    where
        C: Executor<'c, Database = Postgres>,
    {
        query_as!(UserData, r#"SELECT id AS "id!", realm AS "realm!: UserRealm", name AS "name!", email AS "email!" FROM users WHERE LOWER(name) LIKE $1 OR LOWER(email) LIKE $1;"#, query.to_lowercase())
        .fetch_all(conn)
        .await.map_err(|err| anyhow!("Failed to get users: {}", err))
    }
    pub async fn select_map<'c, C>(ids: Vec<String>, conn: C) -> Result<HashMap<String, Self>>
    where
        C: Executor<'c, Database = Postgres>,
    {
        let data = query_as!(
            UserData,
            r#"
            SELECT id AS "id!", realm AS "realm!: UserRealm", name AS "name!", email AS "email!"
            FROM users WHERE id IN (SELECT UNNEST($1::VARCHAR[]))
            "#,
            &ids
        )
        .fetch_all(conn)
        .await
        .map_err(|err| anyhow!("Failed to get users: {}", err))?;
        Ok(HashMap::from_iter(
            data.iter().map(|user| (user.id.clone(), user.clone())),
        ))
    }
    pub async fn select_one<'c, C>(id: String, conn: C) -> Result<Option<Self>>
    where
        C: Executor<'c, Database = Postgres>,
    {
        query_as!(
            UserData,
            r#"
            SELECT users.id AS "id!", users.realm AS "realm!: UserRealm", users.name AS "name!", users.email AS "email!"
            FROM users where id = $1;
            "#, id
        ).fetch_optional(conn).await
        .map_err(|err| anyhow!("Failed to Get Events: {}", err))
    }
}
