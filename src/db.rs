use crate::models::{PlaylistDetail, Track};
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

    Ok(conn)
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
) -> Result<()> {
    conn.execute(
        "INSERT INTO tracks (id, title, artist, album, sample_rate, bit_depth, duration_seconds, file_path, cover_art_filename, file_modified)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
         ON CONFLICT(file_path) DO UPDATE SET
             title = excluded.title,
             artist = excluded.artist,
             album = excluded.album,
             sample_rate = excluded.sample_rate,
             bit_depth = excluded.bit_depth,
             duration_seconds = excluded.duration_seconds,
             cover_art_filename = excluded.cover_art_filename,
             file_modified = excluded.file_modified",
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
            file_modified
        ],
    )?;
    Ok(())
}

pub fn get_all_tracks(conn: &Connection) -> Result<Vec<Track>> {
    let mut stmt = conn.prepare(
        "SELECT id, title, artist, album, sample_rate, bit_depth, duration_seconds, file_path, cover_art_filename
         FROM tracks ORDER BY artist, album, title",
    )?;

    let tracks = stmt
        .query_map([], |row| {
            let cover_filename: Option<String> = row.get(8)?;
            let cover_art_url = cover_filename.map(|f| format!("/api/covers/{}", f));
            Ok(Track {
                id: row.get(0)?,
                title: row.get(1)?,
                artist: row.get(2)?,
                album: row.get(3)?,
                sample_rate: row.get(4)?,
                bit_depth: row.get(5)?,
                duration_seconds: row.get(6)?,
                file_path: row.get(7)?,
                cover_art_url,
            })
        })?
        .collect::<Result<Vec<_>>>()?;

    Ok(tracks)
}

pub fn get_playlists_with_tracks(conn: &Connection) -> Result<Vec<PlaylistDetail>> {
    let mut playlist_stmt = conn.prepare("SELECT id, name FROM playlists ORDER BY name")?;
    let playlists: Vec<(i64, String)> = playlist_stmt
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<Result<Vec<_>>>()?;

    let mut track_stmt = conn.prepare(
        "SELECT t.id, t.title, t.artist, t.album, t.sample_rate, t.bit_depth,
                t.duration_seconds, t.file_path, t.cover_art_filename
         FROM playlist_tracks pt
         JOIN tracks t ON t.id = pt.track_id
         WHERE pt.playlist_id = ?1
         ORDER BY pt.position",
    )?;

    let mut result = Vec::new();
    for (pid, pname) in playlists {
        let tracks = track_stmt
            .query_map(params![pid], |row| {
                let cover_filename: Option<String> = row.get(8)?;
                let cover_art_url = cover_filename.map(|f| format!("/api/covers/{}", f));
                Ok(Track {
                    id: row.get(0)?,
                    title: row.get(1)?,
                    artist: row.get(2)?,
                    album: row.get(3)?,
                    sample_rate: row.get(4)?,
                    bit_depth: row.get(5)?,
                    duration_seconds: row.get(6)?,
                    file_path: row.get(7)?,
                    cover_art_url,
                })
            })?
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
                t.duration_seconds, t.file_path, t.cover_art_filename,
                COUNT(p.id) as play_count, SUM(p.duration_listened) as total_duration
         FROM play_history p
         JOIN tracks t ON t.id = p.track_id
         WHERE p.played_at >= ?1 AND p.played_at < ?2
         GROUP BY t.id
         ORDER BY total_duration DESC, play_count DESC
         LIMIT 10",
    )?;
    let tracks_iter = stmt.query_map(params![start_ts, end_ts], |row| {
        let cover_filename: Option<String> = row.get(8)?;
        let cover_art_url = cover_filename.map(|f| format!("/api/covers/{}", f));
        let track = Track {
            id: row.get(0)?,
            title: row.get(1)?,
            artist: row.get(2)?,
            album: row.get(3)?,
            sample_rate: row.get(4)?,
            bit_depth: row.get(5)?,
            duration_seconds: row.get(6)?,
            file_path: row.get(7)?,
            cover_art_url,
        };
        Ok(TopTrack {
            track,
            play_count: row.get(9)?,
            total_duration: row.get(10)?,
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
