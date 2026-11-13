//! Centralized keyboard handling: key → [`Action`] mapping and applying.
//!
//! Both egui viewports call the same [`map_key`]/[`apply`] pair on their key
//! events; this is the single place where keybindings are defined.
//!
//! Consumed by the presenter UI (later tasks); dead-code is expected until
//! then.
#![allow(dead_code)]

use std::time::Instant;

use crate::state::DeckState;
use crate::timer::Timer;

/// A presenter command, produced by [`map_key`] and executed by [`apply`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    NextPage,
    PrevPage,
    NextSlide,
    PrevSlide,
    ToggleBlank,
    ToggleOverview,
    ToggleHighlight,
    ToggleHelp,
    ResetTimer,
    Quit,
    /// Key not bound to anything.
    None,
}

/// Maps a key press to its [`Action`]. Modifier keys are ignored: the
/// bindings are plain keys, so any modifier combination maps the same way.
///
/// Bindings: Right/Space/PageDown = NextPage, Left/PageUp = PrevPage,
/// Down = NextSlide, Up = PrevSlide, `b` = ToggleBlank, `g` = ToggleOverview,
/// `h` = ToggleHighlight, `?` = ToggleHelp, `t` = ResetTimer, `q` = Quit.
pub fn map_key(key: egui::Key, _modifiers: egui::Modifiers) -> Action {
    match key {
        egui::Key::ArrowRight | egui::Key::Space | egui::Key::PageDown => Action::NextPage,
        egui::Key::ArrowLeft | egui::Key::PageUp => Action::PrevPage,
        egui::Key::ArrowDown => Action::NextSlide,
        egui::Key::ArrowUp => Action::PrevSlide,
        egui::Key::B => Action::ToggleBlank,
        egui::Key::G => Action::ToggleOverview,
        egui::Key::H => Action::ToggleHighlight,
        egui::Key::Questionmark => Action::ToggleHelp,
        egui::Key::T => Action::ResetTimer,
        egui::Key::Q => Action::Quit,
        _ => Action::None,
    }
}

/// Executes `action` against the presentation state, returning `true` if the
/// app should quit.
///
/// [`Action::ToggleOverview`], [`Action::ToggleHighlight`] and
/// [`Action::ToggleHelp`] are mapped but not applied here: their state lives
/// in the app layer, so they return `false` and leave the state unchanged.
pub fn apply(state: &mut DeckState, action: Action, timer: &mut Timer) -> bool {
    match action {
        Action::NextPage => state.next_page(),
        Action::PrevPage => state.prev_page(),
        Action::NextSlide => state.next_slide(),
        Action::PrevSlide => state.prev_slide(),
        Action::ToggleBlank => {
            state.toggle_blank();
        }
        Action::ResetTimer => timer.reset(Instant::now()),
        // Overview/highlight/help state lives in the app layer (later
        // tasks); until wired up these are no-ops that do not quit.
        Action::ToggleOverview | Action::ToggleHighlight | Action::ToggleHelp => {}
        Action::Quit => return true,
        Action::None => {}
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pdf::SlideIndex;
    use std::time::Duration;

    #[test]
    fn arrow_keys_map_to_page_steps() {
        assert_eq!(
            map_key(egui::Key::ArrowRight, Default::default()),
            Action::NextPage
        );
        assert_eq!(
            map_key(egui::Key::Space, Default::default()),
            Action::NextPage
        );
        assert_eq!(
            map_key(egui::Key::ArrowDown, Default::default()),
            Action::NextSlide
        );
        assert_eq!(
            map_key(egui::Key::B, Default::default()),
            Action::ToggleBlank
        );
    }

    #[test]
    fn apply_executes_state_changes() {
        let mut s = DeckState::new(SlideIndex::from_labels(&["1", "2"].map(String::from)));
        apply(&mut s, Action::NextPage, &mut Timer::new(None));
        assert_eq!(s.current_page, 1);
        assert!(!apply(&mut s, Action::ToggleBlank, &mut Timer::new(None)));
        assert!(s.blanked);
    }

    #[test]
    fn all_documented_keys_map_to_their_actions() {
        let m = Default::default();
        assert_eq!(map_key(egui::Key::PageDown, m), Action::NextPage);
        assert_eq!(map_key(egui::Key::ArrowLeft, m), Action::PrevPage);
        assert_eq!(map_key(egui::Key::PageUp, m), Action::PrevPage);
        assert_eq!(map_key(egui::Key::ArrowUp, m), Action::PrevSlide);
        assert_eq!(map_key(egui::Key::G, m), Action::ToggleOverview);
        assert_eq!(map_key(egui::Key::H, m), Action::ToggleHighlight);
        assert_eq!(map_key(egui::Key::Questionmark, m), Action::ToggleHelp);
        assert_eq!(map_key(egui::Key::T, m), Action::ResetTimer);
        assert_eq!(map_key(egui::Key::Q, m), Action::Quit);
        assert_eq!(map_key(egui::Key::A, m), Action::None);
        assert_eq!(map_key(egui::Key::Enter, m), Action::None);
    }

    #[test]
    fn apply_quit_returns_true_and_changes_nothing() {
        let mut s = DeckState::new(SlideIndex::from_labels(&["1", "2"].map(String::from)));
        let mut t = Timer::new(None);
        assert!(apply(&mut s, Action::Quit, &mut t));
        assert_eq!((s.current_page, s.current_slide, s.blanked), (0, 0, false));
    }

    #[test]
    fn apply_reset_timer_restarts_the_clock() {
        let mut s = DeckState::new(SlideIndex::from_labels(&["1", "2"].map(String::from)));
        let mut t = Timer::new(None);
        let t0 = Instant::now();
        t.start(t0 - Duration::from_secs(50)); // pretend the talk started 50 s ago
        assert_eq!(t.elapsed(t0), Duration::from_secs(50));
        assert!(!apply(&mut s, Action::ResetTimer, &mut t));
        // Reset happened "now", so the elapsed time collapsed to ~zero.
        assert!(t.elapsed(Instant::now()) < Duration::from_secs(1));
    }

    #[test]
    fn apply_overview_highlight_and_help_are_no_ops_until_wired_up() {
        let mut s = DeckState::new(SlideIndex::from_labels(&["1", "2"].map(String::from)));
        let mut t = Timer::new(None);
        assert!(!apply(&mut s, Action::ToggleOverview, &mut t));
        assert!(!apply(&mut s, Action::ToggleHighlight, &mut t));
        assert!(!apply(&mut s, Action::ToggleHelp, &mut t));
        assert_eq!((s.current_page, s.current_slide, s.blanked), (0, 0, false));
    }

    #[test]
    fn apply_none_is_a_no_op() {
        let mut s = DeckState::new(SlideIndex::from_labels(&["1", "2"].map(String::from)));
        let mut t = Timer::new(None);
        assert!(!apply(&mut s, Action::None, &mut t));
        assert_eq!((s.current_page, s.current_slide, s.blanked), (0, 0, false));
    }
}
