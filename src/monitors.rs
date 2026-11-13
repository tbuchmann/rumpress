//! Monitor discovery and assignment via Hyprland IPC.
//!
//! `hyprctl -j monitors` reports the connected outputs as JSON; we parse it
//! into [`Monitor`] values and decide which monitor hosts the presentation
//! (beamer) window and which hosts the presenter window. Placement itself is
//! done with Hyprland window rules (`hyprctl keyword windowrulev2 ...`)
//! targeting the window class, which egui sets from the viewport app id.

use crate::cli::Args;

/// One connected monitor as reported by `hyprctl -j monitors`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Monitor {
    /// Connector name, e.g. `DP-1` or `eDP-1`.
    pub name: String,
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Whether this is the currently focused (primary) monitor.
    pub primary: bool,
}

/// Parses the JSON output of `hyprctl -j monitors`.
///
/// Hyprland reports the focused output as `"focused": true`; that becomes
/// [`Monitor::primary`]. Entries missing `name` get a placeholder name.
pub fn parse_monitors(json: &str) -> Result<Vec<Monitor>, serde_json::Error> {
    let raw: Vec<serde_json::Value> = serde_json::from_str(json)?;
    raw.into_iter()
        .map(|m| {
            Ok(Monitor {
                name: m["name"]
                    .as_str()
                    .map(str::to_owned)
                    .unwrap_or_else(|| String::from("<unnamed>")),
                width: m["width"].as_u64().unwrap_or(0) as u32,
                height: m["height"].as_u64().unwrap_or(0) as u32,
                primary: m["focused"].as_bool().unwrap_or(false),
            })
        })
        .collect()
}

/// Runs `hyprctl -j monitors` and parses its output.
///
/// Returns an error when `hyprctl` is missing, fails, or reports unparsable
/// JSON — callers fall back to window-manager defaults in that case.
pub fn list() -> Result<Vec<Monitor>, Box<dyn std::error::Error>> {
    let out = std::process::Command::new("hyprctl")
        .args(["-j", "monitors"])
        .output()
        .map_err(|e| format!("could not run hyprctl: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "hyprctl -j monitors failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        )
        .into());
    }
    Ok(parse_monitors(std::str::from_utf8(&out.stdout)?)?)
}

/// Decides which monitor hosts the presentation and presenter windows.
///
/// Returns `(presentation, presenter)` monitor names. Explicit `--present-monitor` /
/// `--presenter-monitor` flags are 0-based indices into `monitors` and win over
/// the defaults. Default: presenter on the focused (primary) monitor — or the
/// largest when none is focused — and the presentation on the other one. With a
/// single monitor there is no second output: presentation is `None` and the
/// caller falls back to a windowed presenter.
pub fn assign(args: &Args, monitors: &[Monitor]) -> (Option<String>, Option<String>) {
    let name = |i: usize| monitors.get(i).map(|m| m.name.clone());

    let present = args.present_monitor.and_then(|i| {
        let m = name(i);
        if m.is_none() {
            log::warn!(
                "--present-monitor {i} is out of range ({} monitor(s) detected); ignoring it",
                monitors.len()
            );
        }
        m
    });
    let presenter = args.presenter_monitor.and_then(|i| {
        let m = name(i);
        if m.is_none() {
            log::warn!(
                "--presenter-monitor {i} is out of range ({} monitor(s) detected); ignoring it",
                monitors.len()
            );
        }
        m
    });

    if monitors.len() < 2 {
        // Single monitor (or none): no separate presentation output.
        return (
            None,
            presenter.or_else(|| monitors.first().map(|m| m.name.clone())),
        );
    }

    // Default presenter monitor: the focused one, else the largest.
    let default_presenter = monitors
        .iter()
        .find(|m| m.primary)
        .or_else(|| monitors.iter().max_by_key(|m| m.width * m.height))
        .expect("monitors is non-empty");
    let default_present = monitors
        .iter()
        .filter(|m| m.name != default_presenter.name)
        .max_by_key(|m| m.width * m.height)
        .expect("at least one other monitor");

    (
        present.or_else(|| Some(default_present.name.clone())),
        presenter.or_else(|| Some(default_presenter.name.clone())),
    )
}

/// The outcome of monitor placement, consumed by the app layer.
///
/// `show_beamer` decides whether the beamer viewport is created at all;
/// `fullscreen_beamer` decides whether the one-shot fullscreen command is
/// sent once that viewport exists. They differ only in the detection-failure
/// case: the beamer window still opens (windowed, so a static hyprland.conf
/// rule can pin it), but must not fullscreen over the presenter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Placement {
    /// Whether the beamer viewport is shown at all. False only in the
    /// single-monitor case.
    pub show_beamer: bool,
    /// Whether the beamer window may be fullscreened. True only when a
    /// presentation monitor was actually assigned.
    pub fullscreen_beamer: bool,
}

impl Placement {
    /// Monitors detected and assigned: beamer shown and fullscreened.
    pub fn assigned() -> Self {
        Self {
            show_beamer: true,
            fullscreen_beamer: true,
        }
    }

    /// Single monitor: no beamer window at all.
    pub fn single_monitor() -> Self {
        Self {
            show_beamer: false,
            fullscreen_beamer: false,
        }
    }

    /// Detection failed: both windows open windowed at window-manager
    /// defaults.
    pub fn detection_failed() -> Self {
        Self {
            show_beamer: true,
            fullscreen_beamer: false,
        }
    }
}

/// Writes a Hyprland window rule pinning windows of `class` to `monitor`.
///
/// Failures are not fatal: the documented fallback is static window rules in
/// `hyprland.conf`, so a missing `hyprctl` only costs automatic placement.
pub fn set_monitor_rule(monitor: &str, class: &str) {
    let rule = format!("monitor {monitor}, class:^({class})$");
    match std::process::Command::new("hyprctl")
        .args(["keyword", "windowrulev2", &rule])
        .output()
    {
        Ok(out) if out.status.success() => {
            log::info!("hyprctl windowrulev2: {rule}");
        }
        Ok(out) => {
            let stderr = String::from_utf8_lossy(&out.stderr);
            log::warn!("hyprctl windowrulev2 failed ({rule}): {}", stderr.trim());
        }
        Err(e) => {
            log::warn!("could not run hyprctl for windowrulev2 ({rule}): {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_hyprctl_monitors_json() {
        let json = r#"[{"name":"DP-1","width":3840,"height":2160,"focused":false},
                       {"name":"eDP-1","width":1920,"height":1080,"focused":true}]"#;
        let ms = parse_monitors(json).unwrap();
        assert_eq!(ms.len(), 2);
        assert_eq!(ms[0].name, "DP-1");
        assert_eq!(ms[1].width, 1920);
    }

    #[test]
    fn assign_defaults_presenter_to_focused_largest() {
        let ms = vec![
            Monitor {
                name: "DP-1".into(),
                width: 3840,
                height: 2160,
                primary: false,
            },
            Monitor {
                name: "eDP-1".into(),
                width: 1920,
                height: 1080,
                primary: true,
            },
        ];
        let (present, presenter) = assign(&Args::test_default(), &ms);
        assert_eq!(presenter.as_deref(), Some("eDP-1")); // primary
        assert_eq!(present.as_deref(), Some("DP-1"));
    }

    #[test]
    fn assign_single_monitor_falls_back() {
        let ms = vec![Monitor {
            name: "eDP-1".into(),
            width: 1920,
            height: 1080,
            primary: true,
        }];
        let (present, presenter) = assign(&Args::test_default(), &ms);
        assert_eq!(present, None);
        assert_eq!(presenter.as_deref(), Some("eDP-1"));
    }

    #[test]
    fn assign_explicit_flags_win_over_defaults() {
        let ms = vec![
            Monitor {
                name: "DP-1".into(),
                width: 3840,
                height: 2160,
                primary: false,
            },
            Monitor {
                name: "eDP-1".into(),
                width: 1920,
                height: 1080,
                primary: true,
            },
        ];
        let mut args = Args::test_default();
        args.present_monitor = Some(1);
        args.presenter_monitor = Some(0);
        let (present, presenter) = assign(&args, &ms);
        assert_eq!(present.as_deref(), Some("eDP-1"));
        assert_eq!(presenter.as_deref(), Some("DP-1"));
    }
}
