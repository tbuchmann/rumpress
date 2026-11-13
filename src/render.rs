//! Render thread and texture cache.
//!
//! A background thread owns the [`pdf::Document`] exclusively. The UI side
//! talks to it through [`RenderHandle`]: request renders, poll for finished
//! bitmaps, reload the document.
//!
//! Consumed by the egui app (later tasks); dead-code is expected until then.
#![allow(dead_code)]

use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use crossbeam_channel::{Receiver, Sender, unbounded};

use crate::pdf::{Bitmap, Document, Half};

/// Identifies a renderable texture: a page (or half of one).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PageKey {
    pub page: u16,
    pub half: Half,
}

/// Identifies a rendered bitmap: a [`PageKey`] at a specific scale.
///
/// The scale is part of the identity: the same page rendered at 2.0 and
/// 0.3 pixels per point produces different bitmaps, and caching one under
/// the other would show a blurry thumbnail where the full-resolution slide
/// belongs. `f32` is not `Eq`/`Hash`, so the scale is compared by bit
/// pattern (scales are program constants, never NaN).
#[derive(Debug, Clone, Copy)]
pub struct RenderKey {
    pub page: PageKey,
    /// Pixels per point.
    pub scale: f32,
}

impl RenderKey {
    /// Builds the key for `page` rendered at `scale` pixels per point.
    pub fn new(page: PageKey, scale: f32) -> RenderKey {
        RenderKey { page, scale }
    }
}

impl PartialEq for RenderKey {
    fn eq(&self, other: &Self) -> bool {
        self.page == other.page && self.scale.to_bits() == other.scale.to_bits()
    }
}
impl Eq for RenderKey {}

impl Hash for RenderKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.page.hash(state);
        self.scale.to_bits().hash(state);
    }
}

/// Snapshot of document metadata, sent to the UI on open/reload.
#[derive(Debug, Clone, PartialEq)]
pub struct DocumentInfo {
    pub page_count: u16,
    pub page_size: (f32, f32),
    pub labels: Vec<String>,
}

/// Messages the render thread accepts.
enum Request {
    Render(PageKey, f32),
    Reload(PathBuf),
}

/// Maximum number of cached bitmaps; evicts oldest by insertion order.
const CACHE_CAP: usize = 64;

/// Handle to the render thread. Cheap to clone; the UI keeps one.
#[derive(Clone)]
pub struct RenderHandle {
    tx: Sender<Request>,
    bitmaps: Receiver<(RenderKey, Bitmap)>,
    infos: Receiver<DocumentInfo>,
    /// Requests sent but not yet delivered by `poll`; identical requests are
    /// dropped while one is in flight.
    in_flight: Arc<Mutex<HashSet<RenderKey>>>,
}

impl RenderHandle {
    /// Spawns the render thread for the document at `path`.
    pub fn spawn(path: PathBuf) -> RenderHandle {
        let (tx, rx) = unbounded();
        let (bmp_tx, bmp_rx) = unbounded();
        let (info_tx, info_rx) = unbounded();
        std::thread::Builder::new()
            .name("render".into())
            .spawn(move || render_loop(path, rx, bmp_tx, info_tx))
            .expect("spawn render thread");
        RenderHandle {
            tx,
            bitmaps: bmp_rx,
            infos: info_rx,
            in_flight: Arc::default(),
        }
    }

    /// Requests a render of `key` at `scale` pixels per point.
    ///
    /// A request identical to one still in flight is dropped. A request for a
    /// page the thread has already rendered is answered again from its cache,
    /// so callers that evicted the texture can get it back.
    pub fn request(&self, key: PageKey, scale: f32) {
        let rkey = RenderKey::new(key, scale);
        if !self.in_flight.lock().unwrap_or_else(|e| e.into_inner()).insert(rkey) {
            return;
        }
        let _ = self.tx.send(Request::Render(key, scale));
    }

    /// Replaces the document, clearing the cache. Emits new [`DocumentInfo`].
    pub fn reload(&self, path: PathBuf) {
        self.in_flight.lock().unwrap_or_else(|e| e.into_inner()).clear();
        let _ = self.tx.send(Request::Reload(path));
    }

    /// Returns the next finished bitmap, if any. Non-blocking.
    pub fn poll(&self) -> Option<(RenderKey, Bitmap)> {
        let item = self.bitmaps.try_recv().ok()?;
        self.in_flight
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&item.0);
        Some(item)
    }

    /// Returns the pending [`DocumentInfo`], if any. Non-blocking.
    pub fn take_document_info(&self) -> Option<DocumentInfo> {
        self.infos.try_recv().ok()
    }
}

fn render_loop(
    original: PathBuf,
    rx: Receiver<Request>,
    bmp_out: Sender<(RenderKey, Bitmap)>,
    info_out: Sender<DocumentInfo>,
) {
    let mut doc = match Document::open(&original) {
        Ok(doc) => {
            emit_info(&doc, &info_out);
            Some(doc)
        }
        Err(e) => {
            log::error!("failed to open {}: {e}", original.display());
            None
        }
    };

    let mut cache: HashMap<RenderKey, Bitmap> = HashMap::new();
    let mut order: Vec<RenderKey> = Vec::new(); // insertion order for eviction

    while let Ok(msg) = rx.recv() {
        match msg {
            Request::Reload(path) => {
                cache.clear();
                order.clear();
                // Drop the old document first: only one `Document` may be
                // alive at a time (see `pdf::Document`), so opening the new
                // one while holding the old would deadlock.
                doc = None;
                match Document::open(&path) {
                    Ok(new_doc) => {
                        emit_info(&new_doc, &info_out);
                        doc = Some(new_doc);
                    }
                    Err(e) => {
                        // The replacement may be a half-written file caught
                        // mid-write; the original may still be valid, so try
                        // to fall back to it before giving up.
                        match Document::open(&original) {
                            Ok(old_doc) => {
                                log::warn!(
                                    "reload of {} failed ({e}); keeping the original {}",
                                    path.display(),
                                    original.display()
                                );
                                emit_info(&old_doc, &info_out);
                                doc = Some(old_doc);
                            }
                            Err(orig_e) => {
                                log::error!(
                                    "failed to reload {}: {e}; re-opening the original {} \
                                     also failed: {orig_e} — renders are suspended until a \
                                     successful reload",
                                    path.display(),
                                    original.display()
                                );
                            }
                        }
                    }
                }
            }
            Request::Render(key, scale) => {
                let rkey = RenderKey::new(key, scale);
                // Already rendered: answer again from the cache. The UI evicts
                // textures it no longer shows, so it may legitimately ask for
                // a page we still hold (e.g. stepping back one slide).
                if let Some(bmp) = cache.get(&rkey) {
                    if bmp_out.send((rkey, bmp.clone())).is_err() {
                        break; // UI gone
                    }
                    continue;
                }
                let Some(doc) = doc.as_ref() else { continue };
                match doc.render(key.page, key.half, scale) {
                    Ok(bmp) => {
                        if cache.len() >= CACHE_CAP
                            && let Some(oldest) = order.first().copied()
                        {
                            cache.remove(&oldest);
                            order.remove(0);
                        }
                        cache.insert(rkey, bmp.clone());
                        order.push(rkey);
                        if bmp_out.send((rkey, bmp)).is_err() {
                            break; // UI gone
                        }
                    }
                    Err(e) => log::warn!("render of {key:?} failed: {e}"),
                }
            }
        }
    }
}

fn emit_info(doc: &Document, out: &Sender<DocumentInfo>) {
    let page_count = doc.page_count();
    let page_size = doc.page_size(0).unwrap_or((0.0, 0.0));
    let labels = (0..page_count)
        .filter_map(|p| doc.page_label(p).ok())
        .collect();
    let _ = out.send(DocumentInfo {
        page_count,
        page_size,
        labels,
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use std::time::Duration;

    const TWO_PAGE: &str = "tests/fixtures/two-page.pdf";
    const ONE_PAGE: &str = "tests/fixtures/one-page.pdf";

    /// Polls `f` until it returns `Some` or ~1s elapses.
    fn wait_for<T>(mut f: impl FnMut() -> Option<T>) -> T {
        for _ in 0..100 {
            if let Some(v) = f() {
                return v;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("condition not met within 1s");
    }

    #[test]
    fn returns_requested_bitmap_once() {
        let h = RenderHandle::spawn(Path::new(TWO_PAGE).into());
        h.request(
            PageKey {
                page: 0,
                half: Half::Left,
            },
            1.0,
        );
        let (key, bmp) = wait_for(|| h.poll());
        assert_eq!(key.page.page, 0);
        assert_eq!(bmp.width, 640);
        // second poll: nothing new
        assert!(h.poll().is_none());
    }

    #[test]
    fn reload_replaces_document() {
        let h = RenderHandle::spawn(Path::new(TWO_PAGE).into());
        h.reload(Path::new(ONE_PAGE).into());
        // The initial open also emits info (2 pages); skip it and wait for
        // the one describing the reloaded document.
        let info = wait_for(|| h.take_document_info().filter(|i| i.page_count == 1));
        assert_eq!(info.page_count, 1);
    }

    #[test]
    fn document_info_arrives_on_spawn() {
        let h = RenderHandle::spawn(Path::new(TWO_PAGE).into());
        let info = wait_for(|| h.take_document_info());
        assert_eq!(info.page_count, 2);
        assert_eq!(info.page_size, (1280.0, 360.0));
        assert_eq!(info.labels, vec!["1".to_string(), "2".to_string()]);
    }

    #[test]
    fn duplicate_requests_yield_one_bitmap() {
        let h = RenderHandle::spawn(Path::new(TWO_PAGE).into());
        h.request(
            PageKey {
                page: 0,
                half: Half::Full,
            },
            1.0,
        );
        h.request(
            PageKey {
                page: 0,
                half: Half::Full,
            },
            1.0,
        );
        h.request(
            PageKey {
                page: 0,
                half: Half::Full,
            },
            1.0,
        );
        let (key, _) = wait_for(|| h.poll());
        assert_eq!(key.page.page, 0);
        // Give the thread time to process any (wrongly) duplicated requests.
        std::thread::sleep(Duration::from_millis(100));
        assert!(h.poll().is_none(), "duplicate requests were not deduped");
    }

    #[test]
    fn evicted_page_can_be_requested_again() {
        // Regression: the UI drops textures it no longer shows. Stepping
        // forward and back re-requests a page the thread still has cached;
        // it must be delivered again, not silently skipped (blank slide).
        let h = RenderHandle::spawn(Path::new(TWO_PAGE).into());
        let a = PageKey {
            page: 0,
            half: Half::Full,
        };
        let b = PageKey {
            page: 1,
            half: Half::Full,
        };
        h.request(a, 1.0);
        let (first, bmp1) = wait_for(|| h.poll());
        assert_eq!(first.page, a);
        h.request(b, 1.0);
        wait_for(|| h.poll());
        // Back to `a`: the caller holds no texture for it any more.
        h.request(a, 1.0);
        let (again, bmp2) = wait_for(|| h.poll());
        assert_eq!(again.page, a);
        assert_eq!((bmp2.width, bmp2.height), (bmp1.width, bmp1.height));
        assert_eq!(bmp2.rgba, bmp1.rgba);
    }

    #[test]
    fn same_page_at_different_scales_is_rendered_twice() {
        // The cache is keyed by (page, scale): a thumbnail render must not
        // satisfy a later full-resolution request for the same page.
        let h = RenderHandle::spawn(Path::new(TWO_PAGE).into());
        let key = PageKey {
            page: 0,
            half: Half::Full,
        };
        h.request(key, 0.3);
        let (_, thumb) = wait_for(|| h.poll());
        h.request(key, 1.0);
        let (_, full) = wait_for(|| h.poll());
        assert!(
            full.width > thumb.width,
            "full-scale request was served from the thumbnail cache"
        );
        // And the reverse order: the cached full render must not swallow a
        // new thumbnail request; it is served again at the thumbnail size.
        h.request(key, 0.3);
        let (_, thumb2) = wait_for(|| h.poll());
        assert_eq!(thumb2.width, thumb.width);
    }

    #[test]
    fn reload_clears_cache() {
        let h = RenderHandle::spawn(Path::new(TWO_PAGE).into());
        let key = PageKey {
            page: 0,
            half: Half::Full,
        };
        h.request(key, 1.0);
        wait_for(|| h.poll());
        h.reload(Path::new(ONE_PAGE).into());
        let info = wait_for(|| h.take_document_info().filter(|i| i.page_count == 1));
        assert_eq!(info.page_count, 1);
        // Page 0 of the new document is 612x792, so a fresh render differs
        // from the cached 1280x360 one — proving the cache was cleared.
        h.request(key, 1.0);
        let (_, bmp) = wait_for(|| h.poll());
        assert_eq!(bmp.width, 612);
        assert_eq!(bmp.height, 792);
    }

    #[test]
    fn out_of_range_request_is_dropped_not_fatal() {
        let h = RenderHandle::spawn(Path::new(TWO_PAGE).into());
        h.request(
            PageKey {
                page: 99,
                half: Half::Full,
            },
            1.0,
        );
        // The thread must survive and still serve a later valid request.
        h.request(
            PageKey {
                page: 0,
                half: Half::Left,
            },
            1.0,
        );
        let (key, _) = wait_for(|| h.poll());
        assert_eq!(key.page.page, 0);
    }
}
