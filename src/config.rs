use std::env;

#[derive(Clone)]
pub struct Config {
    pub music_library_path: String,
    pub cover_cache_dir: String,
    pub database_path: String,
    pub bearer_token: String,
    pub bind_address: String,
}

impl Config {
    pub fn from_env() -> Self {
        Config {
            music_library_path: env::var("MUSIC_LIBRARY_PATH")
                .expect("MUSIC_LIBRARY_PATH must be set"),
            cover_cache_dir: env::var("COVER_CACHE_DIR")
                .unwrap_or_else(|_| "./covers".to_string()),
            database_path: env::var("DATABASE_PATH")
                .unwrap_or_else(|_| "./library.db".to_string()),
            bearer_token: env::var("BEARER_TOKEN")
                .expect("BEARER_TOKEN must be set"),
            bind_address: env::var("BIND_ADDRESS")
                .unwrap_or_else(|_| "127.0.0.1:4040".to_string()),
        }
    }
}
