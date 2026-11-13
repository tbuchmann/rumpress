use std::error::Error;

use clap::Parser;

mod cli;
mod pdf;
mod render;
mod run;

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
