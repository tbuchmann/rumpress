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

## Presenter window

The presenter window opens at 1600×900 and is laid out for reading at a glance:

- **Current slide** large on the left, with a "Slide n / N" counter.
- **Next-page preview** on the right, with the **notes** below it when the deck
  has notes pages (see [Deck format](#deck-format)).
- **Annotation toolbar** above the slide (see [Annotation tools](#annotation-tools)).
- **Timer bar** pinned to the bottom: elapsed time, wall clock, and — with
  `--time` — the pace indicator and a time progress bar (`Time: 02:24 / 20:00`).
  A second bar always shows slide progress (`Slide 5 / 12 — 7 remaining`); it
  counts logical slides, so overlay steps of one slide do not advance it.

### Jumping to a slide

Type the slide number and press `Enter` — for example `1` `2` `Enter` goes to
slide 12. The typed number appears next to the slide counter. `Backspace`
deletes a digit and `Esc` cancels. A number past the end goes to the last
slide. (The overview grid, `g`, lets you click a thumbnail instead.)

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

| Key                        | Action                                        |
| -------------------------- | --------------------------------------------- |
| `Right` / `Space` / `PgDn` | next page (overlay step)                      |
| `Left` / `PgUp`            | previous page                                 |
| `Down`                     | next slide (skips overlays)                   |
| `Up`                       | previous slide                                |
| `0`–`9`, `Enter`           | type a slide number, `Enter` jumps to it      |
| `b`                        | blank / unblank the beamer                    |
| `g`                        | toggle overview grid                          |
| `t`                        | reset timer                                   |
| `?`                        | toggle help overlay                           |
| `Esc`                      | cancel typed number / close overview / switch off the annotation tool / close help |
| `q`                        | quit                                          |

Annotation keys are listed under [Annotation tools](#annotation-tools).
Press `?` in the presenter window to see the full list in the app.

## Annotation tools

Draw on the current slide from the presenter window; everything appears on the
beamer as you draw. Pick a tool with its key or from the toolbar above the
slide; pressing the active tool's key again (or `Esc`) goes back to normal
navigation.

| Key | Tool    | What it does                                            |
| --- | ------- | ------------------------------------------------------- |
| `l` | Laser   | glowing dot with a short fading tail that follows the mouse over the slide, on both screens; leaves nothing behind |
| `p` | Pen     | freehand drawing                                        |
| `h` | Marker  | wide, translucent freehand highlighter                  |
| `f` | Area    | drag a filled, translucent rectangle to mark a region   |
| `n` | Line    | drag a straight line                                    |
| `a` | Arrow   | drag an arrow                                           |
| `r` | Box     | drag a rectangle outline                                |
| `o` | Oval    | drag an ellipse outline                                 |
| `e` | Eraser  | drag over annotations to remove them                    |

| Key             | Action                                   |
| --------------- | ---------------------------------------- |
| `c`             | next color (each tool has its own palette) |
| `w`             | next width (S / M / L)                   |
| `u` / `Ctrl+Z`  | undo the last annotation on this page    |
| `x`             | clear all annotations on this page       |

The toolbar also has color swatches, width buttons, and Undo / Clear buttons.

Annotations belong to a PDF page, so every overlay step of a slide has its
own, and they are still there when you come back to the page. They survive a
live reload of the deck but are not saved when you quit.

## Deck format

rumpress expects 2×1 pages: two 16:9 halves side by side on one PDF page —
the slide on one half, speaker notes on the other. The slide half goes to
the beamer, the notes half to the presenter; `--swap-halves` exchanges them.

A PDF that is not 2×1 (aspect ratio below ~1.9) is shown as a full page on
both screens, with a yellow warning banner on the presenter and no notes.

### Beamer (LaTeX)

Notes written with `\note{...}` only end up in the PDF if you ask beamer to
render them. Add this to the preamble and recompile:

```latex
\setbeameroption{show notes on second screen=right}
```

Each page then becomes two halves side by side (about 3.56:1 for a 16:9 deck),
the slide on the left and the notes on the right — rumpress's default. With
`=left`, start rumpress with `--swap-halves`. Long notes may not fit the notes
half; shrink them with e.g. `\setbeamerfont{note page}{size=\small}`. To also
get a clean slides-only PDF to share, compile a second copy without the option.

## Live reload

The PDF file is watched; editing and re-exporting the deck (e.g. from
LaTeX) reloads it in place after a 500 ms debounce, keeping your current
position clamped to the new page count. A write landing inside the debounce
window is deferred and loaded once the window expires, so a final touch-up
save is never lost. A failed reload (e.g. a half-written file) is logged and
rumpress falls back to re-opening the original file, leaving the
presentation running; the next successful change reloads normally.

## Known limitations

- Annotations are not saved between runs, and are drawn with the mouse only
  (no pen-tablet pressure).
- Monitor placement and fullscreen on a second display depend on Hyprland; on
  a single monitor only the presenter window opens.
