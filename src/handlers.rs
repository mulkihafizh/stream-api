use crate::config::Config;
use crate::db;
use crate::models::{
    AddTrackRequest, CreatePlaylistRequest, LibraryResponse, LyricLine, LyricsPayload,
    LyricsResponse, Playlist, PlaylistsResponse, RecordPlayRequest, UnsyncedLyrics,
};
use actix_files::NamedFile;
use actix_web::{web, HttpRequest, HttpResponse};
use regex::Regex;
use rusqlite::Connection;
use std::path::PathBuf;
use std::sync::Mutex;

// ══════════════════════════════════════════════════════════
//  B-5: Songs endpoints
// ══════════════════════════════════════════════════════════

pub async fn get_songs(db_handle: web::Data<Mutex<Connection>>) -> HttpResponse {
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
                "error": "Failed to retrieve songs"
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

pub async fn get_song_by_id(
    db_handle: web::Data<Mutex<Connection>>,
    path: web::Path<String>,
) -> HttpResponse {
    let track_id = path.into_inner();
    let db_handle = db_handle.clone();

    let result = web::block(move || {
        let conn = db_handle.lock().unwrap();
        db::get_track_by_id(&conn, &track_id)
    })
    .await;

    match result {
        Ok(Ok(Some(track))) => HttpResponse::Ok().json(track),
        Ok(Ok(None)) => HttpResponse::NotFound().json(serde_json::json!({
            "error": "Song not found"
        })),
        Ok(Err(e)) => {
            log::error!("Database error: {}", e);
            HttpResponse::InternalServerError().json(serde_json::json!({
                "error": "Failed to retrieve song"
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

// ══════════════════════════════════════════════════════════
//  B-5: Albums endpoints
// ══════════════════════════════════════════════════════════

pub async fn get_albums(db_handle: web::Data<Mutex<Connection>>) -> HttpResponse {
    let db_handle = db_handle.clone();
    let result = web::block(move || {
        let conn = db_handle.lock().unwrap();
        db::get_all_albums(&conn)
    })
    .await;

    match result {
        Ok(Ok(albums)) => HttpResponse::Ok().json(serde_json::json!({
            "total": albums.len(),
            "albums": albums
        })),
        Ok(Err(e)) => {
            log::error!("Database error: {}", e);
            HttpResponse::InternalServerError().json(serde_json::json!({
                "error": "Failed to retrieve albums"
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

pub async fn get_album_by_id(
    db_handle: web::Data<Mutex<Connection>>,
    path: web::Path<String>,
) -> HttpResponse {
    let album_id = path.into_inner();
    let db_handle = db_handle.clone();

    let result = web::block(move || {
        let conn = db_handle.lock().unwrap();
        db::get_album_detail(&conn, &album_id)
    })
    .await;

    match result {
        Ok(Ok(Some(album))) => HttpResponse::Ok().json(album),
        Ok(Ok(None)) => HttpResponse::NotFound().json(serde_json::json!({
            "error": "Album not found"
        })),
        Ok(Err(e)) => {
            log::error!("Database error: {}", e);
            HttpResponse::InternalServerError().json(serde_json::json!({
                "error": "Failed to retrieve album"
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

// ══════════════════════════════════════════════════════════
//  B-2: Lyrics endpoint
// ══════════════════════════════════════════════════════════

pub async fn get_lyrics(
    db_handle: web::Data<Mutex<Connection>>,
    path: web::Path<String>,
) -> HttpResponse {
    let song_id = path.into_inner();
    let db_handle = db_handle.clone();

    let result = web::block(move || {
        let conn = db_handle.lock().unwrap();
        db::get_track_lyrics(&conn, &song_id)
    })
    .await;

    match result {
        Ok(Ok(Some((id, lyrics_type, lyrics_raw, kana_text, romaji_text)))) => {
            let payload = match lyrics_type.as_deref() {
                Some("synced") => {
                    let lines =
                        parse_lrc_lines(&lyrics_raw.unwrap_or_default(), kana_text, romaji_text);
                    LyricsPayload::Synced { lyrics: lines }
                }
                Some("unsynced") => {
                    let raw = lyrics_raw.unwrap_or_default();
                    LyricsPayload::Unsynced {
                        lyrics: UnsyncedLyrics {
                            text: raw,
                            kana: kana_text,
                            romaji: romaji_text,
                        },
                    }
                }
                _ => LyricsPayload::None { lyrics: None },
            };

            HttpResponse::Ok().json(LyricsResponse {
                song_id: id,
                payload,
            })
        }
        Ok(Ok(None)) => HttpResponse::NotFound().json(serde_json::json!({
            "error": "Song not found"
        })),
        Ok(Err(e)) => {
            log::error!("Database error: {}", e);
            HttpResponse::InternalServerError().json(serde_json::json!({
                "error": "Failed to retrieve lyrics"
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

/// Parse LRC-format lyrics into structured LyricLine objects.
/// Also merges kana/romaji from parallel LRC data if available.
fn parse_lrc_lines(
    raw: &str,
    kana_raw: Option<String>,
    romaji_raw: Option<String>,
) -> Vec<LyricLine> {
    let lrc_regex = Regex::new(r"\[(\d{2}):(\d{2})\.(\d{2})\](.*)").unwrap();

    let kana_lines: Vec<&str> = kana_raw.as_deref().map_or(Vec::new(), |k| k.lines().collect());
    let romaji_lines: Vec<&str> = romaji_raw
        .as_deref()
        .map_or(Vec::new(), |r| r.lines().collect());

    let mut result = Vec::new();

    for (i, line) in raw.lines().enumerate() {
        if let Some(caps) = lrc_regex.captures(line) {
            let mm: u64 = caps[1].parse().unwrap_or(0);
            let ss: u64 = caps[2].parse().unwrap_or(0);
            let cs: u64 = caps[3].parse().unwrap_or(0);
            let time_ms = (mm * 60 + ss) * 1000 + cs * 10;
            let text = caps[4].trim().to_string();

            // Extract kana/romaji from parallel lines
            let kana = kana_lines.get(i).and_then(|kl| {
                lrc_regex.captures(kl).map(|c| c[4].trim().to_string())
            }).filter(|s| !s.is_empty());

            let romaji = romaji_lines.get(i).and_then(|rl| {
                lrc_regex.captures(rl).map(|c| c[4].trim().to_string())
            }).filter(|s| !s.is_empty());

            result.push(LyricLine {
                time_ms,
                text,
                kana,
                romaji,
            });
        }
    }

    result
}

// ══════════════════════════════════════════════════════════
//  Existing endpoints (preserved, updated with new Track fields)
// ══════════════════════════════════════════════════════════

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

// ══════════════════════════════════════════════════════════
//  B-4: Stream file (NamedFile handles Range natively)
// ══════════════════════════════════════════════════════════

pub async fn stream_file(
    path: web::Path<String>,
    config: web::Data<Config>,
    req: HttpRequest,
) -> actix_web::Result<HttpResponse> {
    let raw_path = path.into_inner();
    let relative_path = raw_path.replace('+', " ");
    log::debug!(
        "Stream request: raw='{}' resolved='{}'",
        raw_path,
        relative_path
    );

    let base = PathBuf::from(&config.music_library_path);
    let target = base.join(&relative_path);

    // Use web::block for fs operations to avoid blocking the Actix event loop
    let canonical = web::block(move || -> Result<(PathBuf, PathBuf), std::io::Error> {
        let cb = std::fs::canonicalize(&base)?;
        let ct = std::fs::canonicalize(&target)?;
        Ok((cb, ct))
    })
    .await
    .map_err(|_| actix_web::error::ErrorInternalServerError("IO error"))?
    .map_err(|e| {
        log::warn!("File not found: {}", e);
        actix_web::error::ErrorNotFound("File not found")
    })?;

    let (canonical_base, canonical_target) = canonical;

    if !canonical_target.starts_with(&canonical_base) {
        return Err(actix_web::error::ErrorForbidden("Access denied"));
    }

    // NamedFile handles Range requests (Accept-Ranges, 206, 416) automatically
    let file = NamedFile::open_async(canonical_target).await?;
    Ok(file.into_response(&req))
}

// ══════════════════════════════════════════════════════════
//  B-1: Serve cover art
// ══════════════════════════════════════════════════════════

pub async fn serve_cover(
    filename: web::Path<String>,
    config: web::Data<Config>,
    req: HttpRequest,
) -> actix_web::Result<HttpResponse> {
    let cover_path = PathBuf::from(&config.cover_cache_dir).join(filename.as_str());

    let file = match NamedFile::open_async(&cover_path).await {
        Ok(f) => f,
        Err(_) => return Err(actix_web::error::ErrorNotFound("Cover art not found")),
    };
    
    Ok(file.into_response(&req))
}

// ══════════════════════════════════════════════════════════
//  Play history + Stats (unchanged logic, new Track fields)
// ══════════════════════════════════════════════════════════

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
