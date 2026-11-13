//! Pdfium-backed document wrapper: open, inspect, and render pages or half-pages.
//!
//! Consumed by the render thread (later tasks); dead-code is expected until then.
#![allow(dead_code)]

use std::mem::ManuallyDrop;
use std::path::{Path, PathBuf};
use std::pin::Pin;

use pdfium_render::prelude::*;
use thiserror::Error;

/// Which half of a 2×1 (dual-slide) page to render.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Half {
    Left,
    Right,
    /// The whole page, unscaled by the half logic.
    Full,
}

/// A rendered RGBA bitmap.
#[derive(Debug, Clone)]
pub struct Bitmap {
    pub rgba: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

/// Errors surfaced by the pdf module.
#[derive(Debug, Error)]
pub enum Error {
    /// The Pdfium dynamic library could not be loaded.
    #[error(
        "Pdfium library not found; install libpdfium.so and point RUMPRESS_PDFIUM_PATH \
         at it, or run scripts/fetch-pdfium.sh to download libpdfium.so into \
         vendor/pdfium/ (underlying error: {source:?})"
    )]
    PdfiumUnavailable {
        #[source]
        source: Option<PdfiumError>,
    },

    #[error("page {page} out of range (document has {count} pages)")]
    PageOutOfRange { page: u16, count: u16 },

    #[error("pdfium error: {0}")]
    Pdfium(#[from] PdfiumError),
}

/// A Pdfium-backed PDF document.
pub struct Document {
    document: ManuallyDrop<PdfDocument<'static>>,
    // `pdfium` must be dropped AFTER `document`. The manual `Drop` impl
    // drops `document` first; `_pdfium` then drops automatically.
    _pdfium: Pin<Box<Pdfium>>,
    page_count: u16,
}

impl Drop for Document {
    fn drop(&mut self) {
        // Drop the document before the Pdfium bindings it borrows.
        // SAFETY: `document` is not moved out; we only drop it in place.
        // After this manual drop, `document` must NOT be dropped again by
        // the compiler's field drops — `ManuallyDrop` prevents that.
        unsafe { std::ptr::drop_in_place(&mut self.document as *mut ManuallyDrop<PdfDocument<'static>>) };
        // `_pdfium` drops automatically after this, in the correct order.
    }
}

impl Document {
    /// Opens a document, binding to the Pdfium library in the usual way.
    pub fn open(path: &Path) -> Result<Document, Error> {
        Document::open_with_library(path, None)
    }

    /// Opens a document, binding to the Pdfium library at `library_path` if given.
    ///
    /// This is the testable seam: passing a bogus path forces the
    /// [`Error::PdfiumUnavailable`] path without needing a broken environment.
    pub fn open_with_library(path: &Path, library_path: Option<&Path>) -> Result<Document, Error> {
        let pdfium = Box::pin(Self::bind(library_path)?);
        // SAFETY: `pdfium` is pinned (never moved) and stored in the returned
        // struct, so the self-referential `document` always points at a live
        // `Pdfium`. `PdfDocument<'static>` is sound because the `Pdfium`
        // outlives the document: the manual `Drop` impl drops the document
        // first, and the pinned `Pdfium` is only dropped afterwards.
        let document = unsafe {
            std::mem::transmute::<PdfDocument<'_>, PdfDocument<'static>>(
                pdfium.load_pdf_from_file(path, None).map_err(Error::from)?,
            )
        };
        let page_count = document.pages().len() as u16;
        Ok(Document {
            document: ManuallyDrop::new(document),
            _pdfium: pdfium,
            page_count,
        })
    }
    fn bind(library_path: Option<&Path>) -> Result<Pdfium, Error> {
        type BindAttempt = Box<dyn FnOnce() -> Result<Box<dyn PdfiumLibraryBindings>, PdfiumError>>;

        // An explicit `library_path` is authoritative: if it fails, we fail.
        // (This is the testable seam — a bogus path must produce
        // `PdfiumUnavailable`, not silently fall back to another library.)
        if let Some(path) = library_path {
            return Pdfium::bind_to_library(path)
                .map(Pdfium::new)
                .map_err(|err| Error::PdfiumUnavailable { source: Some(err) });
        }

        let mut attempts: Vec<BindAttempt> = Vec::new();

        if let Ok(env_path) = std::env::var("RUMPRESS_PDFIUM_PATH") {
            let path = PathBuf::from(env_path);
            attempts.push(Box::new(move || Pdfium::bind_to_library(path)));
        }

        attempts.push(Box::new(Pdfium::bind_to_system_library));
        attempts.push(Box::new(|| {
            Pdfium::bind_to_library("vendor/pdfium/libpdfium.so")
        }));

        let mut last_err = None;
        for attempt in attempts {
            match attempt() {
                Ok(bindings) => return Ok(Pdfium::new(bindings)),
                Err(err) => last_err = Some(err),
            }
        }

        Err(Error::PdfiumUnavailable { source: last_err })
    }

    /// Number of pages in the document.
    pub fn page_count(&self) -> u16 {
        self.page_count
    }

    /// Size of the given page in points (width, height).
    pub fn page_size(&self, page: u16) -> Result<(f32, f32), Error> {
        self.check_page(page)?;
        let rect = self.document.pages().page_size(page as PdfPageIndex)?;
        Ok((rect.width().value, rect.height().value))
    }

    /// Label of the given page, falling back to the 1-based page number.
    pub fn page_label(&self, page: u16) -> Result<String, Error> {
        self.check_page(page)?;
        let label = self
            .document
            .pages()
            .get(page as PdfPageIndex)?
            .label()
            .map(str::to_owned)
            .unwrap_or_else(|| (page + 1).to_string());
        Ok(label)
    }

    /// Renders the given page (or half of it) at `scale` pixels per point.
    pub fn render(&self, page: u16, half: Half, scale: f32) -> Result<Bitmap, Error> {
        self.check_page(page)?;
        let (page_w, page_h) = self.page_size(page)?;

        // Render the full page, then crop the half out of the bitmap. Simplest
        // correct approach; optimize only if profiling demands it.
        let full_w = (page_w * scale).round().max(1.0) as u32;
        let full_h = (page_h * scale).round().max(1.0) as u32;

        let pdf_page = self.document.pages().get(page as PdfPageIndex)?;
        let rendered = pdf_page
            .render_with_config(
                &PdfRenderConfig::new()
                    .set_target_size(
                        full_w.try_into().expect("width fits in i32"),
                        full_h.try_into().expect("height fits in i32"),
                    )
                    .set_clear_color(PdfColor::new(255, 255, 255, 255)),
            )
            .map_err(Error::from)?;

        let rgba = rendered.as_rgba_bytes();
        let rendered_w = rendered.width() as u32;
        let rendered_h = rendered.height() as u32;

        let (x0, x1) = match half {
            Half::Full => (0, rendered_w),
            Half::Left => (0, rendered_w / 2),
            Half::Right => (rendered_w / 2, rendered_w),
        };

        let out_w = x1 - x0;
        let mut out = Vec::with_capacity((out_w * rendered_h * 4) as usize);
        for y in 0..rendered_h {
            let row_start = ((y * rendered_w + x0) * 4) as usize;
            let row_end = ((y * rendered_w + x1) * 4) as usize;
            out.extend_from_slice(&rgba[row_start..row_end]);
        }

        Ok(Bitmap {
            rgba: out,
            width: out_w,
            height: rendered_h,
        })
    }

    fn check_page(&self, page: u16) -> Result<(), Error> {
        if page >= self.page_count {
            return Err(Error::PageOutOfRange {
                page,
                count: self.page_count,
            });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn fixture() -> &'static Path {
        Path::new("tests/fixtures/two-page.pdf")
    }

    #[test]
    fn opens_and_reports_pages() {
        let doc = Document::open(fixture()).unwrap();
        assert_eq!(doc.page_count(), 2);
        let (w, h) = doc.page_size(0).unwrap();
        assert_eq!((w, h), (720.0, 405.0));
    }

    #[test]
    fn renders_left_half_at_correct_dimensions() {
        let doc = Document::open(fixture()).unwrap();
        let bmp = doc.render(0, Half::Left, 1.0).unwrap();
        assert_eq!((bmp.width, bmp.height), (360, 405));
        assert_eq!(bmp.rgba.len(), (360 * 405 * 4) as usize);
    }

    #[test]
    fn renders_right_half_at_correct_dimensions() {
        let doc = Document::open(fixture()).unwrap();
        let bmp = doc.render(0, Half::Right, 1.0).unwrap();
        assert_eq!((bmp.width, bmp.height), (360, 405));
        assert_eq!(bmp.rgba.len(), (360 * 405 * 4) as usize);
    }

    #[test]
    fn renders_full_page_at_scale() {
        let doc = Document::open(fixture()).unwrap();
        let bmp = doc.render(0, Half::Full, 2.0).unwrap();
        assert_eq!((bmp.width, bmp.height), (1440, 810));
        assert_eq!(bmp.rgba.len(), (1440 * 810 * 4) as usize);
    }

    #[test]
    fn page_label_falls_back_to_page_number() {
        let doc = Document::open(fixture()).unwrap();
        assert_eq!(doc.page_label(0).unwrap(), "1");
        assert_eq!(doc.page_label(1).unwrap(), "2");
    }

    #[test]
    fn out_of_range_page_is_rejected() {
        let doc = Document::open(fixture()).unwrap();
        assert!(doc.page_size(2).is_err());
        assert!(doc.page_label(2).is_err());
        assert!(doc.render(2, Half::Full, 1.0).is_err());
    }

    #[test]
    fn missing_pdfium_gives_clear_error() {
        // Bind with a bogus path to force the unavailable path.
        let err = match Document::open_with_library(
            fixture(),
            Some(Path::new("/nonexistent/libpdfium.so")),
        ) {
            Ok(_) => panic!("expected PdfiumUnavailable error"),
            Err(err) => err,
        };
        let msg = err.to_string();
        assert!(msg.contains("libpdfium.so"), "message: {msg}");
        assert!(msg.contains("scripts/fetch-pdfium.sh"), "message: {msg}");
    }
}
