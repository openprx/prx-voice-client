//! Settings, history, debug and call translation as their own native windows.
//!
//! The orb window stays small and never changes size or position because of
//! them. Each tool surface is an egui viewport with a normal OS title bar,
//! movable and resizable between a minimum and a maximum size. This module
//! holds the open/focus/close state and the placement rules (pure, unit
//! tested) and the one function that shows a window.
//!
//! Immediate viewports are used: the window content borrows the UI state
//! directly, with no shared-state plumbing. See [`show`] for the platform
//! notes. The settings window also remembers the monitor it was on: a
//! remembered position is used only while the orb's monitor has the same
//! size (design 2.1a, multi-monitor rule 1).

use egui::{
    Pos2, Rect, Vec2, ViewportBuilder, ViewportClass, ViewportCommand, ViewportId, pos2, vec2,
};

use crate::theme::{Tokens, layout, space};

/// The tool surfaces that open in their own window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ToolKind {
    /// The settings window.
    Settings,
    /// The session history window.
    History,
    /// The debug window.
    Debug,
    /// The Call translation window: everything needed to run a translated call.
    Call,
}

/// Width of the settings content: the standard's 480 px.
pub const SETTINGS_CONTENT_WIDTH: f32 = layout::DIALOG_WIDTH;
/// Padding between the window edge and the content.
pub const WINDOW_PADDING: f32 = space::S24;
/// Gap between the orb window and a window opened beside it.
pub const PLACEMENT_GAP: f32 = space::S12;
/// The strip of a window (its title bar) that must stay on screen: the top 40 px
/// across 200 px of width (design 2.1a, multi-monitor rule 1).
const GRAB_STRIP: Vec2 = vec2(200.0, 40.0);

impl ToolKind {
    /// Every kind.
    pub const ALL: [ToolKind; 4] = [
        ToolKind::Settings,
        ToolKind::History,
        ToolKind::Debug,
        ToolKind::Call,
    ];

    /// The viewport id of this window.
    pub fn viewport_id(self) -> ViewportId {
        ViewportId::from_hash_of(match self {
            ToolKind::Settings => "prxvc-window-settings",
            ToolKind::History => "prxvc-window-history",
            ToolKind::Debug => "prxvc-window-debug",
            ToolKind::Call => "prxvc-window-call",
        })
    }

    /// Key under which the window geometry is persisted.
    pub fn storage_key(self) -> &'static str {
        match self {
            ToolKind::Settings => "window_settings",
            ToolKind::History => "window_history",
            ToolKind::Debug => "window_debug",
            ToolKind::Call => "window_call",
        }
    }

    /// Translated window title.
    pub fn title(self) -> String {
        prxvc_i18n::t(match self {
            ToolKind::Settings => "settings.title",
            ToolKind::History => "history.title",
            ToolKind::Debug => "debug.title",
            ToolKind::Call => "callwin.title",
        })
    }

    /// Inner size on first open: the content width plus its padding.
    pub fn default_size(self) -> Vec2 {
        match self {
            ToolKind::Settings => vec2(SETTINGS_CONTENT_WIDTH + 2.0 * WINDOW_PADDING, 640.0),
            ToolKind::History | ToolKind::Debug => vec2(420.0, 520.0),
            ToolKind::Call => vec2(420.0, 640.0),
        }
    }

    /// Smallest inner size.
    pub fn min_size(self) -> Vec2 {
        match self {
            ToolKind::Settings => vec2(layout::DIALOG_WIDTH, 480.0),
            ToolKind::History | ToolKind::Debug => vec2(320.0, 300.0),
            ToolKind::Call => vec2(360.0, 360.0),
        }
    }

    /// Largest inner size.
    pub fn max_size(self) -> Vec2 {
        match self {
            ToolKind::Settings => vec2(1100.0, 1000.0),
            ToolKind::History | ToolKind::Debug | ToolKind::Call => vec2(760.0, 1000.0),
        }
    }
}

/// Position (outer top-left) and inner size of a window, in points.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Geometry {
    /// Outer top-left corner.
    pub pos: Pos2,
    /// Inner size.
    pub size: Vec2,
}

impl Geometry {
    /// `(x, y, width, height)`, the form stored in the eframe storage.
    pub fn to_tuple(self) -> (f32, f32, f32, f32) {
        (self.pos.x, self.pos.y, self.size.x, self.size.y)
    }

    /// Inverse of [`to_tuple`](Self::to_tuple); `None` for values that are
    /// not finite or have no area.
    pub fn from_tuple((x, y, w, h): (f32, f32, f32, f32)) -> Option<Self> {
        let finite = [x, y, w, h].iter().all(|v| v.is_finite());
        (finite && w > 0.0 && h > 0.0).then(|| Self {
            pos: pos2(x, y),
            size: vec2(w, h),
        })
    }
}

/// Move `pos` so that a window of `size` lies inside `screen` (pinned to the
/// top-left when it is larger than the screen).
pub fn clamp_into(screen: Rect, pos: Pos2, size: Vec2) -> Pos2 {
    let max_x = (screen.right() - size.x).max(screen.left());
    let max_y = (screen.bottom() - size.y).max(screen.top());
    pos2(
        pos.x.clamp(screen.left(), max_x),
        pos.y.clamp(screen.top(), max_y),
    )
}

/// Where a window opens for the first time: beside the orb window (right of
/// it, or left when there is no room), top-aligned with it, and fully on
/// screen.
pub fn first_open_position(orb: Rect, size: Vec2, screen: Rect) -> Pos2 {
    let right = orb.right() + PLACEMENT_GAP;
    let x = if right + size.x <= screen.right() {
        right
    } else {
        orb.left() - PLACEMENT_GAP - size.x
    };
    clamp_into(screen, pos2(x, orb.top()), size)
}

/// Clamp a size to the window's limits.
pub fn clamp_size(kind: ToolKind, size: Vec2) -> Vec2 {
    let (min, max) = (kind.min_size(), kind.max_size());
    vec2(size.x.clamp(min.x, max.x), size.y.clamp(min.y, max.y))
}

/// Move `pos` so that the title strip of a window of `size` lies on `screen`:
/// at least `GRAB_STRIP` of it is inside, the rest may hang over an edge.
pub fn clamp_title_strip(screen: Rect, pos: Pos2, size: Vec2) -> Pos2 {
    let strip_w = GRAB_STRIP.x.min(size.x);
    let min_x = screen.left() - (size.x - strip_w);
    let max_x = (screen.right() - strip_w).max(min_x);
    let max_y = (screen.bottom() - GRAB_STRIP.y).max(screen.top());
    pos2(pos.x.clamp(min_x, max_x), pos.y.clamp(screen.top(), max_y))
}

/// Geometry for opening `kind`: the remembered one, its size clamped to the
/// limits and its title bar moved onto `screen` when it is not there (design
/// 2.1a rule 1), otherwise a fresh placement beside `orb`. Without a known
/// screen the remembered position is trusted and a fresh placement falls back
/// to the orb's corner.
pub fn initial_geometry(
    kind: ToolKind,
    saved: Option<Geometry>,
    orb: Option<Rect>,
    screen: Option<Rect>,
) -> Geometry {
    if let Some(saved) = saved {
        let size = clamp_size(kind, saved.size);
        let pos = match screen {
            Some(screen) => clamp_title_strip(screen, saved.pos, size),
            None => saved.pos,
        };
        return Geometry { pos, size };
    }
    let size = kind.default_size();
    let orb = orb.unwrap_or_else(|| Rect::from_min_size(pos2(40.0, 40.0), vec2(220.0, 320.0)));
    let pos = match screen {
        Some(screen) => first_open_position(orb, size, screen),
        None => pos2(orb.right() + PLACEMENT_GAP, orb.top()),
    };
    Geometry { pos, size }
}

/// Open/focus/close state and remembered geometry of one window.
#[derive(Debug, Clone, Default)]
pub struct ToolWindowState {
    open: bool,
    focus_pending: bool,
    /// Geometry the window was created with (kept constant while open so the
    /// builder passed every frame does not move the window).
    placed: Option<Geometry>,
    /// Latest observed geometry, persisted across launches.
    current: Option<Geometry>,
    /// Size of the monitor the window was on when `current` was recorded.
    monitor: Option<Vec2>,
}

impl ToolWindowState {
    /// Whether the window is open.
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Open the window, or, when it is already open, ask for it to be brought
    /// to the front instead of opening another.
    pub fn request_open(&mut self) {
        self.open = true;
        self.focus_pending = true;
    }

    /// Close the window.
    pub fn close(&mut self) {
        self.open = false;
        self.placed = None;
    }

    /// Whether a focus request is waiting; consumes it.
    pub fn take_focus(&mut self) -> bool {
        std::mem::take(&mut self.focus_pending)
    }

    /// Remember geometry loaded from storage (used by the next first open).
    pub fn restore(&mut self, geometry: Geometry) {
        self.current = Some(geometry);
    }

    /// Remember geometry and the monitor size recorded with it (loaded from
    /// the configuration).
    pub fn restore_on_monitor(&mut self, geometry: Geometry, monitor: Option<Vec2>) {
        self.current = Some(geometry);
        self.monitor = monitor;
    }

    /// Record the geometry the OS reports for the open window.
    pub fn observe(&mut self, geometry: Geometry) {
        self.current = Some(geometry);
    }

    /// Record the size of the monitor the open window is on.
    pub fn observe_monitor(&mut self, monitor: Vec2) {
        self.monitor = Some(monitor);
    }

    /// Size of the monitor recorded with the latest geometry.
    pub fn monitor(&self) -> Option<Vec2> {
        self.monitor
    }

    /// Latest known geometry, for persistence.
    pub fn geometry(&self) -> Option<Geometry> {
        self.current
    }

    /// Geometry to create the window with; fixed until the window closes.
    fn placement(
        &mut self,
        kind: ToolKind,
        orb: Option<Rect>,
        screen: Option<Rect>,
        monitor: Option<Vec2>,
    ) -> Geometry {
        // A position recorded on a monitor of another size is not trusted.
        let same_monitor = match (self.monitor, monitor) {
            (Some(saved), Some(now)) => (saved - now).abs().max_elem() < 1.0,
            _ => true,
        };
        let saved = self.current.filter(|_| same_monitor);
        *self
            .placed
            .get_or_insert_with(|| initial_geometry(kind, saved, orb, screen))
    }
}

/// The four tool windows.
#[derive(Debug, Clone, Default)]
pub struct ToolWindows {
    /// Settings.
    pub settings: ToolWindowState,
    /// History.
    pub history: ToolWindowState,
    /// Debug.
    pub debug: ToolWindowState,
    /// Call translation.
    pub call: ToolWindowState,
}

impl ToolWindows {
    /// State of `kind`.
    pub fn get(&self, kind: ToolKind) -> &ToolWindowState {
        match kind {
            ToolKind::Settings => &self.settings,
            ToolKind::History => &self.history,
            ToolKind::Debug => &self.debug,
            ToolKind::Call => &self.call,
        }
    }

    /// Mutable state of `kind`.
    pub fn get_mut(&mut self, kind: ToolKind) -> &mut ToolWindowState {
        match kind {
            ToolKind::Settings => &mut self.settings,
            ToolKind::History => &mut self.history,
            ToolKind::Debug => &mut self.debug,
            ToolKind::Call => &mut self.call,
        }
    }

    /// Open `kind` or focus it when already open.
    pub fn request_open(&mut self, kind: ToolKind) {
        self.get_mut(kind).request_open();
    }
}

/// Where the orb window is on screen, as far as the platform tells.
#[derive(Debug, Clone, Copy, Default)]
pub struct Placement {
    /// Size of the monitor the orb is on, as the platform reports it.
    pub monitor: Option<Vec2>,
    /// The orb window's outer rectangle in screen points.
    pub orb: Option<Rect>,
    /// The screen the orb is on. winit reports only the monitor size, not its
    /// origin, so this is known only when the orb lies within a rectangle of
    /// that size at the origin (the primary monitor).
    pub screen: Option<Rect>,
}

impl Placement {
    /// Read the placement from the root viewport.
    pub fn from_root(ctx: &egui::Context) -> Self {
        let (orb, monitor) = ctx.input(|i| {
            let v = i.viewport();
            (v.outer_rect, v.monitor_size)
        });
        let screen = match (orb, monitor) {
            (Some(orb), Some(m)) => {
                let screen = Rect::from_min_size(Pos2::ZERO, m);
                screen.contains(orb.center()).then_some(screen)
            }
            _ => None,
        };
        Self {
            monitor,
            orb,
            screen,
        }
    }
}

/// Show the window of `kind` if it is open.
///
/// `add_body` draws the content into a panel that fills the window.
///
/// Platform notes (eframe 0.31.1, glow): immediate viewports are supported
/// and are rendered from inside the parent's frame, so the orb window must
/// keep running for the settings window to update (it repaints every frame
/// for the orb animation, and input in the child makes eframe repaint the
/// parent). eframe persists only the root window, hence the geometry kept
/// here. Every viewport is cleared with a transparent colour, but the child
/// window is created opaque and its panel fills it. Window creation happens
/// on the main thread, where `update` runs.
pub fn show(
    ctx: &egui::Context,
    tk: &Tokens,
    kind: ToolKind,
    state: &mut ToolWindowState,
    placement: Placement,
    mut add_body: impl FnMut(&mut egui::Ui),
) {
    show_raw(ctx, tk, kind, state, placement, |ui| {
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| add_body(ui));
    });
}

/// The padded, filled frame every tool window draws its content in.
pub fn window_frame(tk: &Tokens) -> egui::Frame {
    egui::Frame::NONE
        .fill(tk.bg)
        .inner_margin(egui::Margin::same(WINDOW_PADDING as i8))
}

/// Like [`show`], but `add_body` draws straight into the padded frame and
/// does its own scrolling (the settings window scrolls only its content
/// column, not its section list).
pub fn show_raw(
    ctx: &egui::Context,
    tk: &Tokens,
    kind: ToolKind,
    state: &mut ToolWindowState,
    placement: Placement,
    mut body: impl FnMut(&mut egui::Ui),
) {
    if !state.open {
        return;
    }
    let geometry = state.placement(kind, placement.orb, placement.screen, placement.monitor);
    let title = kind.title();
    let builder = ViewportBuilder::default()
        .with_title(title.clone())
        .with_position(geometry.pos)
        .with_inner_size(geometry.size)
        .with_min_inner_size(kind.min_size())
        .with_max_inner_size(kind.max_size())
        .with_resizable(true)
        .with_decorations(true)
        .with_transparent(false);

    ctx.show_viewport_immediate(kind.viewport_id(), builder, |ui, class| {
        let ctx = ui.ctx().clone();
        let frame = window_frame(tk);
        let (close_requested, escape, close_chord) = ctx.input_mut(|i| {
            (
                i.viewport().close_requested(),
                i.key_pressed(egui::Key::Escape),
                i.consume_key(egui::Modifiers::COMMAND, egui::Key::W),
            )
        });
        // Escape closes this window only: not while a drop-down is open or a
        // text field has the keyboard (the first press leaves the field).
        let escape = escape && !ctx.any_popup_open() && !ctx.egui_wants_keyboard_input();
        if close_requested || escape || close_chord {
            state.close();
            return;
        }
        if class == ViewportClass::EmbeddedWindow {
            // No native windows (tests, backends without viewport support):
            // egui has already wrapped this callback in a `Window`.
            frame.show(ui, |ui| body(ui));
            return;
        }

        if state.take_focus() {
            ctx.send_viewport_cmd(ViewportCommand::Focus);
        }
        let (outer, inner, monitor) = ctx.input(|i| {
            let v = i.viewport();
            (v.outer_rect, v.inner_rect, v.monitor_size)
        });
        if let (Some(outer), Some(inner)) = (outer, inner) {
            state.observe(Geometry {
                pos: outer.min,
                size: inner.size(),
            });
            if let Some(monitor) = monitor {
                state.observe_monitor(monitor);
            }
        }
        egui::CentralPanel::default()
            .frame(frame)
            .show(ui, |ui| body(ui));
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCREEN: Rect = Rect::from_min_max(pos2(0.0, 0.0), pos2(1440.0, 900.0));

    fn orb_at(x: f32, y: f32) -> Rect {
        Rect::from_min_size(pos2(x, y), vec2(220.0, 320.0))
    }

    #[test]
    fn open_when_closed_opens_and_asks_for_focus() {
        let mut s = ToolWindowState::default();
        assert!(!s.is_open());
        s.request_open();
        assert!(s.is_open());
        assert!(s.take_focus());
        assert!(!s.take_focus(), "the request is consumed");
    }

    #[test]
    fn second_open_request_focuses_the_existing_window() {
        let mut s = ToolWindowState::default();
        s.request_open();
        s.take_focus();
        s.request_open();
        assert!(s.is_open());
        assert!(s.take_focus(), "a repeated request asks for focus");
    }

    #[test]
    fn close_closes_only_that_window_and_it_can_reopen() {
        let mut w = ToolWindows::default();
        w.request_open(ToolKind::Settings);
        w.request_open(ToolKind::Debug);
        w.get_mut(ToolKind::Settings).close();
        assert!(!w.settings.is_open());
        assert!(w.debug.is_open());
        w.request_open(ToolKind::Settings);
        assert!(w.settings.is_open());
    }

    #[test]
    fn placement_is_fixed_while_open_and_recomputed_after_close() {
        let mut s = ToolWindowState::default();
        s.request_open();
        let a = s.placement(
            ToolKind::Settings,
            Some(orb_at(100.0, 100.0)),
            Some(SCREEN),
            None,
        );
        // The window was moved by the user; the creation geometry stays.
        s.observe(Geometry {
            pos: pos2(500.0, 300.0),
            size: vec2(600.0, 700.0),
        });
        let b = s.placement(
            ToolKind::Settings,
            Some(orb_at(900.0, 100.0)),
            Some(SCREEN),
            None,
        );
        assert_eq!(a, b);
        s.close();
        let c = s.placement(
            ToolKind::Settings,
            Some(orb_at(900.0, 100.0)),
            Some(SCREEN),
            None,
        );
        assert_eq!(c.pos, pos2(500.0, 300.0), "reopens where the user left it");
        assert_eq!(c.size, vec2(600.0, 700.0));
    }

    #[test]
    fn first_open_is_right_of_the_orb_and_on_screen() {
        let size = ToolKind::Settings.default_size();
        let pos = first_open_position(orb_at(100.0, 100.0), size, SCREEN);
        assert_eq!(pos, pos2(100.0 + 220.0 + PLACEMENT_GAP, 100.0));
    }

    #[test]
    fn first_open_goes_left_when_there_is_no_room_on_the_right() {
        let size = ToolKind::Settings.default_size();
        let orb = orb_at(1200.0, 100.0);
        let pos = first_open_position(orb, size, SCREEN);
        assert_eq!(pos.x, 1200.0 - PLACEMENT_GAP - size.x);
        assert!(pos.x >= 0.0);
    }

    #[test]
    fn first_open_is_clamped_vertically_and_never_off_screen() {
        let size = ToolKind::Settings.default_size();
        for orb in [
            orb_at(100.0, 800.0),
            orb_at(-50.0, -50.0),
            orb_at(1400.0, 880.0),
            orb_at(0.0, 0.0),
        ] {
            let pos = first_open_position(orb, size, SCREEN);
            let r = Rect::from_min_size(pos, size);
            assert!(SCREEN.contains_rect(r), "{orb:?} -> {r:?}");
        }
    }

    #[test]
    fn a_window_larger_than_the_screen_is_pinned_to_the_corner() {
        let small = Rect::from_min_max(pos2(0.0, 0.0), pos2(300.0, 300.0));
        assert_eq!(
            clamp_into(small, pos2(200.0, 200.0), vec2(528.0, 640.0)),
            pos2(0.0, 0.0)
        );
    }

    #[test]
    fn saved_geometry_is_restored_when_reachable() {
        let saved = Geometry {
            pos: pos2(300.0, 200.0),
            size: vec2(600.0, 700.0),
        };
        let g = initial_geometry(
            ToolKind::Settings,
            Some(saved),
            Some(orb_at(10.0, 10.0)),
            Some(SCREEN),
        );
        assert_eq!(g, saved);
    }

    #[test]
    fn saved_size_is_clamped_to_the_limits() {
        let saved = Geometry {
            pos: pos2(300.0, 200.0),
            size: vec2(5000.0, 10.0),
        };
        let g = initial_geometry(ToolKind::Settings, Some(saved), None, Some(SCREEN));
        assert_eq!(
            g.size,
            vec2(
                ToolKind::Settings.max_size().x,
                ToolKind::Settings.min_size().y
            )
        );
    }

    #[test]
    fn saved_geometry_off_screen_is_moved_back_so_the_title_bar_is_reachable() {
        let saved = Geometry {
            pos: pos2(5000.0, 3000.0),
            size: vec2(528.0, 640.0),
        };
        let g = initial_geometry(
            ToolKind::Settings,
            Some(saved),
            Some(orb_at(100.0, 100.0)),
            Some(SCREEN),
        );
        let strip = Rect::from_min_size(g.pos, vec2(g.size.x, 40.0)).intersect(SCREEN);
        assert!(strip.width() >= 200.0 && strip.height() >= 40.0, "{g:?}");
        assert_eq!(g.size, saved.size);
    }

    #[test]
    fn a_window_partly_off_the_left_edge_stays_while_200_px_of_its_title_bar_show() {
        let saved = Geometry {
            pos: pos2(-300.0, 50.0),
            size: vec2(528.0, 640.0),
        };
        let g = initial_geometry(ToolKind::Settings, Some(saved), None, Some(SCREEN));
        assert_eq!(
            g.pos, saved.pos,
            "228 px of the title bar are still visible"
        );
        let saved = Geometry {
            pos: pos2(-400.0, 50.0),
            size: vec2(528.0, 640.0),
        };
        let g = initial_geometry(ToolKind::Settings, Some(saved), None, Some(SCREEN));
        assert_eq!(g.pos.x, -328.0, "moved right until 200 px show");
    }

    #[test]
    fn a_position_from_a_monitor_of_another_size_is_not_used() {
        let mut s = ToolWindowState::default();
        s.restore_on_monitor(
            Geometry {
                pos: pos2(300.0, 200.0),
                size: vec2(600.0, 700.0),
            },
            Some(vec2(2560.0, 1440.0)),
        );
        s.request_open();
        let g = s.placement(
            ToolKind::Settings,
            Some(orb_at(100.0, 100.0)),
            Some(SCREEN),
            Some(vec2(1440.0, 900.0)),
        );
        assert_eq!(
            g.pos,
            pos2(100.0 + 220.0 + PLACEMENT_GAP, 100.0),
            "beside the orb"
        );
        assert_eq!(g.size, ToolKind::Settings.default_size());
    }

    #[test]
    fn a_position_from_a_monitor_of_the_same_size_is_restored() {
        let mut s = ToolWindowState::default();
        let saved = Geometry {
            pos: pos2(300.0, 200.0),
            size: vec2(600.0, 700.0),
        };
        s.restore_on_monitor(saved, Some(vec2(1440.0, 900.0)));
        s.request_open();
        let g = s.placement(
            ToolKind::Settings,
            Some(orb_at(100.0, 100.0)),
            Some(SCREEN),
            Some(vec2(1440.0, 900.0)),
        );
        assert_eq!(g, saved);
    }

    #[test]
    fn without_a_known_screen_saved_geometry_is_trusted() {
        let saved = Geometry {
            pos: pos2(5000.0, 3000.0),
            size: vec2(528.0, 640.0),
        };
        assert_eq!(
            initial_geometry(ToolKind::Settings, Some(saved), None, None),
            saved
        );
    }

    #[test]
    fn geometry_tuple_round_trips_and_rejects_garbage() {
        let g = Geometry {
            pos: pos2(1.0, -2.0),
            size: vec2(528.0, 640.0),
        };
        assert_eq!(Geometry::from_tuple(g.to_tuple()), Some(g));
        assert_eq!(Geometry::from_tuple((f32::NAN, 0.0, 10.0, 10.0)), None);
        assert_eq!(Geometry::from_tuple((0.0, 0.0, 0.0, 10.0)), None);
    }

    #[test]
    fn settings_default_width_gives_the_standard_content_width() {
        assert_eq!(
            ToolKind::Settings.default_size().x - 2.0 * WINDOW_PADDING,
            SETTINGS_CONTENT_WIDTH
        );
        assert_eq!(SETTINGS_CONTENT_WIDTH, 480.0);
    }
}
