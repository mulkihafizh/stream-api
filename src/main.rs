mod auth;
mod config;
mod db;
mod handlers;
mod models;
mod scanner;

use actix_web::{web, App, HttpServer};
use actix_web_httpauth::middleware::HttpAuthentication;
use std::sync::Mutex;

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    dotenvy::dotenv().ok();
    env_logger::init();

    let config = config::Config::from_env();
    let bind_address = config.bind_address.clone();

    std::fs::create_dir_all(&config.cover_cache_dir).expect("Failed to create cover cache dir");

    let conn =
        db::initialize_db(&config.database_path).expect("Failed to initialize SQLite database");
    let db = web::Data::new(Mutex::new(conn));
    let app_config = web::Data::new(config);

    let scan_db = db.clone();
    let scan_config = app_config.clone();

    tokio::task::spawn(async move {
        let result = tokio::task::spawn_blocking(move || {
            scanner::scan_library(&scan_config, &scan_db)
        })
        .await;

        match result {
            Ok(Ok(count)) => log::info!("Library scan complete: {} tracks indexed", count),
            Ok(Err(e)) => log::error!("Library scan failed: {}", e),
            Err(e) => log::error!("Library scan task panicked: {}", e),
        }
    });

    log::info!("Starting stream-api on {}", bind_address);

    HttpServer::new(move || {
        let auth = HttpAuthentication::bearer(auth::validator);

        App::new()
            .app_data(db.clone())
            .app_data(app_config.clone())
            .wrap(actix_web::middleware::Logger::default())
            .service(
                web::scope("/api")
                    .wrap(auth)
                    .route("/library", web::get().to(handlers::get_library))
                    .route("/playlists", web::get().to(handlers::get_playlists))
                    .route("/playlists", web::post().to(handlers::create_playlist))
                    .route(
                        "/playlists/{id}/tracks",
                        web::post().to(handlers::add_track_to_playlist),
                    )
                    .route("/stream/{path:.*}", web::get().to(handlers::stream_file))
                    .route("/covers/{filename:.*}", web::get().to(handlers::serve_cover)),
            )
    })
    .bind(&bind_address)?
    .run()
    .await
}
