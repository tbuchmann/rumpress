//! Centralized keyboard handling: key → [`Action`] mapping and applying.
//!
//! Both egui viewports call the same [`map_key`]/[`apply`] pair on their key
//! events; this is the single place where keybindings are defined.

use std::time::Instant;

use crate::state::DeckState;
use crate::timer::Timer;
use crate::ui::annotate::Tool;

/// A presenter command, produced by [`map_key`] and executed by [`apply`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    NextPage,
    PrevPage,
    NextSlide,
    PrevSlide,
    ToggleBlank,
    ToggleOverview,
    /// Selects an annotation tool; selecting the active tool again turns
    /// annotating off. Applied in the app layer.
    SelectTool(Tool),
    /// Next color for the active tool. Applied in the app layer.
    CycleColor,
    /// Next stroke width. Applied in the app layer.
    CycleWidth,
    /// Removes the latest annotation on the current page.
    Undo,
    /// Removes all annotations on the current page.
    ClearAnnotations,
    ToggleHelp,
    ResetTimer,
    /// A digit 0-9: appends to the "go to slide" number being typed.
    /// Applied in the app layer.
    Digit(u8),
    /// Backspace: deletes the last typed digit. Applied in the app layer.
    Backspace,
    /// Enter: jumps to the typed slide number. Applied in the app layer.
    Confirm,
    /// Esc: closes whichever overlay mode is active (overview, highlight
    /// pen, help); a no-op in normal mode. Applied in the app layer.
    Escape,
    Quit,
    /// Key not bound to anything.
    None,
}

/// Maps a key press to its [`Action`]. Modifier keys are ignored: the
/// bindings are plain keys, so any modifier combination maps the same way.
///
/// Bindings: Right/Space/PageDown = NextPage, Left/PageUp = PrevPage,
/// Down = NextSlide, Up = PrevSlide, `b` = ToggleBlank, `g` = ToggleOverview,
/// `?` = ToggleHelp, `t` = ResetTimer, `q` = Quit,
/// Esc = Escape (closes the active overlay or tool; no-op in normal mode),
/// annotation tools `l` laser, `p` pen, `h` marker, `f` area, `n` line,
/// `a` arrow, `r` box, `o` oval, `e` eraser; `c` color, `w` width,
/// `u` / Ctrl+Z undo, `x` clear,
/// `0`-`9` + Enter = go to slide (Backspace edits the number, Esc cancels).
pub fn map_key(key: egui::Key, modifiers: egui::Modifiers) -> Action {
    match key {
        egui::Key::Z if modifiers.command => Action::Undo,
        egui::Key::ArrowRight | egui::Key::Space | egui::Key::PageDown => Action::NextPage,
        egui::Key::ArrowLeft | egui::Key::PageUp => Action::PrevPage,
        egui::Key::ArrowDown => Action::NextSlide,
        egui::Key::ArrowUp => Action::PrevSlide,
        egui::Key::B => Action::ToggleBlank,
        egui::Key::G => Action::ToggleOverview,
        egui::Key::L => Action::SelectTool(Tool::Laser),
        egui::Key::P => Action::SelectTool(Tool::Pen),
        egui::Key::H => Action::SelectTool(Tool::Highlighter),
        egui::Key::F => Action::SelectTool(Tool::Area),
        egui::Key::N => Action::SelectTool(Tool::Line),
        egui::Key::A => Action::SelectTool(Tool::Arrow),
        egui::Key::R => Action::SelectTool(Tool::Rect),
        egui::Key::O => Action::SelectTool(Tool::Ellipse),
        egui::Key::E => Action::SelectTool(Tool::Eraser),
        egui::Key::C => Action::CycleColor,
        egui::Key::W => Action::CycleWidth,
        egui::Key::U => Action::Undo,
        egui::Key::X => Action::ClearAnnotations,
        egui::Key::Questionmark => Action::ToggleHelp,
        egui::Key::T => Action::ResetTimer,
        egui::Key::Q => Action::Quit,
        egui::Key::Escape => Action::Escape,
        egui::Key::Enter => Action::Confirm,
        egui::Key::Backspace => Action::Backspace,
        egui::Key::Num0 => Action::Digit(0),
        egui::Key::Num1 => Action::Digit(1),
        egui::Key::Num2 => Action::Digit(2),
        egui::Key::Num3 => Action::Digit(3),
        egui::Key::Num4 => Action::Digit(4),
        egui::Key::Num5 => Action::Digit(5),
        egui::Key::Num6 => Action::Digit(6),
        egui::Key::Num7 => Action::Digit(7),
        egui::Key::Num8 => Action::Digit(8),
        egui::Key::Num9 => Action::Digit(9),
        _ => Action::None,
    }
}

/// Executes `action` against the presentation state, returning `true` if the
/// app should quit.
///
/// [`Action::ToggleOverview`], [`Action::ToggleHighlight`],
/// [`Action::ToggleHelp`] and [`Action::Escape`] are mapped but not applied
/// here: their state lives in the app layer, so they return `false` and
/// leave the state unchanged.
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
        // Overview/highlight/help/escape state lives in the app layer; these
        // are applied there and do not quit.
        Action::ToggleOverview
        | Action::SelectTool(_)
        | Action::CycleColor
        | Action::CycleWidth
        | Action::Undo
        | Action::ClearAnnotations
        | Action::ToggleHelp
        | Action::Escape
        | Action::Digit(_)
        | Action::Backspace
        | Action::Confirm => {}
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
        assert_eq!(
            map_key(egui::Key::H, m),
            Action::SelectTool(Tool::Highlighter)
        );
        assert_eq!(map_key(egui::Key::Questionmark, m), Action::ToggleHelp);
        assert_eq!(map_key(egui::Key::T, m), Action::ResetTimer);
        assert_eq!(map_key(egui::Key::Q, m), Action::Quit);
        assert_eq!(map_key(egui::Key::J, m), Action::None);
        assert_eq!(map_key(egui::Key::Enter, m), Action::Confirm);
        assert_eq!(map_key(egui::Key::Backspace, m), Action::Backspace);
        assert_eq!(map_key(egui::Key::Num0, m), Action::Digit(0));
        assert_eq!(map_key(egui::Key::Num7, m), Action::Digit(7));
        assert_eq!(map_key(egui::Key::Num9, m), Action::Digit(9));
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
        assert!(!apply(&mut s, Action::SelectTool(Tool::Pen), &mut t));
        assert!(!apply(&mut s, Action::ToggleHelp, &mut t));
        assert_eq!((s.current_page, s.current_slide, s.blanked), (0, 0, false));
    }

    #[test]
    fn escape_maps_to_escape_and_does_not_quit() {
        // Esc maps to Action::Escape, which the app layer applies to close
        // the active overlay; in `apply` it is a no-op that never quits.
        assert_eq!(
            map_key(egui::Key::Escape, Default::default()),
            Action::Escape
        );
        let mut s = DeckState::new(SlideIndex::from_labels(&["1", "2"].map(String::from)));
        let mut t = Timer::new(None);
        assert!(!apply(&mut s, Action::Escape, &mut t));
        assert_eq!((s.current_page, s.current_slide, s.blanked), (0, 0, false));
    }

    #[test]
    fn apply_none_is_a_no_op() {
        let mut s = DeckState::new(SlideIndex::from_labels(&["1", "2"].map(String::from)));
        let mut t = Timer::new(None);
        assert!(!apply(&mut s, Action::None, &mut t));
        assert_eq!((s.current_page, s.current_slide, s.blanked), (0, 0, false));
    }

    #[test]
    fn every_tool_has_a_key_that_maps_back_to_it() {
        let keys = [
            (egui::Key::L, Tool::Laser),
            (egui::Key::P, Tool::Pen),
            (egui::Key::H, Tool::Highlighter),
            (egui::Key::F, Tool::Area),
            (egui::Key::N, Tool::Line),
            (egui::Key::A, Tool::Arrow),
            (egui::Key::R, Tool::Rect),
            (egui::Key::O, Tool::Ellipse),
            (egui::Key::E, Tool::Eraser),
        ];
        assert_eq!(keys.len(), Tool::ALL.len());
        for (key, tool) in keys {
            assert_eq!(map_key(key, Default::default()), Action::SelectTool(tool));
            assert_eq!(
                key.name().chars().next(),
                Some(tool.key_hint()),
                "hint for {tool:?} does not match its key"
            );
        }
    }

    #[test]
    fn annotation_edit_keys() {
        let m = egui::Modifiers::default();
        assert_eq!(map_key(egui::Key::C, m), Action::CycleColor);
        assert_eq!(map_key(egui::Key::W, m), Action::CycleWidth);
        assert_eq!(map_key(egui::Key::U, m), Action::Undo);
        assert_eq!(map_key(egui::Key::X, m), Action::ClearAnnotations);
        // Ctrl+Z undoes too; a bare Z does nothing.
        assert_eq!(
            map_key(egui::Key::Z, egui::Modifiers::COMMAND),
            Action::Undo
        );
        assert_eq!(map_key(egui::Key::Z, m), Action::None);
    }
}
