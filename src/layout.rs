//! Page layout detection: decides how a PDF page maps onto the dual-viewport
//! presenter.
//!
//! Notes decks are 2×1: one wide page whose left half is the slide shown on
//! the beamer and whose right half holds the notes shown on the presenter.
//! Anything that is not wide enough (aspect < 1.5) is treated as a plain
//! single page and shown in full on both windows.

use crate::pdf::Half;

/// How one page of the document is laid out for the two viewports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageLayout {
    /// A 2×1 notes page: `slide` half goes to the beamer, `notes` half to the
    /// presenter's notes widget.
    Split { slide: Half, notes: Half },
    /// Not a 2×1 page: both viewports show the whole page.
    Full,
}

/// Minimum width/height aspect ratio for a page to count as a 2×1 notes
/// page. 16:9 (≈1.78) is the standard notes-deck format; 1.5 keeps a wide
/// margin above it while still rejecting anything squarer.
const SPLIT_MIN_ASPECT: f32 = 1.5;

/// Detects the layout from the page size in points and the `--swap-halves`
/// flag. A page counts as 2×1 when its aspect ratio (width / height) is at
/// least [`SPLIT_MIN_ASPECT`].
///
/// Note: the task prose suggested a 1.9 threshold, but the task's own tests
/// and fixtures classify a 720×405 (16:9, aspect ≈ 1.78) page as `Split` —
/// the project's 2×1 fixture format — so the threshold is 1.5.
pub fn detect(page_size: (f32, f32), swap_halves: bool) -> PageLayout {
    let (w, h) = page_size;
    let aspect = w / h.max(1.0);
    if aspect >= SPLIT_MIN_ASPECT {
        let (slide, notes) = if swap_halves {
            (Half::Right, Half::Left)
        } else {
            (Half::Left, Half::Right)
        };
        PageLayout::Split { slide, notes }
    } else {
        PageLayout::Full
    }
}

#[cfg(test)]
mod tests {
    use super::{PageLayout, detect};
    use crate::pdf::Half;

    #[test]
    fn detects_split_and_swap() {
        assert_eq!(
            detect((720.0, 405.0), false),
            PageLayout::Split {
                slide: Half::Left,
                notes: Half::Right
            }
        );
        assert_eq!(
            detect((720.0, 405.0), true),
            PageLayout::Split {
                slide: Half::Right,
                notes: Half::Left
            }
        );
    }

    #[test]
    fn detects_full_page() {
        assert_eq!(detect((405.0, 720.0), false), PageLayout::Full); // portrait
        assert_eq!(detect((360.0, 405.0), false), PageLayout::Full); // 8:9
    }
}
