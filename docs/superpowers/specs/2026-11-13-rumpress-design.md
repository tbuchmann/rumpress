# rumpress — design doc

Date: 2026-11-13
Status: draft, pending approval

## Intent and why

A dual-screen PDF presentation tool for LaTeX/beamer decks, written from
scratch in Rust, in the spirit of pympress (which is Python/GTK). The
presenter's notebook shows a control view (timer, next-page preview,
notes, overview); the beamer shows only the slide content, fullscreen.

Decks are compiled with beamer's `\setbeameroption{show notes on second
screen}`: every PDF page is a wide 2×1 layout, slide content on the left
half, notes on the right half. The app splits each page at the midpoint.

## Goals

- Open a PDF and run a presentation on two screens under Hyprland (Wayland).
- Presentation window: fullscreen on the beamer, shows the left half of
  the current page, scaled to fit, black background.
- Presenter window: fullscreen on the notebook, shows:
  - current slide (left half of current page),
  - next page preview (the next *page*, i.e. next animation step),
  - notes (right half of current page),
  - elapsed time, wall-clock time, and a per-slide time estimate,
  - a toggleable overview grid of all logical slides for jumping.
- Logical slides: consecutive PDF pages that belong to one beamer frame
  (overlays from `\pause` etc.) are grouped. Navigation has two steps:
  next *page* (animation) and next *slide* (whole group).
- Blank-screen toggle (beamer shows black, presentation state kept).
- Highlight/pointer mode: a pointer or pen drawn on the beamer output,
  controlled from the presenter window's mouse/touchpad.
- Keyboard-driven in v1.
- Single static binary feel: `rumpress deck.pdf` just works.

## Non-goals (v1)

- Video/multimedia annotation playback (deferred; see Approaches).
- USB presenter clickers / evdev input (planned post-v1).
- Notes from external files, or alternating-page note layouts.
- X11, Windows, macOS.
- Editing, annotating-as-file, or exporting PDFs.
- Remote control / network features.

## Anti-goals (failure even if it works)

- Laggy page turns: advancing must feel instant (pre-render next pages).
- Fiddly setup: if monitor placement needs manual fiddling every talk,
  the tool has failed its purpose.
- A general-purpose PDF viewer creeping in.

## Constraints

- Arch Linux, Hyprland, Wayland. Layer-shell (`wlr-layer-shell`) is
  available for per-monitor fullscreen placement; Hyprland window rules
  (matching on window title/app id) are an acceptable fallback.
- PDF rendering via a C library binding; Pdfium chosen over MuPDF for
  licensing (MuPDF is AGPL, Pdfium permissive) and crate quality
  (`pdfium-render`).
- GUI: egui/eframe with multi-viewport support (two OS windows from one
  process). PDF pages are rendered to RGBA bitmaps and uploaded as egui
  textures.
- Pdfium is not thread-safe: all PDF access funneled through one render
  thread; the UI communicates via messages and receives rendered bitmaps.

## Approaches considered

1. **GTK4 + poppler-rs** (pympress's architecture, in Rust bindings).
   Proven for exactly this app; layer-shell via `gtk4-layer-shell`.
   Rejected: user prefers a pure-Rust-feeling stack; GTK's build weight
   and C-style API.
2. **egui/eframe + pdfium-render** — **chosen**. Immediate-mode UI is
   pleasant to write, multi-viewport covers the two-window need, texture
   upload is trivial, Pdfium licensing is clean. Tradeoff: we build the
   presentation widgets ourselves (no built-in PDF widget), and Wayland
   per-monitor fullscreen needs layer-shell work (see Open points).
3. **Tauri/webview + pdf.js** — rejected: presentation logic would live
   in JavaScript, against the "written from scratch in Rust" intent.
4. **iced + pdfium-render** — viable alternative to egui, rejected:
   multi-window support less mature than egui's viewports at decision
   time.

Video was scoped out of v1 after discussion: beamer embeds multimedia as
PDF annotations and playing them means bridging a media framework
(GStreamer) into the annotation model — the hardest single feature. The
page model keeps annotation access behind a module boundary so a video
player can be added without rework.

## Decided

- Language/stack: Rust, egui/eframe (multi-viewport), pdfium-render.
- Notes layout: beamer second-screen (2×1 pages, split at midpoint).
- Logical slide grouping: needed for animation-aware navigation.
  Detection strategy to be researched during implementation, in this
  order of preference: (a) whatever pympress does (read its source for
  the exact heuristic — likely PDF page labels or outline entries that
  beamer/hyperref emit), (b) page-label-based grouping, (c) fallback:
  every page is its own logical slide, with a manual "group with
  previous" key. The grouping result must be overridable at runtime.
- Input (v1): keyboard. Arrow keys / space / PageUp/Down for page and
  slide steps, `b` blank, `g` overview grid, `h` highlight mode, `t`
  reset timer, `q`/Esc quit (Esc only quits from overview/highlight).
  Exact bindings listed in a help overlay (`?`).
- Timer: elapsed since start, wall clock, and a talk-duration target
  (CLI flag, e.g. `--time 45min`): computes the time each logical slide
  *should* be reached at (proportional to slide index) and shows
  ahead/behind. Per-slide elapsed time is shown for the current slide.
- Highlight mode: pointer position on the presenter window maps to
  normalized coordinates on the current slide; a dot (laser) or
  freehand strokes (pen, cleared on page change) are drawn on the
  presentation window.
- Rendering pipeline: single PDF thread; LRU texture cache keyed by
  (page, half, size); pre-render current ±1 pages and the next slide's
  first page at current scale; re-render on scale change (debounced).
- Monitor placement: prefer layer-shell so the app itself says which
  monitor gets which window; fall back to documented Hyprland window
  rules (`windowrulev2` matching app-id/title). CLI flags
  `--present-monitor N --presenter-monitor M` with a sensible default
  (presenter = primary/biggest, presentation = the other one).
- Live reload (v1): watch the PDF file; on change (recompile), reload
  the document, invalidate the texture cache, and keep the current
  logical slide if the page count still allows it (clamp to the last
  slide otherwise). The document layer is built around replaceable
  documents from the start.
- Non-2×1 PDFs: if the page aspect ratio is not ~2:1, show the full
  page (no split) on both screens and show a warning banner on the
  presenter window. No separate no-notes mode in v1.
- `--swap-halves` flag for beamer's `show notes on second screen=left`
  (notes left, slide right).
- Pdfium distribution: `pdfium-render` dynamically loads `libpdfium.so`,
  so the binary is not self-contained. Document Pdfium as a dependency
  (AUR package) and ship a small script that downloads the matching
  Pdfium build; the app prints a clear error naming the missing library.
- Single-monitor fallback: if only one monitor exists (or a named
  monitor is missing), run the presenter windowed on the remaining
  screen and log/announce the fallback. This doubles as the development
  mode for testing on a single-monitor machine.
- Input handling is centralized: key events from either viewport go to
  one handler, so bindings work regardless of which window has focus.

## Open points left to the builder

- Exact layer-shell integration path with eframe/winit (candidate:
  `winit-layer-shell` crate or a small fork/patch; evaluate early with
  a spike). If layer-shell proves unstable, ship the Hyprland
  window-rule fallback and document it in the README.
- Logical-slide detection heuristic (research pympress source first).
- Whether page halves are cropped at render time (Pdfium render bounds)
  or rendered full and cropped in the bitmap — measure, pick the faster.
- egui viewport API details for fullscreen-on-monitor under Wayland.
- Crate versions and feature flags; keep dependency tree lean.
- Timer start semantics (start on launch vs. on first page turn) —
  builder's call; make it obvious in the UI either way.

## Success check

With a real beamer deck (notes on second screen, a few `\pause`s):
`rumpress deck.pdf --time 30min` puts the slide on the beamer and the
control view on the notebook, page turns are instant, animations step
within a frame, the timer and estimates track the talk, and the overview
grid jumps anywhere in under a second.
