use actix_session::{storage::CookieSessionStore, SessionMiddleware};
use actix_web::{cookie::Key, middleware::Logger, web, App, HttpResponse, HttpServer, Responder};
use anyhow::{anyhow, Result};
use base64::prelude::*;
use include_dir::{include_dir, Dir};
use log::info;
use redis_work_queue::{KeyPrefix, WorkQueue};
use sqlx::postgres::PgPoolOptions;
use std::{
    env,
    sync::{Arc, Mutex},
};

use crate::{
    api,
    app::{ApiError, AppState},
    auth,
    redis::RedisQueue,
};

//mod pings; // Undo this when developing it

// Embed the 'static' directory into the binary
static STATIC_DIR: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/src/frontend/dist");

async fn serve_file(path: web::Path<String>) -> impl Responder {
    let file_path = path.into_inner();
    if let Some(file) = STATIC_DIR.get_file(&file_path) {
        let content = file.contents();
        let mime = mime_guess::from_path(&file_path).first_or_octet_stream();
        HttpResponse::Ok().content_type(mime.as_ref()).body(content)
    } else {
        HttpResponse::NotFound().json(ApiError::from("File not found".to_string()))
    }
}

async fn serve_index() -> impl Responder {
    if let Some(file) = STATIC_DIR.get_file("index.html") {
        let content = file.contents();
        let mime = mime_guess::from_path("index.html").first_or_octet_stream();
        HttpResponse::Ok().content_type(mime.as_ref()).body(content)
    } else {
        HttpResponse::NotFound().json(ApiError::from("File not found".to_string()))
    }
}

pub async fn main() -> Result<()> {
    let host = env::var("HOST").unwrap_or("127.0.0.1".to_string());
    let host_inner = host.clone();
    let port: i32 = match &env::var("PORT").map(|port| port.parse()) {
        Ok(Ok(p)) => *p,
        Ok(Err(_)) => 8080,
        Err(_) => 8080,
    };

    let db_pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&env::var("DATABASE_URL").expect("DATABASE_URL must be set"))
        .await
        .expect("Failed to create pool");

    let session_key = env::var("SESSION_KEY")
        .map_err(|e| anyhow!("Failed to get Env Var: {}", e))
        .and_then(|key64| {
            BASE64_STANDARD
                .decode(key64)
                .map_err(|e| anyhow!("Failed to decode session key: {}", e))
        })
        .map(|key| Key::from(&key))
        .unwrap_or(Key::generate());

    let redis_conn = redis::Client::open(env::var("REDIS_URL").expect("REDIS_URL must be set"))
        .expect("Failed to create Redis Client")
        .get_multiplexed_async_connection()
        .await
        .expect("Failed to create Redis Connection");

    info!("Starting server at http://{host}:{port}");
    HttpServer::new(move || {
        let (google_client, csh_client) = auth::get_clients(&host_inner, port);

        App::new()
            .app_data(web::Data::new(AppState {
                db: db_pool.clone(),
                redis: Arc::new(Mutex::new(RedisQueue {
                    redis: redis_conn.clone(),
                    work_queue: WorkQueue::new(KeyPrefix::from("rideboard")),
                })),
                google_oauth: google_client,
                google_userinfo_url: "https://openidconnect.googleapis.com/v1/userinfo".to_string(),
                csh_oauth: csh_client,
                csh_userinfo_url: env::var("CSH_USERINFO_URL")
                    .expect("Missing Userinfo URL for CSH Auth"),
            }))
            .wrap(
                SessionMiddleware::builder(CookieSessionStore::default(), session_key.clone())
                    .cookie_secure(env::var("DEVELOPMENT").is_err())
                    .build(),
            )
            .wrap(Logger::default())
            .service(api::scope())
            .route("/", web::get().to(serve_index))
            .route("/history", web::get().to(serve_index))
            .route("/login", web::get().to(serve_index))
            .route("/{filename:.*}", web::get().to(serve_file))
    })
    .bind(format!("{host}:{port}"))?
    .run()
    .await
    .map_err(|err| anyhow!("Failed to run server: {}", err))
}
