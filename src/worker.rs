use crate::config::Config;
use crate::scanner;
use rusqlite::Connection;
use std::sync::Mutex;
use tokio::sync::mpsc::Receiver;

/// Jobs that the ingestion worker can process.
pub enum IngestJob {
    /// Scan the entire music library.
    ScanLibrary,
}

/// Background ingestion worker. Processes jobs sequentially from the channel.
/// Errors per-job are logged but never cause the worker to exit.
pub async fn ingestion_worker(
    mut rx: Receiver<IngestJob>,
    db_handle: actix_web::web::Data<Mutex<Connection>>,
    config: actix_web::web::Data<Config>,
) {
    log::info!("Ingestion worker started, waiting for jobs...");

    while let Some(job) = rx.recv().await {
        match job {
            IngestJob::ScanLibrary => {
                log::info!("Ingestion worker: starting library scan");
                let scan_db = db_handle.clone();
                let scan_config = config.clone();

                let result = tokio::task::spawn_blocking(move || {
                    scanner::scan_library(&scan_config, &scan_db)
                })
                .await;

                match result {
                    Ok(Ok(count)) => {
                        log::info!("Ingestion worker: library scan complete — {} tracks indexed", count);
                    }
                    Ok(Err(e)) => {
                        log::error!("Ingestion worker: library scan failed — {}", e);
                    }
                    Err(e) => {
                        log::error!("Ingestion worker: scan task panicked — {}", e);
                    }
                }
            }
        }
    }

    log::info!("Ingestion worker shutting down (channel closed)");
}
