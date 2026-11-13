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
    env_logger::init();

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
    let (_present_monitor, _presenter_monitor, show_beamer) = match monitors::list() {
        Ok(list) => {
            let (present, presenter) = monitors::assign(&args, &list);
            if let Some(m) = &present {
                monitors::set_monitor_rule(m, "rumpress-beamer");
            }
            if let Some(m) = &presenter {
                monitors::set_monitor_rule(m, "rumpress-presenter");
            }
            let show_beamer = present.is_some();
            if show_beamer {
                log::info!("monitors assigned: beamer on {present:?}, presenter on {presenter:?}");
            } else {
                log::info!("single monitor: presenter opens windowed, no separate beamer window");
            }
            (present, presenter, show_beamer)
        }
        Err(e) => {
            log::warn!(
                "monitor detection failed ({e}); monitor placement unavailable — \
                 both windows open windowed at window-manager defaults; a static \
                 window rule in hyprland.conf can still pin rumpress-beamer"
            );
            (None, None, true)
        }
    };

    let app = RumpressApp::new(
        render,
        state,
        info,
        timer,
        args.swap_halves,
        args.pdf,
        show_beamer,
    );

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("rumpress-presenter")
            .with_app_id("rumpress-presenter"),
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
