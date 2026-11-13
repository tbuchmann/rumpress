use std::error::Error;

use crate::cli::Args;

/// Entry point for the presenter application.
///
/// Stub for now: later tasks add the render thread and the dual-viewport
/// egui app here.
pub fn run(args: Args) -> Result<(), Box<dyn Error>> {
    let _ = args;
    Ok(())
}
