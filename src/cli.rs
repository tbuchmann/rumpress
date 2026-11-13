use std::path::PathBuf;

use clap::Parser;

/// Dual-screen PDF presenter: slides on the beamer, notes and timer on the presenter.
#[derive(Parser, Debug)]
#[command(name = "rumpress", version, about)]
pub struct Args {
    /// Path to the PDF deck.
    pub pdf: PathBuf,
    /// Target presentation time in minutes.
    #[arg(long)]
    pub time: Option<u64>,
    /// Monitor index for the presentation (beamer) window.
    #[arg(long = "present-monitor")]
    pub present_monitor: Option<usize>,
    /// Monitor index for the presenter window.
    #[arg(long = "presenter-monitor")]
    pub presenter_monitor: Option<usize>,
    /// Swap slide and notes halves of 2x1 pages.
    #[arg(long)]
    pub swap_halves: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_minimal_args() {
        let a = Args::parse_from(["rumpress", "deck.pdf"]);
        assert_eq!(a.pdf, PathBuf::from("deck.pdf"));
        assert_eq!(a.time, None);
        assert!(!a.swap_halves);
    }

    #[test]
    fn parses_time_and_monitors() {
        let a = Args::parse_from(["rumpress", "deck.pdf", "--time", "45",
            "--present-monitor", "1", "--presenter-monitor", "0", "--swap-halves"]);
        assert_eq!(a.time, Some(45));
        assert_eq!(a.present_monitor, Some(1));
        assert_eq!(a.presenter_monitor, Some(0));
        assert!(a.swap_halves);
    }
}
