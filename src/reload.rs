//! Live reload: watches the PDF file and reports when it changes.
//!
//! [`watch`] starts a `notify` recommended watcher on the file's parent
//! directory and filters events down to the target path. [`ReloadWatcher::poll`]
//! returns `true` once per accepted change; events arriving within
//! [`DEBOUNCE`] of the last accepted change are ignored, so a single save
//! (which editors often split into several filesystem events, delivered
//! asynchronously) is reported once.
//!
//! Consumed by the app loop: on `poll() == true` it calls
//! [`crate::render::RenderHandle::reload`], and the existing
//! `take_document_info` path in `app.rs` does the rest (rebuild slide index,
//! clamp page, clear textures).

use std::path::{Path, PathBuf};
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use notify::Watcher;

/// Minimum interval between accepted changes.
const DEBOUNCE: Duration = Duration::from_millis(500);

/// Error type for [`watch`]: anything the watcher backend reports.
#[derive(Debug)]
pub struct Error(notify::Error);

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl std::error::Error for Error {}

/// Watches one file for changes, with debouncing.
pub struct ReloadWatcher {
    /// One signal per filesystem event on the target file, from the watcher
    /// callback (the path filter runs in the callback).
    events: Receiver<()>,
    /// Time of the last accepted change; later events within [`DEBOUNCE`]
    /// of it are ignored.
    last_change: Option<Instant>,
    /// Kept alive: dropping the watcher unregisters the watch.
    _watcher: notify::RecommendedWatcher,
}

/// Starts watching `path` for changes.
pub fn watch(path: &Path) -> Result<ReloadWatcher, Error> {
    let target: PathBuf = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let (tx, rx) = std::sync::mpsc::channel();
    let filter = target.clone();

    let mut watcher =
        notify::recommended_watcher(move |res: Result<notify::Event, notify::Error>| {
            // Send failures only mean the UI side is gone; nothing to do.
            if let Ok(event) = res
                && event.paths.iter().any(|p| p == &filter)
            {
                let _ = tx.send(());
            }
        })
        .map_err(Error)?;

    // Watch the parent directory: editors replace files (rename over the
    // original) rather than writing in place, and some backends cannot watch
    // a file that does not exist yet. The event filter above narrows the
    // stream back to the target path.
    let watch_dir = target.parent().unwrap_or(Path::new("."));
    watcher
        .watch(watch_dir, notify::RecursiveMode::NonRecursive)
        .map_err(Error)?;

    Ok(ReloadWatcher {
        events: rx,
        last_change: None,
        _watcher: watcher,
    })
}

impl ReloadWatcher {
    /// Returns `true` if the watched file changed since the last poll.
    ///
    /// Debounced: events within [`DEBOUNCE`] of the last accepted change are
    /// ignored, so one save is reported once.
    pub fn poll(&mut self) -> bool {
        // Drain everything queued; a save often produces several events
        // (open, write, close) delivered asynchronously.
        let mut changed = false;
        while self.events.try_recv().is_ok() {
            changed = true;
        }
        if !changed {
            return false;
        }
        let now = Instant::now();
        match self.last_change {
            // Inside the debounce window: drop the events entirely, so the
            // same change is never re-reported.
            Some(t) if now.duration_since(t) < DEBOUNCE => false,
            _ => {
                self.last_change = Some(now);
                true
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::watch;
    use std::time::Duration;

    #[test]
    fn poll_detects_file_change_and_debounces() {
        let dir = std::env::temp_dir().join(format!("rumpress-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("deck.pdf");
        std::fs::write(&file, b"one").unwrap();
        let mut w = watch(&file).unwrap();
        assert!(!w.poll()); // no change yet
        std::thread::sleep(Duration::from_millis(600));
        std::fs::write(&file, b"two").unwrap();
        // inotify delivers events asynchronously: without this grace period
        // a single poll right after the write usually runs before the event
        // arrives (verified with a standalone probe; ~10 ms latency here).
        std::thread::sleep(Duration::from_millis(100));
        assert!(w.poll()); // change detected
        assert!(!w.poll()); // debounce: same change not re-reported
        std::fs::remove_dir_all(&dir).ok();
    }
}
