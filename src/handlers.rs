use crate::config::Config;
use crate::db;
use crate::models::{
    AddTrackRequest, CreatePlaylistRequest, LibraryResponse, Playlist, PlaylistsResponse,
    RecordPlayRequest,
};
use actix_files::NamedFile;
use actix_web::{web, HttpRequest, HttpResponse};
use rusqlite::Connection;
use std::path::PathBuf;
use std::sync::Mutex;

pub async fn get_library(db_handle: web::Data<Mutex<Connection>>) -> HttpResponse {
    let db_handle = db_handle.clone();
    let result = web::block(move || {
        let conn = db_handle.lock().unwrap();
        db::get_all_tracks(&conn)
    })
    .await;

    match result {
        Ok(Ok(tracks)) => HttpResponse::Ok().json(LibraryResponse {
            total: tracks.len(),
            tracks,
        }),
        Ok(Err(e)) => {
            log::error!("Database error: {}", e);
            HttpResponse::InternalServerError().json(serde_json::json!({
                "error": "Failed to retrieve library"
            }))
        }
        Err(e) => {
            log::error!("Blocking error: {}", e);
            HttpResponse::InternalServerError().json(serde_json::json!({
                "error": "Internal server error"
            }))
        }
    }
}

pub async fn get_playlists(db_handle: web::Data<Mutex<Connection>>) -> HttpResponse {
    let db_handle = db_handle.clone();
    let result = web::block(move || {
        let conn = db_handle.lock().unwrap();
        db::get_playlists_with_tracks(&conn)
    })
    .await;

    match result {
        Ok(Ok(playlists)) => HttpResponse::Ok().json(PlaylistsResponse {
            total: playlists.len(),
            playlists,
        }),
        Ok(Err(e)) => {
            log::error!("Database error: {}", e);
            HttpResponse::InternalServerError().json(serde_json::json!({
                "error": "Failed to retrieve playlists"
            }))
        }
        Err(e) => {
            log::error!("Blocking error: {}", e);
            HttpResponse::InternalServerError().json(serde_json::json!({
                "error": "Internal server error"
            }))
        }
    }
}

pub async fn create_playlist(
    db_handle: web::Data<Mutex<Connection>>,
    body: web::Json<CreatePlaylistRequest>,
) -> HttpResponse {
    let name = body.into_inner().name;
    let db_handle = db_handle.clone();

    let result = web::block(move || {
        let conn = db_handle.lock().unwrap();
        db::create_playlist(&conn, &name).map(|id| Playlist { id, name })
    })
    .await;

    match result {
        Ok(Ok(playlist)) => HttpResponse::Created().json(playlist),
        Ok(Err(e)) => {
            log::error!("Database error: {}", e);
            HttpResponse::BadRequest().json(serde_json::json!({
                "error": "Failed to create playlist (name may already exist)"
            }))
        }
        Err(e) => {
            log::error!("Blocking error: {}", e);
            HttpResponse::InternalServerError().json(serde_json::json!({
                "error": "Internal server error"
            }))
        }
    }
}

pub async fn add_track_to_playlist(
    db_handle: web::Data<Mutex<Connection>>,
    path: web::Path<i64>,
    body: web::Json<AddTrackRequest>,
) -> HttpResponse {
    let playlist_id = path.into_inner();
    let track_id = body.into_inner().track_id;
    let db_handle = db_handle.clone();

    let result = web::block(move || {
        let conn = db_handle.lock().unwrap();
        db::add_track_to_playlist(&conn, playlist_id, &track_id)
    })
    .await;

    match result {
        Ok(Ok(())) => HttpResponse::Ok().json(serde_json::json!({
            "status": "ok"
        })),
        Ok(Err(e)) => {
            log::error!("Database error: {}", e);
            HttpResponse::BadRequest().json(serde_json::json!({
                "error": "Failed to add track to playlist"
            }))
        }
        Err(e) => {
            log::error!("Blocking error: {}", e);
            HttpResponse::InternalServerError().json(serde_json::json!({
                "error": "Internal server error"
            }))
        }
    }
}

pub async fn stream_file(
    path: web::Path<String>,
    config: web::Data<Config>,
    req: HttpRequest,
) -> actix_web::Result<HttpResponse> {
    let raw_path = path.into_inner();
    let relative_path = raw_path.replace('+', " ");
    log::debug!("Stream request: raw='{}' resolved='{}'", raw_path, relative_path);

    let base = PathBuf::from(&config.music_library_path);
    let target = base.join(&relative_path);

    let canonical_base = std::fs::canonicalize(&base)
        .map_err(|_| actix_web::error::ErrorNotFound("Library path not found"))?;
    let canonical_target = std::fs::canonicalize(&target).map_err(|e| {
        log::warn!("File not found: {} (error: {})", target.display(), e);
        actix_web::error::ErrorNotFound("File not found")
    })?;

    if !canonical_target.starts_with(&canonical_base) {
        return Err(actix_web::error::ErrorForbidden("Access denied"));
    }

    let file = NamedFile::open(canonical_target)?;
    Ok(file.into_response(&req))
}

pub async fn serve_cover(
    filename: web::Path<String>,
    config: web::Data<Config>,
    req: HttpRequest,
) -> actix_web::Result<HttpResponse> {
    let cover_path = PathBuf::from(&config.cover_cache_dir).join(filename.as_str());

    if !cover_path.exists() {
        return Err(actix_web::error::ErrorNotFound("Cover art not found"));
    }

    let file = NamedFile::open(cover_path)?;
    Ok(file.into_response(&req))
}

pub async fn record_play(
    db_handle: web::Data<Mutex<Connection>>,
    body: web::Json<RecordPlayRequest>,
) -> HttpResponse {
    let track_id = body.track_id.clone();
    let duration = body.duration_listened;

    let result = web::block(move || {
        let conn = db_handle.lock().unwrap();
        db::record_play(&conn, &track_id, duration)
    })
    .await;

    match result {
        Ok(Ok(())) => HttpResponse::Created().json(serde_json::json!({
            "status": "recorded"
        })),
        Ok(Err(e)) => {
            log::error!("Database error recording play: {}", e);
            HttpResponse::InternalServerError().json(serde_json::json!({
                "error": "Failed to record play history"
            }))
        }
        Err(e) => {
            log::error!("Blocking error recording play: {}", e);
            HttpResponse::InternalServerError().json(serde_json::json!({
                "error": "Internal server error"
            }))
        }
    }
}

pub async fn get_annual_stats(
    db_handle: web::Data<Mutex<Connection>>,
    path: web::Path<i32>,
) -> HttpResponse {
    let year = path.into_inner();
    let db_handle = db_handle.clone();

    let result = web::block(move || {
        let conn = db_handle.lock().unwrap();
        db::get_annual_stats(&conn, year)
    })
    .await;

    match result {
        Ok(Ok(stats)) => HttpResponse::Ok().json(stats),
        Ok(Err(e)) => {
            log::error!("Database error fetching stats: {}", e);
            HttpResponse::InternalServerError().json(serde_json::json!({
                "error": "Failed to fetch annual statistics"
            }))
        }
        Err(e) => {
            log::error!("Blocking error fetching stats: {}", e);
            HttpResponse::InternalServerError().json(serde_json::json!({
                "error": "Internal server error"
            }))
        }
    }
}

