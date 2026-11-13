//! Presentation state machine: current page/slide and blanked screen.
//!
//! Pure logic over a [`SlideIndex`]; no PDF access, no threads.
//!
//! Consumed by the presenter loop (later tasks); dead-code is expected
//! until then.
#![allow(dead_code)]

use crate::pdf::SlideIndex;

/// Presenter state: where we are in the deck and whether the screen is blanked.
pub struct DeckState {
    /// Current 0-based page.
    pub current_page: u16,
    /// Current 0-based slide.
    pub current_slide: usize,
    /// Whether the presentation screen is blanked.
    pub blanked: bool,
    /// Slide layout of the deck.
    slides: SlideIndex,
}

impl DeckState {
    /// New state at the first page of the deck, not blanked.
    pub fn new(slides: SlideIndex) -> DeckState {
        DeckState {
            current_page: 0,
            current_slide: 0,
            blanked: false,
            slides,
        }
    }

    /// One page forward: within the current slide's overlays first, then to
    /// the next slide's first page. Clamps at the end of the deck.
    pub fn next_page(&mut self) {
        if self.current_page < self.slides.last_page(self.current_slide) {
            self.current_page += 1;
        } else if let Some(next) = self.slides.next_slide(self.current_slide) {
            self.current_slide = next;
            self.current_page = self.slides.first_page(next);
        }
    }

    /// One page backward: within the current slide's overlays first, then to
    /// the previous slide's last page. Clamps at the start of the deck.
    pub fn prev_page(&mut self) {
        if self.current_page > self.slides.first_page(self.current_slide) {
            self.current_page -= 1;
        } else if let Some(prev) = self.slides.prev_slide(self.current_slide) {
            self.current_slide = prev;
            self.current_page = self.slides.last_page(prev);
        }
    }

    /// Jump to the next slide's first page, skipping overlays. Clamps at the
    /// last slide.
    pub fn next_slide(&mut self) {
        if let Some(next) = self.slides.next_slide(self.current_slide) {
            self.current_slide = next;
            self.current_page = self.slides.first_page(next);
        }
    }

    /// Jump to the previous slide's first page. Clamps at the first slide.
    pub fn prev_slide(&mut self) {
        if let Some(prev) = self.slides.prev_slide(self.current_slide) {
            self.current_slide = prev;
            self.current_page = self.slides.first_page(prev);
        }
    }

    /// Jump to the given slide's first page, clamped to the deck's range.
    pub fn goto_slide(&mut self, slide: usize) {
        let slide = slide.min(self.slides.slide_count() - 1);
        self.current_slide = slide;
        self.current_page = self.slides.first_page(slide);
    }

    /// Replaces the slide layout after a reload: recomputes the current slide
    /// from the current page, clamping the page to the new last page if it is
    /// out of range.
    pub fn apply_slides(&mut self, slides: SlideIndex) {
        let last_page = slides.last_page(slides.slide_count() - 1);
        self.current_page = self.current_page.min(last_page);
        self.current_slide = slides.slide_of_page(self.current_page);
        self.slides = slides;
    }

    /// Toggles the blanked state, returning the new value.
    pub fn toggle_blank(&mut self) -> bool {
        self.blanked = !self.blanked;
        self.blanked
    }

    /// Whether the presenter is on the final slide: true from the last
    /// slide's first page onward, since `goto_slide` and slide navigation
    /// land on first pages (per the brief's tests).
    pub fn at_last_page(&self) -> bool {
        self.slides.next_slide(self.current_slide).is_none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn three_slides() -> SlideIndex {
        // pages: 0,1,2 = slide0 (3 overlays), 3 = slide1, 4,5 = slide2
        SlideIndex::from_labels(&["1", "1", "1", "2", "3", "3"].map(String::from))
    }

    #[test]
    fn next_page_steps_within_slide_then_jumps() {
        let mut s = DeckState::new(three_slides());
        s.next_page();
        s.next_page(); // pages 0 -> 1 -> 2
        assert_eq!(s.current_page, 2);
        s.next_page(); // -> slide 1, page 3
        assert_eq!((s.current_slide, s.current_page), (1, 3));
    }

    #[test]
    fn next_slide_skips_overlays() {
        let mut s = DeckState::new(three_slides());
        s.next_slide();
        assert_eq!((s.current_slide, s.current_page), (1, 3));
        s.prev_slide();
        assert_eq!((s.current_slide, s.current_page), (0, 0)); // first page of slide 0
    }

    #[test]
    fn navigation_clamps_at_ends() {
        let mut s = DeckState::new(three_slides());
        s.prev_page();
        s.prev_slide();
        assert_eq!(s.current_page, 0);
        s.goto_slide(99);
        assert_eq!(s.current_slide, 2);
        assert!(s.at_last_page());
    }

    #[test]
    fn reload_with_fewer_pages_clamps() {
        let mut s = DeckState::new(three_slides());
        s.goto_slide(2); // page 4
        s.apply_slides(SlideIndex::from_labels(&["1", "2"].map(String::from)));
        assert_eq!(s.current_slide, 1); // clamped to last slide
        assert_eq!(s.current_page, 1);
    }

    #[test]
    fn toggle_blank_flips_and_returns_new_state() {
        let mut s = DeckState::new(three_slides());
        assert!(!s.blanked);
        assert!(s.toggle_blank());
        assert!(s.blanked);
        assert!(!s.toggle_blank());
        assert!(!s.blanked);
    }
}
