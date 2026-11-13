//! Logical slide grouping from page labels.
//!
//! A "slide" is one logical presentation step: a page plus any following
//! overlay pages that share its label (pympress `get_last_label_pages`
//! logic). Consecutive pages with equal labels belong to the same slide.
//!
//! Consumed by the presenter state (later tasks); dead-code is expected
//! until then.
#![allow(dead_code)]

/// Index mapping pages to logical slides, built from page labels.
///
/// A slide is a maximal run of consecutive pages sharing the same label:
/// the base page plus its overlays. Pages are 0-based, as are slide numbers.
pub struct SlideIndex {
    /// One entry per slide: (first page, last page), both inclusive.
    slides: Vec<(u16, u16)>,
}

impl SlideIndex {
    /// Builds the index from the document's page labels, in page order.
    pub fn from_labels(labels: &[String]) -> SlideIndex {
        let mut slides: Vec<(u16, u16)> = Vec::new();
        for (page, label) in labels.iter().enumerate() {
            match slides.last_mut() {
                // Same label as the previous page: extend the current slide.
                Some((_, last)) if &labels[*last as usize] == label => *last = page as u16,
                // First page, or a new label: start a new slide.
                _ => slides.push((page as u16, page as u16)),
            }
        }
        SlideIndex { slides }
    }

    /// Number of logical slides.
    pub fn slide_count(&self) -> usize {
        self.slides.len()
    }

    /// First (0-based) page of the given slide.
    pub fn first_page(&self, slide: usize) -> u16 {
        self.slides[slide].0
    }

    /// Last (0-based) page of the given slide (its last overlay, if any).
    pub fn last_page(&self, slide: usize) -> u16 {
        self.slides[slide].1
    }

    /// Slide containing the given (0-based) page.
    pub fn slide_of_page(&self, page: u16) -> usize {
        self.slides
            .binary_search_by(|&(first, last)| {
                if page < first {
                    std::cmp::Ordering::Greater
                } else if page > last {
                    std::cmp::Ordering::Less
                } else {
                    std::cmp::Ordering::Equal
                }
            })
            .expect("page out of range for SlideIndex")
    }

    /// The slide after `slide`, if any.
    pub fn next_slide(&self, slide: usize) -> Option<usize> {
        (slide + 1 < self.slides.len()).then_some(slide + 1)
    }

    /// The slide before `slide`, if any.
    pub fn prev_slide(&self, slide: usize) -> Option<usize> {
        slide.checked_sub(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn consecutive_equal_labels_group_into_one_slide() {
        let labels = ["1", "1", "1", "2", "3", "3"]; // frame 1 has 2 overlays
        let s = SlideIndex::from_labels(&labels.map(String::from));
        assert_eq!(s.slide_count(), 3);
        assert_eq!(s.first_page(0), 0);
        assert_eq!(s.last_page(0), 2);
        assert_eq!(s.first_page(1), 3);
        // Brief typo correction: with labels ["1","1","1","2","3","3"] the
        // slides are {0-2},{3},{4-5}, so page 4 is in slide 2 (the brief's
        // `1` would require the duplicate label on "2", not "3").
        assert_eq!(s.slide_of_page(4), 2);
        assert_eq!(s.next_slide(2), None);
        assert_eq!(s.prev_slide(1), Some(0));
    }

    #[test]
    fn all_distinct_labels_means_one_page_per_slide() {
        let labels = ["1", "2", "3"];
        let s = SlideIndex::from_labels(&labels.map(String::from));
        assert_eq!(s.slide_count(), 3);
        assert_eq!(s.last_page(0), 0);
    }
}
