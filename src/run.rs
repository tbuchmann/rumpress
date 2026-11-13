//! Application entry point: wires the render thread, state, and eframe.

use std::error::Error;
use std::time::{Duration, Instant};

use crate::app::RumpressApp;
use crate::cli::Args;
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

    let app = RumpressApp::new(render, state, info, timer, args.swap_halves, args.pdf);

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_title("rumpress-presenter"),
        ..Default::default()
    };
    eframe::run_native(
        "rumpress",
        options,
        Box::new(|_cc| Ok(Box::new(app))),
    )
    .map_err(|e| -> Box<dyn Error> { format!("eframe failed: {e}").into() })?;
    Ok(())
}

/// Polls for the initial [`crate::render::DocumentInfo`] for up to ~5 s.
fn wait_for_document_info(
    render: &RenderHandle,
) -> Option<crate::render::DocumentInfo> {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if let Some(info) = render.take_document_info() {
            return Some(info);
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    None
}
