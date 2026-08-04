use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
    thread,
    time::{SystemTime, UNIX_EPOCH},
};

use rusqlite::{Connection, OptionalExtension as _, params};
use tokio::sync::{Mutex, broadcast, watch};

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
    Stop(u64, mpsc::Sender<()>),
}

impl RecorderService {
    #[must_use]
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            active: Arc::new(Mutex::new(HashMap::new())),
            next_id: Arc::new(AtomicU64::new(1)),
        }
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
        initialize(&self.path)?;
        let connection = Connection::open(&self.path).map_err(database_error)?;
        connection
            .execute(
                "INSERT INTO recordings (id, watch_id, label, token_ids_json, started_at_ms, active) VALUES (?1, ?2, ?3, ?4, ?5, 1)",
                params![
                    recording_id,
                    watch_id,
                    label,
                    serde_json::to_string(&token_ids).map_err(database_error)?,
                    i64::try_from(started_at_ms).unwrap_or(i64::MAX),
                ],
            )
            .map_err(database_error)?;
        drop(connection);

        let (writer, writer_rx) = mpsc::channel();
        let writer_path = self.path.clone();
        let writer_recording_id = recording_id.clone();
        thread::spawn(move || writer_loop(&writer_path, &writer_recording_id, writer_rx));
        for book in initial.books {
            writer
                .send(WriterCommand::Insert(Box::new(book)))
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
                                if task_writer.send(WriterCommand::Insert(Box::new(update.book))).is_err() { break; }
                            }
                            Ok(_) => {}
                            Err(broadcast::error::RecvError::Lagged(count)) => {
                                let _ = task_writer.send(WriterCommand::Gap(count));
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
            active: true,
        })
    }

    pub async fn stop(&self, recording_id: &str) -> Result<RecordingInfo, AppError> {
        if let Some(active) = self.active.lock().await.remove(recording_id) {
            let stopped_at = now_ms();
            let _ = active.cancel.send(true);
            let (ack, ack_rx) = mpsc::channel();
            active
                .writer
                .send(WriterCommand::Stop(stopped_at, ack))
                .map_err(database_error)?;
            ack_rx.recv().map_err(database_error)?;
        }
        self.recording(recording_id)?
            .ok_or_else(|| AppError::InvalidInput(format!("unknown recording_id {recording_id}")))
    }

    pub fn list(&self, limit: u16) -> Result<ListRecordingsOutput, AppError> {
        initialize(&self.path)?;
        let connection = Connection::open(&self.path).map_err(database_error)?;
        let mut statement = connection
            .prepare("SELECT id, watch_id, label, token_ids_json, started_at_ms, stopped_at_ms, snapshot_count, dropped_update_count, active FROM recordings ORDER BY started_at_ms DESC LIMIT ?1")
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
                        bids: serde_json::from_str::<Vec<PriceLevel>>(&bids_json)
                            .unwrap_or_default(),
                        asks: serde_json::from_str::<Vec<PriceLevel>>(&asks_json)
                            .unwrap_or_default(),
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
                "SELECT id, watch_id, label, token_ids_json, started_at_ms, stopped_at_ms, snapshot_count, dropped_update_count, active FROM recordings WHERE id = ?1",
                [recording_id],
                recording_from_row,
            )
            .optional()
            .map_err(database_error)
    }
}

fn writer_loop(path: &Path, recording_id: &str, receiver: mpsc::Receiver<WriterCommand>) {
    let Ok(connection) = Connection::open(path) else {
        tracing::error!(recording_id, "failed to open recording database");
        return;
    };
    for command in receiver {
        let result = match command {
            WriterCommand::Insert(book) => insert_book(&connection, recording_id, &book),
            WriterCommand::Gap(count) => connection.execute(
                "UPDATE recordings SET dropped_update_count = dropped_update_count + ?2 WHERE id = ?1",
                params![recording_id, i64::try_from(count).unwrap_or(i64::MAX)],
            ),
            WriterCommand::Stop(timestamp, ack) => {
                let result = connection.execute(
                    "UPDATE recordings SET stopped_at_ms = ?2, active = 0 WHERE id = ?1",
                    params![recording_id, i64::try_from(timestamp).unwrap_or(i64::MAX)],
                );
                if let Err(error) = &result {
                    tracing::error!(recording_id, %error, "failed to stop recording");
                }
                let _ = ack.send(());
                break;
            }
        };
        if let Err(error) = result {
            tracing::error!(recording_id, %error, "failed to write recording data");
        }
    }
}

fn insert_book(
    connection: &Connection,
    recording_id: &str,
    book: &LiveOrderBook,
) -> rusqlite::Result<usize> {
    let bids = serde_json::to_string(&book.bids).unwrap_or_else(|_| "[]".to_owned());
    let asks = serde_json::to_string(&book.asks).unwrap_or_else(|_| "[]".to_owned());
    let changed = connection.execute(
        "INSERT INTO book_snapshots (recording_id, source, token_id, condition_id, upstream_timestamp_ms, received_at_ms, hash, best_bid, best_ask, spread, midpoint, bids_json, asks_json) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
        params![recording_id, book.source, book.token_id, book.condition_id, book.upstream_timestamp_ms, i64::try_from(book.received_at_ms).unwrap_or(i64::MAX), book.hash, book.best_bid, book.best_ask, book.spread, book.midpoint, bids, asks],
    )?;
    connection.execute(
        "UPDATE recordings SET snapshot_count = snapshot_count + 1 WHERE id = ?1",
        [recording_id],
    )?;
    Ok(changed)
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
        .map_err(database_error)
}

fn recording_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<RecordingInfo> {
    let tokens_json: String = row.get(3)?;
    Ok(RecordingInfo {
        recording_id: row.get(0)?,
        watch_id: row.get(1)?,
        label: row.get(2)?,
        token_ids: serde_json::from_str(&tokens_json).unwrap_or_default(),
        started_at_ms: row.get::<_, i64>(4)?.max(0) as u64,
        stopped_at_ms: row
            .get::<_, Option<i64>>(5)?
            .map(|value| value.max(0) as u64),
        snapshot_count: row.get::<_, i64>(6)?.max(0) as u64,
        dropped_update_count: row.get::<_, i64>(7)?.max(0) as u64,
        active: row.get(8)?,
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
        let service = RecorderService::new(directory.path().join("test.sqlite3"));
        let (_updates, receiver) = broadcast::channel(8);
        let initial = LiveSnapshotOutput {
            watch: WatchInfo {
                watch_id: "watch-1".to_owned(),
                token_ids: vec!["123".to_owned()],
                started_at_ms: 1,
                last_update_at_ms: Some(2),
                update_count: 1,
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
