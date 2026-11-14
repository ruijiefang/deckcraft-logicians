//! Insert ▸ Commutative Diagram: draw a diagram on a grid (click a cell for an object, drag
//! from one object to another for an arrow) and style it beside the picture.
//!
//! Every change is an engine function of the diagram ([`deckcraft_engine::typeset::cd::edit`]);
//! the picture is the engine's own layout, drawn by the renderer that draws the slide, so what the
//! canvas shows is what goes on the slide. The slide changes once, through `insert.latex` or
//! `latex.set`, so the document keeps tikz-cd LaTeX and gets one undo step.

use deckcraft_engine::typeset::cd::edit::{self, Selection};
use deckcraft_engine::typeset::cd::{self, At, Body, Diagram, End, Frame, Grid, Head, Loop, Rgb, Sep, Side, Tail};
use deckcraft_engine::typeset::{Part, Region};
use deckcraft_geom::{BezPath, PathEl};
use deckcraft_model::ShapeId;
use egui::text::{CCursor, CCursorRange};
use egui::{
    Align, Color32, Event, Id, Key, Layout, Modifiers, Pos2, Rect, RichText, ScrollArea, Sense, Stroke, StrokeKind, TextureHandle, Ui, Vec2, pos2,
    vec2,
};
use serde_json::{Value, json};

use super::{Dialog, SlideApp, Tokens};

/// Steps of undo the editor keeps.
const HISTORY: usize = 200;
/// Width of the side panel.
const SIDE: f32 = 340.0;
/// The renderer's margin around a rendered shape, in points.
const MARGIN: f64 = 8.0;
/// The largest preview texture side, in pixels.
const MAX_PX: f64 = 4096.0;
/// Empty rows and columns around the diagram, room to grow.
const RING: usize = 1;
/// How near an arrow a click selects it, in screen pixels.
const NEAR: f32 = 7.0;

/// Symbols the strip inserts into the label being edited: (shown, LaTeX, shown when the
/// interface font can't draw it).
const SYMBOLS: &[(&str, &str, &str)] = &[
    ("∘", r"\circ ", "∘"),
    ("×", r"\times ", "×"),
    ("⊗", r"\otimes ", "⊗"),
    ("⊕", r"\oplus ", "⊕"),
    ("≅", r"\cong ", "≅"),
    ("≃", r"\simeq ", "≃"),
    ("⊣", r"\dashv ", "⊣"),
    ("⊥", r"\bot ", "⊥"),
    ("∃!", r"\exists! ", "∃!"),
    ("•", r"\bullet ", "•"),
    ("𝒞", r"\mathcal{C}", "cal C"),
    ("𝒟", r"\mathcal{D}", "cal D"),
    ("α", r"\alpha ", "α"),
    ("β", r"\beta ", "β"),
    ("η", r"\eta ", "η"),
    ("ε", r"\varepsilon ", "ε"),
    ("ι", r"\iota ", "ι"),
    ("π", r"\pi ", "π"),
    ("φ", r"\varphi ", "φ"),
    ("⌟", r"\lrcorner ", "⌟"),
];

/// Colours offered beside the picker: (name, RGB); the first is the text colour.
const PALETTE: &[(&str, Option<Rgb>)] = &[
    ("Text colour", None),
    ("Red", Some([214, 72, 72])),
    ("Orange", Some([230, 130, 30])),
    ("Green", Some([60, 150, 80])),
    ("Teal", Some([0, 128, 128])),
    ("Blue", Some([60, 100, 210])),
    ("Purple", Some([140, 70, 170])),
    ("Gray", Some([128, 128, 128])),
];

/// What the window does after this frame.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Outcome {
    Stay,
    /// Cancel, Esc: asks first when there are changes.
    Close,
    Discard,
    /// Insert (or Update); `true`: insert a new diagram even when editing one.
    Submit(bool),
    /// Switch to the Equation dialog on this LaTeX.
    EditLatex,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Action {
    Undo,
    Redo,
    Copy,
    Cut,
    Paste,
    Delete,
    Reverse,
    FlipAcross,
    FlipUpDown,
    Rotate,
    Corner,
    SelectAll,
}

/// What a label field types into.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Typing {
    Object(At),
    Arrow(usize),
}

/// A drag in progress on the picture.
#[derive(Clone, Debug)]
enum Drag {
    /// Drawing an arrow from an object (or an arrow).
    Arrow { from: End },
    /// Moving objects by whole cells (Alt-drag), from this cell.
    Move { objects: Vec<At>, from: (i64, i64) },
    /// Bending an arrow by its handle.
    Bend { arrow: usize },
    /// Moving a label along or across its arrow.
    Label { arrow: usize, label: usize },
    /// Selecting what a box covers.
    Box { start: Pos2 },
}

#[derive(Clone)]
struct Snap {
    d: Diagram,
    sel: Selection,
}

/// The picture of the diagram: its rendering and where its parts and grid are.
struct Canvas {
    key: u64,
    shape: Option<deckcraft_model::Shape>,
    regions: Vec<Region>,
    grid: Grid,
    w: f64,
    h: f64,
    tex: Option<TextureHandle>,
    /// Pixels per slide point of `tex`.
    px: f64,
    error: Option<String>,
}

/// What a point on the picture is over.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Hit {
    Object(At),
    Label(usize, usize),
    Arrow(usize),
    /// An empty cell, as a diagram row and column (one outside the diagram is -1 or its size).
    Empty(i64, i64),
}

/// The canvas's mapping between slide points and the screen.
#[derive(Clone, Copy)]
struct View {
    origin: Pos2,
    /// Screen points per slide point.
    k: f64,
}

impl View {
    fn to_screen(self, x: f64, y: f64) -> Pos2 {
        self.origin + vec2(((x + MARGIN) * self.k) as f32, ((y + MARGIN) * self.k) as f32)
    }
    fn to_slide(self, p: Pos2) -> (f64, f64) {
        let k = if self.k > 1e-9 { self.k } else { 1.0 };
        (f64::from(p.x - self.origin.x) / k - MARGIN, f64::from(p.y - self.origin.y) / k - MARGIN)
    }
    fn rect(self, r: &Region) -> Rect {
        Rect::from_min_max(self.to_screen(r.x, r.y), self.to_screen(r.x + r.w, r.y + r.h))
    }
}

/// The editor while its window is open (dropped when it closes).
pub struct Editor {
    pub(crate) d: Diagram,
    pub(crate) sel: Selection,
    /// The cell the keyboard is at, as a diagram row and column.
    cursor: (i64, i64),
    undo: Vec<Snap>,
    redo: Vec<Snap>,
    /// The diagram shape this updates; `None` inserts a new one.
    target: Option<u64>,
    size: f64,
    font: String,
    notices: Vec<String>,
    status: Option<String>,
    clipboard: Option<Diagram>,
    source: Option<String>,
    source_error: Option<String>,
    canvas: Option<Canvas>,
    zoom: f64,
    /// Focus the label field next frame (selecting all its text when `true`).
    focus_label: Option<bool>,
    typing: Option<Typing>,
    typing_backup: Option<Diagram>,
    drag: Option<Drag>,
    confirm_discard: bool,
    dirty: bool,
    label_had_focus: bool,
}

pub(super) fn dialog(app: &mut SlideApp, ui: &mut Ui, d: &mut Dialog) -> bool {
    let mut e = match app.cd_editor.take() {
        Some(e) => e,
        None => Editor::open(app, d),
    };
    let close = e.show(app, ui);
    if !close {
        app.cd_editor = Some(e);
    }
    close
}

/// Plain text for the interface: math spaces as ordinary spaces.
fn ui_text(s: &str) -> String {
    s.chars().map(|c| if matches!(c, '\u{2000}'..='\u{200B}' | '\u{205F}' | '\u{A0}') { ' ' } else { c }).collect()
}

/// `text` when the interface fonts can draw all of it, else `fallback`.
fn drawable(ctx: &egui::Context, text: &str, fallback: impl FnOnce() -> String) -> String {
    let font = egui::FontId::proportional(14.0);
    let text = ui_text(text);
    if ctx.fonts_mut(|f| f.has_glyphs(&font, &text)) { text } else { fallback() }
}

/// An object or label as the interface can show it: typeset text, else its LaTeX.
fn shown(ctx: &egui::Context, src: &str) -> String {
    let src = src.trim();
    if src.is_empty() {
        return "(empty)".into();
    }
    drawable(ctx, &cd::plain(src), || src.to_string())
}

fn label_id() -> Id {
    Id::unique("cd-label")
}

/// Points along each piece of `path`, for egui to draw.
fn polylines(path: &BezPath, map: impl Fn(f64, f64) -> Pos2) -> Vec<Vec<Pos2>> {
    let mut out: Vec<Vec<Pos2>> = Vec::new();
    let mut last = (0.0, 0.0);
    for el in path.elements() {
        match *el {
            PathEl::MoveTo(p) => {
                out.push(vec![map(p.x, p.y)]);
                last = (p.x, p.y);
            }
            PathEl::LineTo(p) => {
                if let Some(l) = out.last_mut() {
                    l.push(map(p.x, p.y));
                }
                last = (p.x, p.y);
            }
            PathEl::QuadTo(a, p) => {
                for i in 1..=8 {
                    let t = f64::from(i) / 8.0;
                    let u = 1.0 - t;
                    let x = u * u * last.0 + 2.0 * u * t * a.x + t * t * p.x;
                    let y = u * u * last.1 + 2.0 * u * t * a.y + t * t * p.y;
                    if let Some(l) = out.last_mut() {
                        l.push(map(x, y));
                    }
                }
                last = (p.x, p.y);
            }
            PathEl::CurveTo(a, b, p) => {
                for i in 1..=12 {
                    let t = f64::from(i) / 12.0;
                    let u = 1.0 - t;
                    let x = u * u * u * last.0 + 3.0 * u * u * t * a.x + 3.0 * u * t * t * b.x + t * t * t * p.x;
                    let y = u * u * u * last.1 + 3.0 * u * u * t * a.y + 3.0 * u * t * t * b.y + t * t * t * p.y;
                    if let Some(l) = out.last_mut() {
                        l.push(map(x, y));
                    }
                }
                last = (p.x, p.y);
            }
            PathEl::ClosePath => {
                if let Some(l) = out.last_mut()
                    && let Some(&first) = l.first()
                {
                    l.push(first);
                }
            }
        }
    }
    out
}

/// Distance from `p` to the polyline `poly`.
fn distance(p: Pos2, poly: &[Pos2]) -> f32 {
    let mut best = f32::INFINITY;
    for w in poly.windows(2) {
        let (a, b) = (w[0], w[1]);
        let ab = b - a;
        let len = ab.length_sq();
        let t = if len > 1e-6 { ((p - a).dot(ab) / len).clamp(0.0, 1.0) } else { 0.0 };
        best = best.min((a + ab * t - p).length());
    }
    if poly.len() == 1 {
        best = best.min((poly[0] - p).length());
    }
    best
}

/// A small picture of an arrow's look, drawn by the diagram's own code.
fn look_button(ui: &mut Ui, look: (Tail, Body, Head), selected: bool, enabled: bool, tip: &str) -> egui::Response {
    let t = Tokens::get(ui.ctx());
    let size = vec2(50.0, 28.0);
    let (rect, resp) = ui.allocate_exact_size(size, if enabled { Sense::click() } else { Sense::hover() });
    let resp = resp.on_hover_text(tip);
    resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, enabled, selected, tip));
    let fill = if selected {
        t.selected
    } else if resp.hovered() && enabled {
        t.hover
    } else {
        Color32::TRANSPARENT
    };
    ui.painter().rect(rect, 4.0, fill, Stroke::new(1.0, if selected { t.accent } else { t.border }), StrokeKind::Inside);
    let color = if enabled { t.text } else { t.text_faint };
    let (tail, body, head) = look;
    if body == Body::Phantom {
        ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, "none", egui::FontId::proportional(11.0), color);
        return resp;
    }
    let len = 36.0;
    let em = 16.0;
    let left = rect.center().x - len as f32 / 2.0;
    let map = |x: f64, y: f64| pos2(left + x as f32, rect.center().y + y as f32);
    for (path, width, dash) in cd::sample(tail, body, head, len, em) {
        let stroke = Stroke::new((width as f32).max(1.0), color);
        for line in polylines(&path, map) {
            match dash {
                Some((on, off)) => {
                    let w = stroke.width;
                    ui.painter().extend(egui::Shape::dashed_line(&line, stroke, on as f32 * w * 0.6, off as f32 * w * 0.6));
                }
                None => {
                    ui.painter().add(egui::Shape::line(line, stroke));
                }
            }
        }
    }
    resp
}

impl Editor {
    fn empty() -> Self {
        Editor {
            d: Diagram::default(),
            sel: Selection::default(),
            cursor: (0, 0),
            undo: Vec::new(),
            redo: Vec::new(),
            target: None,
            size: 24.0,
            font: deckcraft_engine::cmd::latex::DEFAULT_FONT.to_string(),
            notices: Vec::new(),
            status: None,
            clipboard: None,
            source: None,
            source_error: None,
            canvas: None,
            zoom: 1.0,
            focus_label: None,
            typing: None,
            typing_backup: None,
            drag: None,
            confirm_discard: false,
            dirty: false,
            label_had_focus: false,
        }
    }

    /// The editor on the diagram it was opened for: the Equation dialog's LaTeX
    /// (`params.latex`), else the selected diagram, else an empty grid.
    fn open(app: &mut SlideApp, d: &Dialog) -> Self {
        let mut e = Editor::empty();
        let found = app.session.execute("latex.get", &json!({})).ok();
        let selected = found.filter(|v| {
            v.get("kind").and_then(Value::as_str) == Some("typeset") && v.get("latex").and_then(Value::as_str).is_some_and(cd::is_diagram)
        });
        let from_dialog = d.params.get("latex").and_then(Value::as_str).map(str::to_string);
        let editing = from_dialog.is_none() || d.params.get("edit").and_then(Value::as_bool) == Some(true);
        let src = from_dialog.or_else(|| selected.as_ref().and_then(|v| v.get("latex")).and_then(Value::as_str).map(str::to_string));
        if let Some(v) = selected.as_ref().filter(|_| editing) {
            e.target = v.get("id").and_then(Value::as_u64);
            e.size = v.get("size").and_then(Value::as_f64).unwrap_or(e.size);
            e.font = v.get("font").and_then(Value::as_str).unwrap_or(&e.font).to_string();
        }
        if let Some(src) = src {
            match cd::open(&src) {
                Ok(o) => {
                    e.d = o.diagram;
                    e.notices = o.notices;
                }
                Err(err) => {
                    e.status = Some(format!("This LaTeX can't be edited as a diagram ({err}). Starting a new one."));
                    e.target = None;
                }
            }
        }
        e
    }

    // -----------------------------------------------------------------------------------------
    // Changes and undo.

    fn snapshot(&self) -> Snap {
        Snap { d: self.d.clone(), sel: self.sel.clone() }
    }

    fn remember(&mut self, before: Snap) {
        self.undo.push(before);
        if self.undo.len() > HISTORY {
            self.undo.remove(0);
        }
        self.redo.clear();
        self.dirty = true;
    }

    /// Runs an edit: on success one undo step with the selection it returns, on failure the
    /// diagram as it was and the reason shown.
    fn apply(&mut self, f: impl FnOnce(&mut Diagram) -> Result<Selection, String>) -> bool {
        let before = self.snapshot();
        match f(&mut self.d) {
            Ok(sel) => {
                self.remember(before);
                self.sel = sel;
                self.typing = None;
                self.status = None;
                true
            }
            Err(e) => {
                self.d = before.d;
                self.status = Some(e);
                false
            }
        }
    }

    /// The first change to a label since it was selected is one undo step, and what Esc in its
    /// field goes back to.
    fn begin_typing(&mut self, what: Typing) {
        if self.typing != Some(what) {
            let before = self.snapshot();
            self.typing_backup = Some(before.d.clone());
            self.remember(before);
            self.typing = Some(what);
        }
    }

    /// One undo step for a style change of the selection (each change while a control is
    /// dragged stays in the same step).
    fn restyle(&mut self, f: impl Fn(&mut cd::Arrow)) {
        let before = self.snapshot();
        let mut changed = false;
        for &i in &self.sel.arrows {
            if let Some(a) = self.d.arrows.get_mut(i) {
                let old = a.clone();
                f(a);
                changed |= *a != old;
            }
        }
        if changed {
            self.remember(before);
            self.typing = None;
        }
    }

    fn restyle_objects(&mut self, f: impl Fn(&mut cd::Object)) {
        let before = self.snapshot();
        let mut changed = false;
        for o in &mut self.d.objects {
            if self.sel.objects.contains(&o.at) {
                let old = o.clone();
                f(o);
                changed |= *o != old;
            }
        }
        if changed {
            self.remember(before);
            self.typing = None;
        }
    }

    fn undo(&mut self) {
        if let Some(s) = self.undo.pop() {
            let now = self.snapshot();
            self.redo.push(now);
            self.d = s.d;
            self.sel = s.sel;
            self.typing = None;
            self.dirty = true;
        }
    }

    fn redo(&mut self) {
        if let Some(s) = self.redo.pop() {
            let now = self.snapshot();
            self.undo.push(now);
            self.d = s.d;
            self.sel = s.sel;
            self.typing = None;
            self.dirty = true;
        }
    }

    /// Drops selected things that aren't there any more.
    fn check_selection(&mut self) {
        let d = &self.d;
        self.sel.objects.retain(|at| d.object(*at).is_some());
        self.sel.arrows.retain(|i| *i < d.arrows.len());
        if let Some(Typing::Arrow(i)) = self.typing
            && i >= d.arrows.len()
        {
            self.typing = None;
        }
    }

    /// Adds an object at a diagram cell (one outside the diagram grows it), selects it and puts
    /// the keyboard in its label.
    fn add_object(&mut self, row: i64, col: i64) {
        let mut at = None;
        if self.apply(|d| {
            let (a, _) = edit::add_object(d, row, col, r"\bullet")?;
            at = Some(a);
            Ok(Selection::object(a))
        }) && let Some(a) = at
        {
            self.cursor = (a.row as i64, a.col as i64);
            self.focus_label = Some(true);
        }
    }

    fn add_arrow(&mut self, from: End, to: End) {
        if self.apply(|d| edit::add_arrow(d, from, to).map(Selection::arrow)) {
            self.status = None;
        }
    }

    fn act(&mut self, a: Action, ctx: &egui::Context) {
        match a {
            Action::Undo => self.undo(),
            Action::Redo => self.redo(),
            Action::SelectAll => {
                self.sel = Selection { objects: self.d.objects.iter().map(|o| o.at).collect(), arrows: (0..self.d.arrows.len()).collect() };
            }
            Action::Copy | Action::Cut => {
                if self.sel.is_empty() {
                    self.status = Some("Select something to copy first.".into());
                    return;
                }
                let part = edit::extract(&self.d, &self.sel);
                ctx.copy_text(cd::write(&part));
                self.clipboard = Some(part);
                if a == Action::Cut {
                    let sel = self.sel.clone();
                    self.apply(|d| {
                        edit::remove(d, &sel);
                        Ok(Selection::default())
                    });
                }
            }
            Action::Paste => self.paste(None),
            Action::Delete => {
                let sel = self.sel.clone();
                if !sel.is_empty() {
                    self.apply(|d| {
                        edit::remove(d, &sel);
                        Ok(Selection::default())
                    });
                }
            }
            Action::Reverse => {
                let arrows = self.sel.arrows.clone();
                if arrows.is_empty() {
                    self.status = Some("Select an arrow to turn round.".into());
                    return;
                }
                let sel = self.sel.clone();
                self.apply(|d| {
                    for &i in &arrows {
                        edit::reverse(d, i);
                    }
                    Ok(sel)
                });
            }
            Action::FlipAcross | Action::FlipUpDown | Action::Rotate => {
                self.apply(|d| {
                    match a {
                        Action::FlipAcross => edit::flip(d, true),
                        Action::FlipUpDown => edit::flip(d, false),
                        _ => edit::rotate(d),
                    }
                    Ok(Selection::default())
                });
            }
            Action::Corner => self.corner(),
        }
    }

    /// Marks the selected object as a pullback or pushout corner: ⌟ inside the square it spans
    /// with the object diagonally across.
    fn corner(&mut self) {
        let Some(&at) = self.sel.objects.first() else {
            self.status = Some("Select the corner object of a square (its pullback or pushout).".into());
            return;
        };
        let options = [(1i64, 1i64, r"\lrcorner"), (1, -1, r"\llcorner"), (-1, 1, r"\urcorner"), (-1, -1, r"\ulcorner")];
        let found = options.iter().find_map(|&(dr, dc, sym)| {
            let (r, c) = (at.row as i64 + dr, at.col as i64 + dc);
            (r >= 0 && c >= 0 && self.d.object(At::new(r as usize, c as usize)).is_some()).then(|| (At::new(r as usize, c as usize), sym))
        });
        let Some((across, sym)) = found else {
            self.status = Some("A corner needs an object diagonally across from it.".into());
            return;
        };
        self.apply(|d| {
            let i = edit::add_arrow(d, End::Cell(at), End::Cell(across))?;
            if let Some(a) = d.arrows.get_mut(i) {
                a.body = Body::Phantom;
                let mut l = cd::Label::new(sym);
                l.pos = 0.125;
                a.labels.push(l);
            }
            Ok(Selection::arrow(i))
        });
    }

    /// Pastes `text` when it is a diagram, else what was copied, at the keyboard's cell.
    fn paste(&mut self, text: Option<&str>) {
        let from_text = text.filter(|t| cd::is_diagram(t)).and_then(|t| cd::open(t).ok()).map(|o| o.diagram);
        let Some(part) = from_text.or_else(|| self.clipboard.clone()) else {
            self.status = Some("Copy something first (Ctrl+C).".into());
            return;
        };
        let (r, c) = self.cursor;
        let at = At::new(r.max(0) as usize, c.max(0) as usize);
        self.apply(|d| edit::paste(d, &part, at));
    }

    fn template(&mut self, name: &str) {
        let Some(t) = edit::template(name) else { return };
        if self.d.objects.is_empty() && self.d.arrows.is_empty() {
            self.apply(|d| {
                *d = t;
                Ok(Selection::default())
            });
            return;
        }
        // Beside what is there.
        let (_, cols) = self.d.size();
        self.apply(|d| edit::paste(d, &t, At::new(0, cols + 1)));
    }

    fn can_submit(&self) -> bool {
        !self.d.objects.is_empty() || !self.d.arrows.is_empty()
    }

    /// The diagram as it goes on the slide: empty labels dropped, arrows in the cells' order.
    fn final_latex(&self) -> String {
        let mut d = self.d.clone();
        edit::drop_empty_labels(&mut d);
        edit::tidy(&mut d);
        cd::write(&d)
    }

    // -----------------------------------------------------------------------------------------
    // The window.

    fn show(&mut self, app: &mut SlideApp, ui: &mut Ui) -> bool {
        let mut out = Outcome::Stay;
        self.check_selection();
        self.keys(ui, &mut out);
        egui::Panel::top("cd-toolbar").resizable(false).show(ui, |ui| self.toolbar(ui));
        egui::Panel::bottom("cd-footer").resizable(false).show(ui, |ui| {
            self.source_pane(ui, &mut out);
            ui.separator();
            self.footer(ui, &mut out);
        });
        egui::Panel::right("cd-side").exact_size(SIDE).resizable(false).show(ui, |ui| {
            ScrollArea::vertical().id_salt("cd-side").auto_shrink([false, false]).show(ui, |ui| self.side(ui));
        });
        egui::CentralPanel::default().show(ui, |ui| self.canvas(app, ui));
        match out {
            Outcome::Stay => false,
            Outcome::Close if self.dirty && !self.confirm_discard => {
                self.confirm_discard = true;
                false
            }
            Outcome::Close | Outcome::Discard => true,
            Outcome::Submit(new) => self.can_submit() && self.submit(app, new),
            Outcome::EditLatex => {
                self.to_latex_dialog(app);
                true
            }
        }
    }

    fn submit(&mut self, app: &mut SlideApp, as_new: bool) -> bool {
        let mut p = json!({"latex": self.final_latex(), "size": self.size, "font": self.font});
        let r = match self.target.filter(|_| !as_new) {
            Some(id) => {
                p["id"] = json!(id);
                app.run("latex.set", p)
            }
            None => app.run("insert.latex", p),
        };
        match r {
            Ok(_) => true,
            Err(e) => {
                self.status = Some(e.clone());
                app.set_status(e);
                false
            }
        }
    }

    fn to_latex_dialog(&self, app: &mut SlideApp) {
        let mut d = Dialog::new("equation");
        let editing = self.target.is_some().to_string();
        d.fields.insert("eq".into(), self.final_latex());
        d.fields.insert("as".into(), "typeset".into());
        d.fields.insert("found".into(), editing.clone());
        d.fields.insert("edit".into(), editing);
        app.dialog = Some(d);
    }

    fn keys(&mut self, ui: &Ui, out: &mut Outcome) {
        let ctx = ui.ctx().clone();
        let consume = |m: Modifiers, k: Key| ctx.input_mut(|i| i.consume_key(m, k));
        if consume(Modifiers::COMMAND, Key::Enter) {
            *out = Outcome::Submit(false);
            return;
        }
        // Esc in a label puts it back and leaves the field; it doesn't close the editor.
        if self.label_had_focus && consume(Modifiers::NONE, Key::Escape) {
            if let Some(backup) = self.typing_backup.take() {
                self.d = backup;
                self.typing = None;
            }
            ctx.memory_mut(|m| {
                if let Some(id) = m.focused() {
                    m.surrender_focus(id);
                }
            });
            self.label_had_focus = false;
            return;
        }
        if ctx.text_edit_focused() {
            return;
        }
        let action = if consume(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z) || consume(Modifiers::COMMAND, Key::Y) {
            Some(Action::Redo)
        } else if consume(Modifiers::COMMAND, Key::Z) {
            Some(Action::Undo)
        } else if consume(Modifiers::COMMAND, Key::A) {
            Some(Action::SelectAll)
        } else if consume(Modifiers::NONE, Key::Delete) || consume(Modifiers::NONE, Key::Backspace) {
            Some(Action::Delete)
        } else {
            None
        };
        if let Some(a) = action {
            return self.act(a, &ctx);
        }
        // Alt+arrows move the selected objects a cell; arrows move the keyboard's cell.
        for (key, dr, dc) in [(Key::ArrowUp, -1, 0), (Key::ArrowDown, 1, 0), (Key::ArrowLeft, 0, -1), (Key::ArrowRight, 0, 1)] {
            if consume(Modifiers::ALT, key) {
                let objects = self.sel.objects.clone();
                if !objects.is_empty() {
                    let arrows = self.sel.arrows.clone();
                    self.apply(|d| edit::move_objects(d, &objects, dr, dc).map(|o| Selection { objects: o, arrows }));
                }
                return;
            }
            if consume(Modifiers::NONE, key) {
                self.cursor = ((self.cursor.0 + dr).max(-1), (self.cursor.1 + dc).max(-1));
                let at = (self.cursor.0 >= 0 && self.cursor.1 >= 0).then(|| At::new(self.cursor.0 as usize, self.cursor.1 as usize));
                self.sel = match at.filter(|a| self.d.object(*a).is_some()) {
                    Some(a) => Selection::object(a),
                    None => Selection::default(),
                };
                self.typing = None;
                return;
            }
        }
        if consume(Modifiers::NONE, Key::Enter) || consume(Modifiers::NONE, Key::F2) {
            if self.sel.objects.len() + self.sel.arrows.len() == 1 {
                self.focus_label = Some(false);
            } else if self.sel.is_empty() {
                let (r, c) = self.cursor;
                self.add_object(r, c);
            }
            return;
        }
        if consume(Modifiers::NONE, Key::Escape) {
            if self.sel.is_empty() {
                *out = Outcome::Close;
            } else {
                self.sel = Selection::default();
                self.typing = None;
            }
        }
        for e in ctx.input(|i| i.events.clone()) {
            match e {
                Event::Copy => self.act(Action::Copy, &ctx),
                Event::Cut => self.act(Action::Cut, &ctx),
                Event::Paste(t) => self.paste(Some(&t)),
                _ => {}
            }
        }
    }

    fn toolbar(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        let has = !self.sel.is_empty();
        let arrows = !self.sel.arrows.is_empty();
        let any = self.can_submit();
        let mut action = None;
        ui.horizontal_wrapped(|ui| {
            let mut tool = |ui: &mut Ui, label: &str, tip: &str, on: bool, a: Action| {
                if ui.add_enabled(on, egui::Button::new(label)).on_hover_text(tip).clicked() {
                    action = Some(a);
                }
            };
            tool(ui, "Undo", "Undo (Ctrl+Z)", !self.undo.is_empty(), Action::Undo);
            tool(ui, "Redo", "Redo (Ctrl+Shift+Z)", !self.redo.is_empty(), Action::Redo);
            ui.separator();
            tool(ui, "Copy", "Copy the selection (Ctrl+C)", has, Action::Copy);
            tool(ui, "Cut", "Cut the selection (Ctrl+X)", has, Action::Cut);
            tool(ui, "Paste", "Paste at the selected cell (Ctrl+V)", self.clipboard.is_some(), Action::Paste);
            tool(ui, "Delete", "Delete the selection (Delete)", has, Action::Delete);
            ui.separator();
            tool(ui, "Reverse", "Turn the selected arrows round", arrows, Action::Reverse);
            tool(ui, "Flip ↔", "Mirror the diagram left to right", any, Action::FlipAcross);
            tool(ui, "Flip ↕", "Mirror the diagram top to bottom", any, Action::FlipUpDown);
            tool(ui, "Rotate", "Turn the diagram a quarter turn clockwise", any, Action::Rotate);
            tool(ui, "Corner ⌟", "Mark the selected object as a pullback or pushout corner", !self.sel.objects.is_empty(), Action::Corner);
        });
        ui.horizontal_wrapped(|ui| {
            let mut chosen = None;
            ui.menu_button("Templates", |ui| {
                for (name, _) in edit::TEMPLATES {
                    if ui.button(*name).clicked() {
                        chosen = Some(*name);
                        ui.close();
                    }
                }
            })
            .response
            .on_hover_text("Start from a common diagram (added beside what is there)");
            if let Some(name) = chosen {
                self.template(name);
            }
            ui.separator();
            if ui.button("Zoom out").clicked() {
                self.zoom = (self.zoom / 1.25).max(0.25);
            }
            if ui.button("Fit").clicked() {
                self.zoom = 1.0;
            }
            if ui.button("Zoom in").clicked() {
                self.zoom = (self.zoom * 1.25).min(4.0);
            }
            ui.separator();
            ui.label("Size");
            ui.add(egui::DragValue::new(&mut self.size).range(6.0..=96.0).speed(0.5).suffix(" pt"));
            ui.label("Font");
            ui.add(egui::TextEdit::singleline(&mut self.font).desired_width(140.0).hint_text("theme font"));
        });
        if let Some(a) = action {
            self.act(a, &ctx);
        }
    }

    // -----------------------------------------------------------------------------------------
    // The picture.

    fn refresh(&mut self, app: &SlideApp, ctx: &egui::Context, avail: Vec2) {
        use std::hash::{Hash, Hasher};
        let src = cd::write(&self.d);
        let mut h = std::collections::hash_map::DefaultHasher::new();
        (src.as_str(), self.size.to_bits(), self.font.as_str()).hash(&mut h);
        let key = h.finish();
        let Ok(st) = app.session.doc() else { return };
        if self.canvas.as_ref().is_none_or(|c| c.key != key) {
            let font = Some(self.font.trim()).filter(|f| !f.is_empty());
            self.canvas = Some(match deckcraft_engine::cmd::latex::preview_diagram(st, &self.d, self.size, font, RING) {
                Ok(p) => Canvas {
                    key,
                    shape: Some(p.preview.shape),
                    regions: p.preview.regions,
                    grid: p.grid,
                    w: p.preview.w,
                    h: p.preview.h,
                    tex: None,
                    px: 0.0,
                    error: None,
                },
                Err(e) => Canvas { key, shape: None, regions: Vec::new(), grid: Grid::default(), w: 0.0, h: 0.0, tex: None, px: 0.0, error: Some(e) },
            });
        }
        let Some(c) = self.canvas.as_mut() else { return };
        let Some(shape) = c.shape.clone() else { return };
        let ppp = f64::from(ctx.pixels_per_point());
        let fit = ((f64::from(avail.x) - 24.0) / (c.w + 2.0 * MARGIN)).min((f64::from(avail.y) - 24.0) / (c.h + 2.0 * MARGIN)).clamp(0.3, 2.5);
        let px = (fit * self.zoom * ppp).min(MAX_PX / (c.w.max(c.h) + 2.0 * MARGIN)).max(0.05);
        if c.tex.is_some() && (px - c.px).abs() < 0.01 {
            return;
        }
        let pres = &st.doc;
        let Some(mut slide) = pres.slides.get(st.selection.slide).or_else(|| pres.slides.first()).map(|s| s.as_ref().clone()) else { return };
        let id: ShapeId = shape.id;
        slide.shapes = vec![shape];
        c.px = px;
        // An empty diagram renders nothing; the grid is drawn regardless.
        c.tex = deckcraft_render::render_shape(pres, &slide, id, px)
            .map(|img| crate::textures::upload(ctx, "cd-preview", crate::textures::to_color_image(&img, false)));
    }

    /// The bands of the grid's rows or columns: each from halfway to the one before to halfway
    /// to the one after.
    fn bands(spans: &[(f64, f64)]) -> Vec<(f64, f64)> {
        let n = spans.len();
        (0..n)
            .map(|i| {
                let (lo, hi) = spans[i];
                let before = if i > 0 { (spans[i - 1].1 + lo) / 2.0 } else { lo - spans.get(1).map_or(10.0, |s| (s.0 - hi) / 2.0).max(4.0) };
                let after = if i + 1 < n {
                    (hi + spans[i + 1].0) / 2.0
                } else {
                    hi + spans.get(n.wrapping_sub(2)).map_or(10.0, |s| (lo - s.1) / 2.0).max(4.0)
                };
                (before, after)
            })
            .collect()
    }

    /// What the slide point (`x`, `y`) is over.
    fn hit(&self, c: &Canvas, view: &View, p: Pos2) -> Option<Hit> {
        let objects = c.regions.iter().filter(|r| r.part == Part::Object);
        for r in objects.clone() {
            if view.rect(r).expand(2.0).contains(p) {
                return Some(Hit::Object(At::new(r.path.first()?.checked_sub(RING)?, r.path.get(1)?.checked_sub(RING)?)));
            }
        }
        for r in c.regions.iter().filter(|r| r.part == Part::ArrowLabel) {
            if view.rect(r).contains(p) {
                return Some(Hit::Label(*r.path.first()?, *r.path.get(1)?));
            }
        }
        let mut best: Option<(f32, usize)> = None;
        for r in c.regions.iter().filter(|r| r.part == Part::Arrow) {
            let poly: Vec<Pos2> = r.poly.iter().map(|(x, y)| view.to_screen(*x, *y)).collect();
            let dist = distance(p, &poly);
            if dist <= NEAR && best.is_none_or(|(b, _)| dist < b) {
                best = Some((dist, *r.path.first()?));
            }
        }
        if let Some((_, i)) = best {
            return Some(Hit::Arrow(i));
        }
        let (x, y) = view.to_slide(p);
        let rows = Self::bands(&c.grid.rows);
        let cols = Self::bands(&c.grid.cols);
        let r = rows.iter().position(|(a, b)| y >= *a && y < *b)?;
        let col = cols.iter().position(|(a, b)| x >= *a && x < *b)?;
        let (dr, dc) = (r as i64 - RING as i64, col as i64 - RING as i64);
        if dr >= 0 && dc >= 0 && self.d.object(At::new(dr as usize, dc as usize)).is_some() {
            return Some(Hit::Object(At::new(dr as usize, dc as usize)));
        }
        Some(Hit::Empty(dr, dc))
    }

    /// Where an end of an arrow is on screen: an object's middle, an arrow's.
    fn end_point(&self, c: &Canvas, view: &View, e: End) -> Option<Pos2> {
        match e {
            End::Cell(at) => {
                c.regions.iter().find(|r| r.part == Part::Object && r.path == [at.row + RING, at.col + RING]).map(|r| view.rect(r).center()).or_else(
                    || {
                        let row = c.grid.rows.get(at.row + RING)?;
                        let col = c.grid.cols.get(at.col + RING)?;
                        Some(view.to_screen((col.0 + col.1) / 2.0, (row.0 + row.1) / 2.0))
                    },
                )
            }
            End::Arrow(i) => {
                let r = c.regions.iter().find(|r| r.part == Part::Arrow && r.path == [i])?;
                let mid = r.poly.get(r.poly.len() / 2)?;
                Some(view.to_screen(mid.0, mid.1))
            }
        }
    }

    fn canvas(&mut self, app: &SlideApp, ui: &mut Ui) {
        let avail = ui.available_size();
        self.refresh(app, ui.ctx(), avail);
        let Some(c) = self.canvas.take() else { return };
        if let Some(e) = &c.error {
            ui.colored_label(Color32::from_rgb(0xC0, 0x30, 0x30), format!("The diagram can't be drawn: {e}"));
            self.canvas = Some(c);
            return;
        }
        let ppp = f64::from(ui.ctx().pixels_per_point());
        let k = if c.px > 0.0 { c.px / ppp } else { 1.0 };
        let img = vec2(((c.w + 2.0 * MARGIN) * k) as f32, ((c.h + 2.0 * MARGIN) * k) as f32);
        ScrollArea::both().id_salt("cd-canvas").auto_shrink([false, false]).show(ui, |ui| {
            let size = img.max(ui.available_size() - vec2(4.0, 4.0));
            let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
            let origin = rect.min + ((size - img) / 2.0).max(Vec2::ZERO);
            let view = View { origin, k };
            ui.painter().rect_filled(rect, 4.0, Color32::WHITE);
            self.grid_lines(ui, &c, &view);
            if let Some(tex) = &c.tex {
                ui.painter().image(tex.id(), Rect::from_min_size(origin, img), Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
            }
            self.cells(ui, &c, &view);
            self.highlights(ui, &c, &view);
            self.bend_handle(ui, &c, &view);
            self.dragging(ui, &c, &view);
        });
        self.canvas = Some(c);
    }

    fn grid_lines(&self, ui: &Ui, c: &Canvas, view: &View) {
        let rows = Self::bands(&c.grid.rows);
        let cols = Self::bands(&c.grid.cols);
        let (Some(top), Some(bottom)) = (rows.first().map(|r| r.0), rows.last().map(|r| r.1)) else { return };
        let (Some(left), Some(right)) = (cols.first().map(|c| c.0), cols.last().map(|c| c.1)) else { return };
        let stroke = Stroke::new(1.0, Color32::from_gray(232));
        let p = ui.painter();
        for (i, (a, _)) in rows.iter().enumerate() {
            if i > 0 {
                p.line_segment([view.to_screen(left, *a), view.to_screen(right, *a)], stroke);
            }
        }
        for (i, (a, _)) in cols.iter().enumerate() {
            if i > 0 {
                p.line_segment([view.to_screen(*a, top), view.to_screen(*a, bottom)], stroke);
            }
        }
        p.rect_stroke(Rect::from_min_max(view.to_screen(left, top), view.to_screen(right, bottom)), 2.0, stroke, StrokeKind::Inside);
    }

    /// One interactive cell per grid cell (what is clicked, dragged and read out), and arrows'
    /// handles.
    fn cells(&mut self, ui: &mut Ui, c: &Canvas, view: &View) {
        let t = Tokens::get(ui.ctx());
        let rows = Self::bands(&c.grid.rows);
        let cols = Self::bands(&c.grid.cols);
        for (r, (y0, y1)) in rows.iter().enumerate() {
            for (col, (x0, x1)) in cols.iter().enumerate() {
                let cell = Rect::from_min_max(view.to_screen(*x0, *y0), view.to_screen(*x1, *y1));
                let (dr, dc) = (r as i64 - RING as i64, col as i64 - RING as i64);
                let at = (dr >= 0 && dc >= 0).then(|| At::new(dr as usize, dc as usize));
                let object = at.and_then(|a| self.d.object(a));
                let label = match object {
                    Some(o) => format!("Object: {}", shown(ui.ctx(), &o.label)),
                    None => format!("Empty cell {}, {}", dr + 1, dc + 1),
                };
                let selected = at.is_some_and(|a| self.sel.objects.contains(&a));
                let resp = ui.interact(cell, Id::unique(("cd-cell", r, col)), Sense::click_and_drag());
                resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, selected, &label));
                if resp.hovered() && self.drag.is_none() && object.is_none() {
                    let p = ui.painter();
                    p.rect_stroke(cell.shrink(3.0), 3.0, Stroke::new(1.0, t.text_faint), StrokeKind::Inside);
                    p.text(cell.center(), egui::Align2::CENTER_CENTER, "+", egui::FontId::proportional(16.0), t.text_faint);
                }
                self.cell_input(ui, c, view, &resp, (dr, dc));
            }
        }
        // Each arrow's handle in the middle: a target to click, and what is read out.
        for r in c.regions.iter().filter(|r| r.part == Part::Arrow) {
            let Some(&i) = r.path.first() else { continue };
            let Some(mid) = r.poly.get(r.poly.len() / 2) else { continue };
            let at = view.to_screen(mid.0, mid.1);
            let rect = Rect::from_center_size(at, vec2(12.0, 12.0));
            let resp = ui.interact(rect, Id::unique(("cd-arrow", i)), Sense::click_and_drag());
            let name = self.arrow_name(ui.ctx(), i);
            let selected = self.sel.arrows.contains(&i);
            resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, selected, format!("Arrow: {name}")));
            self.cell_input(ui, c, view, &resp, (-2, -2));
        }
    }

    /// An arrow in words: `f: A → B`.
    fn arrow_name(&self, ctx: &egui::Context, i: usize) -> String {
        let Some(a) = self.d.arrows.get(i) else { return String::new() };
        let end = |e: End| match e {
            End::Cell(at) => self.d.object(at).map_or_else(|| "·".to_string(), |o| shown(ctx, &o.label)),
            End::Arrow(j) => format!("arrow {}", j + 1),
        };
        let line = format!("{} → {}", end(a.from), end(a.to));
        match a.labels.first() {
            Some(l) if !l.text.trim().is_empty() => format!("{}: {line}", shown(ctx, &l.text)),
            _ => line,
        }
    }

    /// Clicks, double-clicks and drags starting on the picture.
    fn cell_input(&mut self, ui: &Ui, c: &Canvas, view: &View, resp: &egui::Response, cell: (i64, i64)) {
        let mods = ui.input(|i| i.modifiers);
        let pointer = resp.interact_pointer_pos().or_else(|| resp.hover_pos());
        let hit = || pointer.and_then(|p| self.hit(c, view, p));
        if resp.double_clicked() {
            match hit() {
                Some(Hit::Object(at)) => {
                    self.sel = Selection::object(at);
                    self.focus_label = Some(false);
                }
                Some(Hit::Arrow(i) | Hit::Label(i, _)) => {
                    self.sel = Selection::arrow(i);
                    self.focus_label = Some(false);
                }
                Some(Hit::Empty(r, col)) if self.d.object_at_signed(r, col).is_none() => {}
                _ => {}
            }
            return;
        }
        if resp.clicked() {
            let toggle = mods.shift || mods.command;
            match hit() {
                Some(Hit::Object(at)) => {
                    self.cursor = (at.row as i64, at.col as i64);
                    if toggle {
                        if let Some(k) = self.sel.objects.iter().position(|a| *a == at) {
                            self.sel.objects.remove(k);
                        } else {
                            self.sel.objects.push(at);
                        }
                    } else {
                        self.sel = Selection::object(at);
                    }
                    self.typing = None;
                }
                Some(Hit::Arrow(i) | Hit::Label(i, _)) => {
                    if toggle {
                        if let Some(k) = self.sel.arrows.iter().position(|a| *a == i) {
                            self.sel.arrows.remove(k);
                        } else {
                            self.sel.arrows.push(i);
                        }
                    } else {
                        self.sel = Selection::arrow(i);
                    }
                    self.typing = None;
                }
                Some(Hit::Empty(r, col)) => {
                    self.cursor = (r, col);
                    // The first click away from a selection only lets it go; while a new object's
                    // label is being typed, the next empty cell gets the next object.
                    if self.sel.is_empty() || matches!(self.typing, Some(Typing::Object(_))) {
                        self.add_object(r, col);
                    } else {
                        self.sel = Selection::default();
                        self.typing = None;
                    }
                }
                None => {
                    let _ = cell;
                }
            }
            return;
        }
        if resp.drag_started() {
            // Where the press began: a drag is reported once the pointer has moved away.
            let start = ui.input(|i| i.pointer.press_origin()).or_else(|| resp.interact_pointer_pos()).unwrap_or_default();
            self.drag = match self.hit(c, view, start) {
                Some(Hit::Object(at)) if mods.alt => {
                    let objects = if self.sel.objects.contains(&at) { self.sel.objects.clone() } else { vec![at] };
                    Some(Drag::Move { objects, from: (at.row as i64, at.col as i64) })
                }
                Some(Hit::Object(at)) => Some(Drag::Arrow { from: End::Cell(at) }),
                Some(Hit::Label(i, l)) if self.sel.arrows == [i] => {
                    let before = self.snapshot();
                    self.remember(before);
                    Some(Drag::Label { arrow: i, label: l })
                }
                Some(Hit::Arrow(i) | Hit::Label(i, _)) => Some(Drag::Arrow { from: End::Arrow(i) }),
                Some(Hit::Empty(..)) | None => Some(Drag::Box { start }),
            };
        }
    }

    fn highlights(&self, ui: &Ui, c: &Canvas, view: &View) {
        let t = Tokens::get(ui.ctx());
        let p = ui.painter();
        for r in c.regions.iter().filter(|r| r.part == Part::Object) {
            let at = (r.path.first().and_then(|v| v.checked_sub(RING)), r.path.get(1).and_then(|v| v.checked_sub(RING)));
            if let (Some(row), Some(col)) = at
                && self.sel.objects.contains(&At::new(row, col))
            {
                p.rect_stroke(view.rect(r).expand(3.0), 3.0, Stroke::new(2.0, t.accent), StrokeKind::Outside);
            }
        }
        let glow = Color32::from_rgba_unmultiplied(t.accent.r(), t.accent.g(), t.accent.b(), 90);
        for r in c.regions.iter().filter(|r| r.part == Part::Arrow) {
            if r.path.first().is_some_and(|i| self.sel.arrows.contains(i)) {
                let poly: Vec<Pos2> = r.poly.iter().map(|(x, y)| view.to_screen(*x, *y)).collect();
                p.add(egui::Shape::line(poly, Stroke::new(7.0, glow)));
            }
        }
        for r in c.regions.iter().filter(|r| r.part == Part::ArrowLabel) {
            if r.path.first().is_some_and(|i| self.sel.arrows.contains(i)) {
                p.rect_stroke(view.rect(r), 2.0, Stroke::new(1.0, t.accent), StrokeKind::Outside);
            }
        }
        // The keyboard's cell, when nothing is selected.
        if self.sel.is_empty() {
            let rows = Self::bands(&c.grid.rows);
            let cols = Self::bands(&c.grid.cols);
            let (r, col) = ((self.cursor.0 + RING as i64) as usize, (self.cursor.1 + RING as i64) as usize);
            if let (Some(y), Some(x)) = (rows.get(r), cols.get(col)) {
                let rect = Rect::from_min_max(view.to_screen(x.0, y.0), view.to_screen(x.1, y.1)).shrink(2.0);
                p.rect_stroke(rect, 3.0, Stroke::new(1.0, t.accent), StrokeKind::Inside);
            }
        }
    }

    /// The selected arrow's handle: drag it sideways to bend the arrow.
    fn bend_handle(&mut self, ui: &mut Ui, c: &Canvas, view: &View) {
        let [i] = self.sel.arrows.as_slice() else { return };
        let i = *i;
        let Some(a) = self.d.arrows.get(i) else { return };
        if a.from == a.to || !self.sel.objects.is_empty() {
            return;
        }
        let Some(r) = c.regions.iter().find(|r| r.part == Part::Arrow && r.path == [i]) else { return };
        let Some(mid) = r.poly.get(r.poly.len() / 2) else { return };
        let at = view.to_screen(mid.0, mid.1);
        let t = Tokens::get(ui.ctx());
        let rect = Rect::from_center_size(at, vec2(14.0, 14.0));
        let resp = ui.interact(rect, Id::unique(("cd-bend", i)), Sense::drag());
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Slider, true, "Bend handle: drag sideways to bend the arrow"));
        let resp = resp.on_hover_text("Drag sideways to bend the arrow");
        ui.painter().circle(at, 5.5, t.handle_fill, Stroke::new(1.5, t.accent));
        if resp.drag_started() {
            let before = self.snapshot();
            self.remember(before);
            self.drag = Some(Drag::Bend { arrow: i });
        }
    }

    /// A drag in progress: its preview, and what it does when released.
    fn dragging(&mut self, ui: &Ui, c: &Canvas, view: &View) {
        let Some(drag) = self.drag.clone() else { return };
        let t = Tokens::get(ui.ctx());
        let pointer = ui.ctx().pointer_interact_pos();
        let released = ui.input(|i| i.pointer.any_released());
        let p = ui.painter();
        let Some(pos) = pointer else {
            if released {
                self.drag = None;
            }
            return;
        };
        match &drag {
            Drag::Arrow { from } => {
                if let Some(a) = self.end_point(c, view, *from) {
                    p.line_segment([a, pos], Stroke::new(2.0, t.accent));
                    p.circle_filled(pos, 3.5, t.accent);
                }
                if let Some(h) = self.hit(c, view, pos) {
                    self.mark_target(ui, c, view, h);
                }
                if released {
                    let target = self.hit(c, view, pos);
                    self.finish_arrow(*from, target);
                }
            }
            Drag::Move { objects, from } => {
                let target = self.cell_under(c, view, pos);
                if let Some((r, col)) = target {
                    self.mark_target(ui, c, view, Hit::Empty(r, col));
                    if released {
                        let (dr, dc) = (r - from.0, col - from.1);
                        let objects = objects.clone();
                        self.apply(|d| edit::move_objects(d, &objects, dr, dc).map(|o| Selection { objects: o, arrows: Vec::new() }));
                    }
                }
            }
            Drag::Bend { arrow } => {
                if let Some(r) = c.regions.iter().find(|r| r.part == Part::Arrow && r.path == [*arrow])
                    && let (Some(a), Some(b)) = (r.poly.first(), r.poly.last())
                {
                    let (a, b) = (view.to_screen(a.0, a.1), view.to_screen(b.0, b.1));
                    let chord = b - a;
                    let len = chord.length().max(1.0);
                    // Left of the arrow is positive; the middle of a bent arrow is 0.29 × its
                    // chord × sin(bend) to the side.
                    let side = (pos - a).x * chord.y - (pos - a).y * chord.x;
                    let h = side / len;
                    let s = (h / (0.2936 * len)).clamp(-1.0, 1.0);
                    let mut deg = f64::from(s.asin().to_degrees());
                    deg = (deg / 5.0).round() * 5.0;
                    if let Some(x) = self.d.arrows.get_mut(*arrow) {
                        x.bend = deg;
                    }
                }
            }
            Drag::Label { arrow, label } => {
                if let Some(r) = c.regions.iter().find(|r| r.part == Part::Arrow && r.path == [*arrow]) {
                    let poly: Vec<Pos2> = r.poly.iter().map(|(x, y)| view.to_screen(*x, *y)).collect();
                    let n = poly.len().saturating_sub(1).max(1);
                    let (k, _) = poly.iter().enumerate().fold((0, f32::INFINITY), |b, (k, q)| {
                        let d = (*q - pos).length();
                        if d < b.1 { (k, d) } else { b }
                    });
                    let pos_t = ((k as f64 / n as f64) * 20.0).round() / 20.0;
                    let (a, b) = (poly.get(k.saturating_sub(1)).copied(), poly.get((k + 1).min(n)).copied());
                    let side = match (a, b) {
                        (Some(a), Some(b)) => {
                            let d = b - a;
                            let cross = (pos - a).x * d.y - (pos - a).y * d.x;
                            if cross >= 0.0 { Side::Left } else { Side::Right }
                        }
                        _ => Side::Left,
                    };
                    if let Some(l) = self.d.arrows.get_mut(*arrow).and_then(|x| x.labels.get_mut(*label)) {
                        l.pos = pos_t;
                        if matches!(l.side, Side::Left | Side::Right) {
                            l.side = side;
                        }
                    }
                }
            }
            Drag::Box { start } => {
                let rect = Rect::from_two_pos(*start, pos);
                p.rect(
                    rect,
                    0.0,
                    Color32::from_rgba_unmultiplied(t.accent.r(), t.accent.g(), t.accent.b(), 30),
                    Stroke::new(1.0, t.accent),
                    StrokeKind::Inside,
                );
                if released {
                    self.select_box(c, view, rect);
                }
            }
        }
        if released {
            self.drag = None;
        }
    }

    /// The diagram cell under `pos`.
    fn cell_under(&self, c: &Canvas, view: &View, pos: Pos2) -> Option<(i64, i64)> {
        let (x, y) = view.to_slide(pos);
        let r = Self::bands(&c.grid.rows).iter().position(|(a, b)| y >= *a && y < *b)?;
        let col = Self::bands(&c.grid.cols).iter().position(|(a, b)| x >= *a && x < *b)?;
        Some((r as i64 - RING as i64, col as i64 - RING as i64))
    }

    fn mark_target(&self, ui: &Ui, c: &Canvas, view: &View, h: Hit) {
        let t = Tokens::get(ui.ctx());
        let p = ui.painter();
        match h {
            Hit::Object(at) => {
                if let Some(r) = c.regions.iter().find(|r| r.part == Part::Object && r.path == [at.row + RING, at.col + RING]) {
                    p.rect_stroke(view.rect(r).expand(3.0), 3.0, Stroke::new(2.0, t.accent), StrokeKind::Outside);
                }
            }
            Hit::Arrow(i) | Hit::Label(i, _) => {
                if let Some(r) = c.regions.iter().find(|r| r.part == Part::Arrow && r.path == [i]) {
                    let poly: Vec<Pos2> = r.poly.iter().map(|(x, y)| view.to_screen(*x, *y)).collect();
                    p.add(egui::Shape::line(poly, Stroke::new(5.0, t.accent)));
                }
            }
            Hit::Empty(r, col) => {
                let rows = Self::bands(&c.grid.rows);
                let cols = Self::bands(&c.grid.cols);
                if let (Some(y), Some(x)) = (rows.get((r + RING as i64).max(0) as usize), cols.get((col + RING as i64).max(0) as usize)) {
                    let rect = Rect::from_min_max(view.to_screen(x.0, y.0), view.to_screen(x.1, y.1)).shrink(3.0);
                    p.rect_stroke(rect, 3.0, Stroke::new(2.0, t.accent), StrokeKind::Inside);
                }
            }
        }
    }

    /// Ends an arrow being drawn from `from` at what the pointer is over: an object, an empty
    /// cell (which gets an object), or an arrow (a 2-cell).
    fn finish_arrow(&mut self, from: End, target: Option<Hit>) {
        let to = match target {
            Some(Hit::Object(at)) => End::Cell(at),
            Some(Hit::Arrow(i) | Hit::Label(i, _)) => End::Arrow(i),
            Some(Hit::Empty(r, col)) => {
                // A new object there, then the arrow (one undo step for both).
                let mut made = None;
                let ok = self.apply(|d| {
                    let (at, (sr, sc)) = edit::add_object(d, r, col, r"\bullet")?;
                    let moved = |e: End| match e {
                        End::Cell(a) => End::Cell(At::new(a.row + sr, a.col + sc)),
                        other => other,
                    };
                    let i = edit::add_arrow(d, moved(from), End::Cell(at))?;
                    made = Some(at);
                    Ok(Selection::arrow(i))
                });
                if ok && let Some(at) = made {
                    self.cursor = (at.row as i64, at.col as i64);
                }
                return;
            }
            None => return,
        };
        self.add_arrow(from, to);
    }

    fn select_box(&mut self, c: &Canvas, view: &View, rect: Rect) {
        let mut sel = Selection::default();
        for r in c.regions.iter() {
            match r.part {
                Part::Object if rect.intersects(view.rect(r)) => {
                    if let (Some(row), Some(col)) =
                        (r.path.first().and_then(|v| v.checked_sub(RING)), r.path.get(1).and_then(|v| v.checked_sub(RING)))
                    {
                        sel.objects.push(At::new(row, col));
                    }
                }
                Part::Arrow if r.poly.iter().any(|(x, y)| rect.contains(view.to_screen(*x, *y))) => {
                    if let Some(&i) = r.path.first() {
                        sel.arrows.push(i);
                    }
                }
                _ => {}
            }
        }
        self.sel = sel;
        self.typing = None;
    }

    // -----------------------------------------------------------------------------------------
    // The side panel.

    fn side(&mut self, ui: &mut Ui) {
        self.label_had_focus = false;
        let (no, na) = (self.sel.objects.len(), self.sel.arrows.len());
        match (no, na) {
            (0, 0) => self.diagram_panel(ui),
            (1, 0) => {
                if let Some(&at) = self.sel.objects.first() {
                    ui.strong("Object");
                    self.label_field(ui, Typing::Object(at));
                    self.symbol_strip(ui, Typing::Object(at));
                }
                ui.add_space(6.0);
                self.object_style(ui);
            }
            (_, 0) => {
                ui.strong(format!("{no} objects"));
                self.object_style(ui);
            }
            (0, 1) => {
                if let Some(&i) = self.sel.arrows.first() {
                    ui.strong("Arrow");
                    ui.weak(self.arrow_name(ui.ctx(), i));
                    self.label_field(ui, Typing::Arrow(i));
                    self.symbol_strip(ui, Typing::Arrow(i));
                    self.label_placement(ui, i);
                }
                ui.add_space(6.0);
                self.arrow_style(ui);
            }
            (0, _) => {
                ui.strong(format!("{na} arrows"));
                self.arrow_style(ui);
            }
            _ => {
                ui.strong(format!("{no} objects and {na} arrows"));
                ui.weak("Select only objects or only arrows to style them.");
            }
        }
    }

    fn diagram_panel(&mut self, ui: &mut Ui) {
        ui.strong("Diagram");
        if !self.can_submit() {
            ui.label("Click a cell to add an object, or start from a template.");
        }
        ui.weak("Click an object or an arrow to change it.");
        ui.add_space(6.0);
        for (rows, title) in [(true, "Row spacing"), (false, "Column spacing")] {
            let current = if rows { self.d.row_sep } else { self.d.col_sep };
            let name = current.name().map_or_else(|| "custom".to_string(), str::to_string);
            let mut chosen = None;
            ui.horizontal(|ui| {
                ui.label(title);
                craft_ui::ComboBox::from_id_salt(("cd-sep", rows)).selected_text(name).width(110.0).show_ui(ui, |ui| {
                    for s in Sep::NAMED {
                        if ui.selectable_label(current == s, s.name().unwrap_or_default()).clicked() {
                            chosen = Some(s);
                        }
                    }
                });
                let mut em = if rows { current.row_em() } else { current.col_em() };
                if ui.add(egui::DragValue::new(&mut em).range(0.0..=20.0).speed(0.05).suffix(" em")).changed() {
                    chosen = Some(Sep::Em((em * 100.0).round() / 100.0));
                }
            });
            if let Some(s) = chosen.filter(|s| *s != current) {
                self.apply(|d| {
                    if rows {
                        d.row_sep = s;
                    } else {
                        d.col_sep = s;
                    }
                    Ok(Selection::default())
                });
            }
        }
        ui.add_space(8.0);
        ui.weak("Drag from one object to another for an arrow; onto an empty cell to add the object too; onto an arrow for an arrow between arrows. Alt-drag moves objects.");
    }

    fn label_text(&self, what: Typing) -> String {
        match what {
            Typing::Object(at) => self.d.object(at).map(|o| o.label.clone()).unwrap_or_default(),
            Typing::Arrow(i) => self.d.arrows.get(i).and_then(|a| a.labels.first()).map(|l| l.text.clone()).unwrap_or_default(),
        }
    }

    fn set_label_text(&mut self, what: Typing, text: String) {
        match what {
            Typing::Object(at) => {
                if let Some(o) = self.d.objects.iter_mut().find(|o| o.at == at) {
                    o.label = text;
                }
            }
            Typing::Arrow(i) => {
                if text.is_empty() {
                    if let Some(a) = self.d.arrows.get_mut(i)
                        && !a.labels.is_empty()
                    {
                        a.labels.remove(0);
                    }
                } else if let Some(l) = edit::ensure_label(&mut self.d, i) {
                    l.text = text;
                }
            }
        }
    }

    fn label_field(&mut self, ui: &mut Ui, what: Typing) {
        let caption = ui.label("Label (LaTeX)");
        let mut text = self.label_text(what);
        let resp = ui
            .add(
                egui::TextEdit::singleline(&mut text)
                    .id(label_id())
                    .code_editor()
                    .desired_width(f32::INFINITY)
                    .hint_text(r"f, \pi_1, F(A), X \times_Z Y"),
            )
            .labelled_by(caption.id);
        if let Some(select_all) = self.focus_label.take() {
            resp.request_focus();
            if select_all {
                let mut state = egui::text_edit::TextEditState::load(ui.ctx(), label_id()).unwrap_or_default();
                state.cursor.set_char_range(Some(CCursorRange::two(CCursor::new(0), CCursor::new(text.chars().count()))));
                state.store(ui.ctx(), label_id());
            }
            self.typing = Some(what);
            self.typing_backup = Some(self.d.clone());
        }
        self.label_had_focus = resp.has_focus();
        if resp.changed() {
            self.begin_typing(what);
            self.set_label_text(what, text);
        }
    }

    fn symbol_strip(&mut self, ui: &mut Ui, what: Typing) {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(2.0, 2.0);
            for (glyph, cmd, short) in SYMBOLS {
                let shown = drawable(ui.ctx(), glyph, || drawable(ui.ctx(), short, || cmd.trim().to_string()));
                let b = egui::Button::new(RichText::new(shown).size(15.0)).min_size(vec2(24.0, 24.0));
                let resp = ui.add(b).on_hover_text(cmd.trim());
                resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, format!("Insert {}", cmd.trim())));
                if resp.clicked() {
                    self.insert_symbol(ui.ctx(), what, cmd);
                }
            }
        });
    }

    /// Puts `cmd` into the label at its field's cursor (or selection), else at the end.
    fn insert_symbol(&mut self, ctx: &egui::Context, what: Typing, cmd: &str) {
        let mut state = egui::text_edit::TextEditState::load(ctx, label_id()).unwrap_or_default();
        self.begin_typing(what);
        let mut text = self.label_text(what);
        let len = text.chars().count();
        let (a, b) = state
            .cursor
            .char_range()
            .map(|r| {
                let (x, y) = (r.primary.index.0.min(len), r.secondary.index.0.min(len));
                (x.min(y), x.max(y))
            })
            .unwrap_or((len, len));
        let byte = |i: usize| text.char_indices().nth(i).map_or(text.len(), |(b, _)| b);
        let (ba, bb) = (byte(a), byte(b));
        text.replace_range(ba..bb, cmd);
        let at = a + cmd.chars().count();
        self.set_label_text(what, text);
        state.cursor.set_char_range(Some(CCursorRange::one(CCursor::new(at))));
        state.store(ctx, label_id());
        self.focus_label = Some(false);
        // Keep the step this edit belongs to.
        self.typing = Some(what);
    }

    fn label_placement(&mut self, ui: &mut Ui, i: usize) {
        let Some(l) = self.d.arrows.get(i).and_then(|a| a.labels.first()).cloned() else { return };
        ui.horizontal(|ui| {
            ui.label("Label");
            for (side, name, tip) in [
                (Side::Left, "Left", "Left of the arrow's direction"),
                (Side::Right, "Right", "Right of the arrow's direction"),
                (Side::On, "On", "On the arrow, over a patch of background"),
                (Side::Over, "Over", "On the arrow, nothing behind it"),
            ] {
                if ui.selectable_label(l.side == side, name).on_hover_text(tip).clicked() && l.side != side {
                    let before = self.snapshot();
                    if let Some(x) = self.d.arrows.get_mut(i).and_then(|a| a.labels.first_mut()) {
                        x.side = side;
                    }
                    self.remember(before);
                }
            }
        });
        let mut pos = l.pos;
        ui.horizontal(|ui| {
            ui.label("Along");
            if ui.add(egui::Slider::new(&mut pos, 0.0..=1.0).step_by(0.05).show_value(true)).changed() {
                let before = self.snapshot();
                if let Some(x) = self.d.arrows.get_mut(i).and_then(|a| a.labels.first_mut()) {
                    x.pos = pos;
                }
                if self.typing != Some(Typing::Arrow(i)) {
                    self.remember(before);
                    self.typing = Some(Typing::Arrow(i));
                }
            }
        });
    }

    /// The selected arrows' first look (what the controls show).
    fn first_arrow(&self) -> Option<cd::Arrow> {
        self.sel.arrows.first().and_then(|&i| self.d.arrows.get(i)).cloned()
    }

    fn arrow_style(&mut self, ui: &mut Ui) {
        let Some(a) = self.first_arrow() else { return };
        let double = a.body == Body::Double;
        ui.label("Start");
        let mut set_tail = None;
        ui.horizontal_wrapped(|ui| {
            for (tail, tip) in [
                (Tail::None, "Nothing"),
                (Tail::Mono, "Monomorphism tail ↣"),
                (Tail::Hook, "Hook ↪ (left)"),
                (Tail::HookRight, "Hook (right)"),
                (Tail::Bar, "Bar ↦"),
                (Tail::Head, "Arrowhead ↔"),
            ] {
                let ok = !double || matches!(tail, Tail::None | Tail::Head | Tail::Bar);
                if look_button(ui, (tail, a.body.min_drawn(), Head::None), a.tail == tail, ok, tip).clicked() {
                    set_tail = Some(tail);
                }
            }
        });
        ui.label("Line");
        let mut set_body = None;
        ui.horizontal_wrapped(|ui| {
            for (body, tip) in [
                (Body::Solid, "Solid"),
                (Body::Dashed, "Dashed"),
                (Body::Dotted, "Dotted"),
                (Body::Squiggly, "Squiggly"),
                (Body::Double, "Double ⇒"),
                (Body::Phantom, "None: only its labels, full size (⊣ between adjoints, ⌟ corners)"),
            ] {
                if look_button(ui, (Tail::None, body, Head::Arrow), a.body == body || (body == Body::Phantom && a.body == Body::Invisible), true, tip)
                    .clicked()
                {
                    set_body = Some(body);
                }
            }
        });
        ui.label("End");
        let mut set_head = None;
        ui.horizontal_wrapped(|ui| {
            for (head, tip) in [
                (Head::Arrow, "Arrowhead"),
                (Head::TwoHeads, "Two heads ↠ (epimorphism)"),
                (Head::Harpoon, "Harpoon (left)"),
                (Head::HarpoonRight, "Harpoon (right)"),
                (Head::None, "Nothing"),
            ] {
                let ok = !double || matches!(head, Head::Arrow | Head::None);
                if look_button(ui, (Tail::None, a.body.min_drawn(), head), a.head == head, ok, tip).clicked() {
                    set_head = Some(head);
                }
            }
        });
        if let Some(t) = set_tail {
            self.restyle(|x| x.tail = t);
        }
        if let Some(b) = set_body {
            self.restyle(|x| {
                x.body = b;
                // A double line has tikz-cd's double tips only.
                if b == Body::Double {
                    if !matches!(x.tail, Tail::None | Tail::Head | Tail::Bar) {
                        x.tail = Tail::None;
                    }
                    if !matches!(x.head, Head::Arrow | Head::None) {
                        x.head = Head::Arrow;
                    }
                }
            });
        }
        if let Some(h) = set_head {
            self.restyle(|x| x.head = h);
        }
        ui.add_space(6.0);
        self.shape_controls(ui, &a);
        ui.add_space(6.0);
        self.width_and_colour(ui, &a);
        let extra: Vec<String> = self.sel.arrows.iter().filter_map(|&i| self.d.arrows.get(i)).flat_map(|a| a.extra.iter().cloned()).collect();
        if !extra.is_empty() {
            ui.weak(format!("Kept as written (not drawn): {}", extra.join(", ")));
        }
    }

    fn shape_controls(&mut self, ui: &mut Ui, a: &cd::Arrow) {
        let looped = a.from == a.to;
        if looped {
            ui.horizontal(|ui| {
                ui.label("Loop");
                for l in [Loop::Above, Loop::Right, Loop::Below, Loop::Left] {
                    if ui.selectable_label(a.looping.unwrap_or_default() == l, l.name()).clicked() {
                        self.restyle(|x| x.looping = Some(l));
                    }
                }
            });
        } else {
            let mut bend = a.bend;
            ui.horizontal(|ui| {
                ui.label("Curve");
                let r = ui.add(egui::Slider::new(&mut bend, -90.0..=90.0).step_by(5.0).suffix("°"));
                if r.changed() {
                    self.restyle_continuous(r.drag_started(), |x| x.bend = bend);
                }
                if ui.small_button("Straight").clicked() {
                    self.restyle(|x| x.bend = 0.0);
                }
            });
            let mut shift = a.shift;
            ui.horizontal(|ui| {
                ui.label("Offset");
                let r = ui.add(egui::Slider::new(&mut shift, -5.0..=5.0).step_by(0.5));
                r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Slider, true, "Offset (parallel arrows)"));
                if r.changed() {
                    self.restyle_continuous(r.drag_started(), |x| x.shift = shift);
                }
            });
        }
        let (mut s0, mut s1) = a.shorten;
        ui.horizontal(|ui| {
            ui.label("Shorten start");
            if ui.add(egui::DragValue::new(&mut s0).range(0.0..=50.0).speed(0.25).suffix(" pt")).changed() {
                self.restyle(|x| x.shorten.0 = s0);
            }
            ui.label("end");
            if ui.add(egui::DragValue::new(&mut s1).range(0.0..=50.0).speed(0.25).suffix(" pt")).changed() {
                self.restyle(|x| x.shorten.1 = s1);
            }
        });
        let mut crossing = a.crossing;
        if ui.checkbox(&mut crossing, "Crossing over").on_hover_text("Break the arrows it crosses (drawn after them)").changed() {
            self.restyle(|x| x.crossing = crossing);
        }
    }

    /// A slider's drag is one undo step.
    fn restyle_continuous(&mut self, started: bool, f: impl Fn(&mut cd::Arrow)) {
        if started || self.typing.is_some() {
            self.restyle(f);
            return;
        }
        for &i in &self.sel.arrows {
            if let Some(a) = self.d.arrows.get_mut(i) {
                f(a);
            }
        }
        self.dirty = true;
    }

    fn width_and_colour(&mut self, ui: &mut Ui, a: &cd::Arrow) {
        ui.horizontal_wrapped(|ui| {
            ui.label("Width");
            for (w, name) in [(None, "normal"), (Some(0.4), "thin"), (Some(0.8), "thick"), (Some(1.2), "very thick")] {
                let on = match (a.width, w) {
                    (None, None) => true,
                    (Some(x), Some(y)) => (x - y).abs() < 1e-9,
                    _ => false,
                };
                if ui.selectable_label(on, name).clicked() {
                    self.restyle(|x| x.width = w);
                }
            }
            let mut pt = a.width.unwrap_or(0.4);
            if ui.add(egui::DragValue::new(&mut pt).range(0.1..=10.0).speed(0.05).suffix(" pt")).changed() {
                self.restyle(|x| x.width = Some(pt));
            }
        });
        if let Some(c) = self.colour_row(ui, a.color) {
            self.restyle(|x| {
                x.color = c;
                for l in &mut x.labels {
                    l.color = c;
                }
            });
        }
    }

    /// Colour swatches and a picker; the colour chosen (`Some(None)`: the text colour).
    fn colour_row(&self, ui: &mut Ui, current: Option<Rgb>) -> Option<Option<Rgb>> {
        let mut chosen = None;
        ui.horizontal_wrapped(|ui| {
            ui.label("Colour");
            let t = Tokens::get(ui.ctx());
            for (name, c) in PALETTE {
                let fill = c.map_or(t.text, |c| Color32::from_rgb(c[0], c[1], c[2]));
                let (rect, resp) = ui.allocate_exact_size(vec2(20.0, 20.0), Sense::click());
                let on = current == *c;
                ui.painter().rect(
                    rect.shrink(2.0),
                    3.0,
                    fill,
                    Stroke::new(if on { 2.0 } else { 1.0 }, if on { t.accent } else { t.border }),
                    StrokeKind::Outside,
                );
                resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, on, *name));
                if resp.on_hover_text(*name).clicked() {
                    chosen = Some(*c);
                }
            }
            let mut rgb = current.unwrap_or([0, 0, 0]);
            if egui::color_picker::color_edit_button_srgb(ui, &mut rgb).changed() {
                chosen = Some(Some(rgb));
            }
        });
        chosen
    }

    fn object_style(&mut self, ui: &mut Ui) {
        let Some(o) = self.sel.objects.first().and_then(|at| self.d.object(*at)).cloned() else { return };
        ui.horizontal(|ui| {
            ui.label("Frame");
            for (f, name) in [(Frame::None, "None"), (Frame::Box, "Box"), (Frame::Circle, "Circle")] {
                if ui.selectable_label(o.frame == f, name).clicked() {
                    self.restyle_objects(|x| x.frame = f);
                }
            }
        });
        if let Some(c) = self.colour_row(ui, o.color) {
            self.restyle_objects(|x| x.color = c);
        }
        if !o.extra.is_empty() {
            ui.weak(format!("Kept as written (not drawn): {}", o.extra.join(", ")));
        }
    }

    // -----------------------------------------------------------------------------------------
    // The LaTeX pane and the footer.

    fn source_pane(&mut self, ui: &mut Ui, out: &mut Outcome) {
        egui::CollapsingHeader::new("LaTeX (tikz-cd)").id_salt("cd-source").default_open(false).show(ui, |ui| {
            let id = Id::unique("cd-source-text");
            let focused = ui.memory(|m| m.has_focus(id));
            let mut text = match (&self.source, focused) {
                (Some(s), true) => s.clone(),
                _ => self.final_latex(),
            };
            let resp = ui.add(egui::TextEdit::multiline(&mut text).id(id).code_editor().desired_rows(6).desired_width(f32::INFINITY));
            if resp.changed() {
                self.source_edited(&text);
            }
            if resp.lost_focus() {
                self.source = None;
                self.source_error = None;
            }
            if let Some(e) = &self.source_error {
                ui.colored_label(Color32::from_rgb(0xC0, 0x30, 0x30), e);
            }
            ui.horizontal(|ui| {
                if ui.button("Copy LaTeX").on_hover_text("For a document with \\usepackage{tikz-cd}").clicked() {
                    ui.ctx().copy_text(self.final_latex());
                }
                if ui.button("Edit as LaTeX…").on_hover_text("Open this diagram's LaTeX in the Equation dialog").clicked() {
                    *out = Outcome::EditLatex;
                }
            });
        });
    }

    /// Typed LaTeX becomes the diagram as soon as it reads as one.
    fn source_edited(&mut self, text: &str) {
        self.source = Some(text.to_string());
        match cd::open(text) {
            Ok(o) => {
                if o.diagram != self.d {
                    let before = self.snapshot();
                    self.remember(before);
                    self.d = o.diagram;
                    self.sel = Selection::default();
                    self.typing = None;
                }
                self.notices = o.notices;
                self.source_error = None;
            }
            Err(e) => self.source_error = Some(e),
        }
    }

    fn footer(&mut self, ui: &mut Ui, out: &mut Outcome) {
        let t = Tokens::get(ui.ctx());
        self.messages(ui);
        if self.confirm_discard {
            ui.horizontal(|ui| {
                ui.label("Discard the changes to this diagram?");
                if ui.button("Discard").clicked() {
                    *out = Outcome::Discard;
                }
                if ui.button("Keep editing").clicked() {
                    self.confirm_discard = false;
                }
            });
        }
        let insert = crate::ribbon::pretty_shortcut("Cmd+Enter");
        ui.weak(format!(
            "Click a cell to add an object; drag from an object to another for an arrow. Double-click to edit a label; drag an arrow's \
             handle to bend it. Arrows move between cells, Alt+arrows move objects · Delete removes · {insert} inserts."
        ));
        ui.horizontal(|ui| {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let ok = if self.target.is_some() { "Update" } else { "Insert" };
                let primary = egui::Button::new(RichText::new(ok).color(t.accent_text)).fill(t.accent).min_size(vec2(72.0, 24.0));
                if ui.add_enabled(self.can_submit(), primary).clicked() {
                    *out = Outcome::Submit(false);
                }
                if self.target.is_some() && ui.add_enabled(self.can_submit(), egui::Button::new("Insert as new")).clicked() {
                    *out = Outcome::Submit(true);
                }
                if ui.add(egui::Button::new("Cancel").min_size(vec2(72.0, 24.0))).clicked() {
                    *out = Outcome::Close;
                }
            });
        });
    }

    fn messages(&self, ui: &mut Ui) {
        let red = Color32::from_rgb(0xC0, 0x30, 0x30);
        if let Some(s) = &self.status {
            ui.colored_label(red, s);
        }
        for p in cd::preamble(&self.d) {
            ui.weak(format!("Squiggly arrows need {p} in your document's preamble."));
        }
        for n in &self.notices {
            ui.weak(n);
        }
    }
}

/// Small helpers the editor wants on the engine's types.
trait DiagramExt {
    fn object_at_signed(&self, row: i64, col: i64) -> Option<&cd::Object>;
}

impl DiagramExt for Diagram {
    fn object_at_signed(&self, row: i64, col: i64) -> Option<&cd::Object> {
        (row >= 0 && col >= 0).then(|| self.object(At::new(row as usize, col as usize))).flatten()
    }
}

trait BodyExt {
    /// A body to show tips on (a line that isn't drawn shows as solid).
    fn min_drawn(self) -> Body;
}

impl BodyExt for Body {
    fn min_drawn(self) -> Body {
        if self.drawn() { self } else { Body::Solid }
    }
}

#[cfg(test)]
#[path = "cd_editor_tests.rs"]
mod tests;
