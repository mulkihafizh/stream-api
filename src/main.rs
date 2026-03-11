mod auth;
mod config;
mod db;
mod handlers;
mod models;
mod romaji;
mod scanner;
mod worker;

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

    // --- Phase 4: Background worker pipeline ---
    // Bounded channel (capacity 100) prevents unbounded memory growth.
    let (tx, rx) = tokio::sync::mpsc::channel::<worker::IngestJob>(100);

    // Spawn the ingestion worker before the server starts.
    let worker_db = db.clone();
    let worker_config = app_config.clone();
    tokio::spawn(async move {
        worker::ingestion_worker(rx, worker_db, worker_config).await;
    });

    // Enqueue the initial library scan (non-blocking — server starts immediately).
    if let Err(e) = tx.send(worker::IngestJob::ScanLibrary).await {
        log::error!("Failed to enqueue initial library scan: {}", e);
    }

    let ingest_tx = web::Data::new(tx);

    log::info!("Starting stream-api on {}", bind_address);

    HttpServer::new(move || {
        let auth = HttpAuthentication::bearer(auth::validator);

        App::new()
            .app_data(db.clone())
            .app_data(app_config.clone())
            .app_data(ingest_tx.clone())
            .wrap(actix_web::middleware::Logger::default())
            .service(
                web::scope("/api")
                    .wrap(auth)
                    // Existing routes (preserved)
                    .route("/library", web::get().to(handlers::get_library))
                    .route("/playlists", web::get().to(handlers::get_playlists))
                    .route("/playlists", web::post().to(handlers::create_playlist))
                    .route(
                        "/playlists/{id}/tracks",
                        web::post().to(handlers::add_track_to_playlist),
                    )
                    .route("/history", web::post().to(handlers::record_play))
                    .route("/stats/{year}", web::get().to(handlers::get_annual_stats))
                    .route("/stream/{path:.*}", web::get().to(handlers::stream_file))
                    .route(
                        "/covers/{filename:.*}",
                        web::get().to(handlers::serve_cover),
                    )
                    // New routes (B-5: API surface completeness)
                    .route("/songs", web::get().to(handlers::get_songs))
                    .route("/songs/{id}", web::get().to(handlers::get_song_by_id))
                    .route("/albums", web::get().to(handlers::get_albums))
                    .route("/albums/{id}", web::get().to(handlers::get_album_by_id))
                    .route("/lyrics/{id}", web::get().to(handlers::get_lyrics)),
            )
    })
    .bind(&bind_address)?
    .run()
    .await
}
