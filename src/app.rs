//! The egui application: presenter window plus presentation (beamer) viewport.
//!
//! The root viewport is the presenter window; the beamer window is an
//! immediate child viewport that only *reads* presentation state and
//! textures, so the mutable borrows stay in [`RumpressApp::update`].

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use egui::load::SizedTexture;
use egui::{
    CentralPanel, Color32, ColorImage, Context, Event, Frame, Image, ProgressBar, Rect, RichText,
    TextureHandle, TextureOptions, ViewportBuilder, ViewportCommand, ViewportId, vec2,
};

use crate::input::{self, Action};
use crate::layout::{PageLayout, detect};
use crate::pdf::{Half, SlideIndex};
use crate::reload::{self, ReloadWatcher};
use crate::render::{DocumentInfo, PageKey, RenderHandle, RenderKey};
use crate::state::DeckState;
use crate::timer::{self, Timer};
use crate::ui::highlight::{self, Strokes};
use crate::ui::overview::grid_geometry;

/// Pixels per point used when rendering pages.
///
/// Fixed for now: every texture is rendered at this scale and scaled down/up
/// by the GPU when drawn. Dynamic per-viewport scaling is a later refinement.
const RENDER_SCALE: f32 = 2.0;

/// Pixels per point used for overview-grid thumbnails.
const OVERVIEW_SCALE: f32 = 0.3;

/// How often to wake up for the timer display.
const REPAINT_INTERVAL: Duration = Duration::from_millis(200);

/// The dual-viewport presenter application.
pub struct RumpressApp {
    render: RenderHandle,
    state: DeckState,
    /// Copy of the slide index, for prefetching and the slide counter.
    /// (`DeckState` keeps its own private copy.)
    slides: SlideIndex,
    timer: Timer,
    /// Uploaded GPU textures by render key (page + scale), replaced as new
    /// bitmaps arrive. The scale is part of the key so overview thumbnails
    /// never replace the full-resolution main-view textures.
    textures: HashMap<RenderKey, TextureHandle>,
    /// The `--swap-halves` flag, kept so the layout can be re-derived when
    /// the document reloads and fresh `DocumentInfo` arrives.
    swap_halves: bool,
    /// How a page maps onto the two viewports, derived from the page size
    /// when `DocumentInfo` arrives (recomputed on reload).
    layout: PageLayout,
    /// Whether the overview grid is shown in the presenter window.
    overview_visible: bool,
    /// Whether dragging on the presenter's slide draws highlight strokes.
    highlight_mode: bool,
    /// Freehand highlight strokes for the current page, in normalized
    /// 0..1 slide coordinates. Cleared when the page changes.
    strokes: Strokes,
    /// Page shown at the previous frame, to detect page changes.
    last_page: u16,
    /// Key actions collected from both viewports, applied once per frame.
    pending_actions: RefCell<Vec<Action>>,
    /// Watches the PDF file; when it changes the document is reloaded.
    reload_watcher: Option<ReloadWatcher>,
    /// Path of the loaded PDF, for reload requests.
    pdf_path: PathBuf,
    /// Set when either window is closed by the window manager.
    close_requested: Cell<bool>,
    /// Whether the beamer viewport is shown at all. False only in the
    /// single-monitor case; on detection failure the beamer still opens
    /// (windowed) so a static hyprland.conf rule can pin it.
    show_beamer: bool,
    /// Ensures the one-shot beamer fullscreen command is sent only once.
    beamer_fullscreen: Cell<bool>,
}

impl RumpressApp {
    /// Builds the app from the parts assembled in [`crate::run::run`].
    ///
    /// `pdf_path` is the loaded document; when a file watcher can be set up
    /// for it, changes trigger a live reload. Watcher failure is not fatal
    /// (the deck just never live-reloads), so it is logged and skipped.
    pub fn new(
        render: RenderHandle,
        state: DeckState,
        info: DocumentInfo,
        timer: Timer,
        swap_halves: bool,
        pdf_path: PathBuf,
        show_beamer: bool,
    ) -> RumpressApp {
        let slides = SlideIndex::from_labels(&info.labels);
        let layout = detect(info.page_size, swap_halves);
        let reload_watcher = match reload::watch(&pdf_path) {
            Ok(w) => Some(w),
            Err(e) => {
                log::warn!("live reload unavailable for {}: {e}", pdf_path.display());
                None
            }
        };
        RumpressApp {
            render,
            state,
            slides,
            timer,
            textures: HashMap::new(),
            swap_halves,
            layout,
            overview_visible: false,
            highlight_mode: false,
            strokes: Strokes::default(),
            last_page: 0,
            pending_actions: RefCell::new(Vec::new()),
            reload_watcher,
            pdf_path,
            close_requested: Cell::new(false),
            show_beamer,
            beamer_fullscreen: Cell::new(false),
        }
    }

    /// Which half of the page the beamer shows. The whole page when the
    /// layout is [`PageLayout::Full`].
    fn slide_half(&self) -> Half {
        match self.layout {
            PageLayout::Split { slide, .. } => slide,
            PageLayout::Full => Half::Full,
        }
    }

    /// Which half of the page the presenter's notes widget shows. The whole
    /// page when the layout is [`PageLayout::Full`] (the widget renders
    /// nothing in that case).
    fn notes_half(&self) -> Half {
        match self.layout {
            PageLayout::Split { notes, .. } => notes,
            PageLayout::Full => Half::Full,
        }
    }

    /// The texture for `key` rendered at `scale`, if it has arrived.
    fn texture_at(&self, key: PageKey, scale: f32) -> Option<&TextureHandle> {
        self.textures.get(&RenderKey::new(key, scale))
    }

    /// Drains render-thread output into textures and (re)requests everything
    /// the UI needs. The render thread dedups and caches, so re-requesting
    /// every frame is cheap.
    fn poll_render(&mut self, ctx: &Context) {
        while let Some(info) = self.render.take_document_info() {
            // A fresh DocumentInfo means the document was (re)opened: drop
            // stale textures and re-derive the slide layout.
            self.textures.clear();
            self.layout = detect(info.page_size, self.swap_halves);
            self.slides = SlideIndex::from_labels(&info.labels);
            self.state
                .apply_slides(SlideIndex::from_labels(&info.labels));
        }
        while let Some((rkey, bmp)) = self.render.poll() {
            let image = ColorImage::from_rgba_unmultiplied(
                [bmp.width as usize, bmp.height as usize],
                &bmp.rgba,
            );
            if let Some(tex) = self.textures.get_mut(&rkey) {
                tex.set(image, TextureOptions::default());
            } else {
                let name = format!(
                    "page-{}-{:?}-{}",
                    rkey.page.page, rkey.page.half, rkey.scale
                );
                let tex = ctx.load_texture(name, image, TextureOptions::default());
                self.textures.insert(rkey, tex);
            }
        }

        let scale = RENDER_SCALE;
        let slide_half = self.slide_half();
        let notes_half = self.notes_half();
        let page = self.state.current_page;
        self.render.request(
            PageKey {
                page,
                half: slide_half,
            },
            scale,
        );
        self.render.request(
            PageKey {
                page: page.saturating_add(1),
                half: slide_half,
            },
            scale,
        );
        self.render.request(
            PageKey {
                page,
                half: notes_half,
            },
            scale,
        );
        if let Some(next) = self.slides.next_slide(self.state.current_slide) {
            let first = self.slides.first_page(next);
            self.render.request(
                PageKey {
                    page: first,
                    half: slide_half,
                },
                scale,
            );
        }

        // Overview thumbnails: first page of every slide, at low scale. The
        // render thread dedups and caches, so this is cheap per frame.
        if self.overview_visible {
            for slide in 0..self.slides.slide_count() {
                let first = self.slides.first_page(slide);
                self.render.request(
                    PageKey {
                        page: first,
                        half: slide_half,
                    },
                    OVERVIEW_SCALE,
                );
            }
        }
    }

    /// Checks the file watcher; on change, asks the render thread to reload
    /// the document. The new `DocumentInfo` is picked up by `poll_render`
    /// (rebuilds the slide index, clamps the page, clears textures).
    fn poll_reload(&mut self) {
        let Some(watcher) = self.reload_watcher.as_mut() else {
            return;
        };
        if watcher.poll() {
            log::info!("detected change in {}, reloading", self.pdf_path.display());
            self.render.reload(self.pdf_path.clone());
        }
    }

    /// Applies collected key actions; returns `true` if the app should quit.
    fn handle_actions(&mut self) -> bool {
        let actions = std::mem::take(&mut *self.pending_actions.borrow_mut());
        let mut quit = self.close_requested.replace(false);
        for action in actions {
            match action {
                // Overview visibility lives in the app layer, not DeckState.
                Action::ToggleOverview => self.overview_visible = !self.overview_visible,
                // Highlight mode likewise lives in the app layer.
                Action::ToggleHighlight => self.highlight_mode = !self.highlight_mode,
                _ => quit |= input::apply(&mut self.state, action, &mut self.timer),
            }
        }
        quit
    }

    /// Shows the beamer viewport. The closure only reads `self`, so it can
    /// borrow immutably while `update` keeps `&mut self` for the rest.
    fn show_presentation(&mut self, ctx: &Context) {
        if !self.show_beamer {
            return; // single monitor: no separate beamer window
        }
        ctx.show_viewport_immediate(
            ViewportId::from_hash_of("presentation"),
            ViewportBuilder::default()
                .with_title("rumpress-beamer")
                .with_app_id("rumpress-beamer"),
            |ctx, _class| self.draw_presentation(ctx),
        );
        // Fullscreen the beamer once, now that its viewport exists; the
        // window rule already placed it on the assigned monitor.
        if !self.beamer_fullscreen.replace(true) {
            ctx.send_viewport_cmd_to(
                ViewportId::from_hash_of("presentation"),
                ViewportCommand::Fullscreen(true),
            );
        }
    }

    /// Content of the beamer window: the current slide on black, letterboxed.
    fn draw_presentation(&self, ctx: &Context) {
        self.collect_key_actions(ctx);
        if ctx.input(|i| i.viewport().close_requested()) {
            self.close_requested.set(true);
        }

        CentralPanel::default()
            .frame(Frame::NONE.fill(Color32::BLACK))
            .show(ctx, |ui| {
                if self.state.blanked {
                    return; // black background is already in place
                }
                let key = PageKey {
                    page: self.state.current_page,
                    half: self.slide_half(),
                };
                let Some(tex) = self.texture_at(key, RENDER_SCALE) else {
                    return;
                };
                let tex_size = tex.size_vec2();
                if tex_size.x <= 0.0 || tex_size.y <= 0.0 {
                    return;
                }
                // Fit inside the window, centered: letterbox with black.
                let avail = ui.max_rect().size();
                let scale = (avail.x / tex_size.x).min(avail.y / tex_size.y);
                let size = tex_size * scale;
                let rect = Rect::from_center_size(ui.max_rect().center(), size);
                Image::from_texture(SizedTexture::new(tex.id(), tex_size)).paint_at(ui, rect);
                // Highlight strokes over the slide, mapped from normalized
                // coordinates into the on-screen slide rectangle.
                highlight::draw(ui.painter(), rect, &self.strokes);
            });
    }

    /// Content of the presenter window: current slide, next-page preview,
    /// notes, timer bar, and (toggled with `g`) the overview grid.
    fn draw_presenter(&mut self, ctx: &Context) {
        CentralPanel::default().show(ctx, |ui| {
            if self.layout == PageLayout::Full {
                ui.label(
                    RichText::new("PDF is not a 2×1 notes deck — showing full page")
                        .color(Color32::YELLOW),
                );
            }
            let row = ui.available_height() / 2.0;
            let next = PageKey {
                page: self.state.current_page.saturating_add(1),
                half: self.slide_half(),
            };
            let notes = PageKey {
                page: self.state.current_page,
                half: self.notes_half(),
            };
            ui.horizontal(|ui| {
                ui.set_height(row);
                let half_w = ui.available_width() / 2.0;
                ui.allocate_ui(vec2(half_w, row), |ui| {
                    ui.label(format!(
                        "Slide {} / {}",
                        self.state.current_slide + 1,
                        self.slides.slide_count()
                    ));
                    self.draw_current_slide(ui);
                });
                ui.allocate_ui(vec2(half_w, row), |ui| {
                    ui.label("Next:");
                    show_fitted(ui, self.texture_at(next, RENDER_SCALE));
                });
            });
            ui.horizontal(|ui| {
                ui.set_height(row);
                let half_w = ui.available_width() / 2.0;
                ui.allocate_ui(vec2(half_w, row), |ui| {
                    draw_timer_bar(
                        ui,
                        &self.timer,
                        self.state.current_slide,
                        self.slides.slide_count(),
                    );
                });
                ui.allocate_ui(vec2(half_w, row), |ui| {
                    ui.label("Notes:");
                    if self.layout != PageLayout::Full {
                        show_fitted(ui, self.texture_at(notes, RENDER_SCALE));
                    }
                });
            });

            if self.overview_visible {
                self.draw_overview(ui);
            }
        });
    }

    /// Full-panel overview grid: one thumbnail per logical slide; clicking a
    /// thumbnail jumps to that slide.
    fn draw_overview(&mut self, ui: &mut egui::Ui) {
        let slide_count = self.slides.slide_count();
        if slide_count == 0 {
            return;
        }
        // Slide aspect from the first page's texture, falling back to 16:9.
        let aspect = self
            .texture_at(
                PageKey {
                    page: 0,
                    half: self.slide_half(),
                },
                OVERVIEW_SCALE,
            )
            .map(|tex| {
                let size = tex.size_vec2();
                (size.x / size.y.max(1.0)).max(f32::EPSILON)
            })
            .unwrap_or(16.0 / 9.0);

        let viewport = ui.max_rect();
        let rects = grid_geometry(viewport, slide_count, aspect);
        let slide_half = self.slide_half();
        for (slide, rect) in rects.iter().enumerate() {
            let first = self.slides.first_page(slide);
            let key = PageKey {
                page: first,
                half: slide_half,
            };
            let response = ui.allocate_rect(*rect, egui::Sense::click());
            if let Some(tex) = self.texture_at(key, OVERVIEW_SCALE) {
                let tex_size = tex.size_vec2();
                if tex_size.x > 0.0 && tex_size.y > 0.0 {
                    // Letterbox the thumbnail inside its cell.
                    let scale = (rect.width() / tex_size.x).min(rect.height() / tex_size.y);
                    let size = tex_size * scale;
                    let draw = Rect::from_center_size(rect.center(), size);
                    Image::from_texture(SizedTexture::new(tex.id(), tex_size)).paint_at(ui, draw);
                }
            }
            // Highlight the current slide; label every cell.
            let text = format!("{}", slide + 1);
            let color = if slide == self.state.current_slide {
                Color32::LIGHT_BLUE
            } else {
                Color32::GRAY
            };
            ui.painter().text(
                rect.left_bottom() + vec2(4.0, -4.0),
                egui::Align2::LEFT_BOTTOM,
                text,
                egui::TextStyle::Small.resolve(ui.style()),
                color,
            );
            if response.clicked() {
                self.state.goto_slide(slide);
            }
        }
    }

    /// The presenter's current-slide widget: the slide image fitted into the
    /// available space, with highlight strokes drawn over it. In highlight
    /// mode, dragging on the slide draws new strokes in normalized slide
    /// coordinates.
    fn draw_current_slide(&mut self, ui: &mut egui::Ui) {
        let key = PageKey {
            page: self.state.current_page,
            half: self.slide_half(),
        };
        let Some(tex) = self.texture_at(key, RENDER_SCALE) else {
            return;
        };
        let tex_size = tex.size_vec2();
        if tex_size.x <= 0.0 || tex_size.y <= 0.0 {
            return;
        }
        let avail = ui.available_size();
        let scale = (avail.x / tex_size.x).min(avail.y / tex_size.y);
        let size = tex_size * scale;
        let rect = Rect::from_min_size(ui.cursor().min, size);
        Image::from_texture(SizedTexture::new(tex.id(), tex_size)).paint_at(ui, rect);
        highlight::draw(ui.painter(), rect, &self.strokes);

        if !self.highlight_mode {
            return;
        }
        let response = ui.allocate_rect(rect, egui::Sense::drag());
        if let Some(pos) = response.interact_pointer_pos() {
            let norm = egui::pos2(
                ((pos.x - rect.left()) / rect.width()).clamp(0.0, 1.0),
                ((pos.y - rect.top()) / rect.height()).clamp(0.0, 1.0),
            );
            self.strokes.push_point(norm, response.dragged());
        } else {
            self.strokes.push_point(egui::pos2(0.0, 0.0), false); // pen up
        }
    }

    /// Reads key events from `ctx` (whichever viewport is being drawn) into
    /// the pending queue. Auto-repeat events are dropped: every action is a
    /// step or a toggle, so holding a key should not machine-gun.
    fn collect_key_actions(&self, ctx: &Context) {
        let actions: Vec<Action> = ctx.input(|i| {
            i.events
                .iter()
                .filter_map(|event| match event {
                    Event::Key {
                        key,
                        pressed: true,
                        repeat: false,
                        modifiers,
                        ..
                    } => Some(input::map_key(*key, *modifiers)),
                    _ => None,
                })
                .collect()
        });
        self.pending_actions.borrow_mut().extend(actions);
    }
}

impl eframe::App for RumpressApp {
    fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        // Idempotent: the first frame starts the clock.
        self.timer.start(Instant::now());

        self.collect_key_actions(ctx);
        if ctx.input(|i| i.viewport().close_requested()) {
            self.close_requested.set(true);
        }

        self.poll_reload();
        self.poll_render(ctx);
        self.show_presentation(ctx);
        // Highlight strokes belong to one page: clear them when the page
        // changes (including jumps via the overview grid).
        if self.state.current_page != self.last_page {
            self.strokes.clear();
            self.last_page = self.state.current_page;
        }

        if self.handle_actions() {
            ctx.send_viewport_cmd(ViewportCommand::Close);
            return;
        }
        self.draw_presenter(ctx);

        ctx.request_repaint_after(REPAINT_INTERVAL);
    }
}

/// Draws the current slide image fitted into `ui`, if its texture exists.
fn show_fitted(ui: &mut egui::Ui, tex: Option<&TextureHandle>) {
    let Some(tex) = tex else { return };
    let size = tex.size_vec2();
    if size.x <= 0.0 || size.y <= 0.0 {
        return;
    }
    ui.add(Image::from_texture(SizedTexture::new(tex.id(), size)).max_size(ui.available_size()));
}

/// Timer readout: elapsed, wall clock, target, and pace, plus a progress bar.
fn draw_timer_bar(ui: &mut egui::Ui, timer: &Timer, slide: usize, slide_count: usize) {
    let now = Instant::now();
    let elapsed = timer.elapsed(now);
    ui.horizontal(|ui| {
        ui.label(format!("⏱ {}", format_duration(elapsed)));
        ui.label(wall_clock_string());
        if let Some(pace) = pace_text(timer, now, slide, slide_count) {
            ui.label(pace);
        }
    });
    if let Some(target) = timer.target()
        && target.as_secs_f32() > 0.0
    {
        ui.label(format!("Target: {}", format_duration(target)));
        let frac = (elapsed.as_secs_f32() / target.as_secs_f32()).clamp(0.0, 1.0);
        ui.add(ProgressBar::new(frac).show_percentage());
    }
}

/// The current wall-clock time as `HH:MM`, from `SystemTime`.
///
/// Limitation: this is UTC, not local time — the standard library offers no
/// timezone conversion, and pulling in a chrono/tz dependency for a clock
/// readout is not worth it yet. The clock will read wrong for anyone not
/// living on UTC.
fn wall_clock_string() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let (h, m) = ((secs / 3600) % 24, (secs / 60) % 60);
    format!("{h:02}:{m:02}")
}

/// Pace as colored text: green OnTrack, yellow within ±2 min, red otherwise,
/// with signed minutes. `None` when no target is set.
fn pace_text(timer: &Timer, now: Instant, slide: usize, slide_count: usize) -> Option<RichText> {
    timer.target()?;
    let (color, text) = match timer::pace(timer, now, slide, slide_count) {
        timer::Pace::Ahead(d) if d >= Duration::from_secs(120) => {
            (Color32::RED, format!("ahead +{}", format_minutes(d)))
        }
        timer::Pace::Ahead(d) => (Color32::YELLOW, format!("ahead +{}", format_minutes(d))),
        timer::Pace::Behind(d) if d >= Duration::from_secs(120) => {
            (Color32::RED, format!("behind -{}", format_minutes(d)))
        }
        timer::Pace::Behind(d) => (Color32::YELLOW, format!("behind -{}", format_minutes(d))),
        timer::Pace::OnTrack => (Color32::GREEN, "on track".to_string()),
    };
    Some(RichText::new(text).color(color))
}

/// Formats a duration as whole minutes, e.g. `2 min`. Rounds to the nearest
/// minute instead of truncating, so a 30–59 s deviation is not hidden as
/// `+0 min`.
fn format_minutes(d: Duration) -> String {
    format!("{} min", d.as_secs().div_ceil(60))
}

/// Formats a duration as `MM:SS` (hours fold into minutes).
fn format_duration(d: Duration) -> String {
    let secs = d.as_secs();
    format!("{:02}:{:02}", secs / 60, secs % 60)
}
