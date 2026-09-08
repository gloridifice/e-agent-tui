//! Project-local execution-history persistence owned by the DSH adapter.

use std::{
    collections::HashMap,
    fs::{self, File, OpenOptions},
    io::{BufRead, BufReader, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::{mpsc, Arc, Mutex},
    thread,
};

use e_tui::execution_history::{ExecutionRecord, TraceIdentity, TraceLine, TRACE_VERSION};

const QUEUE_CAPACITY: usize = 1_024;
const IGNORE_ENTRY: &str = "/e-dsh/execution-history/";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistorySnapshot {
    pub path: PathBuf,
    pub watermark: u64,
    pub records: Vec<ExecutionRecord>,
    pub warnings: Vec<String>,
    pub next_offset: u64,
    pub has_more: bool,
}

#[derive(Debug)]
enum Command {
    Record(ExecutionRecord),
    Snapshot {
        after_offset: u64,
        watermark: Option<u64>,
        limit: usize,
        reply: mpsc::SyncSender<Result<HistorySnapshot, String>>,
    },
    Flush(mpsc::SyncSender<Result<(), String>>),
    Shutdown(mpsc::SyncSender<Result<(), String>>),
}

pub struct HistoryStore {
    path: PathBuf,
    sender: mpsc::SyncSender<Command>,
    health: Arc<Mutex<Option<String>>>,
    worker: Option<thread::JoinHandle<()>>,
}

impl HistoryStore {
    pub fn open(identity: TraceIdentity) -> Result<Self, String> {
        let path = trace_path(&identity);
        let (sender, receiver) = mpsc::sync_channel(QUEUE_CAPACITY);
        let (ready_tx, ready_rx) = mpsc::sync_channel(0);
        let health = Arc::new(Mutex::new(None));
        let worker_health = Arc::clone(&health);
        let worker_path = path.clone();
        let worker = thread::Builder::new()
            .name("e-dsh-history".into())
            .spawn(move || {
                let opened = Writer::open(worker_path, identity);
                match opened {
                    Ok(mut writer) => {
                        let _ = ready_tx.send(Ok(()));
                        writer.run(receiver, &worker_health);
                    }
                    Err(error) => {
                        set_health(&worker_health, error.clone());
                        let _ = ready_tx.send(Err(error));
                    }
                }
            })
            .map_err(|error| format!("start execution-history writer: {error}"))?;
        ready_rx
            .recv()
            .map_err(|_| "execution-history writer stopped during startup".to_owned())??;
        Ok(Self {
            path,
            sender,
            health,
            worker: Some(worker),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn record(&self, record: ExecutionRecord) -> Result<(), String> {
        self.check_health()?;
        self.sender
            .try_send(Command::Record(record))
            .map_err(|error| {
                let message = match error {
                    mpsc::TrySendError::Full(_) => "execution-history queue is full".to_owned(),
                    mpsc::TrySendError::Disconnected(_) => {
                        "execution-history writer is unavailable".to_owned()
                    }
                };
                set_health(&self.health, message.clone());
                message
            })
    }

    pub fn snapshot(&self, limit: usize) -> Result<HistorySnapshot, String> {
        self.snapshot_page(0, None, limit)
    }

    pub fn snapshot_page(
        &self,
        after_offset: u64,
        watermark: Option<u64>,
        limit: usize,
    ) -> Result<HistorySnapshot, String> {
        self.check_health()?;
        let (reply_tx, reply_rx) = mpsc::sync_channel(0);
        self.sender
            .send(Command::Snapshot {
                after_offset,
                watermark,
                limit,
                reply: reply_tx,
            })
            .map_err(|_| "execution-history writer is unavailable".to_owned())?;
        reply_rx
            .recv()
            .map_err(|_| "execution-history snapshot was interrupted".to_owned())?
    }

    pub fn flush(&self) -> Result<(), String> {
        self.check_health()?;
        let (reply_tx, reply_rx) = mpsc::sync_channel(0);
        self.sender
            .send(Command::Flush(reply_tx))
            .map_err(|_| "execution-history writer is unavailable".to_owned())?;
        reply_rx
            .recv()
            .map_err(|_| "execution-history flush was interrupted".to_owned())?
    }

    pub fn health(&self) -> Result<(), String> {
        self.check_health()
    }

    fn file_watermark(&self) -> Result<u64, String> {
        self.flush()?;
        fs::metadata(&self.path)
            .map(|metadata| metadata.len())
            .map_err(|error| format!("inspect execution history: {error}"))
    }

    fn check_health(&self) -> Result<(), String> {
        match self.health.lock() {
            Ok(health) => match health.as_ref() {
                Some(error) => Err(error.clone()),
                None => Ok(()),
            },
            Err(_) => Err("execution-history health state is unavailable".into()),
        }
    }
}

impl Drop for HistoryStore {
    fn drop(&mut self) {
        let (reply_tx, reply_rx) = mpsc::sync_channel(0);
        if self.sender.send(Command::Shutdown(reply_tx)).is_ok() {
            let _ = reply_rx.recv();
        }
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

struct Writer {
    path: PathBuf,
    file: File,
    lock_path: PathBuf,
    _lock: File,
    startup_warnings: Vec<String>,
}

impl Writer {
    fn open(path: PathBuf, identity: TraceIdentity) -> Result<Self, String> {
        let directory = path
            .parent()
            .ok_or_else(|| "execution-history path has no parent".to_owned())?;
        fs::create_dir_all(directory).map_err(|error| {
            format!(
                "create execution-history directory {}: {error}",
                directory.display()
            )
        })?;
        update_ignore(Path::new(&identity.cwd))?;
        let lock_path = path.with_extension("jsonl.lock");
        let lock = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&lock_path)
            .map_err(|error| {
                format!(
                    "execution-history is already in use or cannot be locked at {}: {error}",
                    lock_path.display()
                )
            })?;
        let result = (|| {
            let mut startup_warnings = Vec::new();
            validate_or_initialize(&path, &identity, &mut startup_warnings)?;
            let file = OpenOptions::new()
                .append(true)
                .read(true)
                .open(&path)
                .map_err(|error| format!("open execution history {}: {error}", path.display()))?;
            Ok(Self {
                path,
                file,
                lock_path: lock_path.clone(),
                _lock: lock,
                startup_warnings,
            })
        })();
        if result.is_err() {
            let _ = fs::remove_file(&lock_path);
        }
        result
    }

    fn run(&mut self, receiver: mpsc::Receiver<Command>, health: &Mutex<Option<String>>) {
        while let Ok(command) = receiver.recv() {
            let result = match command {
                Command::Record(record) => write_line(&mut self.file, &TraceLine::Event { record }),
                Command::Snapshot {
                    after_offset,
                    watermark,
                    limit,
                    reply,
                } => {
                    let result = self
                        .flush()
                        .and_then(|_| self.read_snapshot(after_offset, watermark, limit));
                    let failed = result.as_ref().err().cloned();
                    let _ = reply.send(result);
                    if let Some(error) = failed {
                        set_health(health, error);
                    }
                    continue;
                }
                Command::Flush(reply) => {
                    let result = self.flush();
                    let failed = result.as_ref().err().cloned();
                    let _ = reply.send(result);
                    if let Some(error) = failed {
                        set_health(health, error);
                    }
                    continue;
                }
                Command::Shutdown(reply) => {
                    let result = self.flush();
                    let _ = reply.send(result);
                    break;
                }
            };
            if let Err(error) = result {
                set_health(health, error);
                break;
            }
        }
    }

    fn flush(&mut self) -> Result<(), String> {
        self.file
            .flush()
            .and_then(|_| self.file.sync_data())
            .map_err(|error| format!("flush execution history {}: {error}", self.path.display()))
    }

    fn read_snapshot(
        &self,
        after_offset: u64,
        requested_watermark: Option<u64>,
        limit: usize,
    ) -> Result<HistorySnapshot, String> {
        let current_len = self
            .file
            .metadata()
            .map_err(|error| format!("inspect execution history: {error}"))?
            .len();
        let watermark = requested_watermark.unwrap_or(current_len).min(current_len);
        read_snapshot(
            &self.path,
            after_offset,
            watermark,
            limit,
            &self.startup_warnings,
        )
    }
}

impl Drop for Writer {
    fn drop(&mut self) {
        let _ = self.file.flush();
        let _ = fs::remove_file(&self.lock_path);
    }
}

fn set_health(health: &Mutex<Option<String>>, error: String) {
    if let Ok(mut health) = health.lock() {
        if health.is_none() {
            *health = Some(error);
        }
    }
}

fn write_line(file: &mut File, line: &TraceLine) -> Result<(), String> {
    serde_json::to_writer(&mut *file, line)
        .map_err(|error| format!("encode execution-history record: {error}"))?;
    file.write_all(b"\n")
        .map_err(|error| format!("write execution-history record: {error}"))
}

fn validate_or_initialize(
    path: &Path,
    identity: &TraceIdentity,
    warnings: &mut Vec<String>,
) -> Result<(), String> {
    let mut file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(path)
        .map_err(|error| format!("open execution history {}: {error}", path.display()))?;
    if file.metadata().map_err(|error| error.to_string())?.len() == 0 {
        write_line(
            &mut file,
            &TraceLine::Header {
                version: TRACE_VERSION,
                identity: identity.clone(),
            },
        )?;
        file.flush().map_err(|error| error.to_string())?;
        return Ok(());
    }
    let mut first = String::new();
    BufReader::new(&file)
        .read_line(&mut first)
        .map_err(|error| format!("read execution-history header: {error}"))?;
    let header: TraceLine = serde_json::from_str(first.trim_end())
        .map_err(|error| format!("malformed execution-history header: {error}"))?;
    match header {
        TraceLine::Header {
            version,
            identity: _,
        } if version != TRACE_VERSION => {
            return Err(format!("unsupported execution-history version {version}"));
        }
        TraceLine::Header {
            identity: found, ..
        } if found != *identity => {
            return Err("execution-history identity does not match this session and cwd".into());
        }
        TraceLine::Header { .. } => {}
        TraceLine::Event { .. } => return Err("execution-history header is missing".into()),
    }
    file.seek(SeekFrom::End(-1))
        .map_err(|error| format!("inspect execution-history tail: {error}"))?;
    let mut last = [0];
    file.read_exact(&mut last)
        .map_err(|error| format!("inspect execution-history tail: {error}"))?;
    if last[0] != b'\n' {
        warnings.push("partial final execution-history record was preserved".into());
        file.seek(SeekFrom::End(0))
            .map_err(|error| error.to_string())?;
        file.write_all(b"\n")
            .map_err(|error| format!("separate partial execution-history tail: {error}"))?;
    }
    Ok(())
}

fn read_snapshot(
    path: &Path,
    after_offset: u64,
    watermark: u64,
    limit: usize,
    startup_warnings: &[String],
) -> Result<HistorySnapshot, String> {
    let mut file = File::open(path)
        .map_err(|error| format!("read execution history {}: {error}", path.display()))?;
    let start = after_offset.min(watermark);
    file.seek(SeekFrom::Start(start))
        .map_err(|error| format!("seek execution-history snapshot: {error}"))?;
    let mut reader = BufReader::new(file.take(watermark.saturating_sub(start)));
    let mut warnings = startup_warnings.to_vec();
    let mut records = Vec::new();
    let mut offset = start;
    let mut line_number = 0usize;
    let mut has_more = false;
    loop {
        let mut line = String::new();
        let read = reader
            .read_line(&mut line)
            .map_err(|error| format!("read execution-history snapshot: {error}"))?;
        if read == 0 {
            break;
        }
        line_number += 1;
        let line_start = offset;
        offset = offset.saturating_add(read as u64);
        if start == 0 && line_number == 1 {
            continue;
        }
        if line.trim().is_empty() {
            continue;
        }
        match serde_json::from_str::<TraceLine>(line.trim_end()) {
            Ok(TraceLine::Event { record }) if records.len() < limit => records.push(record),
            Ok(TraceLine::Event { .. }) => {
                has_more = true;
                offset = line_start;
                break;
            }
            Ok(TraceLine::Header { .. }) => warnings.push(format!(
                "unexpected execution-history header near byte {line_start}"
            )),
            Err(error) => warnings.push(format!(
                "malformed execution-history record near byte {line_start}: {error}"
            )),
        }
    }
    Ok(HistorySnapshot {
        path: path.to_owned(),
        watermark,
        records,
        warnings,
        next_offset: offset,
        has_more,
    })
}

pub fn trace_path(identity: &TraceIdentity) -> PathBuf {
    Path::new(&identity.cwd)
        .join(".e")
        .join("e-dsh")
        .join("execution-history")
        .join(format!("{}.jsonl", session_key(&identity.session_id)))
}

fn session_key(session_id: &str) -> String {
    let prefix: String = session_id
        .chars()
        .filter(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
        .take(48)
        .collect();
    let prefix = if prefix.is_empty() {
        "session"
    } else {
        &prefix
    };
    let hash = session_id
        .as_bytes()
        .iter()
        .fold(0xcbf29ce484222325_u64, |hash, byte| {
            (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
        });
    format!("{prefix}-{hash:016x}")
}

fn update_ignore(cwd: &Path) -> Result<(), String> {
    let directory = cwd.join(".e");
    fs::create_dir_all(&directory)
        .map_err(|error| format!("create {}: {error}", directory.display()))?;
    let path = directory.join(".gitignore");
    let existing = match fs::read_to_string(&path) {
        Ok(value) => value,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(format!("read {}: {error}", path.display())),
    };
    if existing.lines().any(|line| line.trim() == IGNORE_ENTRY) {
        return Ok(());
    }
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|error| format!("open {}: {error}", path.display()))?;
    if !existing.is_empty() && !existing.ends_with('\n') {
        file.write_all(b"\n").map_err(|error| error.to_string())?;
    }
    writeln!(file, "{IGNORE_ENTRY}").map_err(|error| format!("update {}: {error}", path.display()))
}

pub struct HistoryRecorder {
    frontend: &'static str,
    active_identity: Option<TraceIdentity>,
    store: Option<HistoryStore>,
    capture: Option<e_tui::execution_capture::ExecutionCapture>,
    origin: std::time::Instant,
    generation: u64,
    resume_metrics: HashMap<String, e_tui::agent::timeline::ToolExecutionMetrics>,
}

impl HistoryRecorder {
    pub fn new(frontend: &'static str) -> Self {
        Self {
            frontend,
            active_identity: None,
            store: None,
            capture: None,
            origin: std::time::Instant::now(),
            generation: 0,
            resume_metrics: HashMap::new(),
        }
    }

    pub fn observe(&mut self, event: &e_tui::AgentEvent) -> Result<(), String> {
        if let e_tui::AgentEvent::Session(e_tui::agent::SessionEvent::Attached(session)) = event {
            let cwd = session.workspace.as_ref().ok_or_else(|| {
                "execution history unavailable: attached session has no confirmed cwd".to_owned()
            })?;
            let identity = TraceIdentity {
                frontend: self.frontend.into(),
                session_id: session.id.clone(),
                cwd: cwd.clone(),
            };
            if self.active_identity.as_ref() == Some(&identity) {
                return Ok(());
            }
            self.close_active()?;
            self.generation = self.generation.saturating_add(1);
            let now = observed_now(self.origin);
            let run_id = format!(
                "{}-{}-{}",
                std::process::id(),
                now.wall_unix_ms,
                self.generation
            );
            let store = HistoryStore::open(identity.clone())?;
            self.resume_metrics = load_trace_metrics(&store)?;
            let mut capture = e_tui::execution_capture::ExecutionCapture::new(run_id);
            store.record(capture.attached(now))?;
            self.active_identity = Some(identity);
            self.store = Some(store);
            self.capture = Some(capture);
            return Ok(());
        }
        let now = observed_now(self.origin);
        let Some(capture) = self.capture.as_mut() else {
            return Ok(());
        };
        let records = capture.observe(event, now);
        let store = self
            .store
            .as_ref()
            .ok_or_else(|| "execution-history capture has no active writer".to_owned())?;
        for record in records {
            store.record(record)?;
        }
        store.health()
    }

    pub fn enrich_resume(&self, event: &mut e_tui::AgentEvent) {
        e_tui::execution_capture::enrich_historical_tool_metrics(event, &self.resume_metrics);
    }

    pub fn path(&self) -> Option<&Path> {
        self.store.as_ref().map(HistoryStore::path)
    }

    pub fn snapshot(&self, limit: usize) -> Result<HistorySnapshot, String> {
        self.store
            .as_ref()
            .ok_or_else(|| "execution history is unavailable for this session".to_owned())?
            .snapshot(limit)
    }

    pub fn query(
        &self,
        request: &e_tui::execution_history::HistoryQueryRequest,
    ) -> Result<e_tui::execution_history::HistoryQueryResult, String> {
        if self.active_identity.as_ref() != Some(&request.identity) {
            return Err("execution-history request does not match the active session".into());
        }
        let store = self
            .store
            .as_ref()
            .ok_or_else(|| "execution history is unavailable for this session".to_owned())?;
        if request.kind == e_tui::execution_history::HistoryQueryKind::Path {
            store.flush()?;
            return Ok(e_tui::execution_history::HistoryQueryResult {
                path: store.path().to_string_lossy().into_owned(),
                records: Vec::new(),
                ranked_calls: Vec::new(),
                warnings: Vec::new(),
                watermark: store.file_watermark()?,
                next_offset: 0,
                has_more: false,
            });
        }
        let full_session = matches!(
            request.kind,
            e_tui::execution_history::HistoryQueryKind::Copy
                | e_tui::execution_history::HistoryQueryKind::CopyLongest10
                | e_tui::execution_history::HistoryQueryKind::Longest50
        );
        let mut page = store.snapshot_page(
            request.after_offset,
            request.watermark,
            if full_session { 1_024 } else { 2_048 },
        )?;
        if full_session {
            let watermark = page.watermark;
            while page.has_more {
                let next = store.snapshot_page(page.next_offset, Some(watermark), 1_024)?;
                page.next_offset = next.next_offset;
                page.has_more = next.has_more;
                page.records.extend(next.records);
                page.warnings.extend(next.warnings);
            }
            page.warnings.sort();
            page.warnings.dedup();
        }
        let ranked_calls = if request.kind == e_tui::execution_history::HistoryQueryKind::Longest50
        {
            let calls = e_tui::execution_history::calls_from_records(&page.records);
            e_tui::execution_history::longest_calls(&calls, 50)
                .into_iter()
                .cloned()
                .collect()
        } else {
            Vec::new()
        };
        Ok(e_tui::execution_history::HistoryQueryResult {
            path: page.path.to_string_lossy().into_owned(),
            records: if request.kind == e_tui::execution_history::HistoryQueryKind::Longest50 {
                Vec::new()
            } else {
                page.records
            },
            ranked_calls,
            warnings: page.warnings,
            watermark: page.watermark,
            next_offset: page.next_offset,
            has_more: page.has_more,
        })
    }

    pub fn shutdown(&mut self) -> Result<(), String> {
        self.close_active()
    }

    fn close_active(&mut self) -> Result<(), String> {
        let mut result = Ok(());
        if let (Some(store), Some(capture)) = (self.store.as_ref(), self.capture.as_mut()) {
            if let Err(error) = store.record(capture.detached(observed_now(self.origin))) {
                result = Err(error);
            } else if let Err(error) = store.flush() {
                result = Err(error);
            }
        }
        self.capture = None;
        self.store = None;
        self.active_identity = None;
        self.resume_metrics.clear();
        result
    }
}

fn load_trace_metrics(
    store: &HistoryStore,
) -> Result<HashMap<String, e_tui::agent::timeline::ToolExecutionMetrics>, String> {
    let mut page = store.snapshot_page(0, None, 1_024)?;
    let watermark = page.watermark;
    while page.has_more {
        let next = store.snapshot_page(page.next_offset, Some(watermark), 1_024)?;
        page.next_offset = next.next_offset;
        page.has_more = next.has_more;
        page.records.extend(next.records);
    }
    Ok(e_tui::execution_capture::trace_tool_metrics(&page.records))
}

fn observed_now(origin: std::time::Instant) -> e_tui::execution_capture::ObservedAt {
    let wall_unix_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u128::from(u64::MAX)) as u64;
    let monotonic_ms = origin.elapsed().as_millis().min(u128::from(u64::MAX)) as u64;
    e_tui::execution_capture::ObservedAt {
        wall_unix_ms,
        monotonic_ms,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use e_tui::execution_history::{ExecutionEvent, ExecutionOutcome, OperationFinish};

    fn identity(cwd: &Path, session: &str) -> TraceIdentity {
        TraceIdentity {
            frontend: "e-dsh".into(),
            session_id: session.into(),
            cwd: cwd.to_string_lossy().into_owned(),
        }
    }

    fn attached(cwd: &Path, session: &str) -> e_tui::AgentEvent {
        e_tui::AgentEvent::Session(e_tui::agent::SessionEvent::Attached(
            e_tui::agent::AttachedSession {
                protocol_version: None,
                max_frame_bytes: None,
                id: session.into(),
                status: e_tui::agent::AgentStatus::Idle,
                provider: None,
                model: None,
                mode: None,
                title: None,
                workspace: Some(cwd.to_string_lossy().into_owned()),
            },
        ))
    }

    fn record(sequence: u64) -> ExecutionRecord {
        ExecutionRecord {
            sequence,
            run_id: "run-1".into(),
            time_unix_ms: sequence,
            event: ExecutionEvent::Finished(OperationFinish {
                call_id: format!("call-{sequence}"),
                outcome: ExecutionOutcome::Success,
                duration: None,
                output_lines: None,
            }),
        }
    }

    #[test]
    fn nested_cwd_path_is_stable_safe_and_ignore_is_preserved() {
        let root = tempfile::tempdir().unwrap();
        let cwd = root.path().join("repo").join("nested");
        fs::create_dir_all(cwd.join(".e")).unwrap();
        fs::write(cwd.join(".e/.gitignore"), "keep-me\n").unwrap();
        let id = identity(&cwd, "../../session:unsafe/名");
        let first = trace_path(&id);
        assert!(first.starts_with(&cwd));
        assert_eq!(first, trace_path(&id));
        assert!(!first.file_name().unwrap().to_string_lossy().contains('/'));
        let store = HistoryStore::open(id).unwrap();
        store.record(record(1)).unwrap();
        store.flush().unwrap();
        drop(store);
        let ignore = fs::read_to_string(cwd.join(".e/.gitignore")).unwrap();
        assert!(ignore.starts_with("keep-me\n"));
        assert_eq!(ignore.matches(IGNORE_ENTRY).count(), 1);
    }

    #[test]
    fn resume_appends_and_competing_writer_is_rejected() {
        let root = tempfile::tempdir().unwrap();
        let id = identity(root.path(), "session");
        let store = HistoryStore::open(id.clone()).unwrap();
        store.record(record(1)).unwrap();
        assert!(HistoryStore::open(id.clone()).is_err());
        drop(store);
        let resumed = HistoryStore::open(id).unwrap();
        resumed.record(record(2)).unwrap();
        let snapshot = resumed.snapshot(10).unwrap();
        assert_eq!(
            snapshot
                .records
                .iter()
                .map(|record| record.sequence)
                .collect::<Vec<_>>(),
            [1, 2]
        );
    }

    #[test]
    fn partial_tail_is_diagnosed_and_never_concatenated() {
        let root = tempfile::tempdir().unwrap();
        let id = identity(root.path(), "session");
        let path = trace_path(&id);
        drop(HistoryStore::open(id.clone()).unwrap());
        let mut file = OpenOptions::new().append(true).open(&path).unwrap();
        file.write_all(b"{partial").unwrap();
        drop(file);
        let store = HistoryStore::open(id).unwrap();
        store.record(record(3)).unwrap();
        let snapshot = store.snapshot(10).unwrap();
        assert_eq!(snapshot.records.len(), 1);
        assert!(snapshot
            .warnings
            .iter()
            .any(|warning| warning.contains("partial")));
        let source = fs::read_to_string(path).unwrap();
        assert!(source.contains("{partial\n{"));
    }

    #[test]
    fn recorder_waits_for_attachment_and_switches_exclusive_session_writers() {
        let root = tempfile::tempdir().unwrap();
        let mut recorder = HistoryRecorder::new("e-dsh");
        let before = e_tui::AgentEvent::Timeline(e_tui::agent::TimelineEvent::Snapshot {
            records: Vec::new(),
            truncated: false,
        });
        recorder.observe(&before).unwrap();
        assert!(recorder.path().is_none());
        assert!(!root.path().join(".e").exists());

        recorder.observe(&attached(root.path(), "one")).unwrap();
        let first = recorder.path().unwrap().to_owned();
        assert_eq!(recorder.snapshot(10).unwrap().records.len(), 1);
        recorder.observe(&attached(root.path(), "one")).unwrap();
        assert_eq!(recorder.snapshot(10).unwrap().records.len(), 1);
        recorder.observe(&attached(root.path(), "two")).unwrap();
        let second = recorder.path().unwrap().to_owned();
        assert_ne!(first, second);
        let first_text = fs::read_to_string(first).unwrap();
        assert!(first_text.contains("\"event\":\"attached\""));
        assert!(first_text.contains("\"event\":\"detached\""));
        recorder.shutdown().unwrap();
        assert!(fs::read_to_string(second)
            .unwrap()
            .contains("\"event\":\"detached\""));
    }

    #[test]
    fn recorder_loads_existing_trace_metrics_for_native_snapshot_enrichment() {
        let root = tempfile::tempdir().unwrap();
        let id = identity(root.path(), "session");
        let store = HistoryStore::open(id).unwrap();
        store
            .record(ExecutionRecord {
                sequence: 1,
                run_id: "old-run".into(),
                time_unix_ms: 100,
                event: ExecutionEvent::Started(e_tui::execution_history::OperationStart {
                    call_id: "call".into(),
                    turn_id: Some("turn".into()),
                    parent_id: None,
                    kind: e_tui::execution_history::OperationKind::Command,
                    name: "bash".into(),
                    summary: e_tui::execution_history::OperationSummary::Identity,
                }),
            })
            .unwrap();
        store
            .record(ExecutionRecord {
                sequence: 2,
                run_id: "old-run".into(),
                time_unix_ms: 350,
                event: ExecutionEvent::Finished(OperationFinish {
                    call_id: "call".into(),
                    outcome: ExecutionOutcome::Success,
                    duration: Some(e_tui::execution_history::MeasuredDuration {
                        duration_ms: 250,
                        source: e_tui::execution_history::TimingSource::Backend,
                    }),
                    output_lines: Some(e_tui::execution_history::ObservedOutputLines {
                        count: 7,
                        truncated: true,
                    }),
                }),
            })
            .unwrap();
        drop(store);

        let mut recorder = HistoryRecorder::new("e-dsh");
        recorder.observe(&attached(root.path(), "session")).unwrap();
        let mut event = e_tui::AgentEvent::Timeline(e_tui::agent::TimelineEvent::Snapshot {
            records: vec![e_tui::agent::timeline::TimelineRecord {
                sequence: None,
                time_ms: None,
                surface: None,
                source_sequences: Vec::new(),
                fact: e_tui::agent::timeline::TimelineFact::ToolResult {
                    activity_id: "call".into(),
                    output: "native output".into(),
                    state: e_tui::agent::tool::ActivityState::Success,
                    output_truncated: false,
                    execution_metrics: None,
                    starts_thinking: false,
                    mutation_diff: None,
                    mutation_hunks: Vec::new(),
                },
            }],
            truncated: false,
        });
        recorder.enrich_resume(&mut event);
        let e_tui::AgentEvent::Timeline(e_tui::agent::TimelineEvent::Snapshot { records, .. }) =
            event
        else {
            panic!()
        };
        let e_tui::agent::timeline::TimelineFact::ToolResult {
            execution_metrics: Some(metrics),
            output,
            ..
        } = &records[0].fact
        else {
            panic!()
        };
        assert_eq!(metrics.duration_ms, Some(250));
        assert_eq!(metrics.output_lines, Some(7));
        assert!(metrics.output_lines_truncated);
        assert_eq!(output, "native output");
    }

    #[test]
    fn unavailable_cwd_and_malformed_records_are_explicit() {
        let root = tempfile::tempdir().unwrap();
        let blocked = root.path().join("not-a-directory");
        fs::write(&blocked, "file").unwrap();
        assert!(HistoryStore::open(identity(&blocked, "session")).is_err());

        let id = identity(root.path(), "session");
        let path = trace_path(&id);
        drop(HistoryStore::open(id.clone()).unwrap());
        OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap()
            .write_all(b"{malformed}\n")
            .unwrap();
        let store = HistoryStore::open(id).unwrap();
        assert!(store
            .snapshot(10)
            .unwrap()
            .warnings
            .iter()
            .any(|warning| warning.contains("malformed")));
    }

    #[test]
    fn identity_version_and_bounded_snapshot_are_checked() {
        let root = tempfile::tempdir().unwrap();
        let id = identity(root.path(), "session");
        let store = HistoryStore::open(id.clone()).unwrap();
        for sequence in 0..4 {
            store.record(record(sequence)).unwrap();
        }
        let first_page = store.snapshot(2).unwrap();
        assert_eq!(first_page.records.len(), 2);
        assert!(first_page.has_more);
        store.record(record(4)).unwrap();
        let second_page = store
            .snapshot_page(first_page.next_offset, Some(first_page.watermark), 10)
            .unwrap();
        assert_eq!(second_page.records.len(), 2);
        assert!(!second_page.has_more);
        assert!(second_page.records.iter().all(|record| record.sequence < 4));
        drop(store);
        let path = trace_path(&id);
        let source = fs::read_to_string(&path).unwrap();
        fs::write(
            &path,
            source.replacen("\"session_id\":\"session\"", "\"session_id\":\"other\"", 1),
        )
        .unwrap();
        assert!(HistoryStore::open(id.clone())
            .err()
            .unwrap()
            .contains("identity"));
        fs::write(&path, source.replacen("\"version\":1", "\"version\":99", 1)).unwrap();
        assert!(HistoryStore::open(id)
            .err()
            .unwrap()
            .contains("unsupported"));
    }
}
