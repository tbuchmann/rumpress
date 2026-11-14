//! Application entry point: wires the render thread, state, and eframe.

use std::error::Error;
use std::time::{Duration, Instant};

use crate::app::RumpressApp;
use crate::cli::Args;
use crate::monitors;
use crate::pdf::SlideIndex;
use crate::render::RenderHandle;
use crate::state::DeckState;
use crate::timer::Timer;

/// Runs the presenter application until the user quits.
pub fn run(args: Args) -> Result<(), Box<dyn Error>> {
    // Logging is initialized in `main` (default filter `info`).

    // Fail fast with a clear message before any window or render thread
    // exists: a missing or unreadable file would otherwise only surface as
    // a vague "render thread did not report document info" after a 5 s wait.
    check_pdf_readable(&args.pdf)?;

    let render = RenderHandle::spawn(args.pdf.clone());

    // The render thread emits DocumentInfo right after opening; wait for it
    // so the app starts with a valid slide index instead of an empty one.
    let info = wait_for_document_info(&render)
        .ok_or("render thread did not report document info (failed to open the PDF?)")?;
    let state = DeckState::new(SlideIndex::from_labels(&info.labels));
    let timer = Timer::new(args.time.map(|m| Duration::from_secs(m * 60)));

    // Resolve monitor placement before any window exists: ask Hyprland which
    // outputs are available, decide where each window goes, and write window
    // rules so Hyprland maps the windows onto those monitors. Three states:
    // (a) monitors detected and assigned — window rules + fullscreen commands
    // as usual; (b) a single monitor — presenter only, windowed, no beamer
    // window; (c) detection failed — both windows still open, windowed at
    // window-manager defaults, so a static rule in hyprland.conf can pin the
    // beamer window.
    let placement = match monitors::list() {
        Ok(list) => {
            let (present, presenter) = monitors::assign(&args, &list);
            if let Some(m) = &present {
                monitors::set_monitor_rule(m, "rumpress-beamer");
            }
            if let Some(m) = &presenter {
                monitors::set_monitor_rule(m, "rumpress-presenter");
            }
            if present.is_some() {
                log::info!("monitors assigned: beamer on {present:?}, presenter on {presenter:?}");
                monitors::Placement::assigned()
            } else {
                log::warn!("single monitor: presenter opens windowed, no separate beamer window");
                monitors::Placement::single_monitor()
            }
        }
        Err(e) => {
            log::warn!(
                "monitor detection failed ({e}); monitor placement unavailable — \
                 both windows open windowed at window-manager defaults; a static \
                 window rule in hyprland.conf can still pin rumpress-beamer"
            );
            monitors::Placement::detection_failed()
        }
    };

    let app = RumpressApp::new(
        render,
        state,
        info,
        timer,
        args.swap_halves,
        args.pdf,
        placement,
    );

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("rumpress-presenter")
            .with_app_id("rumpress-presenter")
            .with_inner_size([1600.0, 900.0])
            .with_min_inner_size([800.0, 450.0]),
        ..Default::default()
    };
    eframe::run_native("rumpress", options, Box::new(|_cc| Ok(Box::new(app))))
        .map_err(|e| -> Box<dyn Error> { format!("eframe failed: {e}").into() })?;
    Ok(())
}

/// Polls for the initial [`crate::render::DocumentInfo`] for up to ~5 s.
fn wait_for_document_info(render: &RenderHandle) -> Option<crate::render::DocumentInfo> {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if let Some(info) = render.take_document_info() {
            return Some(info);
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    None
}

/// Verifies the PDF path exists and is a readable file, returning a
/// user-facing error otherwise. Called before the render thread starts so a
/// bad path fails immediately with a message naming the path.
fn check_pdf_readable(path: &std::path::Path) -> Result<(), Box<dyn Error>> {
    match std::fs::metadata(path) {
        Ok(meta) if meta.is_file() => Ok(()),
        Ok(meta) if meta.is_dir() => {
            Err(format!("'{}' is a directory, not a PDF file", path.display()).into())
        }
        Ok(_) => Err(format!("'{}' is not a regular file", path.display()).into()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            Err(format!("file not found: '{}'", path.display()).into())
        }
        Err(e) => Err(format!("cannot read '{}': {e}", path.display()).into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_file_gives_clear_error() {
        let err = check_pdf_readable(std::path::Path::new("no/such/deck.pdf")).unwrap_err();
        assert!(err.to_string().contains("file not found"), "msg: {err}");
        assert!(err.to_string().contains("no/such/deck.pdf"), "msg: {err}");
    }

    #[test]
    fn directory_gives_clear_error() {
        let err = check_pdf_readable(std::path::Path::new("tests/fixtures")).unwrap_err();
        assert!(err.to_string().contains("is a directory"), "msg: {err}");
    }

    #[test]
    fn existing_file_passes() {
        check_pdf_readable(std::path::Path::new("tests/fixtures/two-page.pdf")).unwrap();
    }
}
