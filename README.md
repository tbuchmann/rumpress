# rumpress

A dual-screen PDF presenter for Hyprland: slides on the beamer, notes, timer
and overview on your laptop screen. It reads 2×1 "notes" pages (slide on one
half, notes on the other) and splits them across the two screens; plain
single-slide PDFs work too.

## Building

```sh
cargo build --release
```

The binary lands in `target/release/rumpress`.

## Pdfium setup

rumpress renders PDFs through [Pdfium](https://pdfium.googlesource.com/pdfium/)
via the `pdfium-render` crate, which needs `libpdfium.so` at runtime. Either:

- run `scripts/fetch-pdfium.sh` — downloads the Linux x64 build into
  `vendor/pdfium/libpdfium.so`, where rumpress looks by default; or
- install a distribution package (e.g. AUR `pdfium` / `libpdfium`) that puts
  `libpdfium.so` on the loader path.

To point at a specific library, set:

```sh
export RUMPRESS_PDFIUM_PATH=/path/to/libpdfium.so
```

## Usage

```sh
rumpress deck.pdf
rumpress deck.pdf --time 45 --swap-halves
rumpress deck.pdf --present-monitor 1 --presenter-monitor 0
```

- `--time MINUTES` sets a target talk time; the presenter shows a progress
  bar and a pace indicator (on track / ahead / behind).
- `--swap-halves` swaps which half of a 2×1 page is the slide and which is
  the notes.
- `--present-monitor` / `--presenter-monitor` choose the outputs (Hyprland
  monitor indices). Without them rumpress picks automatically.

## Monitor placement

On Hyprland, rumpress asks `hyprctl` for the connected outputs and writes
window rules so `rumpress-beamer` lands on the presentation monitor and
`rumpress-presenter` on yours, then fullscreens the beamer.

If Hyprland detection fails (other compositors, `hyprctl` missing), both
windows open windowed at window-manager defaults. You can still pin them
statically with window rules in `hyprland.conf`. On Hyprland ≥ 0.53 (current
syntax, one property per rule):

```ini
windowrule = monitor 1, match:class ^(rumpress-beamer)$
windowrule = fullscreen on, match:class ^(rumpress-beamer)$
windowrule = monitor 0, match:class ^(rumpress-presenter)$
```

On older Hyprland, use the `windowrulev2` spelling instead:

```ini
windowrulev2 = monitor 1, class:^(rumpress-beamer)$
windowrulev2 = fullscreen, class:^(rumpress-beamer)$
windowrulev2 = monitor 0, class:^(rumpress-presenter)$
```

Each rule carries exactly one property — combining `monitor` and `fullscreen`
in a single rule silently does nothing. (The runtime path rumpress itself uses
still sends the deprecated `windowrulev2` keyword; Hyprland aliases it, so it
works but logs a deprecation notice.)

With only one monitor, rumpress opens just the presenter window.

## Keybindings

| Key                        | Action                          |
| -------------------------- | ------------------------------- |
| `Right` / `Space` / `PgDn` | next page (overlay step)        |
| `Left` / `PgUp`            | previous page                   |
| `Down`                     | next slide (skips overlays)     |
| `Up`                       | previous slide                  |
| `b`                        | blank / unblank the beamer      |
| `g`                        | toggle overview grid            |
| `h`                        | toggle highlight pen            |
| `t`                        | reset timer                     |
| `?`                        | toggle help overlay             |
| `Esc`                      | close overview / highlight / help |
| `q`                        | quit                            |

Press `?` in the presenter window to see this list in the app.

## Deck format

rumpress expects 2×1 pages: two 16:9 halves side by side on one PDF page —
the slide on one half, speaker notes on the other. The slide half goes to
the beamer, the notes half to the presenter; `--swap-halves` exchanges them.

A PDF that is not 2×1 (aspect ratio below ~1.9) is shown as a full page on
both screens, with a yellow warning banner on the presenter.

## Live reload

The PDF file is watched; editing and re-exporting the deck (e.g. from
LaTeX) reloads it in place after a 500 ms debounce, keeping your current
position clamped to the new page count. A write landing inside the debounce
window is deferred and loaded once the window expires, so a final touch-up
save is never lost. A failed reload (e.g. a half-written file) is logged and
rumpress falls back to re-opening the original file, leaving the
presentation running; the next successful change reloads normally.

## Known limitations

None currently documented.
