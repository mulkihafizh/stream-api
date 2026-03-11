use crate::config::Config;
use crate::db;
use crate::romaji;
use lofty::file::{AudioFile, TaggedFileExt};
use regex::Regex;
use rusqlite::Connection;
use sha2::{Digest, Sha256};
use std::path::Path;
use std::sync::Mutex;
use std::time::UNIX_EPOCH;
use uuid::Uuid;
use walkdir::WalkDir;

/// Compute an album deduplication hash: sha256(normalize(artist) + "|" + normalize(album)),
/// truncated to 16 hex chars.
pub fn compute_album_hash(artist: &str, album: &str) -> String {
    let normalized = format!(
        "{}|{}",
        artist.trim().to_lowercase(),
        album.trim().to_lowercase()
    );
    let hash = Sha256::digest(normalized.as_bytes());
    hex::encode(&hash[..8]) // 8 bytes = 16 hex chars
}

/// Helper to encode bytes as hex (since we don't have the `hex` crate,
/// we use a manual implementation).
mod hex {
    pub fn encode(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{:02x}", b)).collect()
    }
}

pub fn scan_library(
    config: &Config,
    db_handle: &Mutex<Connection>,
) -> Result<usize, Box<dyn std::error::Error + Send + Sync>> {
    log::info!("Starting library scan: {}", config.music_library_path);
    let mut count: usize = 0;

    // Build lindera tokenizer once for Romaji processing
    let tokenizer = match romaji::build_tokenizer() {
        Ok(t) => {
            log::info!("Lindera tokenizer initialized for Romaji conversion");
            Some(t)
        }
        Err(e) => {
            log::warn!("Failed to initialize lindera tokenizer (Romaji disabled): {}", e);
            None
        }
    };

    // Compile LRC regex once
    let lrc_regex = Regex::new(r"\[\d{2}:\d{2}\.\d{2}\]").unwrap();

    let supported_extensions = ["flac", "mp3", "m4a", "aac", "ogg", "opus", "wav"];

    for entry in WalkDir::new(&config.music_library_path)
        .follow_links(true)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();

        let is_audio = path.extension().map_or(false, |ext| {
            let ext_lower = ext.to_ascii_lowercase();
            supported_extensions.iter().any(|e| ext_lower == *e)
        });

        if !is_audio || !path.is_file() {
            continue;
        }

        let abs_path_str = path.to_string_lossy().to_string();

        let base = std::path::Path::new(&config.music_library_path);
        let file_path_str = match path.strip_prefix(base) {
            Ok(rel) => rel.to_string_lossy().to_string(),
            Err(_) => {
                log::warn!("Track outside library root, skipping: {}", abs_path_str);
                continue;
            }
        };

        let metadata = match std::fs::metadata(path) {
            Ok(m) => m,
            Err(e) => {
                log::warn!("Cannot stat {}: {}", abs_path_str, e);
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
                log::warn!("Failed to read tags from {}: {}", abs_path_str, e);
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

        // --- Cover art extraction with album-level deduplication ---
        let album_hash = compute_album_hash(&artist, &album);
        let cover_filename = extract_cover_art_dedup(tag, &config.cover_cache_dir, &album_hash);

        // --- Lyrics extraction ---
        let (lyrics_type, lyrics_raw) = extract_lyrics(tag, &lrc_regex);

        // --- Romaji processing ---
        let (kana_text, romaji_text) = if let (Some(ref raw), Some(ref tokenizer)) =
            (&lyrics_raw, &tokenizer)
        {
            process_romaji(tokenizer, raw, lyrics_type.as_deref())
        } else {
            (None, None)
        };

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
                Some(&album_hash),
                lyrics_type.as_deref(),
                lyrics_raw.as_deref(),
                kana_text.as_deref(),
                romaji_text.as_deref(),
            ) {
                log::warn!("Failed to upsert {}: {}", abs_path_str, e);
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

/// Extract cover art with album-level deduplication.
/// Uses album_hash as filename — skips if the file already exists.
fn extract_cover_art_dedup(
    tag: Option<&lofty::tag::Tag>,
    cache_dir: &str,
    album_hash: &str,
) -> Option<String> {
    let filename = format!("{}.jpg", album_hash);
    let output_path = Path::new(cache_dir).join(&filename);

    // Skip if cover already extracted for this album
    if output_path.exists() {
        return Some(filename);
    }

    let tag = tag?;
    let pictures = tag.pictures();

    let cover = pictures
        .iter()
        .find(|p| p.pic_type() == lofty::picture::PictureType::CoverFront)
        .or_else(|| pictures.first());

    let picture = cover?;

    match image::load_from_memory(picture.data()) {
        Ok(img) => match img.save(&output_path) {
            Ok(_) => {
                log::debug!("Saved cover art: {}", output_path.display());
                Some(filename)
            }
            Err(e) => {
                log::warn!("Failed to save cover art {}: {}", output_path.display(), e);
                None
            }
        },
        Err(e) => {
            log::warn!(
                "Failed to decode cover art for album hash {}: {}",
                album_hash,
                e
            );
            None
        }
    }
}

/// Extract lyrics from audio tags.
/// Looks for LYRICS or UNSYNCEDLYRICS Vorbis comments.
/// Returns (lyrics_type, lyrics_raw).
fn extract_lyrics(
    tag: Option<&lofty::tag::Tag>,
    lrc_regex: &Regex,
) -> (Option<String>, Option<String>) {
    let tag = match tag {
        Some(t) => t,
        None => return (None, None),
    };

    // Try LYRICS first, then UNSYNCEDLYRICS
    let lyrics_text = tag
        .get_string(&lofty::tag::ItemKey::Lyrics)
        .or_else(|| {
            // Try UNSYNCEDLYRICS via custom key
            tag.get_string(&lofty::tag::ItemKey::Unknown("UNSYNCEDLYRICS".to_string()))
        });

    let raw = match lyrics_text {
        Some(text) if !text.trim().is_empty() => text.to_string(),
        _ => return (None, None),
    };

    // Detect synced vs unsynced
    let lyrics_type = if raw.starts_with('[') && lrc_regex.is_match(&raw) {
        "synced".to_string()
    } else {
        "unsynced".to_string()
    };

    (Some(lyrics_type), Some(raw))
}

/// Process romaji for lyrics using lindera tokenizer.
/// For synced lyrics, processes each line independently.
/// For unsynced lyrics, processes the full text.
fn process_romaji(
    tokenizer: &lindera::tokenizer::Tokenizer,
    lyrics_raw: &str,
    lyrics_type: Option<&str>,
) -> (Option<String>, Option<String>) {
    // Check if any Japanese exists
    if !romaji::contains_japanese(lyrics_raw) {
        return (None, None);
    }

    match lyrics_type {
        Some("synced") => {
            // For synced (LRC), process each line separately
            // Store kana/romaji as parallel LRC-like lines separated by newlines
            let lrc_line_regex = Regex::new(r"\[(\d{2}):(\d{2})\.(\d{2})\](.*)").unwrap();
            let mut kana_lines = Vec::new();
            let mut romaji_lines = Vec::new();

            for line in lyrics_raw.lines() {
                if let Some(caps) = lrc_line_regex.captures(line) {
                    let timestamp = format!(
                        "[{}:{}.{}]",
                        &caps[1], &caps[2], &caps[3]
                    );
                    let text = caps[4].trim();

                    if romaji::contains_japanese(text) {
                        let (kana, rom) = romaji::to_kana_and_romaji(tokenizer, text);
                        kana_lines.push(format!("{}{}", timestamp, kana));
                        romaji_lines.push(format!("{}{}", timestamp, rom));
                    } else {
                        kana_lines.push(line.to_string());
                        romaji_lines.push(line.to_string());
                    }
                } else {
                    kana_lines.push(line.to_string());
                    romaji_lines.push(line.to_string());
                }
            }

            (Some(kana_lines.join("\n")), Some(romaji_lines.join("\n")))
        }
        Some("unsynced") | _ => {
            // For unsynced, process full text
            let (kana, rom) = romaji::to_kana_and_romaji(tokenizer, lyrics_raw);
            (Some(kana), Some(rom))
        }
    }
}
