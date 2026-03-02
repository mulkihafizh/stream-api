use crate::config::Config;
use crate::db;
use rusqlite::Connection;
use lofty::file::{AudioFile, TaggedFileExt};
use std::path::Path;
use std::sync::Mutex;
use std::time::UNIX_EPOCH;
use uuid::Uuid;
use walkdir::WalkDir;

pub fn scan_library(
    config: &Config,
    db_handle: &Mutex<Connection>,
) -> Result<usize, Box<dyn std::error::Error + Send + Sync>> {
    log::info!("Starting library scan: {}", config.music_library_path);
    let mut count: usize = 0;

    for entry in WalkDir::new(&config.music_library_path)
        .follow_links(true)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();

        let is_flac = path
            .extension()
            .map_or(false, |ext| ext.to_ascii_lowercase() == "flac");

        if !is_flac || !path.is_file() {
            continue;
        }

        let file_path_str = path.to_string_lossy().to_string();

        let metadata = match std::fs::metadata(path) {
            Ok(m) => m,
            Err(e) => {
                log::warn!("Cannot stat {}: {}", file_path_str, e);
                continue;
            }
        };

        let modified = metadata
            .modified()
            .unwrap_or(UNIX_EPOCH)
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;

        {
            let conn = db_handle.lock().unwrap();
            if let Ok(Some(stored)) = db::get_track_modified_time(&conn, &file_path_str) {
                if stored == modified {
                    count += 1;
                    continue;
                }
            }
        }

        let tagged_file = match lofty::read_from_path(path) {
            Ok(f) => f,
            Err(e) => {
                log::warn!("Failed to read tags from {}: {}", file_path_str, e);
                continue;
            }
        };

        let properties = tagged_file.properties();
        let sample_rate = properties.sample_rate().unwrap_or(0);
        let bit_depth = properties.bit_depth().unwrap_or(0) as u16;
        let duration = properties.duration().as_secs_f64();

        let tag = tagged_file
            .primary_tag()
            .or_else(|| tagged_file.first_tag());

        let title;
        let artist;
        let album;

        if let Some(t) = tag {
            title = t
                .get_string(&lofty::tag::ItemKey::TrackTitle)
                .unwrap_or("Unknown Title")
                .to_string();
            artist = t
                .get_string(&lofty::tag::ItemKey::TrackArtist)
                .unwrap_or("Unknown Artist")
                .to_string();
            album = t
                .get_string(&lofty::tag::ItemKey::AlbumTitle)
                .unwrap_or("Unknown Album")
                .to_string();
        } else {
            title = path
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| "Unknown Title".to_string());
            artist = "Unknown Artist".to_string();
            album = "Unknown Album".to_string();
        }

        let track_id = Uuid::new_v4().to_string();

        let cover_filename = extract_cover_art(tag, &config.cover_cache_dir, &track_id);

        {
            let conn = db_handle.lock().unwrap();
            if let Err(e) = db::upsert_track(
                &conn,
                &track_id,
                &title,
                &artist,
                &album,
                sample_rate,
                bit_depth,
                duration,
                &file_path_str,
                cover_filename.as_deref(),
                modified,
            ) {
                log::warn!("Failed to upsert {}: {}", file_path_str, e);
                continue;
            }
        }

        count += 1;
        if count % 100 == 0 {
            log::info!("Scanned {} tracks so far...", count);
        }
    }

    log::info!("Library scan complete: {} tracks indexed", count);
    Ok(count)
}

fn extract_cover_art(
    tag: Option<&lofty::tag::Tag>,
    cache_dir: &str,
    track_id: &str,
) -> Option<String> {
    let tag = tag?;
    let pictures = tag.pictures();

    let cover = pictures
        .iter()
        .find(|p| p.pic_type() == lofty::picture::PictureType::CoverFront)
        .or_else(|| pictures.first());

    let picture = cover?;

    let filename = format!("{}.jpg", track_id);
    let output_path = Path::new(cache_dir).join(&filename);

    match image::load_from_memory(picture.data()) {
        Ok(img) => match img.save(&output_path) {
            Ok(_) => Some(filename),
            Err(e) => {
                log::warn!("Failed to save cover art {}: {}", output_path.display(), e);
                None
            }
        },
        Err(e) => {
            log::warn!("Failed to decode cover art for track {}: {}", track_id, e);
            None
        }
    }
}
