use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    thread,
    time::{SystemTime, UNIX_EPOCH},
};

use rusqlite::{Connection, OptionalExtension as _, params};
use tokio::sync::{Mutex, broadcast, mpsc, oneshot, watch};

use crate::{
    error::AppError,
    realtime::RealtimeUpdate,
    types::{
        ListRecordingsOutput, LiveOrderBook, LiveSnapshotOutput, PriceLevel, RecordingInfo,
        ReplayMarketOutput,
    },
};

#[derive(Clone, Debug)]
pub struct RecorderService {
    path: PathBuf,
    active: Arc<Mutex<HashMap<String, ActiveRecording>>>,
    next_id: Arc<AtomicU64>,
}

#[derive(Debug)]
struct ActiveRecording {
    cancel: watch::Sender<bool>,
    writer: mpsc::Sender<WriterCommand>,
}

#[derive(Debug)]
enum WriterCommand {
    Insert(Box<LiveOrderBook>),
    Gap(u64),
    Stop(u64, oneshot::Sender<Result<(), String>>),
}

impl RecorderService {
    pub fn new(path: PathBuf) -> Result<Self, AppError> {
        initialize(&path)?;
        let connection = Connection::open(&path).map_err(database_error)?;
        connection
            .execute(
                "UPDATE recordings SET active = 0, stopped_at_ms = ?1, writer_error_count = writer_error_count + 1, last_writer_error = 'recording interrupted by a previous server shutdown' WHERE active = 1",
                [i64::try_from(now_ms()).unwrap_or(i64::MAX)],
            )
            .map_err(database_error)?;
        Ok(Self {
            path,
            active: Arc::new(Mutex::new(HashMap::new())),
            next_id: Arc::new(AtomicU64::new(1)),
        })
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub async fn start(
        &self,
        watch_id: String,
        label: Option<String>,
        initial: LiveSnapshotOutput,
        mut updates: broadcast::Receiver<RealtimeUpdate>,
    ) -> Result<RecordingInfo, AppError> {
        let recording_id = format!(
            "recording-{}-{}",
            now_ms(),
            self.next_id.fetch_add(1, Ordering::Relaxed)
        );
        let started_at_ms = now_ms();
        let token_ids = initial.watch.token_ids.clone();

        let (writer, writer_rx) = mpsc::channel(1_024);
        let writer_path = self.path.clone();
        let writer_recording_id = recording_id.clone();
        let (ready, ready_rx) = oneshot::channel();
        thread::spawn(move || writer_loop(&writer_path, &writer_recording_id, writer_rx, ready));
        ready_rx
            .await
            .map_err(database_error)?
            .map_err(database_error)?;

        let metadata_path = self.path.clone();
        let metadata_recording_id = recording_id.clone();
        let metadata_watch_id = watch_id.clone();
        let metadata_label = label.clone();
        let metadata_tokens = token_ids.clone();
        tokio::task::spawn_blocking(move || {
            initialize(&metadata_path)?;
            let connection = Connection::open(&metadata_path).map_err(database_error)?;
            connection
                .execute(
                    "INSERT INTO recordings (id, watch_id, label, token_ids_json, started_at_ms, active) VALUES (?1, ?2, ?3, ?4, ?5, 1)",
                    params![metadata_recording_id, metadata_watch_id, metadata_label, serde_json::to_string(&metadata_tokens).map_err(database_error)?, i64::try_from(started_at_ms).unwrap_or(i64::MAX)],
                )
                .map_err(database_error)?;
            Ok::<_, AppError>(())
        })
        .await
        .map_err(database_error)??;
        for book in initial.books {
            writer
                .send(WriterCommand::Insert(Box::new(book)))
                .await
                .map_err(database_error)?;
        }

        let (cancel, mut cancel_rx) = watch::channel(false);
        let accepted_tokens = token_ids.iter().cloned().collect::<HashSet<_>>();
        let task_writer = writer.clone();
        let task_watch_id = watch_id.clone();
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    changed = cancel_rx.changed() => {
                        if changed.is_err() || *cancel_rx.borrow() { break; }
                    }
                    update = updates.recv() => {
                        match update {
                            Ok(update) if update.watch_id == task_watch_id && accepted_tokens.contains(&update.book.token_id) => {
                                if task_writer.send(WriterCommand::Insert(Box::new(update.book))).await.is_err() { break; }
                            }
                            Ok(_) => {}
                            Err(broadcast::error::RecvError::Lagged(count)) => {
                                let _ = task_writer.send(WriterCommand::Gap(count)).await;
                            }
                            Err(broadcast::error::RecvError::Closed) => break,
                        }
                    }
                }
            }
        });
        self.active
            .lock()
            .await
            .insert(recording_id.clone(), ActiveRecording { cancel, writer });

        Ok(RecordingInfo {
            recording_id,
            watch_id,
            label,
            token_ids,
            started_at_ms,
            stopped_at_ms: None,
            snapshot_count: 0,
            dropped_update_count: 0,
            writer_error_count: 0,
            last_writer_error: None,
            active: true,
        })
    }

    pub async fn stop(&self, recording_id: &str) -> Result<RecordingInfo, AppError> {
        if let Some(active) = self.active.lock().await.remove(recording_id) {
            let stopped_at = now_ms();
            let _ = active.cancel.send(true);
            let (ack, ack_rx) = oneshot::channel();
            active
                .writer
                .send(WriterCommand::Stop(stopped_at, ack))
                .await
                .map_err(database_error)?;
            ack_rx
                .await
                .map_err(database_error)?
                .map_err(database_error)?;
        }
        self.recording(recording_id)?
            .ok_or_else(|| AppError::InvalidInput(format!("unknown recording_id {recording_id}")))
    }

    pub fn list(&self, limit: u16) -> Result<ListRecordingsOutput, AppError> {
        initialize(&self.path)?;
        let connection = Connection::open(&self.path).map_err(database_error)?;
        let mut statement = connection
            .prepare("SELECT id, watch_id, label, token_ids_json, started_at_ms, stopped_at_ms, snapshot_count, dropped_update_count, writer_error_count, last_writer_error, active FROM recordings ORDER BY started_at_ms DESC LIMIT ?1")
            .map_err(database_error)?;
        let rows = statement
            .query_map([i64::from(limit)], recording_from_row)
            .map_err(database_error)?;
        let recordings = rows
            .collect::<Result<Vec<_>, _>>()
            .map_err(database_error)?;
        Ok(ListRecordingsOutput {
            count: recordings.len(),
            recordings,
        })
    }

    pub fn replay(
        &self,
        recording_id: &str,
        token_id: Option<&str>,
        start_ms: Option<i64>,
        end_ms: Option<i64>,
        limit: u32,
    ) -> Result<ReplayMarketOutput, AppError> {
        initialize(&self.path)?;
        if self.recording(recording_id)?.is_none() {
            return Err(AppError::InvalidInput(format!(
                "unknown recording_id {recording_id}"
            )));
        }
        let connection = Connection::open(&self.path).map_err(database_error)?;
        let mut statement = connection
            .prepare(
                "SELECT source, token_id, condition_id, upstream_timestamp_ms, received_at_ms, hash, best_bid, best_ask, spread, midpoint, bids_json, asks_json FROM book_snapshots WHERE recording_id = ?1 AND (?2 IS NULL OR token_id = ?2) AND (?3 IS NULL OR upstream_timestamp_ms >= ?3) AND (?4 IS NULL OR upstream_timestamp_ms <= ?4) ORDER BY upstream_timestamp_ms, id LIMIT ?5",
            )
            .map_err(database_error)?;
        let rows = statement
            .query_map(
                params![recording_id, token_id, start_ms, end_ms, i64::from(limit)],
                |row| {
                    let bids_json: String = row.get(10)?;
                    let asks_json: String = row.get(11)?;
                    Ok(LiveOrderBook {
                        source: row.get(0)?,
                        token_id: row.get(1)?,
                        condition_id: row.get(2)?,
                        upstream_timestamp_ms: row.get(3)?,
                        received_at_ms: row.get::<_, i64>(4)?.max(0) as u64,
                        feed_age_ms: None,
                        hash: row.get(5)?,
                        best_bid: row.get(6)?,
                        best_ask: row.get(7)?,
                        spread: row.get(8)?,
                        midpoint: row.get(9)?,
                        bids: decode_levels(&bids_json, 10)?,
                        asks: decode_levels(&asks_json, 11)?,
                    })
                },
            )
            .map_err(database_error)?;
        let books = rows
            .collect::<Result<Vec<_>, _>>()
            .map_err(database_error)?;
        Ok(ReplayMarketOutput {
            recording_id: recording_id.to_owned(),
            count: books.len(),
            books,
        })
    }

    fn recording(&self, recording_id: &str) -> Result<Option<RecordingInfo>, AppError> {
        initialize(&self.path)?;
        let connection = Connection::open(&self.path).map_err(database_error)?;
        connection
            .query_row(
                "SELECT id, watch_id, label, token_ids_json, started_at_ms, stopped_at_ms, snapshot_count, dropped_update_count, writer_error_count, last_writer_error, active FROM recordings WHERE id = ?1",
                [recording_id],
                recording_from_row,
            )
            .optional()
            .map_err(database_error)
    }
}

fn writer_loop(
    path: &Path,
    recording_id: &str,
    mut receiver: mpsc::Receiver<WriterCommand>,
    ready: oneshot::Sender<Result<(), String>>,
) {
    let mut connection = match Connection::open(path) {
        Ok(connection) => connection,
        Err(error) => {
            let message = error.to_string();
            let _ = ready.send(Err(message.clone()));
            tracing::error!(recording_id, %message, "failed to open recording database");
            return;
        }
    };
    let _ = ready.send(Ok(()));
    while let Some(command) = receiver.blocking_recv() {
        let (result, should_stop) = match command {
            WriterCommand::Insert(book) => (insert_book(&mut connection, recording_id, &book), false),
            WriterCommand::Gap(count) => (
                connection
                    .execute(
                        "UPDATE recordings SET dropped_update_count = dropped_update_count + ?2 WHERE id = ?1",
                        params![recording_id, i64::try_from(count).unwrap_or(i64::MAX)],
                    )
                    .map(|_| ())
                    .map_err(|error| error.to_string()),
                false,
            ),
            WriterCommand::Stop(timestamp, ack) => {
                let result = connection.execute(
                    "UPDATE recordings SET stopped_at_ms = ?2, active = 0 WHERE id = ?1",
                    params![recording_id, i64::try_from(timestamp).unwrap_or(i64::MAX)],
                ).map(|_| ()).map_err(|error| error.to_string());
                if let Err(error) = &result {
                    tracing::error!(recording_id, %error, "failed to stop recording");
                }
                let _ = ack.send(result.clone());
                (result, true)
            }
        };
        if let Err(error) = result {
            tracing::error!(recording_id, %error, "failed to write recording data");
            let _ = connection.execute(
                "UPDATE recordings SET writer_error_count = writer_error_count + 1, last_writer_error = ?2 WHERE id = ?1",
                params![recording_id, error],
            );
        }
        if should_stop {
            break;
        }
    }
}

fn insert_book(
    connection: &mut Connection,
    recording_id: &str,
    book: &LiveOrderBook,
) -> Result<(), String> {
    let bids = serde_json::to_string(&book.bids).map_err(|error| error.to_string())?;
    let asks = serde_json::to_string(&book.asks).map_err(|error| error.to_string())?;
    let transaction = connection
        .transaction()
        .map_err(|error| error.to_string())?;
    transaction.execute(
        "INSERT INTO book_snapshots (recording_id, source, token_id, condition_id, upstream_timestamp_ms, received_at_ms, hash, best_bid, best_ask, spread, midpoint, bids_json, asks_json) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
        params![recording_id, book.source, book.token_id, book.condition_id, book.upstream_timestamp_ms, i64::try_from(book.received_at_ms).unwrap_or(i64::MAX), book.hash, book.best_bid, book.best_ask, book.spread, book.midpoint, bids, asks],
    ).map_err(|error| error.to_string())?;
    transaction
        .execute(
            "UPDATE recordings SET snapshot_count = snapshot_count + 1 WHERE id = ?1",
            [recording_id],
        )
        .map_err(|error| error.to_string())?;
    transaction.commit().map_err(|error| error.to_string())
}

fn initialize(path: &Path) -> Result<(), AppError> {
    let connection = Connection::open(path).map_err(database_error)?;
    connection
        .execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA foreign_keys = ON;
             CREATE TABLE IF NOT EXISTS recordings (
                id TEXT PRIMARY KEY,
                watch_id TEXT NOT NULL,
                label TEXT,
                token_ids_json TEXT NOT NULL,
                started_at_ms INTEGER NOT NULL,
                stopped_at_ms INTEGER,
                snapshot_count INTEGER NOT NULL DEFAULT 0,
                dropped_update_count INTEGER NOT NULL DEFAULT 0,
                writer_error_count INTEGER NOT NULL DEFAULT 0,
                last_writer_error TEXT,
                active INTEGER NOT NULL DEFAULT 1
             );
             CREATE TABLE IF NOT EXISTS book_snapshots (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                recording_id TEXT NOT NULL REFERENCES recordings(id),
                source TEXT NOT NULL,
                token_id TEXT NOT NULL,
                condition_id TEXT NOT NULL,
                upstream_timestamp_ms INTEGER NOT NULL,
                received_at_ms INTEGER NOT NULL,
                hash TEXT,
                best_bid TEXT,
                best_ask TEXT,
                spread TEXT,
                midpoint TEXT,
                bids_json TEXT NOT NULL,
                asks_json TEXT NOT NULL
             );
             CREATE INDEX IF NOT EXISTS book_snapshots_replay ON book_snapshots(recording_id, token_id, upstream_timestamp_ms, id);",
        )
        .map_err(database_error)?;
    ensure_column(
        &connection,
        "writer_error_count",
        "ALTER TABLE recordings ADD COLUMN writer_error_count INTEGER NOT NULL DEFAULT 0",
    )?;
    ensure_column(
        &connection,
        "last_writer_error",
        "ALTER TABLE recordings ADD COLUMN last_writer_error TEXT",
    )?;
    Ok(())
}

fn ensure_column(connection: &Connection, name: &str, migration: &str) -> Result<(), AppError> {
    let exists = connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM pragma_table_info('recordings') WHERE name = ?1)",
            [name],
            |row| row.get::<_, bool>(0),
        )
        .map_err(database_error)?;
    if !exists {
        connection
            .execute_batch(migration)
            .map_err(database_error)?;
    }
    Ok(())
}

fn recording_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<RecordingInfo> {
    let tokens_json: String = row.get(3)?;
    Ok(RecordingInfo {
        recording_id: row.get(0)?,
        watch_id: row.get(1)?,
        label: row.get(2)?,
        token_ids: decode_strings(&tokens_json, 3)?,
        started_at_ms: row.get::<_, i64>(4)?.max(0) as u64,
        stopped_at_ms: row
            .get::<_, Option<i64>>(5)?
            .map(|value| value.max(0) as u64),
        snapshot_count: row.get::<_, i64>(6)?.max(0) as u64,
        dropped_update_count: row.get::<_, i64>(7)?.max(0) as u64,
        writer_error_count: row.get::<_, i64>(8)?.max(0) as u64,
        last_writer_error: row.get(9)?,
        active: row.get(10)?,
    })
}

fn decode_levels(json: &str, column: usize) -> rusqlite::Result<Vec<PriceLevel>> {
    serde_json::from_str(json).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(
            column,
            rusqlite::types::Type::Text,
            Box::new(error),
        )
    })
}

fn decode_strings(json: &str, column: usize) -> rusqlite::Result<Vec<String>> {
    serde_json::from_str(json).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(
            column,
            rusqlite::types::Type::Text,
            Box::new(error),
        )
    })
}

fn database_error(error: impl ToString) -> AppError {
    AppError::Upstream {
        service: "SQLite recorder",
        message: error.to_string(),
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::WatchInfo;

    #[tokio::test]
    async fn records_stops_lists_and_replays_seeded_books() {
        let directory = tempfile::tempdir().unwrap();
        let service = RecorderService::new(directory.path().join("test.sqlite3")).unwrap();
        let (_updates, receiver) = broadcast::channel(8);
        let initial = LiveSnapshotOutput {
            watch: WatchInfo {
                watch_id: "watch-1".to_owned(),
                token_ids: vec!["123".to_owned()],
                started_at_ms: 1,
                last_update_at_ms: Some(2),
                connection_state: "Connected".to_owned(),
                update_count: 1,
                rest_seed_count: 1,
                websocket_update_count: 0,
                price_change_count: 0,
                reconnect_count: 0,
                error_count: 0,
                last_error: None,
                snapshot_count: 1,
            },
            books: vec![LiveOrderBook {
                source: "rest_seed".to_owned(),
                token_id: "123".to_owned(),
                condition_id: format!("0x{}", "00".repeat(32)),
                upstream_timestamp_ms: 1,
                received_at_ms: 2,
                feed_age_ms: Some(1),
                hash: Some("hash".to_owned()),
                best_bid: Some("0.4".to_owned()),
                best_ask: Some("0.6".to_owned()),
                spread: Some("0.2".to_owned()),
                midpoint: Some("0.5".to_owned()),
                bids: vec![PriceLevel {
                    price: "0.4".to_owned(),
                    size: "10".to_owned(),
                }],
                asks: vec![PriceLevel {
                    price: "0.6".to_owned(),
                    size: "10".to_owned(),
                }],
            }],
        };
        let recording = service
            .start(
                "watch-1".to_owned(),
                Some("test".to_owned()),
                initial,
                receiver,
            )
            .await
            .unwrap();
        let stopped = service.stop(&recording.recording_id).await.unwrap();
        assert!(!stopped.active);
        assert_eq!(stopped.snapshot_count, 1);

        let listed = service.list(10).unwrap();
        assert_eq!(listed.count, 1);
        let replay = service
            .replay(&recording.recording_id, Some("123"), None, None, 10)
            .unwrap();
        assert_eq!(replay.count, 1);
        assert_eq!(replay.books[0].bids[0].price, "0.4");
    }
}
