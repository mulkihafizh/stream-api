use crate::models::{Album, AlbumDetail, PlaylistDetail, Track};
use rusqlite::{params, Connection, Result};

pub fn initialize_db(path: &str) -> Result<Connection> {
    let conn = Connection::open(path)?;

    conn.execute_batch(
        "PRAGMA journal_mode=WAL;
         PRAGMA synchronous=NORMAL;
         PRAGMA foreign_keys=ON;

         CREATE TABLE IF NOT EXISTS tracks (
             id TEXT PRIMARY KEY,
             title TEXT NOT NULL,
             artist TEXT NOT NULL DEFAULT 'Unknown Artist',
             album TEXT NOT NULL DEFAULT 'Unknown Album',
             sample_rate INTEGER NOT NULL DEFAULT 0,
             bit_depth INTEGER NOT NULL DEFAULT 0,
             duration_seconds REAL NOT NULL DEFAULT 0.0,
             file_path TEXT NOT NULL UNIQUE,
             cover_art_filename TEXT,
             file_modified INTEGER NOT NULL DEFAULT 0
         );

         CREATE TABLE IF NOT EXISTS playlists (
             id INTEGER PRIMARY KEY AUTOINCREMENT,
             name TEXT NOT NULL UNIQUE
         );

         CREATE TABLE IF NOT EXISTS playlist_tracks (
             playlist_id INTEGER NOT NULL,
             track_id TEXT NOT NULL,
             position INTEGER NOT NULL,
             PRIMARY KEY (playlist_id, track_id),
             FOREIGN KEY (playlist_id) REFERENCES playlists(id) ON DELETE CASCADE,
             FOREIGN KEY (track_id) REFERENCES tracks(id) ON DELETE CASCADE
         );

         CREATE TABLE IF NOT EXISTS play_history (
             id INTEGER PRIMARY KEY AUTOINCREMENT,
             track_id TEXT NOT NULL,
             played_at INTEGER NOT NULL,
             duration_listened REAL NOT NULL,
             FOREIGN KEY (track_id) REFERENCES tracks(id) ON DELETE CASCADE
         );

         CREATE INDEX IF NOT EXISTS idx_tracks_file_path ON tracks(file_path);
         CREATE INDEX IF NOT EXISTS idx_tracks_artist ON tracks(artist);
         CREATE INDEX IF NOT EXISTS idx_tracks_album ON tracks(album);
         CREATE INDEX IF NOT EXISTS idx_play_history_played_at ON play_history(played_at);
         CREATE INDEX IF NOT EXISTS idx_play_history_track_id ON play_history(track_id);",
    )?;

    // --- Migration: add lyrics and cover hash columns ---
    run_migrations(&conn)?;

    Ok(conn)
}

fn run_migrations(conn: &Connection) -> Result<()> {
    // Check if lyrics_type column exists on tracks
    let has_lyrics_type = conn
        .prepare("SELECT lyrics_type FROM tracks LIMIT 0")
        .is_ok();

    if !has_lyrics_type {
        log::info!("Running migration: adding lyrics and romaji columns to tracks");
        conn.execute_batch(
            "ALTER TABLE tracks ADD COLUMN lyrics_type TEXT;
             ALTER TABLE tracks ADD COLUMN lyrics_raw TEXT;
             ALTER TABLE tracks ADD COLUMN kana_text TEXT;
             ALTER TABLE tracks ADD COLUMN romaji_text TEXT;",
        )?;
    }

    // Check if cover_art_hash column exists on tracks
    let has_cover_hash = conn
        .prepare("SELECT cover_art_hash FROM tracks LIMIT 0")
        .is_ok();

    if !has_cover_hash {
        log::info!("Running migration: adding cover_art_hash column to tracks");
        conn.execute_batch("ALTER TABLE tracks ADD COLUMN cover_art_hash TEXT;")?;
    }

    Ok(())
}

pub fn get_track_modified_time(conn: &Connection, file_path: &str) -> Result<Option<i64>> {
    let mut stmt = conn.prepare("SELECT file_modified FROM tracks WHERE file_path = ?1")?;
    let mut rows = stmt.query(params![file_path])?;
    if let Some(row) = rows.next()? {
        Ok(Some(row.get(0)?))
    } else {
        Ok(None)
    }
}

pub fn upsert_track(
    conn: &Connection,
    id: &str,
    title: &str,
    artist: &str,
    album: &str,
    sample_rate: u32,
    bit_depth: u16,
    duration_seconds: f64,
    file_path: &str,
    cover_art_filename: Option<&str>,
    file_modified: i64,
    cover_art_hash: Option<&str>,
    lyrics_type: Option<&str>,
    lyrics_raw: Option<&str>,
    kana_text: Option<&str>,
    romaji_text: Option<&str>,
) -> Result<()> {
    conn.execute(
        "INSERT INTO tracks (id, title, artist, album, sample_rate, bit_depth, duration_seconds,
                             file_path, cover_art_filename, file_modified, cover_art_hash,
                             lyrics_type, lyrics_raw, kana_text, romaji_text)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)
         ON CONFLICT(file_path) DO UPDATE SET
             title = excluded.title,
             artist = excluded.artist,
             album = excluded.album,
             sample_rate = excluded.sample_rate,
             bit_depth = excluded.bit_depth,
             duration_seconds = excluded.duration_seconds,
             cover_art_filename = excluded.cover_art_filename,
             file_modified = excluded.file_modified,
             cover_art_hash = excluded.cover_art_hash,
             lyrics_type = excluded.lyrics_type,
             lyrics_raw = excluded.lyrics_raw,
             kana_text = excluded.kana_text,
             romaji_text = excluded.romaji_text",
        params![
            id,
            title,
            artist,
            album,
            sample_rate,
            bit_depth,
            duration_seconds,
            file_path,
            cover_art_filename,
            file_modified,
            cover_art_hash,
            lyrics_type,
            lyrics_raw,
            kana_text,
            romaji_text
        ],
    )?;
    Ok(())
}

fn row_to_track(row: &rusqlite::Row) -> rusqlite::Result<Track> {
    let cover_hash: Option<String> = row.get("cover_art_hash")?;
    let cover_filename: Option<String> = row.get("cover_art_filename")?;
    // Prefer hash-based URL, fall back to filename-based
    let cover_art_url = cover_hash
        .as_ref()
        .map(|h| format!("/api/covers/{}.jpg", h))
        .or_else(|| cover_filename.map(|f| format!("/api/covers/{}", f)));

    let id: String = row.get("id")?;
    let file_path: String = row.get("file_path")?;
    let stream_url = format!(
        "/api/stream/{}",
        file_path.replace('\\', "/")
    );

    Ok(Track {
        id,
        title: row.get("title")?,
        artist: row.get("artist")?,
        album: row.get("album")?,
        sample_rate: row.get("sample_rate")?,
        bit_depth: row.get("bit_depth")?,
        duration_seconds: row.get("duration_seconds")?,
        file_path,
        cover_art_url,
        stream_url,
    })
}

pub fn get_all_tracks(conn: &Connection) -> Result<Vec<Track>> {
    let mut stmt = conn.prepare(
        "SELECT id, title, artist, album, sample_rate, bit_depth, duration_seconds,
                file_path, cover_art_filename, cover_art_hash
         FROM tracks ORDER BY artist, album, title",
    )?;

    let tracks = stmt
        .query_map([], |row| row_to_track(row))?
        .collect::<Result<Vec<_>>>()?;

    Ok(tracks)
}

pub fn get_track_by_id(conn: &Connection, track_id: &str) -> Result<Option<Track>> {
    let mut stmt = conn.prepare(
        "SELECT id, title, artist, album, sample_rate, bit_depth, duration_seconds,
                file_path, cover_art_filename, cover_art_hash
         FROM tracks WHERE id = ?1",
    )?;

    let mut rows = stmt.query(params![track_id])?;
    if let Some(row) = rows.next()? {
        Ok(Some(row_to_track(row)?))
    } else {
        Ok(None)
    }
}

pub fn get_track_lyrics(
    conn: &Connection,
    track_id: &str,
) -> Result<Option<(String, Option<String>, Option<String>, Option<String>, Option<String>)>> {
    // Returns: (id, lyrics_type, lyrics_raw, kana_text, romaji_text)
    let mut stmt = conn.prepare(
        "SELECT id, lyrics_type, lyrics_raw, kana_text, romaji_text
         FROM tracks WHERE id = ?1",
    )?;

    let mut rows = stmt.query(params![track_id])?;
    if let Some(row) = rows.next()? {
        let id: String = row.get(0)?;
        let lyrics_type: Option<String> = row.get(1)?;
        let lyrics_raw: Option<String> = row.get(2)?;
        let kana_text: Option<String> = row.get(3)?;
        let romaji_text: Option<String> = row.get(4)?;
        Ok(Some((id, lyrics_type, lyrics_raw, kana_text, romaji_text)))
    } else {
        Ok(None)
    }
}

pub fn get_all_albums(conn: &Connection) -> Result<Vec<Album>> {
    let mut stmt = conn.prepare(
        "SELECT DISTINCT album, artist, cover_art_hash, cover_art_filename,
                COUNT(*) as track_count
         FROM tracks
         GROUP BY album, artist
         ORDER BY artist, album",
    )?;

    let albums = stmt
        .query_map([], |row| {
            let album: String = row.get(0)?;
            let artist: String = row.get(1)?;
            let cover_hash: Option<String> = row.get(2)?;
            let cover_filename: Option<String> = row.get(3)?;
            let track_count: usize = row.get(4)?;

            let cover_url = cover_hash
                .as_ref()
                .map(|h| format!("/api/covers/{}.jpg", h))
                .or_else(|| cover_filename.map(|f| format!("/api/covers/{}", f)));

            // Generate a stable "id" from artist|album for URL purposes
            let id = crate::scanner::compute_album_hash(&artist, &album);

            Ok(Album {
                id,
                name: album,
                artist,
                cover_url,
                track_count,
            })
        })?
        .collect::<Result<Vec<_>>>()?;

    Ok(albums)
}

pub fn get_album_detail(conn: &Connection, album_hash: &str) -> Result<Option<AlbumDetail>> {
    // Find tracks matching this album hash
    let mut stmt = conn.prepare(
        "SELECT id, title, artist, album, sample_rate, bit_depth, duration_seconds,
                file_path, cover_art_filename, cover_art_hash
         FROM tracks WHERE cover_art_hash = ?1
         ORDER BY title",
    )?;

    let tracks: Vec<Track> = stmt
        .query_map(params![album_hash], |row| row_to_track(row))?
        .collect::<Result<Vec<_>>>()?;

    if tracks.is_empty() {
        return Ok(None);
    }

    let first = &tracks[0];
    let cover_url = first.cover_art_url.clone();

    Ok(Some(AlbumDetail {
        id: album_hash.to_string(),
        name: first.album.clone(),
        artist: first.artist.clone(),
        cover_url,
        tracks,
    }))
}

pub fn get_playlists_with_tracks(conn: &Connection) -> Result<Vec<PlaylistDetail>> {
    let mut playlist_stmt = conn.prepare("SELECT id, name FROM playlists ORDER BY name")?;
    let playlists: Vec<(i64, String)> = playlist_stmt
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<Result<Vec<_>>>()?;

    let mut track_stmt = conn.prepare(
        "SELECT t.id, t.title, t.artist, t.album, t.sample_rate, t.bit_depth,
                t.duration_seconds, t.file_path, t.cover_art_filename, t.cover_art_hash
         FROM playlist_tracks pt
         JOIN tracks t ON t.id = pt.track_id
         WHERE pt.playlist_id = ?1
         ORDER BY pt.position",
    )?;

    let mut result = Vec::new();
    for (pid, pname) in playlists {
        let tracks = track_stmt
            .query_map(params![pid], |row| row_to_track(row))?
            .collect::<Result<Vec<_>>>()?;

        result.push(PlaylistDetail {
            id: pid,
            name: pname,
            tracks,
        });
    }

    Ok(result)
}

pub fn create_playlist(conn: &Connection, name: &str) -> Result<i64> {
    conn.execute("INSERT INTO playlists (name) VALUES (?1)", params![name])?;
    Ok(conn.last_insert_rowid())
}

pub fn add_track_to_playlist(conn: &Connection, playlist_id: i64, track_id: &str) -> Result<()> {
    let next_position: i64 = conn
        .query_row(
            "SELECT COALESCE(MAX(position), 0) + 1 FROM playlist_tracks WHERE playlist_id = ?1",
            params![playlist_id],
            |row| row.get(0),
        )
        .unwrap_or(1);

    conn.execute(
        "INSERT OR IGNORE INTO playlist_tracks (playlist_id, track_id, position) VALUES (?1, ?2, ?3)",
        params![playlist_id, track_id, next_position],
    )?;
    Ok(())
}

pub fn record_play(conn: &Connection, track_id: &str, duration_listened: f64) -> Result<()> {
    use std::time::{SystemTime, UNIX_EPOCH};
    let played_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;

    conn.execute(
        "INSERT INTO play_history (track_id, played_at, duration_listened) VALUES (?1, ?2, ?3)",
        params![track_id, played_at, duration_listened],
    )?;
    Ok(())
}

pub fn get_annual_stats(
    conn: &Connection,
    year: i32,
) -> Result<crate::models::AnnualStatsResponse> {
    use crate::models::{AnnualStatsResponse, TopAlbum, TopArtist, TopTrack};

    let start_ts = chrono::NaiveDate::from_ymd_opt(year, 1, 1)
        .unwrap()
        .and_hms_opt(0, 0, 0)
        .unwrap()
        .and_utc()
        .timestamp();

    let end_ts = chrono::NaiveDate::from_ymd_opt(year + 1, 1, 1)
        .unwrap()
        .and_hms_opt(0, 0, 0)
        .unwrap()
        .and_utc()
        .timestamp();

    let total_duration: f64 = conn
        .query_row(
            "SELECT COALESCE(SUM(duration_listened), 0.0) FROM play_history WHERE played_at >= ?1 AND played_at < ?2",
            params![start_ts, end_ts],
            |row| row.get(0),
        )?;

    let mut top_artists = Vec::new();
    let mut stmt = conn.prepare(
        "SELECT t.artist, COUNT(p.id) as play_count, SUM(p.duration_listened) as total_duration
         FROM play_history p
         JOIN tracks t ON t.id = p.track_id
         WHERE p.played_at >= ?1 AND p.played_at < ?2
         GROUP BY t.artist
         ORDER BY total_duration DESC, play_count DESC
         LIMIT 5",
    )?;
    let artists_iter = stmt.query_map(params![start_ts, end_ts], |row| {
        Ok(TopArtist {
            artist: row.get(0)?,
            play_count: row.get(1)?,
            total_duration: row.get(2)?,
        })
    })?;
    for artist in artists_iter {
        top_artists.push(artist?);
    }

    let mut top_albums = Vec::new();
    let mut stmt = conn.prepare(
        "SELECT t.album, t.artist, COUNT(p.id) as play_count, SUM(p.duration_listened) as total_duration
         FROM play_history p
         JOIN tracks t ON t.id = p.track_id
         WHERE p.played_at >= ?1 AND p.played_at < ?2
         GROUP BY t.album, t.artist
         ORDER BY total_duration DESC, play_count DESC
         LIMIT 5",
    )?;
    let albums_iter = stmt.query_map(params![start_ts, end_ts], |row| {
        Ok(TopAlbum {
            album: row.get(0)?,
            artist: row.get(1)?,
            play_count: row.get(2)?,
            total_duration: row.get(3)?,
        })
    })?;
    for album in albums_iter {
        top_albums.push(album?);
    }

    let mut top_tracks = Vec::new();
    let mut stmt = conn.prepare(
        "SELECT t.id, t.title, t.artist, t.album, t.sample_rate, t.bit_depth,
                t.duration_seconds, t.file_path, t.cover_art_filename, t.cover_art_hash,
                COUNT(p.id) as play_count, SUM(p.duration_listened) as total_duration
         FROM play_history p
         JOIN tracks t ON t.id = p.track_id
         WHERE p.played_at >= ?1 AND p.played_at < ?2
         GROUP BY t.id
         ORDER BY total_duration DESC, play_count DESC
         LIMIT 10",
    )?;
    let tracks_iter = stmt.query_map(params![start_ts, end_ts], |row| {
        let track = row_to_track(row)?;
        Ok(TopTrack {
            track,
            play_count: row.get(10)?,
            total_duration: row.get(11)?,
        })
    })?;
    for track in tracks_iter {
        top_tracks.push(track?);
    }

    Ok(AnnualStatsResponse {
        year,
        total_duration_seconds: total_duration,
        top_tracks,
        top_artists,
        top_albums,
    })
}
