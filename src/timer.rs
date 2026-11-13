//! Presentation timer with a proportional pace estimate.

// Consumed by later tasks (UI overlay / run loop); unused until wired up.
#![allow(dead_code)]

use std::time::{Duration, Instant};

/// Tracks elapsed time since the presentation started, against an optional target duration.
pub struct Timer {
    start: Option<Instant>,
    target: Option<Duration>,
}

impl Timer {
    pub fn new(target: Option<Duration>) -> Timer {
        Timer {
            start: None,
            target,
        }
    }

    /// Starts the clock at `at`. Idempotent: the first call wins.
    pub fn start(&mut self, at: Instant) {
        if self.start.is_none() {
            self.start = Some(at);
        }
    }

    /// Restarts the clock at `at`, discarding any previous start.
    pub fn reset(&mut self, at: Instant) {
        self.start = Some(at);
    }

    pub fn elapsed(&self, now: Instant) -> Duration {
        match self.start {
            Some(start) => now.saturating_duration_since(start),
            None => Duration::ZERO,
        }
    }

    pub fn target(&self) -> Option<Duration> {
        self.target
    }
}

/// How the current pace compares to the proportional target for slide `slide`.
pub enum Pace {
    /// Elapsed is more than 30 s under the expected time; ahead by this much.
    Ahead(Duration),
    /// Elapsed is within 30 s of the expected time.
    OnTrack,
    /// Elapsed is more than 30 s over the expected time; behind by this much.
    Behind(Duration),
}

/// Tolerance for [`Pace::OnTrack`].
const PACE_TOLERANCE: Duration = Duration::from_secs(30);

/// Compares elapsed time at `now` against the expected time for slide `slide`
/// of `slide_count` (expected = target × slide / slide_count). Both are
/// quantized to whole seconds so callers get exact, stable `Duration`s.
pub fn pace(t: &Timer, now: Instant, slide: usize, slide_count: usize) -> Pace {
    let Some(target) = t.target() else {
        return Pace::OnTrack;
    };
    if slide_count == 0 {
        return Pace::OnTrack;
    }
    let expected = Duration::from_secs((target * slide as u32 / slide_count as u32).as_secs());
    let elapsed = Duration::from_secs(t.elapsed(now).as_secs());
    if elapsed > expected + PACE_TOLERANCE {
        Pace::Behind(elapsed - expected)
    } else if expected > elapsed + PACE_TOLERANCE {
        Pace::Ahead(expected - elapsed)
    } else {
        Pace::OnTrack
    }
}

#[cfg(test)]
mod tests {
    use super::{Pace, Timer, pace};
    use std::time::{Duration, Instant};

    #[test]
    fn elapsed_starts_at_first_start_call() {
        let mut t = Timer::new(None);
        let t0 = Instant::now();
        t.start(t0);
        t.start(t0 + Duration::from_secs(10)); // ignored
        assert_eq!(
            t.elapsed(t0 + Duration::from_secs(5)),
            Duration::from_secs(5)
        );
    }

    #[test]
    fn pace_compares_elapsed_to_proportional_target() {
        let mut t = Timer::new(Some(Duration::from_secs(600))); // 10 min, 5 slides
        t.start(Instant::now());
        let now = Instant::now();
        // 2 min in, at slide 1 of 5: expected 2 min -> on track
        assert!(matches!(
            pace(&t, now + Duration::from_secs(120), 1, 5),
            Pace::OnTrack
        ));
        // 2 min in, at slide 3 of 5: expected 6 min -> ahead by 4 min
        assert!(
            matches!(pace(&t, now + Duration::from_secs(120), 3, 5), Pace::Ahead(d) if d == Duration::from_secs(240))
        );
        // 6 min in, at slide 1 of 5: behind by 4 min
        assert!(
            matches!(pace(&t, now + Duration::from_secs(360), 1, 5), Pace::Behind(d) if d == Duration::from_secs(240))
        );
    }

    #[test]
    fn reset_restarts_the_clock() {
        let mut t = Timer::new(None);
        let t0 = Instant::now();
        t.start(t0);
        t.reset(t0 + Duration::from_secs(100));
        assert_eq!(
            t.elapsed(t0 + Duration::from_secs(105)),
            Duration::from_secs(5)
        );
    }

    #[test]
    fn elapsed_before_start_is_zero() {
        let t = Timer::new(None);
        let t0 = Instant::now();
        assert_eq!(t.elapsed(t0 + Duration::from_secs(5)), Duration::ZERO);
    }

    #[test]
    fn target_round_trips() {
        let t = Timer::new(Some(Duration::from_secs(600)));
        assert_eq!(t.target(), Some(Duration::from_secs(600)));
        assert_eq!(Timer::new(None).target(), None);
    }

    #[test]
    fn pace_without_target_is_on_track() {
        let mut t = Timer::new(None);
        t.start(Instant::now());
        let now = Instant::now();
        assert!(matches!(
            pace(&t, now + Duration::from_secs(9999), 3, 5),
            Pace::OnTrack
        ));
    }

    #[test]
    fn pace_tolerance_is_thirty_seconds() {
        let mut t = Timer::new(Some(Duration::from_secs(600)));
        t.start(Instant::now());
        let now = Instant::now();
        // Slide 2 of 5: expected 4 min. 4 min + 30 s exactly -> on track.
        assert!(matches!(
            pace(&t, now + Duration::from_secs(240 + 30), 2, 5),
            Pace::OnTrack
        ));
        // 31 s over -> behind by 31 s.
        assert!(
            matches!(pace(&t, now + Duration::from_secs(240 + 31), 2, 5), Pace::Behind(d) if d == Duration::from_secs(31))
        );
        // 31 s under -> ahead by 31 s.
        assert!(
            matches!(pace(&t, now + Duration::from_secs(240 - 31), 2, 5), Pace::Ahead(d) if d == Duration::from_secs(31))
        );
    }

    #[test]
    fn pace_at_slide_zero_is_on_track_early() {
        let mut t = Timer::new(Some(Duration::from_secs(600)));
        t.start(Instant::now());
        let now = Instant::now();
        // Slide 0: expected 0; 10 s in is within tolerance.
        assert!(matches!(
            pace(&t, now + Duration::from_secs(10), 0, 5),
            Pace::OnTrack
        ));
        // 60 s in at slide 0 -> behind by 60 s.
        assert!(
            matches!(pace(&t, now + Duration::from_secs(60), 0, 5), Pace::Behind(d) if d == Duration::from_secs(60))
        );
    }
}
