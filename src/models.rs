use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize)]
pub struct Track {
    pub id: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub sample_rate: u32,
    pub bit_depth: u16,
    pub duration_seconds: f64,
    pub file_path: String,
    pub cover_art_url: Option<String>,
    pub stream_url: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Album {
    pub id: String,
    pub name: String,
    pub artist: String,
    pub cover_url: Option<String>,
    pub track_count: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct AlbumDetail {
    pub id: String,
    pub name: String,
    pub artist: String,
    pub cover_url: Option<String>,
    pub tracks: Vec<Track>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Playlist {
    pub id: i64,
    pub name: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct PlaylistDetail {
    pub id: i64,
    pub name: String,
    pub tracks: Vec<Track>,
}

#[derive(Debug, Serialize)]
pub struct LibraryResponse {
    pub total: usize,
    pub tracks: Vec<Track>,
}

#[derive(Debug, Serialize)]
pub struct PlaylistsResponse {
    pub total: usize,
    pub playlists: Vec<PlaylistDetail>,
}

#[derive(Debug, Deserialize)]
pub struct CreatePlaylistRequest {
    pub name: String,
}

#[derive(Debug, Deserialize)]
pub struct AddTrackRequest {
    pub track_id: String,
}

#[derive(Debug, Deserialize)]
pub struct RecordPlayRequest {
    pub track_id: String,
    pub duration_listened: f64,
}

#[derive(Debug, Serialize)]
pub struct TopTrack {
    pub track: Track,
    pub play_count: usize,
    pub total_duration: f64,
}

#[derive(Debug, Serialize)]
pub struct TopArtist {
    pub artist: String,
    pub play_count: usize,
    pub total_duration: f64,
}

#[derive(Debug, Serialize)]
pub struct TopAlbum {
    pub album: String,
    pub artist: String,
    pub play_count: usize,
    pub total_duration: f64,
}

#[derive(Debug, Serialize)]
pub struct AnnualStatsResponse {
    pub year: i32,
    pub total_duration_seconds: f64,
    pub top_tracks: Vec<TopTrack>,
    pub top_artists: Vec<TopArtist>,
    pub top_albums: Vec<TopAlbum>,
}

// --- Lyrics API models ---

#[derive(Debug, Serialize)]
pub struct LyricLine {
    pub time_ms: u64,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kana: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub romaji: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(tag = "type")]
pub enum LyricsPayload {
    #[serde(rename = "synced")]
    Synced { lyrics: Vec<LyricLine> },
    #[serde(rename = "unsynced")]
    Unsynced { lyrics: UnsyncedLyrics },
    #[serde(rename = "none")]
    None { lyrics: Option<()> },
}

#[derive(Debug, Serialize)]
pub struct UnsyncedLyrics {
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kana: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub romaji: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct LyricsResponse {
    pub song_id: String,
    #[serde(flatten)]
    pub payload: LyricsPayload,
}
