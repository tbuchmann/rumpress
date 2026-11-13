//! The egui application: presenter window plus presentation (beamer) viewport.
//!
//! The root viewport is the presenter window; the beamer window is an
//! immediate child viewport that only *reads* presentation state and
//! textures, so the mutable borrows stay in [`RumpressApp::update`].

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::time::{Duration, Instant};

use egui::{
    CentralPanel, Color32, ColorImage, Context, Event, Frame, Image, ProgressBar, Rect,
    TextureHandle, TextureOptions, ViewportBuilder, ViewportId, ViewportCommand, vec2,
};
use egui::load::SizedTexture;

use crate::input::{self, Action};
use crate::pdf::{Half, SlideIndex};
use crate::render::{DocumentInfo, PageKey, RenderHandle};
use crate::state::DeckState;
use crate::timer::Timer;

/// Pixels per point used when rendering pages.
///
/// Fixed for now: every texture is rendered at this scale and scaled down/up
/// by the GPU when drawn. Dynamic per-viewport scaling is a later refinement.
const RENDER_SCALE: f32 = 2.0;

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
    /// Uploaded GPU textures by page key, replaced as new bitmaps arrive.
    textures: HashMap<PageKey, TextureHandle>,
    swap_halves: bool,
    /// Key actions collected from both viewports, applied once per frame.
    pending_actions: RefCell<Vec<Action>>,
    /// Set when either window is closed by the window manager.
    close_requested: Cell<bool>,
}

impl RumpressApp {
    /// Builds the app from the parts assembled in [`crate::run::run`].
    pub fn new(
        render: RenderHandle,
        state: DeckState,
        info: DocumentInfo,
        timer: Timer,
        swap_halves: bool,
    ) -> RumpressApp {
        let slides = SlideIndex::from_labels(&info.labels);
        RumpressApp {
            render,
            state,
            slides,
            timer,
            textures: HashMap::new(),
            swap_halves,
            pending_actions: RefCell::new(Vec::new()),
            close_requested: Cell::new(false),
        }
    }

    /// Which half of the page carries the slide.
    fn slide_half(&self) -> Half {
        if self.swap_halves { Half::Right } else { Half::Left }
    }

    /// Which half of the page carries the notes.
    fn notes_half(&self) -> Half {
        if self.swap_halves { Half::Left } else { Half::Right }
    }

    /// Drains render-thread output into textures and (re)requests everything
    /// the UI needs. The render thread dedups and caches, so re-requesting
    /// every frame is cheap.
    fn poll_render(&mut self, ctx: &Context) {
        while let Some(info) = self.render.take_document_info() {
            // A fresh DocumentInfo means the document was (re)opened: drop
            // stale textures and re-derive the slide layout.
            self.textures.clear();
            self.slides = SlideIndex::from_labels(&info.labels);
            self.state.apply_slides(SlideIndex::from_labels(&info.labels));
        }
        while let Some((key, bmp)) = self.render.poll() {
            let image = ColorImage::from_rgba_unmultiplied(
                [bmp.width as usize, bmp.height as usize],
                &bmp.rgba,
            );
            if let Some(tex) = self.textures.get_mut(&key) {
                tex.set(image, TextureOptions::default());
            } else {
                let name = format!("page-{}-{:?}", key.page, key.half);
                let tex = ctx.load_texture(name, image, TextureOptions::default());
                self.textures.insert(key, tex);
            }
        }

        let scale = RENDER_SCALE;
        let slide_half = self.slide_half();
        let notes_half = self.notes_half();
        let page = self.state.current_page;
        self.render.request(PageKey { page, half: slide_half }, scale);
        self.render.request(PageKey { page: page.saturating_add(1), half: slide_half }, scale);
        self.render.request(PageKey { page, half: notes_half }, scale);
        if let Some(next) = self.slides.next_slide(self.state.current_slide) {
            let first = self.slides.first_page(next);
            self.render.request(PageKey { page: first, half: slide_half }, scale);
        }
    }

    /// Applies collected key actions; returns `true` if the app should quit.
    fn handle_actions(&mut self) -> bool {
        let actions = std::mem::take(&mut *self.pending_actions.borrow_mut());
        let mut quit = self.close_requested.replace(false);
        for action in actions {
            quit |= input::apply(&mut self.state, action, &mut self.timer);
        }
        quit
    }

    /// Shows the beamer viewport. The closure only reads `self`, so it can
    /// borrow immutably while `update` keeps `&mut self` for the rest.
    fn show_presentation(&self, ctx: &Context) {
        ctx.show_viewport_immediate(
            ViewportId::from_hash_of("presentation"),
            ViewportBuilder::default().with_title("rumpress-beamer"),
            |ctx, _class| self.draw_presentation(ctx),
        );
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
                let key = PageKey { page: self.state.current_page, half: self.slide_half() };
                let Some(tex) = self.textures.get(&key) else { return };
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
            });
    }

    /// Content of the presenter window: current slide, next-page preview,
    /// notes, and a timer bar (stub; Tasks 9/10 fill in bar and overview).
    fn draw_presenter(&self, ctx: &Context) {
        CentralPanel::default().show(ctx, |ui| {
            let row = ui.available_height() / 2.0;
            let cur = PageKey { page: self.state.current_page, half: self.slide_half() };
            let next = PageKey {
                page: self.state.current_page.saturating_add(1),
                half: self.slide_half(),
            };
            let notes = PageKey { page: self.state.current_page, half: self.notes_half() };

            ui.horizontal(|ui| {
                ui.set_height(row);
                let half_w = ui.available_width() / 2.0;
                ui.allocate_ui(vec2(half_w, row), |ui| {
                    ui.label(format!(
                        "Slide {} / {}",
                        self.state.current_slide + 1,
                        self.slides.slide_count()
                    ));
                    show_fitted(ui, self.textures.get(&cur));
                });
                ui.allocate_ui(vec2(half_w, row), |ui| {
                    ui.label("Next:");
                    show_fitted(ui, self.textures.get(&next));
                });
            });
            ui.horizontal(|ui| {
                ui.set_height(row);
                let half_w = ui.available_width() / 2.0;
                ui.allocate_ui(vec2(half_w, row), |ui| {
                    draw_timer_bar(ui, &self.timer);
                });
                ui.allocate_ui(vec2(half_w, row), |ui| {
                    ui.label("Notes:");
                    show_fitted(ui, self.textures.get(&notes));
                });
            });
        });
    }

    /// Reads key events from `ctx` (whichever viewport is being drawn) into
    /// the pending queue.
    fn collect_key_actions(&self, ctx: &Context) {
        let actions: Vec<Action> = ctx.input(|i| {
            i.events
                .iter()
                .filter_map(|event| match event {
                    Event::Key { key, pressed: true, modifiers, .. } => {
                        Some(input::map_key(*key, *modifiers))
                    }
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

        self.poll_render(ctx);
        self.show_presentation(ctx);

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

/// Timer readout and progress bar (stub; Task 9 builds the real bar).
fn draw_timer_bar(ui: &mut egui::Ui, timer: &Timer) {
    let elapsed = timer.elapsed(Instant::now());
    ui.label(format!("⏱ {}", format_duration(elapsed)));
    if let Some(target) = timer.target()
        && target.as_secs_f32() > 0.0 {
            let frac = (elapsed.as_secs_f32() / target.as_secs_f32()).clamp(0.0, 1.0);
            ui.add(ProgressBar::new(frac));
        }
}

/// Formats a duration as `MM:SS` (hours fold into minutes).
fn format_duration(d: Duration) -> String {
    let secs = d.as_secs();
    format!("{:02}:{:02}", secs / 60, secs % 60)
}
