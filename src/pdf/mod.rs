//! Pdfium-backed PDF loading and rendering.

mod document;

// Re-exported for downstream use (later tasks); unused in the bin crate for now.
#[allow(unused_imports)]
pub use document::{Bitmap, Document, Error, Half};
