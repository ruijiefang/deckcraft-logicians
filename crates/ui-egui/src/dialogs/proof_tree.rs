//! Insert ▸ Proof Tree: build a proof tree by adding, removing and moving its steps on its
//! picture.
//!
//! The tree is edited with [`deckcraft_engine::typeset::proof_edit`] (each change is an engine
//! function of the tree); the picture is the engine's own typeset of it, drawn by the renderer
//! that draws the slide. The slide changes once, through `insert.latex` or `latex.set`, so the
//! document keeps ebproof LaTeX and gets one undo step.

use deckcraft_engine::typeset::proof_edit::{self as pe, Kind, Path, Place};
use deckcraft_engine::typeset::{Part, Proof, Region, Rule, Stmt};
use deckcraft_model::ShapeId;
use egui::text::{CCursor, CCursorRange};
use egui::{
    Align, Color32, Event, Id, Key, Layout, Modifiers, Rect, RichText, ScrollArea, Sense, Stroke, StrokeKind, TextureHandle, Ui, Vec2, pos2, vec2,
};
use serde_json::{Value, json};

use super::{Dialog, SlideApp, Tokens};

/// Steps of undo the builder keeps.
const HISTORY: usize = 200;
/// Width of the side panel (steps and the selected step).
const SIDE: f32 = 320.0;
/// The renderer's margin around a rendered shape, in points.
const MARGIN: f64 = 8.0;
/// The largest preview texture side, in pixels.
const MAX_PX: f64 = 4096.0;

/// Symbols the strip inserts into the statement being edited: (shown, LaTeX).
const SYMBOLS: &[(&str, &str)] = &[
    ("∀", r"\forall "),
    ("∃", r"\exists "),
    ("¬", r"\neg "),
    ("∧", r"\wedge "),
    ("∨", r"\vee "),
    ("→", r"\to "),
    ("↔", r"\leftrightarrow "),
    ("⊢", r"\vdash "),
    ("⊨", r"\vDash "),
    ("⊥", r"\bot "),
    ("⊤", r"\top "),
    ("□", r"\Box "),
    ("◇", r"\Diamond "),
    ("Γ", r"\Gamma "),
    ("Δ", r"\Delta "),
    ("φ", r"\varphi "),
    ("ψ", r"\psi "),
    ("∈", r"\in "),
    ("≡", r"\equiv "),
    ("⊸", r"\multimap "),
];

/// What the window does after this frame.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Outcome {
    Stay,
    /// Cancel, Esc: asks first when there are changes.
    Close,
    /// Discard confirmed.
    Discard,
    /// Insert (or Update); `true`: insert a new proof even when editing one.
    Submit(bool),
    /// Switch to the Equation dialog on this LaTeX.
    EditLatex,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Action {
    Premise,
    Sibling(bool),
    Below,
    Remove,
    DeleteBranch,
    Move(bool),
    Undo,
    Redo,
    Copy,
    Cut,
    Paste,
    Start,
}

#[derive(Clone)]
struct Snap {
    forest: Vec<Proof>,
    sel: Option<Path>,
}

/// The picture of the proof and where its steps are on it.
struct Canvas {
    /// The source and style it was typeset from.
    key: u64,
    shape: Option<deckcraft_model::Shape>,
    regions: Vec<Region>,
    w: f64,
    h: f64,
    tex: Option<TextureHandle>,
    /// Pixels per slide point of `tex`.
    px: f64,
    error: Option<String>,
}

/// The builder while its window is open (dropped when it closes).
pub struct Builder {
    pub(crate) forest: Vec<Proof>,
    sel: Option<Path>,
    undo: Vec<Snap>,
    redo: Vec<Snap>,
    /// The proof shape this updates; `None` inserts a new one.
    target: Option<u64>,
    size: f64,
    font: String,
    notices: Vec<String>,
    status: Option<String>,
    clipboard: Option<Proof>,
    /// The LaTeX pane's text while it is being typed in.
    source: Option<String>,
    source_error: Option<String>,
    canvas: Option<Canvas>,
    /// Multiplies the fit-to-window scale.
    zoom: f64,
    focus_statement: bool,
    /// The step whose fields are being typed in: one undo step per step edited.
    typing: Option<Path>,
    /// The statement before this step's edit began (Esc in the field puts it back).
    typing_backup: Option<String>,
    drag: Option<Path>,
    confirm_discard: bool,
    dirty: bool,
    /// Whether the statement field had the keyboard last frame (egui drops focus on Esc before
    /// the field sees it).
    statement_had_focus: bool,
}

pub(super) fn dialog(app: &mut SlideApp, ui: &mut Ui, d: &mut Dialog) -> bool {
    let mut b = match app.proof_builder.take() {
        Some(b) => b,
        None => Builder::open(app, d),
    };
    let close = b.show(app, ui);
    if !close {
        app.proof_builder = Some(b);
    }
    close
}

/// Plain text for the interface: the math spaces of a typeset statement as ordinary spaces.
fn ui_text(s: &str) -> String {
    s.chars().map(|c| if matches!(c, '\u{2000}'..='\u{200B}' | '\u{205F}' | '\u{A0}') { ' ' } else { c }).collect()
}

/// `text` when the interface fonts can draw all of it, else `fallback` (the interface has no
/// math font; a missing glyph would be an empty box).
fn drawable(ctx: &egui::Context, text: &str, fallback: impl FnOnce() -> String) -> String {
    let font = egui::FontId::proportional(14.0);
    let text = ui_text(text);
    if ctx.fonts_mut(|f| f.has_glyphs(&font, &text)) { text } else { fallback() }
}

/// A template's name in words, for interfaces without its symbol.
fn spell_out(name: &str) -> String {
    let words = [("∧", "And"), ("∨", "Or"), ("→", "Implies"), ("¬", "Not"), ("⊥", "Falsum"), ("∀", "For all"), ("∃", "Exists"), ("(⋮)", "(dots)")];
    words.iter().fold(name.to_string(), |s, (sym, word)| s.replace(sym, word))
}

/// A step on one line for the list and the drag label: as typeset if the interface can draw it,
/// else its LaTeX.
fn step_text(ctx: &egui::Context, n: &Proof) -> String {
    drawable(ctx, &pe::describe(n), || {
        let src = n.concl.iter().map(|c| c.src.trim()).collect::<Vec<_>>().join(" ");
        if src.is_empty() { "(empty)".to_string() } else { src }
    })
}

fn statement_id(path: &[usize]) -> Id {
    Id::unique(("proof-statement", path.to_vec()))
}

impl Builder {
    fn empty() -> Self {
        Builder {
            forest: Vec::new(),
            sel: None,
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
            focus_statement: false,
            typing: None,
            typing_backup: None,
            drag: None,
            confirm_discard: false,
            dirty: false,
            statement_had_focus: false,
        }
    }

    /// The builder on the proof it was opened for: the Equation dialog's LaTeX (`params.latex`),
    /// else the selected proof, else a new proof with one empty conclusion.
    fn open(app: &mut SlideApp, d: &Dialog) -> Self {
        let mut b = Builder::empty();
        let found = app.session.execute("latex.get", &json!({})).ok();
        let selected_proof = found.filter(|v| {
            v.get("kind").and_then(Value::as_str) == Some("typeset")
                && v.get("latex").and_then(Value::as_str).is_some_and(deckcraft_engine::typeset::is_proof)
        });
        let from_dialog = d.params.get("latex").and_then(Value::as_str).map(str::to_string);
        let editing = from_dialog.is_none() || d.params.get("edit").and_then(Value::as_bool) == Some(true);
        let src = from_dialog.or_else(|| selected_proof.as_ref().and_then(|v| v.get("latex")).and_then(Value::as_str).map(str::to_string));
        if let Some(v) = selected_proof.as_ref().filter(|_| editing) {
            b.target = v.get("id").and_then(Value::as_u64);
            b.size = v.get("size").and_then(Value::as_f64).unwrap_or(b.size);
            b.font = v.get("font").and_then(Value::as_str).unwrap_or(&b.font).to_string();
        }
        if let Some(src) = src {
            match pe::open(&src) {
                Ok(o) => {
                    b.forest = o.forest;
                    b.notices = o.notices;
                    b.sel = Some(vec![0]);
                }
                Err(e) => {
                    b.status = Some(format!("This LaTeX can't be edited as a tree ({e}). Starting a new proof."));
                    b.target = None;
                }
            }
        }
        if b.forest.is_empty() {
            b.forest = vec![pe::blank()];
            b.sel = Some(vec![0]);
            b.focus_statement = true;
        }
        b
    }

    /// The selected step, while it exists.
    fn selected(&self) -> Option<Path> {
        self.sel.clone().filter(|p| pe::node(&self.forest, p).is_some())
    }

    fn select(&mut self, path: Path) {
        if self.sel.as_ref() != Some(&path) {
            self.typing = None;
        }
        self.sel = Some(path);
    }

    fn snapshot(&self) -> Snap {
        Snap { forest: self.forest.clone(), sel: self.sel.clone() }
    }

    fn remember(&mut self, before: Snap) {
        self.undo.push(before);
        if self.undo.len() > HISTORY {
            self.undo.remove(0);
        }
        self.redo.clear();
        self.dirty = true;
    }

    /// Runs an edit; on success it becomes one undo step and moves the selection, on failure the
    /// tree is left as it was and the reason shown.
    fn apply(&mut self, f: impl FnOnce(&mut Vec<Proof>) -> Result<Option<Path>, String>) -> bool {
        let before = self.snapshot();
        match f(&mut self.forest) {
            Ok(sel) => {
                self.remember(before);
                self.sel = sel;
                self.typing = None;
                self.status = None;
                true
            }
            Err(e) => {
                self.forest = before.forest;
                self.status = Some(e);
                false
            }
        }
    }

    /// The first change to a step's fields since it was selected is one undo step, and what Esc
    /// in its statement goes back to.
    fn begin_typing(&mut self, path: &Path) {
        if self.typing.as_ref() != Some(path) {
            let before = self.snapshot();
            self.remember(before);
            self.typing = Some(path.clone());
            self.typing_backup = pe::node(&self.forest, path).and_then(|n| n.concl.first()).map(|c| c.src.clone());
        }
    }

    fn act(&mut self, a: Action, ctx: &egui::Context) {
        match a {
            Action::Undo => return self.undo(),
            Action::Redo => return self.redo(),
            Action::Start => {
                if self.apply(|f| {
                    f.push(pe::blank());
                    Ok(Some(vec![f.len() - 1]))
                }) {
                    self.focus_statement = true;
                }
                return;
            }
            Action::Paste => return self.paste(None),
            _ => {}
        }
        let Some(path) = self.selected() else {
            self.status = Some("Select a step first.".into());
            return;
        };
        let added = match a {
            Action::Premise => self.apply(|f| pe::add_premise(f, &path).map(Some)),
            Action::Sibling(right) => self.apply(|f| pe::add_sibling(f, &path, right).map(Some)),
            Action::Below => self.apply(|f| pe::insert_below(f, &path).map(Some)),
            _ => false,
        };
        if added {
            self.focus_statement = true;
            return;
        }
        match a {
            Action::Remove => {
                self.apply(|f| pe::remove_step(f, &path));
            }
            Action::DeleteBranch => {
                self.apply(|f| pe::delete_branch(f, &path));
            }
            Action::Move(right) => {
                self.apply(|f| pe::move_sibling(f, &path, right).map(Some));
            }
            Action::Copy | Action::Cut => {
                let branch = pe::node(&self.forest, &path).cloned();
                if let Some(b) = &branch {
                    ctx.copy_text(pe::to_ebproof(std::slice::from_ref(b)));
                }
                self.clipboard = branch;
                if a == Action::Cut {
                    self.apply(|f| pe::delete_branch(f, &path));
                }
            }
            _ => {}
        }
    }

    /// Pastes `text` when it is a proof, else the copied branch: into an empty slot, as a premise
    /// of the selected step, or as a tree of its own.
    fn paste(&mut self, text: Option<&str>) {
        let from_text = text.and_then(|t| pe::open(t).ok()).and_then(|o| o.forest.into_iter().next());
        let Some(branch) = from_text.or_else(|| self.clipboard.clone()) else {
            self.status = Some("Copy a step first (Ctrl+C).".into());
            return;
        };
        let at = self.selected();
        self.apply(|f| pe::paste(f, at.as_deref(), branch).map(Some));
    }

    fn template(&mut self, name: &str) {
        let Some(t) = pe::template(name) else { return };
        let at = self.selected();
        self.apply(|f| pe::paste(f, at.as_deref(), t).map(Some));
    }

    fn undo(&mut self) {
        if let Some(s) = self.undo.pop() {
            let now = self.snapshot();
            self.redo.push(now);
            self.forest = s.forest;
            self.sel = s.sel;
            self.typing = None;
            self.dirty = true;
        }
    }

    fn redo(&mut self) {
        if let Some(s) = self.redo.pop() {
            let now = self.snapshot();
            self.undo.push(now);
            self.forest = s.forest;
            self.sel = s.sel;
            self.typing = None;
            self.dirty = true;
        }
    }

    fn problems(&self) -> Vec<String> {
        pe::problems(&self.forest).into_iter().map(|(_, m)| m).collect()
    }

    fn can_submit(&self) -> bool {
        !self.forest.is_empty() && self.problems().is_empty()
    }

    // -----------------------------------------------------------------------------------------
    // The window.

    fn show(&mut self, app: &mut SlideApp, ui: &mut Ui) -> bool {
        let mut out = Outcome::Stay;
        self.keys(ui, &mut out);
        // Panels give the picture whatever room the window has.
        egui::Panel::top("proof-toolbar").resizable(false).show(ui, |ui| self.toolbar(ui));
        egui::Panel::bottom("proof-footer").resizable(false).show(ui, |ui| {
            self.source_pane(ui, &mut out);
            ui.separator();
            self.footer(ui, &mut out);
        });
        egui::Panel::right("proof-side").exact_size(SIDE).resizable(false).show(ui, |ui| {
            ScrollArea::vertical().id_salt("proof-side").auto_shrink([false, false]).show(ui, |ui| self.side(ui));
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
        let latex = pe::to_ebproof(&self.forest);
        let mut p = json!({"latex": latex, "size": self.size, "font": self.font});
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

    /// Opens the Equation dialog on the tree's LaTeX (editing the same proof, if any).
    fn to_latex_dialog(&self, app: &mut SlideApp) {
        let mut d = Dialog::new("equation");
        let editing = self.target.is_some().to_string();
        d.fields.insert("eq".into(), pe::to_ebproof(&self.forest));
        d.fields.insert("as".into(), "typeset".into());
        d.fields.insert("found".into(), editing.clone());
        d.fields.insert("edit".into(), editing);
        app.dialog = Some(d);
    }

    /// The selected step's statement as it was before this edit of it began.
    fn revert_statement(&mut self) {
        let Some(path) = self.selected() else { return };
        if let (Some(backup), Some(node)) = (self.typing_backup.take(), pe::node_mut(&mut self.forest, &path))
            && let Some(c) = node.concl.first_mut()
        {
            c.src = backup;
        }
        self.typing = None;
    }

    fn statement_focused(&self, ctx: &egui::Context) -> bool {
        self.selected().is_some_and(|p| ctx.memory(|m| m.has_focus(statement_id(&p))))
    }

    /// The keyboard: Ctrl+Enter inserts; Ctrl+↑/↓ add a premise or a step below (also while
    /// typing a statement, where Tab adds the next premise); on the picture the arrows move the
    /// selection and Delete, Alt+←/→, Ctrl+Z, Ctrl+C/X/V and Esc act on the tree.
    fn keys(&mut self, ui: &Ui, out: &mut Outcome) {
        let ctx = ui.ctx().clone();
        let consume = |m: Modifiers, k: Key| ctx.input_mut(|i| i.consume_key(m, k));
        if consume(Modifiers::COMMAND, Key::Enter) {
            *out = Outcome::Submit(false);
            return;
        }
        // Esc in the statement field puts the statement back and leaves the field; it doesn't
        // close the builder.
        if self.statement_had_focus && consume(Modifiers::NONE, Key::Escape) {
            self.revert_statement();
            ctx.memory_mut(|m| {
                if let Some(id) = m.focused() {
                    m.surrender_focus(id);
                }
            });
            self.statement_had_focus = false;
            return;
        }
        let typing = ctx.text_edit_focused();
        if typing && !self.statement_focused(&ctx) {
            return;
        }
        if consume(Modifiers::COMMAND, Key::ArrowUp) {
            return self.act(Action::Premise, &ctx);
        }
        if consume(Modifiers::COMMAND, Key::ArrowDown) {
            return self.act(Action::Below, &ctx);
        }
        if typing {
            if consume(Modifiers::NONE, Key::Tab) {
                self.act(Action::Sibling(true), &ctx);
            }
            return;
        }
        self.canvas_keys(&ctx, out);
    }

    fn canvas_keys(&mut self, ctx: &egui::Context, out: &mut Outcome) {
        let consume = |m: Modifiers, k: Key| ctx.input_mut(|i| i.consume_key(m, k));
        // Combinations first: a plain key would also match them.
        let action = if consume(Modifiers::COMMAND, Key::ArrowLeft) {
            Some(Action::Sibling(false))
        } else if consume(Modifiers::COMMAND, Key::ArrowRight) {
            Some(Action::Sibling(true))
        } else if consume(Modifiers::ALT, Key::ArrowLeft) {
            Some(Action::Move(false))
        } else if consume(Modifiers::ALT, Key::ArrowRight) {
            Some(Action::Move(true))
        } else if consume(Modifiers::SHIFT, Key::Delete) {
            Some(Action::DeleteBranch)
        } else if consume(Modifiers::NONE, Key::Delete) || consume(Modifiers::NONE, Key::Backspace) {
            Some(Action::Remove)
        } else if consume(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z) || consume(Modifiers::COMMAND, Key::Y) {
            Some(Action::Redo)
        } else if consume(Modifiers::COMMAND, Key::Z) {
            Some(Action::Undo)
        } else {
            None
        };
        if let Some(a) = action {
            return self.act(a, ctx);
        }
        if let Some(sel) = self.selected() {
            let to = if consume(Modifiers::NONE, Key::ArrowUp) {
                pe::first_premise(&self.forest, &sel)
            } else if consume(Modifiers::NONE, Key::ArrowDown) {
                pe::parent(&sel)
            } else if consume(Modifiers::NONE, Key::ArrowLeft) {
                pe::neighbour(&self.forest, &sel, false)
            } else if consume(Modifiers::NONE, Key::ArrowRight) {
                pe::neighbour(&self.forest, &sel, true)
            } else {
                None
            };
            if let Some(p) = to {
                self.select(p);
            }
            if consume(Modifiers::NONE, Key::Enter) || consume(Modifiers::NONE, Key::F2) {
                self.focus_statement = true;
            }
        }
        if consume(Modifiers::NONE, Key::Escape) {
            *out = Outcome::Close;
        }
        for e in ctx.input(|i| i.events.clone()) {
            match e {
                Event::Copy => self.act(Action::Copy, ctx),
                Event::Cut => self.act(Action::Cut, ctx),
                Event::Paste(t) => self.paste(Some(&t)),
                _ => {}
            }
        }
    }

    fn toolbar(&mut self, ui: &mut Ui) {
        let has = self.selected().is_some();
        let ctx = ui.ctx().clone();
        let mut action = None;
        ui.horizontal_wrapped(|ui| {
            let mut tool = |ui: &mut Ui, label: &str, tip: &str, on: bool, a: Action| {
                if ui.add_enabled(on, egui::Button::new(label)).on_hover_text(tip).clicked() {
                    action = Some(a);
                }
            };
            tool(ui, "+ Premise", "Add a premise above the selected step (Ctrl+Up)", has, Action::Premise);
            tool(ui, "+ Left", "Add a premise beside it, on the left (Ctrl+Left)", has, Action::Sibling(false));
            tool(ui, "+ Right", "Add a premise beside it, on the right (Ctrl+Right; Tab while typing)", has, Action::Sibling(true));
            tool(ui, "+ Below", "Add a step below the selected one (Ctrl+Down)", has, Action::Below);
            tool(ui, "Remove step", "Remove the step; its premises take its place (Delete)", has, Action::Remove);
            tool(ui, "Delete branch", "Delete the step and everything above it (Shift+Delete)", has, Action::DeleteBranch);
            tool(ui, "Move left", "Swap with the premise on the left (Alt+Left)", has, Action::Move(false));
            tool(ui, "Move right", "Swap with the premise on the right (Alt+Right)", has, Action::Move(true));
            ui.separator();
            tool(ui, "Undo", "Undo (Ctrl+Z)", !self.undo.is_empty(), Action::Undo);
            tool(ui, "Redo", "Redo (Ctrl+Shift+Z)", !self.redo.is_empty(), Action::Redo);
            ui.separator();
            tool(ui, "Copy", "Copy the branch (Ctrl+C)", has, Action::Copy);
            tool(ui, "Cut", "Cut the branch (Ctrl+X)", has, Action::Cut);
            tool(ui, "Paste", "Paste the branch above the selected step (Ctrl+V)", self.clipboard.is_some(), Action::Paste);
        });
        ui.horizontal_wrapped(|ui| {
            let mut chosen = None;
            ui.menu_button("Templates", |ui| {
                for (name, _) in pe::TEMPLATES {
                    let shown = drawable(ui.ctx(), name, || spell_out(name));
                    if ui.button(shown).clicked() {
                        chosen = Some(*name);
                        ui.close();
                    }
                }
            })
            .response
            .on_hover_text("Insert a rule: into an empty step, above the selected step, or as a new tree");
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
            ui.add(egui::TextEdit::singleline(&mut self.font).desired_width(150.0).hint_text("theme font"));
        });
        if let Some(a) = action {
            self.act(a, &ctx);
        }
    }

    // -----------------------------------------------------------------------------------------
    // The picture.

    /// Typesets and renders the proof when it, its style or the scale changed.
    fn refresh(&mut self, app: &SlideApp, ctx: &egui::Context, avail: Vec2) {
        use std::hash::{Hash, Hasher};
        let src = pe::to_ebproof_preview(&self.forest);
        let mut h = std::collections::hash_map::DefaultHasher::new();
        (src.as_str(), self.size.to_bits(), self.font.as_str()).hash(&mut h);
        let key = h.finish();
        let Ok(st) = app.session.doc() else { return };
        if self.canvas.as_ref().is_none_or(|c| c.key != key) {
            let font = Some(self.font.trim()).filter(|f| !f.is_empty());
            self.canvas = Some(match deckcraft_engine::cmd::latex::preview(st, &src, self.size, font) {
                Ok(p) => Canvas { key, shape: Some(p.shape), regions: p.regions, w: p.w, h: p.h, tex: None, px: 0.0, error: None },
                Err(e) => Canvas { key, shape: None, regions: Vec::new(), w: 0.0, h: 0.0, tex: None, px: 0.0, error: Some(e) },
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
        if let Some(img) = deckcraft_render::render_shape(pres, &slide, id, px) {
            c.tex = Some(crate::textures::upload(ctx, "proof-tree-preview", crate::textures::to_color_image(&img, false)));
            c.px = px;
        }
    }

    fn canvas(&mut self, app: &SlideApp, ui: &mut Ui) {
        let avail = ui.available_size();
        self.refresh(app, ui.ctx(), avail);
        let Some(c) = self.canvas.as_ref() else { return };
        if let Some(e) = &c.error {
            ui.colored_label(Color32::from_rgb(0xC0, 0x30, 0x30), format!("The proof can't be typeset: {e}"));
            return;
        }
        let Some(tex) = c.tex.clone() else { return };
        let regions = c.regions.clone();
        let ppp = f64::from(ui.ctx().pixels_per_point());
        let k = c.px / ppp;
        let img = vec2(((c.w + 2.0 * MARGIN) * k) as f32, ((c.h + 2.0 * MARGIN) * k) as f32);
        ScrollArea::both().id_salt("proof-canvas").auto_shrink([false, false]).show(ui, |ui| {
            let size = img.max(ui.available_size() - vec2(4.0, 4.0));
            let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
            let origin = rect.min + ((size - img) / 2.0).max(Vec2::ZERO);
            ui.painter().rect_filled(rect, 4.0, Color32::WHITE);
            ui.painter().image(tex.id(), Rect::from_min_size(origin, img), Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
            let to_screen = |r: &Region| {
                Rect::from_min_size(origin + vec2(((r.x + MARGIN) * k) as f32, ((r.y + MARGIN) * k) as f32), vec2((r.w * k) as f32, (r.h * k) as f32))
            };
            self.regions(ui, &regions, &to_screen);
            self.affordances(ui, &regions, &to_screen);
            self.dragging(ui, &regions, &to_screen);
        });
    }

    /// What screen readers and tests call a region.
    fn region_label(&self, r: &Region) -> String {
        let what = pe::node(&self.forest, &r.path).map(pe::describe).unwrap_or_default();
        match r.part {
            Part::Statement => format!("Step: {what}"),
            Part::Rule => format!("Rule under: {what}"),
            Part::LeftLabel => format!("Left label of: {what}"),
            Part::RightLabel => format!("Right label of: {what}"),
            // Diagrams' parts; a proof has none.
            Part::Object | Part::Arrow | Part::ArrowLabel => what,
        }
    }

    fn regions(&mut self, ui: &mut Ui, regions: &[Region], to_screen: &dyn Fn(&Region) -> Rect) {
        let t = Tokens::get(ui.ctx());
        for (n, r) in regions.iter().enumerate() {
            let rect = to_screen(r).expand(2.0);
            let statement = r.part == Part::Statement;
            let sense = if statement { Sense::click_and_drag() } else { Sense::click() };
            let resp = ui.interact(rect, Id::unique(("proof-region", n, r.path.clone())), sense);
            let selected = self.sel.as_ref() == Some(&r.path);
            let label = self.region_label(r);
            resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, selected && statement, &label));
            if resp.hovered() && !selected && statement {
                ui.painter().rect_stroke(rect, 3.0, Stroke::new(1.0, t.text_faint), StrokeKind::Outside);
            }
            if resp.clicked() {
                self.select(r.path.clone());
            }
            if resp.double_clicked() {
                self.select(r.path.clone());
                self.focus_statement = true;
            }
            if statement && resp.drag_started() {
                self.drag = Some(r.path.clone());
            }
            let path = r.path.clone();
            resp.context_menu(|ui| self.context_menu(ui, path));
        }
    }

    fn context_menu(&mut self, ui: &mut Ui, path: Path) {
        self.select(path);
        let ctx = ui.ctx().clone();
        let items = [
            ("Add premise above", Action::Premise),
            ("Add premise on the left", Action::Sibling(false)),
            ("Add premise on the right", Action::Sibling(true)),
            ("Add step below", Action::Below),
            ("Remove step", Action::Remove),
            ("Delete branch", Action::DeleteBranch),
            ("Copy branch", Action::Copy),
            ("Cut branch", Action::Cut),
            ("Paste branch", Action::Paste),
        ];
        for (label, a) in items {
            if ui.button(label).clicked() {
                self.act(a, &ctx);
                ui.close();
            }
        }
    }

    /// The selected step's outline and its + and × buttons.
    fn affordances(&mut self, ui: &mut Ui, regions: &[Region], to_screen: &dyn Fn(&Region) -> Rect) {
        let Some(sel) = self.selected() else { return };
        let Some(r) = regions.iter().find(|r| r.path == sel && r.part == Part::Statement) else { return };
        let t = Tokens::get(ui.ctx());
        let rect = to_screen(r).expand(4.0);
        ui.painter().rect_stroke(rect, 3.0, Stroke::new(2.0, t.accent), StrokeKind::Outside);
        let ctx = ui.ctx().clone();
        let buttons = [
            (rect.center_top() - vec2(0.0, 13.0), "+", "Add a premise above (Ctrl+Up)", Action::Premise),
            (rect.left_center() - vec2(14.0, 0.0), "+", "Add a premise on the left (Ctrl+Left)", Action::Sibling(false)),
            (rect.right_center() + vec2(14.0, 0.0), "+", "Add a premise on the right (Ctrl+Right)", Action::Sibling(true)),
            (rect.center_bottom() + vec2(0.0, 13.0), "+", "Add a step below (Ctrl+Down)", Action::Below),
            (rect.right_top() + vec2(12.0, -12.0), "×", "Remove this step (Delete)", Action::Remove),
        ];
        for (at, text, tip, a) in buttons {
            let b = egui::Button::new(RichText::new(text).strong()).corner_radius(9.0).min_size(vec2(18.0, 18.0));
            let resp = ui.put(Rect::from_center_size(at, vec2(18.0, 18.0)), b).on_hover_text(tip);
            resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, tip));
            if resp.clicked() {
                self.act(a, &ctx);
            }
        }
    }

    /// Dragging a step onto another: on its left or right quarter, beside it; else above it, as
    /// a premise.
    fn dragging(&mut self, ui: &mut Ui, regions: &[Region], to_screen: &dyn Fn(&Region) -> Rect) {
        let Some(from) = self.drag.clone() else { return };
        let t = Tokens::get(ui.ctx());
        let pointer = ui.ctx().pointer_interact_pos();
        let target = pointer.and_then(|p| {
            regions.iter().filter(|r| r.part == Part::Statement && !r.path.starts_with(&from)).find(|r| to_screen(r).expand(6.0).contains(p)).map(
                |r| {
                    let rect = to_screen(r);
                    let f = (p.x - rect.left()) / rect.width().max(1.0);
                    let place = if f < 0.25 {
                        Place::Before
                    } else if f > 0.75 {
                        Place::After
                    } else {
                        Place::Premise
                    };
                    (r.path.clone(), place, rect)
                },
            )
        });
        let painter = ui.painter();
        if let Some((_, place, rect)) = &target {
            let stroke = Stroke::new(3.0, t.accent);
            match place {
                Place::Before => painter.line_segment([rect.left_top() - vec2(6.0, 0.0), rect.left_bottom() - vec2(6.0, 0.0)], stroke),
                Place::After => painter.line_segment([rect.right_top() + vec2(6.0, 0.0), rect.right_bottom() + vec2(6.0, 0.0)], stroke),
                Place::Premise => painter.line_segment([rect.left_top() - vec2(0.0, 6.0), rect.right_top() - vec2(0.0, 6.0)], stroke),
            };
        }
        if let (Some(p), Some(n)) = (pointer, pe::node(&self.forest, &from)) {
            painter.text(p + vec2(14.0, 14.0), egui::Align2::LEFT_TOP, step_text(ui.ctx(), n), egui::FontId::proportional(13.0), t.text_dim);
        }
        if ui.input(|i| i.pointer.any_released()) {
            if let Some((to, place, _)) = target {
                self.apply(|f| pe::move_branch(f, &from, &to, place).map(Some));
            }
            self.drag = None;
        }
    }

    // -----------------------------------------------------------------------------------------
    // The side panel.

    fn side(&mut self, ui: &mut Ui) {
        self.statement_had_focus = false;
        ui.strong("Steps");
        self.outline(ui);
        ui.separator();
        ui.strong("Selected step");
        if self.forest.is_empty() {
            if ui.button("Start with a conclusion").clicked() {
                let ctx = ui.ctx().clone();
                self.act(Action::Start, &ctx);
            }
            return;
        }
        match self.selected() {
            Some(path) => self.inspector(ui, &path),
            None => {
                ui.weak("Select a step on the picture or in the list.");
            }
        }
    }

    fn outline(&mut self, ui: &mut Ui) {
        let mut pick = None;
        for (path, depth) in pe::steps(&self.forest) {
            let Some(n) = pe::node(&self.forest, &path) else { continue };
            let text = match pe::kind(n) {
                Kind::Hypothesis => step_text(ui.ctx(), n),
                Kind::Axiom | Kind::Inference => format!("{} —", step_text(ui.ctx(), n)),
            };
            ui.horizontal(|ui| {
                ui.add_space(depth as f32 * 14.0);
                if ui.selectable_label(self.sel.as_ref() == Some(&path), text).clicked() {
                    pick = Some(path.clone());
                }
            });
        }
        if let Some(p) = pick {
            self.select(p);
        }
    }

    fn inspector(&mut self, ui: &mut Ui, path: &Path) {
        let Some(n) = pe::node(&self.forest, path).cloned() else { return };
        self.statement_field(ui, path, &n);
        self.symbol_strip(ui, path);
        ui.add_space(6.0);
        self.kind_and_rule(ui, path, &n);
        ui.add_space(6.0);
        self.label_fields(ui, path, &n);
    }

    fn statement_field(&mut self, ui: &mut Ui, path: &Path, n: &Proof) {
        let caption = ui.label("Statement (LaTeX)");
        let id = statement_id(path);
        let mut text = n.concl.first().map(|c| c.src.clone()).unwrap_or_default();
        let resp = ui
            .add(egui::TextEdit::singleline(&mut text).id(id).code_editor().desired_width(f32::INFINITY).hint_text(r"\Gamma \vdash A \wedge B"))
            .labelled_by(caption.id);
        if self.focus_statement {
            resp.request_focus();
            self.focus_statement = false;
        }
        self.statement_had_focus = resp.has_focus();
        if resp.changed() {
            self.begin_typing(path);
            if let Some(node) = pe::node_mut(&mut self.forest, path) {
                match node.concl.first_mut() {
                    Some(c) => c.src = text,
                    None => node.concl.push(Stmt { src: text, math: true }),
                }
            }
        }
        let math = n.concl.first().is_none_or(|c| c.math);
        let mut as_text = !math;
        if ui.checkbox(&mut as_text, "Set as text, not math").on_hover_text("Words in upright text, with math in $…$").changed() {
            self.begin_typing(path);
            if let Some(c) = pe::node_mut(&mut self.forest, path).and_then(|node| node.concl.first_mut()) {
                c.math = !as_text;
            }
        }
    }

    fn symbol_strip(&mut self, ui: &mut Ui, path: &Path) {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(2.0, 2.0);
            for (glyph, cmd) in SYMBOLS {
                let shown = drawable(ui.ctx(), glyph, || cmd.trim().to_string());
                let b = egui::Button::new(RichText::new(shown).size(15.0)).min_size(vec2(24.0, 24.0));
                let resp = ui.add(b).on_hover_text(cmd.trim());
                resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, format!("Insert {}", cmd.trim())));
                if resp.clicked() {
                    self.insert_symbol(ui.ctx(), path, cmd);
                }
            }
        });
    }

    /// Puts `cmd` into the statement at its field's cursor (or selection), else at the end.
    fn insert_symbol(&mut self, ctx: &egui::Context, path: &Path, cmd: &str) {
        let id = statement_id(path);
        let mut state = egui::text_edit::TextEditState::load(ctx, id).unwrap_or_default();
        self.begin_typing(path);
        let Some(node) = pe::node_mut(&mut self.forest, path) else { return };
        if node.concl.is_empty() {
            node.concl.push(Stmt { src: String::new(), math: true });
        }
        let Some(c) = node.concl.first_mut() else { return };
        let len = c.src.chars().count();
        let (a, b) = state
            .cursor
            .char_range()
            .map(|r| {
                let (x, y) = (r.primary.index.0.min(len), r.secondary.index.0.min(len));
                (x.min(y), x.max(y))
            })
            .unwrap_or((len, len));
        let byte = |i: usize| c.src.char_indices().nth(i).map_or(c.src.len(), |(b, _)| b);
        let (ba, bb) = (byte(a), byte(b));
        c.src.replace_range(ba..bb, cmd);
        let at = a + cmd.chars().count();
        state.cursor.set_char_range(Some(CCursorRange::one(CCursor::new(at))));
        state.store(ctx, id);
        self.focus_statement = true;
    }

    fn kind_and_rule(&mut self, ui: &mut Ui, path: &Path, n: &Proof) {
        let k = pe::kind(n);
        let mut to = None;
        ui.horizontal(|ui| {
            ui.label("Kind");
            let leaf = n.premises.is_empty();
            for (kind, label, ok) in [(Kind::Hypothesis, "Hypothesis", leaf), (Kind::Axiom, "Axiom", leaf), (Kind::Inference, "Inference", true)] {
                if ui.add_enabled(ok, egui::RadioButton::new(k == kind, label)).clicked() && k != kind {
                    to = Some(kind);
                }
            }
        });
        if let Some(kind) = to {
            let at = path.clone();
            // An inference made from a leaf gets its first premise, selected to be typed in.
            if self.apply(|f| pe::set_kind(f, &at, kind).map(Some)) && self.sel.as_ref() != Some(path) {
                self.focus_statement = true;
            }
        }
        let rules: [(Rule, &str); 5] = [
            (Rule::Single, "Single"),
            (Rule::Double, "Double"),
            (Rule::Dashed, "Dashed"),
            (Rule::None, "No rule"),
            (Rule::Ellipsis, "Ellipsis (dots)"),
        ];
        let current = if n.rule == Rule::Dotted { Rule::Dashed } else { n.rule };
        let name = rules.iter().find(|(r, _)| *r == current).map_or("—", |(_, s)| *s);
        let mut chosen = None;
        ui.horizontal(|ui| {
            ui.label("Rule");
            ui.add_enabled_ui(k != Kind::Hypothesis, |ui| {
                craft_ui::ComboBox::from_id_salt("proof-rule").selected_text(name).width(160.0).show_ui(ui, |ui| {
                    for (r, label) in rules {
                        // An ellipsis stands between one premise and its conclusion.
                        if r == Rule::Ellipsis && n.premises.len() != 1 {
                            continue;
                        }
                        if ui.selectable_label(current == r, label).clicked() {
                            chosen = Some(r);
                        }
                    }
                });
            });
        });
        if let Some(r) = chosen.filter(|r| *r != current) {
            let at = path.clone();
            self.apply(|f| {
                if let Some(node) = pe::node_mut(f, &at) {
                    node.rule = r;
                    if r == Rule::Ellipsis {
                        node.left = None;
                    }
                }
                Ok(Some(at.clone()))
            });
        }
    }

    fn label_fields(&mut self, ui: &mut Ui, path: &Path, n: &Proof) {
        let k = pe::kind(n);
        for (right, title) in [(true, "Right label"), (false, "Left label")] {
            let enabled = k != Kind::Hypothesis && (right || n.rule != Rule::Ellipsis);
            let current = if right { &n.right } else { &n.left };
            let mut text = current.as_ref().map(|s| s.src.clone()).unwrap_or_default();
            let caption = ui.add_enabled(enabled, egui::Label::new(title));
            let resp = ui
                .add_enabled(
                    enabled,
                    egui::TextEdit::singleline(&mut text)
                        .id(Id::unique(("proof-label", right, path.clone())))
                        .code_editor()
                        .desired_width(f32::INFINITY)
                        .hint_text(r"$\wedge$I"),
                )
                .labelled_by(caption.id);
            if resp.changed() {
                self.begin_typing(path);
                let math = current.as_ref().is_some_and(|s| s.math);
                if let Some(node) = pe::node_mut(&mut self.forest, path) {
                    let slot = if right { &mut node.right } else { &mut node.left };
                    *slot = (!text.is_empty()).then_some(Stmt { src: text, math });
                }
            }
        }
        ui.weak("Labels are text: put math in $…$.");
    }

    // -----------------------------------------------------------------------------------------
    // The LaTeX pane and the footer.

    fn source_pane(&mut self, ui: &mut Ui, out: &mut Outcome) {
        egui::CollapsingHeader::new("LaTeX (ebproof)").id_salt("proof-source").default_open(false).show(ui, |ui| {
            let id = Id::unique("proof-source-text");
            let focused = ui.memory(|m| m.has_focus(id));
            let mut text = match (&self.source, focused) {
                (Some(s), true) => s.clone(),
                _ => pe::to_ebproof(&self.forest),
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
                if ui.button("Copy LaTeX").clicked() {
                    ui.ctx().copy_text(pe::to_ebproof(&self.forest));
                }
                if ui.button("Edit as LaTeX…").on_hover_text("Open this proof's LaTeX in the Equation dialog").clicked() {
                    *out = Outcome::EditLatex;
                }
            });
        });
    }

    /// Typed LaTeX becomes the tree as soon as it reads as one.
    fn source_edited(&mut self, text: &str) {
        self.source = Some(text.to_string());
        match pe::open(text) {
            Ok(o) => {
                if !pe::same_look(&o.forest, &self.forest) {
                    let before = self.snapshot();
                    self.remember(before);
                    self.forest = o.forest;
                    if self.selected().is_none() {
                        self.sel = (!self.forest.is_empty()).then(|| vec![0]);
                    }
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
                ui.label("Discard the changes to this proof?");
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
            "Click a step to select it, double-click to edit it, drag it onto another to move it. Arrows move the selection; Enter edits; \
             Ctrl+Up premise · Ctrl+Left/Right beside · Ctrl+Down below · Delete removes · {insert} inserts."
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
        for p in self.problems() {
            ui.colored_label(red, p);
        }
        if let Some(s) = &self.status {
            ui.colored_label(red, s);
        }
        let empty = pe::empty_statements(&self.forest);
        if empty > 0 {
            ui.weak(format!("{empty} step(s) have no statement yet (shown as a box)."));
        }
        for n in &self.notices {
            ui.weak(n);
        }
    }
}

#[cfg(test)]
#[path = "proof_tree_tests.rs"]
mod tests;
