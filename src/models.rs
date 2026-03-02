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
