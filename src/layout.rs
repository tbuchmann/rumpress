//! Page layout detection: decides how a PDF page maps onto the dual-viewport
//! presenter.
//!
//! Notes decks are 2×1: one wide page whose left half is the slide shown on
//! the beamer and whose right half holds the notes shown on the presenter.
//! Anything that is not wide enough (aspect < 1.9) is treated as a plain
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
/// page.
///
/// A real beamer `show notes on second screen` page is two slide-shaped
/// halves side by side, so its aspect is 2 × the slide aspect: ≈3.56 for a
/// 16:9 deck and ≈2.67 for a 4:3 deck. A plain slide-only 16:9 deck is
/// ≈1.778 and must be shown in full on both screens, so the threshold sits
/// between 1.778 and 2.67. 1.9 is the smallest round value with a safe
/// margin on both sides (2.0 would work too but sits closer to the 16:9
/// slide aspect, leaving less headroom for slightly-wide slide formats).
const SPLIT_MIN_ASPECT: f32 = 1.9;

/// Detects the layout from the page size in points and the `--swap-halves`
/// flag. A page counts as 2×1 when its aspect ratio (width / height) is at
/// least [`SPLIT_MIN_ASPECT`].
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
        // 1280×360 = two 640×360 (16:9) halves side by side, the real
        // beamer notes-page shape for a 16:9 deck (aspect ≈ 3.56).
        assert_eq!(
            detect((1280.0, 360.0), false),
            PageLayout::Split {
                slide: Half::Left,
                notes: Half::Right
            }
        );
        assert_eq!(
            detect((1280.0, 360.0), true),
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
        // A plain 16:9 slide-only deck (aspect ≈ 1.778) is not a 2×1 page.
        assert_eq!(detect((720.0, 405.0), false), PageLayout::Full);
    }
}
