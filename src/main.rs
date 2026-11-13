use std::error::Error;

use clap::Parser;

mod app;
mod cli;
mod input;
mod pdf;
mod reload;
mod render;
mod run;
mod state;
mod timer;
mod ui;

fn main() {
    if let Err(e) = actual_main() {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

fn actual_main() -> Result<(), Box<dyn Error>> {
    let args = cli::Args::parse();
    run::run(args)
}
