use std::error::Error;

use clap::Parser;
use env_logger::Env;

mod app;
mod cli;
mod input;
mod layout;
mod monitors;
mod pdf;
mod reload;
mod render;
mod run;
mod state;
mod timer;
mod ui;

fn main() {
    // Default to `info` so the fallback announcements (monitor detection
    // failure, live-reload problems) are visible without RUST_LOG being set.
    env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();
    if let Err(e) = actual_main() {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

fn actual_main() -> Result<(), Box<dyn Error>> {
    let args = cli::Args::parse();
    run::run(args)
}
