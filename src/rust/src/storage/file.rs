//! Filesystem watcher/monitor — mirrors
//! `webmentions.storage.adapters.file`.
//!
//! [`FileSystemWatcher`] watches a directory for content changes (the
//! Rust equivalent of the watchdog-based watcher) and
//! [`FileSystemMonitor`] maps those changes to outgoing Webmention
//! requests through a [`WebmentionsHandler`].

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};

use crate::handlers::WebmentionsHandler;
use crate::model::ContentTextFormat;

/// Content change types — mirrors `ContentChangeType`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContentChangeType {
    /// A file was added.
    Added,
    /// A file was edited.
    Edited,
    /// A file was deleted.
    Deleted,
}

/// Content change model — mirrors `ContentChange`.
#[derive(Debug, Clone)]
pub struct ContentChange {
    /// The kind of change.
    pub change_type: ContentChangeType,
    /// The absolute path of the changed file.
    pub path: String,
    /// The file contents (when readable and not a deletion).
    pub text: Option<String>,
    /// The detected text format (from the file extension).
    pub text_format: Option<ContentTextFormat>,
}

/// Callback invoked for each detected change.
pub type OnChange = Arc<dyn Fn(ContentChange) + Send + Sync>;

/// The default watched extensions — text, HTML and Markdown files.
pub const DEFAULT_EXTENSIONS: &[&str] = &[".md", ".markdown", ".txt", ".html", ".htm"];

const DEFAULT_THROTTLE_SECONDS: f64 = 10.0;

#[derive(Default)]
struct DebounceState {
    pending_paths: HashSet<PathBuf>,
    last_event_at: HashMap<PathBuf, Instant>,
    last_event_type: HashMap<PathBuf, String>,
    last_processed_at: Option<Instant>,
}

/// Watches for changes to the content in the given root directory —
/// mirrors `FileSystemWatcher`.
///
/// :param root_dir: The root directory to watch.
/// :param on_change: The function to call when a change is detected.
/// :param extensions: A tuple of file extensions to watch.
/// :param throttle_seconds: The minimum number of seconds between
///     processing changes, to prevent too many consecutive calls when
///     a file is written multiple times.
pub struct FileSystemWatcher {
    /// The root directory being watched.
    pub root_dir: PathBuf,
    /// The change callback.
    pub on_change: OnChange,
    /// Watched file extensions (lowercase, with dot).
    pub extensions: Vec<String>,
    /// Minimum interval between processed flushes.
    pub throttle: Duration,
    state: Arc<Mutex<DebounceState>>,
    stop: Arc<AtomicBool>,
    thread: Mutex<Option<JoinHandle<()>>>,
    watcher: Mutex<Option<RecommendedWatcher>>,
}

impl FileSystemWatcher {
    /// Create a watcher — mirrors `FileSystemWatcher.__init__`.
    pub fn new(
        root_dir: impl AsRef<Path>,
        on_change: OnChange,
        extensions: Option<Vec<String>>,
        throttle_seconds: Option<f64>,
    ) -> Self {
        FileSystemWatcher {
            root_dir: root_dir.as_ref().to_path_buf(),
            on_change,
            extensions: extensions
                .unwrap_or_else(|| DEFAULT_EXTENSIONS.iter().map(|e| e.to_string()).collect())
                .into_iter()
                .map(|e| e.to_lowercase())
                .collect(),
            throttle: Duration::from_secs_f64(throttle_seconds.unwrap_or(DEFAULT_THROTTLE_SECONDS)),
            state: Arc::new(Mutex::new(DebounceState::default())),
            stop: Arc::new(AtomicBool::new(false)),
            thread: Mutex::new(None),
            watcher: Mutex::new(None),
        }
    }

    /// Starts the watcher — mirrors `FileSystemWatcher.start`.
    pub fn start(&self) {
        let mut watcher_guard = self.watcher.lock().unwrap();
        if watcher_guard.is_some() || !self.root_dir.is_dir() {
            return;
        }

        let (tx, rx) = mpsc::channel::<(String, PathBuf)>();
        let root = self.root_dir.clone();
        let extensions: HashSet<String> = self.extensions.iter().cloned().collect();
        let enqueue = move |event_type: &'static str, path: PathBuf| {
            if Self::is_candidate(&root, &extensions, &path) {
                let _ = tx.send((event_type.to_string(), path));
            }
        };

        let mut watcher = match notify::recommended_watcher(move |res: Result<Event, _>| {
            let Ok(event) = res else { return };
            for path in event.paths {
                if path.is_dir() {
                    continue;
                }
                match event.kind {
                    EventKind::Create(_) => enqueue("created", path),
                    EventKind::Remove(_) => enqueue("deleted", path),
                    EventKind::Modify(notify::event::ModifyKind::Name(
                        notify::event::RenameMode::Both,
                    )) => enqueue("created", path),
                    EventKind::Modify(_) => enqueue("modified", path),
                    _ => {}
                }
            }
        }) {
            Ok(w) => w,
            Err(e) => {
                log::error!("Failed to start webmentions file watcher: {e}");
                return;
            }
        };

        if let Err(e) = watcher.watch(&self.root_dir, RecursiveMode::Recursive) {
            log::error!("Failed to watch {}: {e}", self.root_dir.display());
            return;
        }

        self.stop.store(false, Ordering::SeqCst);
        let state = self.state.clone();
        let stop = self.stop.clone();
        let on_change = self.on_change.clone();
        let throttle = self.throttle;
        let thread = std::thread::Builder::new()
            .name("webmentions-watch-loop".to_string())
            .spawn(move || Self::watch_loop(rx, state, stop, on_change, throttle))
            .expect("failed to spawn watcher thread");

        *self.thread.lock().unwrap() = Some(thread);
        *watcher_guard = Some(watcher);
        log::info!(
            "Webmentions file watcher started on {}",
            self.root_dir.display()
        );
    }

    /// Stops the watcher — mirrors `FileSystemWatcher.stop`.
    pub fn stop(&self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(watcher) = self.watcher.lock().unwrap().take() {
            drop(watcher);
        }
        if let Some(thread) = self.thread.lock().unwrap().take() {
            let _ = thread.join();
        }
        log::info!("Webmentions file watcher stopped");
    }

    fn is_candidate(root: &Path, extensions: &HashSet<String>, path: &Path) -> bool {
        let abs = match path.canonicalize() {
            Ok(p) => p,
            // Deleted paths can't be canonicalized — fall back to the
            // (already absolute) event path.
            Err(_) => path.to_path_buf(),
        };
        let canon_root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
        if !abs.starts_with(&canon_root) {
            return false;
        }
        match path.extension().and_then(|e| e.to_str()) {
            Some(ext) => extensions.contains(&format!(".{}", ext.to_lowercase())),
            None => false,
        }
    }

    fn watch_loop(
        rx: mpsc::Receiver<(String, PathBuf)>,
        state: Arc<Mutex<DebounceState>>,
        stop: Arc<AtomicBool>,
        on_change: OnChange,
        throttle: Duration,
    ) {
        while !stop.load(Ordering::SeqCst) {
            match rx.recv_timeout(Duration::from_millis(500)) {
                Ok((event_type, path)) => {
                    let now = Instant::now();
                    let mut st = state.lock().unwrap();
                    st.pending_paths.insert(path.clone());
                    st.last_event_at.insert(path.clone(), now);
                    st.last_event_type.insert(path, event_type);
                    drop(st);
                    Self::flush_debounced(&state, &on_change, throttle);
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    Self::flush_debounced(&state, &on_change, throttle);
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
    }

    /// Mirrors `_flush_debounced`.
    fn flush_debounced(
        state: &Arc<Mutex<DebounceState>>,
        on_change: &OnChange,
        throttle: Duration,
    ) {
        let now = Instant::now();
        let ready: Vec<(String, PathBuf)> = {
            let mut st = state.lock().unwrap();
            if st.pending_paths.is_empty() {
                return;
            }
            if let Some(last) = st.last_processed_at {
                if now.duration_since(last) < throttle {
                    return;
                }
            }
            let ready_paths: Vec<PathBuf> = st
                .pending_paths
                .iter()
                .filter(|p| {
                    now.duration_since(*st.last_event_at.get(*p).unwrap_or(&now)) >= throttle
                })
                .cloned()
                .collect();
            if ready_paths.is_empty() {
                return;
            }
            st.last_processed_at = Some(now);
            let mut ready = Vec::new();
            for p in ready_paths {
                st.pending_paths.remove(&p);
                st.last_event_at.remove(&p);
                let event_type = st.last_event_type.remove(&p).unwrap_or_default();
                ready.push((event_type, p));
            }
            ready
        };

        for (event_type, path) in ready {
            let Some(change) = Self::build_change(&event_type, &path) else {
                continue;
            };
            let cb = on_change.clone();
            if let Err(payload) =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| cb(change)))
            {
                let _ = payload;
                log::info!("Webmentions change handler failed for {}", path.display());
            }
        }
    }

    /// Mirrors `_build_change`.
    fn build_change(event_type: &str, abs_path: &Path) -> Option<ContentChange> {
        let path_str = abs_path.to_string_lossy().to_string();
        if event_type == "deleted" || !abs_path.is_file() {
            return Some(ContentChange {
                change_type: ContentChangeType::Deleted,
                path: path_str,
                text: None,
                text_format: None,
            });
        }

        let change_type = if event_type == "created" {
            ContentChangeType::Added
        } else {
            ContentChangeType::Edited
        };
        let text = std::fs::read_to_string(abs_path).ok();
        let text_format = Self::guess_text_format(abs_path)?;
        Some(ContentChange {
            change_type,
            path: path_str,
            text,
            text_format: Some(text_format),
        })
    }

    /// Mirrors `_guess_text_format`.
    fn guess_text_format(path: &Path) -> Option<ContentTextFormat> {
        match path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_lowercase())
            .as_deref()
        {
            Some("html" | "htm") => Some(ContentTextFormat::Html),
            Some("md" | "markdown") => Some(ContentTextFormat::Markdown),
            Some("txt" | "text") => Some(ContentTextFormat::Text),
            _ => None,
        }
    }
}

impl Drop for FileSystemWatcher {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Function mapping content file paths to source URLs.
pub type FileToUrlMapper = Arc<dyn Fn(&str) -> String + Send + Sync>;

/// Watches a directory for changes to files and maps file system
/// events to outgoing Webmention requests — mirrors `FileSystemMonitor`.
///
/// :param handler: The Webmentions handler used to dispatch outgoing
///     Webmentions.
/// :param root_dir: The root directory to watch.
/// :param file_to_url_mapper: Maps file paths to URLs for outgoing
///     Webmentions.
/// :param extensions: File extensions to watch (default: text, HTML
///     and Markdown files).
/// :param throttle_seconds: Minimum seconds between processing changes.
pub struct FileSystemMonitor {
    handler: Arc<WebmentionsHandler>,
    file_to_url_mapper: FileToUrlMapper,
    /// The root directory being watched.
    pub root_dir: PathBuf,
    /// Watched file extensions (lowercase, with dot).
    pub extensions: Vec<String>,
    /// Minimum interval between processed flushes, in seconds.
    pub throttle_seconds: f64,
    watcher: Mutex<Option<FileSystemWatcher>>,
}

impl FileSystemMonitor {
    /// Create a monitor — mirrors `FileSystemMonitor.__init__`.
    pub fn new(
        handler: Arc<WebmentionsHandler>,
        root_dir: impl AsRef<Path>,
        file_to_url_mapper: FileToUrlMapper,
        extensions: Option<Vec<String>>,
        throttle_seconds: Option<f64>,
    ) -> Self {
        let root_dir = root_dir
            .as_ref()
            .canonicalize()
            .unwrap_or_else(|_| root_dir.as_ref().to_path_buf());
        FileSystemMonitor {
            handler,
            file_to_url_mapper,
            root_dir,
            extensions: extensions
                .unwrap_or_else(|| DEFAULT_EXTENSIONS.iter().map(|e| e.to_string()).collect())
                .into_iter()
                .map(|e| e.to_lowercase())
                .collect(),
            throttle_seconds: throttle_seconds.unwrap_or(DEFAULT_THROTTLE_SECONDS),
            watcher: Mutex::new(None),
        }
    }

    /// Starts the filesystem watcher — mirrors `FileSystemMonitor.start`.
    pub fn start(&self) {
        let mut guard = self.watcher.lock().unwrap();
        if guard.is_some() {
            return;
        }
        let handler = self.handler.clone();
        let mapper = self.file_to_url_mapper.clone();
        let on_change: OnChange = Arc::new(move |change: ContentChange| {
            let source_url = mapper(&change.path);
            let t_start = Instant::now();
            match change.change_type {
                ContentChangeType::Deleted => {
                    log::info!("Content deleted, processing outgoing Webmentions: {source_url}");
                }
                _ => {
                    log::info!("Content changed, processing outgoing Webmentions: {source_url}");
                }
            }
            let _ = handler.process_outgoing_webmentions(
                &source_url,
                change.text.as_deref(),
                change.text_format,
            );
            log::info!(
                "Processed outgoing Webmentions for {source_url} in {:.2} seconds",
                t_start.elapsed().as_secs_f64()
            );
        });
        let watcher = FileSystemWatcher::new(
            &self.root_dir,
            on_change,
            Some(self.extensions.clone()),
            Some(self.throttle_seconds),
        );
        watcher.start();
        *guard = Some(watcher);
        log::info!("Started webmentions filesystem monitor");
    }

    /// Stops the filesystem watcher — mirrors `FileSystemMonitor.stop`.
    pub fn stop(&self) {
        if let Some(watcher) = self.watcher.lock().unwrap().take() {
            watcher.stop();
        }
    }
}

impl Drop for FileSystemMonitor {
    fn drop(&mut self) {
        self.stop();
    }
}
