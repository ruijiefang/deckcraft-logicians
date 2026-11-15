//! Insert ▸ Pseudo-code: write an algorithm line by line, as algpseudocode sets it.
//!
//! The left pane is the algorithm as it will be on the slide (the engine's layout, drawn by the
//! renderer); each row can be selected, typed into in place, given a line above or below, and
//! dragged. The right pane has the building blocks and the selected line's details; the LaTeX
//! pane below edits both ways. Every change is an engine function
//! ([`deckcraft_engine::typeset::algo::edit`]); the slide changes once, through `insert.latex`
//! or `latex.set`.

use deckcraft_engine::typeset::algo::edit::{self, Slot};
use deckcraft_engine::typeset::algo::{self, Algorithm, Frame, Kind, Line, Part, Path, Row};
use deckcraft_engine::typeset::{Part as RegionPart, Region};
use deckcraft_model::ShapeId;
use egui::text::{CCursor, CCursorRange};
use egui::{Align, Color32, Event, Id, Key, Layout, Modifiers, Pos2, Rect, RichText, ScrollArea, Sense, Stroke, TextureHandle, Ui, pos2, vec2};
use serde_json::{Value, json};

use super::{Dialog, SlideApp, Tokens};

const HISTORY: usize = 200;
const SIDE: f32 = 330.0;
const MARGIN: f64 = 8.0;
/// algorithmicx's measures, in ems: where text starts after the numbers, and one level.
const TEXT: f64 = 1.7;
const INDENT: f64 = 1.5;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Outcome {
    Stay,
    Close,
    Discard,
    Submit(bool),
    EditLatex,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Action {
    Undo,
    Redo,
    Copy,
    Cut,
    Paste,
    Duplicate,
    Delete,
    DeleteAll,
    Up,
    Down,
    Indent,
    Outdent,
}

#[derive(Clone)]
struct Snap {
    a: Algorithm,
    sel: Vec<usize>,
}

struct Canvas {
    key: u64,
    shape: Option<deckcraft_model::Shape>,
    regions: Vec<Region>,
    rows: Vec<Row>,
    w: f64,
    h: f64,
    tex: Option<TextureHandle>,
    px: f64,
    error: Option<String>,
}

/// The tool while its window is open.
pub struct Editor {
    pub(crate) a: Algorithm,
    /// Selected rows (a range), by index in `algo::rows(a, true)`.
    pub(crate) sel: Vec<usize>,
    /// The row being typed in.
    pub(crate) editing: Option<usize>,
    focus: bool,
    select_all: bool,
    undo: Vec<Snap>,
    redo: Vec<Snap>,
    target: Option<u64>,
    size: f64,
    font: String,
    notices: Vec<String>,
    status: Option<String>,
    clipboard: Vec<Line>,
    source: Option<String>,
    source_error: Option<String>,
    canvas: Option<Canvas>,
    zoom: f64,
    /// What the row being typed in was before (Esc puts it back).
    backup: Option<Algorithm>,
    drag: Option<usize>,
    confirm_discard: bool,
    dirty: bool,
    /// Where the cursor goes in the field after a keyword turned its line into a block.
    pending_cursor: Option<usize>,
    /// The text as typed in the row being edited. The field shows this, not the line read back,
    /// so a procedure's `Name(` isn't given its `)` while it is being typed.
    typing: String,
    /// The algorithm before the row being edited was changed: one undo step, taken at the first
    /// change (just looking at a row changes nothing).
    pending_undo: Option<Snap>,
}

pub(super) fn dialog(app: &mut SlideApp, ui: &mut Ui, d: &mut Dialog) -> bool {
    let mut e = match app.algo_editor.take() {
        Some(e) => e,
        None => Editor::open(app, d),
    };
    let close = e.show(app, ui);
    if !close {
        app.algo_editor = Some(e);
    }
    close
}

fn field_id() -> Id {
    Id::unique("algo-field")
}

/// The building blocks: (button, what it inserts, tooltip).
const BLOCKS: &[(&str, Kind, &str)] = &[
    ("Statement", Kind::State, "A line (\\State)"),
    ("Return", Kind::Return, "return … (\\Return)"),
    ("If", Kind::If, "if … then … end if; wraps the selected lines"),
    ("For", Kind::For, "for … do … end for"),
    ("For all", Kind::ForAll, "for all … do … end for"),
    ("While", Kind::While, "while … do … end while"),
    ("Repeat", Kind::Repeat, "repeat … until …"),
    ("Loop", Kind::Loop, "loop … end loop"),
    ("Procedure", Kind::Procedure, "procedure Name(params) … end procedure"),
    ("Function", Kind::Function, "function Name(params) … end function"),
    ("Require", Kind::Require, "Require: … (the input)"),
    ("Ensure", Kind::Ensure, "Ensure: … (the output)"),
    ("LaTeX line", Kind::Raw, "A line of LaTeX kept as written"),
];

impl Editor {
    fn empty() -> Self {
        Editor {
            a: Algorithm { body: vec![Line::state("")], ..Default::default() },
            sel: vec![0],
            editing: Some(0),
            focus: true,
            select_all: false,
            undo: Vec::new(),
            redo: Vec::new(),
            target: None,
            size: deckcraft_engine::cmd::latex::ALGORITHM_SIZE,
            font: deckcraft_engine::cmd::latex::DEFAULT_FONT.to_string(),
            notices: Vec::new(),
            status: None,
            clipboard: Vec::new(),
            source: None,
            source_error: None,
            canvas: None,
            zoom: 1.0,
            backup: None,
            drag: None,
            confirm_discard: false,
            dirty: false,
            pending_cursor: None,
            typing: String::new(),
            pending_undo: None,
        }
    }

    fn open(app: &mut SlideApp, d: &Dialog) -> Self {
        let mut e = Editor::empty();
        // Esc puts even the first line back.
        e.backup = Some(e.a.clone());
        e.pending_undo = Some(e.snapshot());
        let found = app.session.execute("latex.get", &json!({})).ok();
        let selected = found.filter(|v| {
            v.get("kind").and_then(Value::as_str) == Some("typeset") && v.get("latex").and_then(Value::as_str).is_some_and(algo::is_algorithm)
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
            match algo::open(&src) {
                Ok(o) => {
                    e.a = o.algorithm;
                    e.notices = o.notices;
                    e.sel = Vec::new();
                    e.editing = None;
                    e.focus = false;
                }
                Err(err) => {
                    e.status = Some(format!("This LaTeX can't be edited as pseudo-code ({err}). Starting a new algorithm."));
                    e.target = None;
                }
            }
        }
        e
    }

    fn rows(&self) -> Vec<Row> {
        algo::rows(&self.a, true)
    }

    fn row(&self, i: usize) -> Option<Row> {
        self.rows().get(i).cloned()
    }

    // -----------------------------------------------------------------------------------------
    // Undo and edits.

    fn snapshot(&self) -> Snap {
        Snap { a: self.a.clone(), sel: self.sel.clone() }
    }

    fn remember(&mut self, before: Snap) {
        self.undo.push(before);
        if self.undo.len() > HISTORY {
            self.undo.remove(0);
        }
        self.redo.clear();
        self.dirty = true;
    }

    /// Runs an edit returning the path (and part) to select; one undo step, or the reason shown.
    fn apply(&mut self, f: impl FnOnce(&mut Algorithm) -> Result<Option<(Path, Part)>, String>) -> bool {
        let before = self.snapshot();
        match f(&mut self.a) {
            Ok(at) => {
                self.remember(before);
                self.editing = None;
                self.status = None;
                self.sel = at.and_then(|(p, part)| edit::row_of(&self.rows(), &p, part)).into_iter().collect();
                true
            }
            Err(e) => {
                self.a = before.a;
                self.status = Some(e);
                false
            }
        }
    }

    fn undo(&mut self) {
        if let Some(s) = self.undo.pop() {
            let now = self.snapshot();
            self.redo.push(now);
            (self.a, self.sel) = (s.a, s.sel);
            self.editing = None;
            self.dirty = true;
        }
    }

    fn redo(&mut self) {
        if let Some(s) = self.redo.pop() {
            let now = self.snapshot();
            self.undo.push(now);
            (self.a, self.sel) = (s.a, s.sel);
            self.editing = None;
            self.dirty = true;
        }
    }

    /// Starts typing in row `i` (one undo step for the whole edit of it).
    fn edit_row(&mut self, i: usize, select_all: bool) {
        if self.field(i).is_none() {
            return;
        }
        if self.editing != Some(i) {
            self.pending_undo = Some(self.snapshot());
            self.backup = Some(self.a.clone());
        }
        self.typing = self.field(i).unwrap_or_default();
        self.editing = Some(i);
        self.sel = vec![i];
        self.focus = true;
        self.select_all = select_all;
    }

    fn stop_editing(&mut self) {
        self.editing = None;
        self.backup = None;
        self.pending_undo = None;
    }

    /// The row being edited is about to change: its undo step, once.
    fn touch(&mut self) {
        if let Some(before) = self.pending_undo.take() {
            self.remember(before);
        }
    }

    /// What row `i` lets you type: its text, condition, `Name(params)`, or nothing.
    fn field(&self, i: usize) -> Option<String> {
        let r = self.row(i)?;
        let l = edit::line(&self.a, &r.path)?;
        match r.part {
            Part::Line => match l.kind {
                Kind::Repeat | Kind::Loop => None,
                Kind::Procedure | Kind::Function => {
                    Some(if l.args.is_empty() && l.text.is_empty() { String::new() } else { format!("{}({})", l.text, l.args) })
                }
                _ => Some(l.text.clone()),
            },
            Part::Branch(k) => l.branches.get(k)?.cond.clone(),
            Part::End => (l.kind == Kind::Repeat).then(|| l.text.clone()),
        }
    }

    fn set_field(&mut self, i: usize, text: String) {
        let Some(r) = self.row(i) else { return };
        let Some(l) = edit::line_mut(&mut self.a, &r.path) else { return };
        match r.part {
            Part::Line if matches!(l.kind, Kind::Procedure | Kind::Function) => {
                let (n, args) = edit::split_name(&text);
                l.text = n;
                l.args = args;
            }
            Part::Branch(k) => {
                if let Some(b) = l.branches.get_mut(k) {
                    b.cond = Some(text);
                }
            }
            _ => l.text = text,
        }
        self.dirty = true;
    }

    fn comment_of(&self, i: usize) -> Option<String> {
        let r = self.row(i)?;
        let l = edit::line(&self.a, &r.path)?;
        match r.part {
            Part::Line if l.kind != Kind::Raw => Some(l.comment.clone()),
            Part::Branch(k) => l.branches.get(k).map(|b| b.comment.clone()),
            _ => None,
        }
    }

    fn set_comment(&mut self, i: usize, text: String) {
        let Some(r) = self.row(i) else { return };
        let Some(l) = edit::line_mut(&mut self.a, &r.path) else { return };
        match r.part {
            Part::Branch(k) => {
                if let Some(b) = l.branches.get_mut(k) {
                    b.comment = text;
                }
            }
            _ => l.comment = text,
        }
        self.dirty = true;
    }

    /// Adds `line` below row `i` (or at the end) and starts typing in it.
    fn add_below(&mut self, i: Option<usize>, line: Line) {
        let slot = i.and_then(|i| self.row(i)).and_then(|r| edit::slot_below(&self.a, &r)).unwrap_or(Slot {
            parent: Vec::new(),
            list: 0,
            index: self.a.body.len(),
        });
        self.add_at(slot, line);
    }

    fn add_above(&mut self, i: usize, line: Line) {
        if let Some(slot) = self.row(i).and_then(|r| edit::slot_above(&self.a, &r)) {
            self.add_at(slot, line);
        }
    }

    fn add_at(&mut self, slot: Slot, line: Line) {
        let block = line.kind.is_block();
        if self.apply(|a| edit::insert(a, &slot, line).map(|p| Some((p, Part::Line))))
            && let Some(&i) = self.sel.first()
        {
            // A Repeat or Loop has nothing to type on its first row: its first line instead.
            let target = if block && self.field(i).is_none() { i + 1 } else { i };
            self.edit_row(target, true);
        }
    }

    /// The selected rows' lines, outermost first, without lines inside another selected one.
    fn selected_lines(&self) -> Vec<Path> {
        let rows = self.rows();
        let mut out: Vec<Path> = Vec::new();
        for &i in &self.sel {
            let Some(r) = rows.get(i) else { continue };
            if r.part != Part::Line {
                continue;
            }
            if out.iter().any(|p| r.path.len() > p.len() && r.path.get(..p.len()) == Some(&p[..])) {
                continue;
            }
            out.push(r.path.clone());
        }
        out
    }

    fn insert_block(&mut self, kind: Kind) {
        let lines = self.selected_lines();
        if kind.is_block() && lines.len() > 1 {
            self.apply(|a| edit::wrap(a, &lines, kind).map(|p| Some((p, Part::Line))));
            if let Some(&i) = self.sel.first() {
                self.edit_row(i, true);
            }
            return;
        }
        let mut l = if kind.is_block() { edit::block(kind, false) } else { Line::new(kind, "") };
        if kind == Kind::Raw {
            l.numbered = false;
        }
        self.add_below(self.sel.last().copied(), l);
    }

    /// The If at or around row `i`.
    fn enclosing_if(&self, i: usize) -> Option<Path> {
        let r = self.row(i)?;
        let mut p = r.path.clone();
        loop {
            if edit::line(&self.a, &p).is_some_and(|l| l.kind == Kind::If) {
                return Some(p);
            }
            p.pop()?;
        }
    }

    fn add_branch(&mut self, cond: Option<String>) {
        let Some(path) = self.sel.first().and_then(|&i| self.enclosing_if(i)) else {
            self.status = Some("Select an If (or a line inside one) first.".into());
            return;
        };
        let with_cond = cond.is_some();
        let mut k = 0;
        if self.apply(|a| {
            k = edit::add_branch(a, &path, cond)?;
            Ok(Some((path.clone(), Part::Branch(k))))
        }) && let Some(&i) = self.sel.first()
        {
            self.edit_row(if with_cond { i } else { i + 1 }, true);
        }
    }

    /// Turns the statement at row `i` that was typed as `else` or `else if …` into a part of its
    /// If: the lines after it in its part move into the new one.
    fn typed_branch(&mut self, i: usize, text: &str) -> bool {
        let lower = text.trim_start().to_ascii_lowercase();
        let cond = if lower == "else" {
            None
        } else if lower.starts_with("else if ") {
            Some(text.trim_start().get(8..).unwrap_or("").to_string())
        } else {
            return false;
        };
        let Some(r) = self.row(i).filter(|r| r.part == Part::Line && r.path.len() >= 2) else { return false };
        let parent: Path = r.path.get(..r.path.len().saturating_sub(1)).unwrap_or(&[]).to_vec();
        let Some(&(list, index)) = r.path.last() else { return false };
        if edit::line(&self.a, &parent).is_none_or(|l| l.kind != Kind::If) {
            return false;
        }
        let done = self.apply(|a| {
            let l = edit::line_mut(a, &parent).ok_or("That line isn't there any more.")?;
            if cond.is_none() && l.branches.iter().any(|b| b.cond.is_none()) && list < l.branches.len() {
                return Err("It has an else already.".into());
            }
            let from = if list == 0 { &mut l.body } else { &mut l.branches.get_mut(list - 1).ok_or("That part isn't there any more.")?.body };
            let mut rest: Vec<Line> = from.drain(index.min(from.len())..).collect();
            if !rest.is_empty() {
                rest.remove(0);
            }
            if rest.is_empty() {
                rest.push(Line::state(""));
            }
            l.branches.insert(list, algo::Branch { cond, comment: String::new(), body: rest });
            Ok(Some((parent.clone(), Part::Branch(list))))
        });
        // The condition was typed already: on to the part's first line.
        if done && let Some(&b) = self.sel.first() {
            self.edit_row(b + 1, false);
        }
        done
    }

    fn act(&mut self, a: Action, ctx: &egui::Context) {
        let lines = self.selected_lines();
        let first = self.sel.first().copied();
        match a {
            Action::Undo => self.undo(),
            Action::Redo => self.redo(),
            Action::Copy | Action::Cut => {
                if lines.is_empty() {
                    self.status = Some("Select lines to copy first.".into());
                    return;
                }
                self.clipboard = lines.iter().filter_map(|p| edit::line(&self.a, p).cloned()).collect();
                let part = Algorithm { body: self.clipboard.clone(), frame: Frame::None, ..Default::default() };
                ctx.copy_text(algo::write(&part));
                if a == Action::Cut {
                    self.apply(|al| {
                        for p in lines.iter().rev() {
                            edit::remove(al, p);
                        }
                        Ok(None)
                    });
                }
            }
            Action::Paste => self.paste(None),
            Action::Duplicate => {
                let Some(p) = lines.last().cloned() else { return };
                let Some(l) = edit::line(&self.a, &p).cloned() else { return };
                let mut slot_path = p.clone();
                if let Some(last) = slot_path.last_mut() {
                    last.1 += 1;
                }
                let (parent, (list, index)) =
                    (slot_path.get(..slot_path.len().saturating_sub(1)).unwrap_or(&[]).to_vec(), slot_path.last().copied().unwrap_or((0, 0)));
                self.apply(|al| edit::insert(al, &Slot { parent, list, index }, l).map(|p| Some((p, Part::Line))));
            }
            Action::Delete | Action::DeleteAll => {
                let Some(i) = first else { return };
                let Some(r) = self.row(i) else { return };
                match r.part {
                    Part::Branch(k) => {
                        self.apply(|al| edit::remove_branch(al, &r.path, k).map(|_| Some((r.path.clone(), Part::Line))));
                    }
                    _ => {
                        let block = edit::line(&self.a, &r.path).is_some_and(|l| l.kind.is_block());
                        if block && a == Action::Delete && lines.len() <= 1 {
                            self.apply(|al| edit::unwrap(al, &r.path).map(|p| Some((p, Part::Line))));
                        } else {
                            let targets = if lines.is_empty() { vec![r.path.clone()] } else { lines };
                            self.apply(|al| {
                                for p in targets.iter().rev() {
                                    edit::remove(al, p);
                                }
                                Ok(None)
                            });
                            self.sel = vec![i.min(self.rows().len().saturating_sub(1))];
                        }
                    }
                }
            }
            Action::Up | Action::Down | Action::Indent | Action::Outdent => self.restructure(a),
        }
    }

    /// Moves the first selected line: up, down, into the block above, or out of its block.
    fn restructure(&mut self, a: Action) {
        let Some(p) = self.selected_lines().first().cloned() else { return };
        self.apply(|al| {
            let to = match a {
                Action::Up => edit::move_by(al, &p, true),
                Action::Down => edit::move_by(al, &p, false),
                Action::Indent => edit::indent(al, &p),
                _ => edit::outdent(al, &p),
            }?;
            Ok(Some((to, Part::Line)))
        });
    }

    fn paste(&mut self, text: Option<&str>) {
        let from_text = text.filter(|t| algo::is_algorithm(t)).and_then(|t| algo::open(t).ok()).map(|o| o.algorithm.body);
        let lines = from_text.unwrap_or_else(|| self.clipboard.clone());
        if lines.is_empty() {
            if let Some(t) = text.filter(|t| !t.trim().is_empty()) {
                // Plain text: a line of it, at the selection.
                self.add_below(self.sel.last().copied(), Line::state(t.trim()));
            }
            return;
        }
        let slot = self.sel.last().and_then(|&i| self.row(i)).and_then(|r| edit::slot_below(&self.a, &r)).unwrap_or(Slot {
            parent: Vec::new(),
            list: 0,
            index: self.a.body.len(),
        });
        self.apply(|a| {
            let mut last = None;
            for (k, l) in lines.into_iter().enumerate() {
                last = Some(edit::insert(a, &Slot { index: slot.index + k, ..slot.clone() }, l)?);
            }
            Ok(last.map(|p| (p, Part::Line)))
        });
    }

    fn template(&mut self, name: &str) {
        let Some(t) = edit::template(name) else { return };
        let empty = self.a.body.iter().all(|l| l.kind == Kind::State && l.text.trim().is_empty());
        if empty {
            self.apply(|a| {
                *a = t;
                Ok(None)
            });
        } else {
            self.clipboard = t.body;
            self.paste(None);
        }
    }

    fn can_submit(&self) -> bool {
        self.a.body.iter().any(|l| l.kind != Kind::State || !l.text.trim().is_empty() || !l.comment.trim().is_empty())
    }

    // -----------------------------------------------------------------------------------------
    // The window.

    fn show(&mut self, app: &mut SlideApp, ui: &mut Ui) -> bool {
        let mut out = Outcome::Stay;
        let n = self.rows().len();
        self.sel.retain(|&i| i < n);
        if self.editing.is_some_and(|i| i >= n) {
            self.editing = None;
        }
        self.keys(ui, &mut out);
        egui::Panel::top("algo-toolbar").resizable(false).show(ui, |ui| self.toolbar(ui));
        egui::Panel::bottom("algo-footer").resizable(false).show(ui, |ui| {
            self.source_pane(ui, &mut out);
            ui.separator();
            self.footer(ui, &mut out);
        });
        egui::Panel::right("algo-side").exact_size(SIDE).resizable(false).show(ui, |ui| {
            ScrollArea::vertical().id_salt("algo-side").auto_shrink([false, false]).show(ui, |ui| self.side(ui));
        });
        egui::CentralPanel::default().show(ui, |ui| self.canvas(app, ui, &mut out));
        match out {
            Outcome::Stay => false,
            Outcome::Close if self.dirty && !self.confirm_discard => {
                self.confirm_discard = true;
                false
            }
            Outcome::Close | Outcome::Discard => true,
            Outcome::Submit(new) => self.can_submit() && self.submit(app, new),
            Outcome::EditLatex => {
                let mut d = Dialog::new("equation");
                let editing = self.target.is_some().to_string();
                d.fields.insert("eq".into(), algo::write(&self.a));
                d.fields.insert("as".into(), "typeset".into());
                d.fields.insert("found".into(), editing.clone());
                d.fields.insert("edit".into(), editing);
                app.dialog = Some(d);
                true
            }
        }
    }

    fn submit(&mut self, app: &mut SlideApp, as_new: bool) -> bool {
        let mut p = json!({"latex": algo::write(&self.a), "size": self.size, "font": self.font});
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

    /// Keys when no row is being typed in (the row's own field handles them while it is).
    fn keys(&mut self, ui: &Ui, out: &mut Outcome) {
        let ctx = ui.ctx().clone();
        let consume = |m: Modifiers, k: Key| ctx.input_mut(|i| i.consume_key(m, k));
        if consume(Modifiers::COMMAND, Key::Enter) {
            *out = Outcome::Submit(false);
            return;
        }
        if self.editing.is_some() || ctx.text_edit_focused() {
            return;
        }
        let action = [
            (Modifiers::COMMAND | Modifiers::SHIFT, Key::Z, Action::Redo),
            (Modifiers::COMMAND, Key::Y, Action::Redo),
            (Modifiers::COMMAND, Key::Z, Action::Undo),
            (Modifiers::COMMAND, Key::D, Action::Duplicate),
            (Modifiers::COMMAND, Key::ArrowUp, Action::Up),
            (Modifiers::COMMAND, Key::ArrowDown, Action::Down),
            (Modifiers::SHIFT, Key::Tab, Action::Outdent),
            (Modifiers::NONE, Key::Tab, Action::Indent),
            (Modifiers::SHIFT, Key::Delete, Action::DeleteAll),
            (Modifiers::NONE, Key::Delete, Action::Delete),
            (Modifiers::NONE, Key::Backspace, Action::Delete),
        ]
        .into_iter()
        .find(|(m, k, _)| consume(*m, *k))
        .map(|(_, _, a)| a);
        if let Some(a) = action {
            return self.act(a, &ctx);
        }
        let n = self.rows().len();
        for (key, down) in [(Key::ArrowUp, false), (Key::ArrowDown, true)] {
            for shift in [false, true] {
                let m = if shift { Modifiers::SHIFT } else { Modifiers::NONE };
                if consume(m, key) && n > 0 {
                    let cur = if down { self.sel.last().copied() } else { self.sel.first().copied() };
                    let next = match cur {
                        Some(c) if down => (c + 1).min(n - 1),
                        Some(c) => c.saturating_sub(1),
                        None => 0,
                    };
                    if shift && !self.sel.is_empty() {
                        let (a, b) = (self.sel.first().copied().unwrap_or(next).min(next), self.sel.last().copied().unwrap_or(next).max(next));
                        self.sel = (a..=b).collect();
                    } else {
                        self.sel = vec![next];
                    }
                    return;
                }
            }
        }
        if consume(Modifiers::NONE, Key::Enter) || consume(Modifiers::NONE, Key::F2) {
            match self.sel.first().copied() {
                Some(i) if self.field(i).is_some() => self.edit_row(i, false),
                Some(i) => self.add_below(Some(i), Line::state("")),
                None => self.add_below(None, Line::state("")),
            }
            return;
        }
        if consume(Modifiers::NONE, Key::Escape) {
            if self.sel.is_empty() {
                *out = Outcome::Close;
            } else {
                self.sel.clear();
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
            tool(ui, "Cut", "Cut the selected lines (Ctrl+X)", has, Action::Cut);
            tool(ui, "Copy", "Copy the selected lines (Ctrl+C)", has, Action::Copy);
            tool(ui, "Paste", "Paste below the selection (Ctrl+V)", !self.clipboard.is_empty(), Action::Paste);
            tool(ui, "Duplicate", "Duplicate the line (Ctrl+D)", has, Action::Duplicate);
            tool(ui, "Delete", "Delete the line; a block's lines stay (Delete). Shift+Delete deletes a block with its lines", has, Action::Delete);
            ui.separator();
            tool(ui, "Move up", "Move the line up (Ctrl+Up)", has, Action::Up);
            tool(ui, "Move down", "Move the line down (Ctrl+Down)", has, Action::Down);
            tool(ui, "Indent", "Into the block above (Tab)", has, Action::Indent);
            tool(ui, "Outdent", "Out of its block (Shift+Tab)", has, Action::Outdent);
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
            .on_hover_text("Start from a known algorithm (pasted below the selection when there is one)");
            if let Some(n) = chosen {
                self.template(n);
            }
            ui.separator();
            if ui.button("Zoom out").clicked() {
                self.zoom = (self.zoom / 1.25).max(0.4);
            }
            if ui.button("Fit").clicked() {
                self.zoom = 1.0;
            }
            if ui.button("Zoom in").clicked() {
                self.zoom = (self.zoom * 1.25).min(3.0);
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

    fn refresh(&mut self, app: &SlideApp, ctx: &egui::Context, width: f32) {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        (algo::write(&self.a), self.size.to_bits(), self.font.as_str()).hash(&mut h);
        let key = h.finish();
        let Ok(st) = app.session.doc() else { return };
        if self.canvas.as_ref().is_none_or(|c| c.key != key) {
            let font = Some(self.font.trim()).filter(|f| !f.is_empty());
            self.canvas = Some(match deckcraft_engine::cmd::latex::preview_algorithm(st, &self.a, self.size, font) {
                Ok((p, rows)) => Canvas { key, shape: Some(p.shape), regions: p.regions, rows, w: p.w, h: p.h, tex: None, px: 0.0, error: None },
                Err(e) => Canvas { key, shape: None, regions: Vec::new(), rows: Vec::new(), w: 0.0, h: 0.0, tex: None, px: 0.0, error: Some(e) },
            });
        }
        let Some(c) = self.canvas.as_mut() else { return };
        let Some(shape) = c.shape.clone() else { return };
        let ppp = f64::from(ctx.pixels_per_point());
        // Fit to the width; a long algorithm scrolls instead of shrinking.
        let fit = ((f64::from(width) - 24.0) / (c.w + 2.0 * MARGIN)).clamp(0.4, 1.6);
        let px = (fit * self.zoom * ppp).max(0.05);
        if c.tex.is_some() && (px - c.px).abs() < 0.01 {
            return;
        }
        let pres = &st.doc;
        let Some(mut slide) = pres.slides.get(st.selection.slide).or_else(|| pres.slides.first()).map(|s| s.as_ref().clone()) else { return };
        let id: ShapeId = shape.id;
        slide.shapes = vec![shape];
        c.px = px;
        c.tex = deckcraft_render::render_shape(pres, &slide, id, px)
            .map(|img| crate::textures::upload(ctx, "algo-preview", crate::textures::to_color_image(&img, false)));
    }

    fn canvas(&mut self, app: &SlideApp, ui: &mut Ui, out: &mut Outcome) {
        let width = ui.available_width();
        self.refresh(app, ui.ctx(), width);
        let Some(c) = self.canvas.take() else { return };
        if let Some(e) = &c.error {
            ui.colored_label(Color32::from_rgb(0xC0, 0x30, 0x30), format!("The algorithm can't be drawn: {e}"));
            self.canvas = Some(c);
            return;
        }
        let ppp = f64::from(ui.ctx().pixels_per_point());
        let k = if c.px > 0.0 { c.px / ppp } else { 1.0 };
        let img = vec2(((c.w + 2.0 * MARGIN) * k) as f32, ((c.h + 2.0 * MARGIN) * k) as f32);
        ScrollArea::both().id_salt("algo-canvas").auto_shrink([false, false]).show(ui, |ui| {
            let size = vec2(img.x.max(ui.available_width() - 4.0), img.y + 60.0);
            let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
            let origin = pos2(rect.min.x + ((size.x - img.x) / 2.0).max(0.0), rect.min.y);
            ui.painter().rect_filled(Rect::from_min_size(origin, img), 4.0, Color32::WHITE);
            if let Some(tex) = &c.tex {
                ui.painter().image(tex.id(), Rect::from_min_size(origin, img), Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
            }
            let to_screen = |x: f64, y: f64| origin + vec2(((x + MARGIN) * k) as f32, ((y + MARGIN) * k) as f32);
            self.rows_ui(ui, &c, &to_screen, k, out);
            // Below the last line: a click adds a line at the end.
            let below = Rect::from_min_max(pos2(origin.x, origin.y + img.y), pos2(origin.x + img.x, origin.y + img.y + 50.0));
            let resp = ui.interact(below, Id::unique("algo-below"), Sense::click());
            resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, "Add a line at the end"));
            if resp.hovered() {
                ui.painter().text(
                    below.center(),
                    egui::Align2::CENTER_CENTER,
                    "+ add a line",
                    egui::FontId::proportional(13.0),
                    Tokens::get(ui.ctx()).text_faint,
                );
            }
            if resp.clicked() {
                self.add_below(None, Line::state(""));
            }
        });
        self.canvas = Some(c);
    }

    /// The text a row's words describe, for the interface.
    fn row_label(&self, r: &Row) -> String {
        let text = edit::line(&self.a, &r.path).map(|l| match r.part {
            Part::Line => format!("{} {}", l.kind.name(), algo::plain(&l.text)),
            Part::Branch(k) => l.branches.get(k).map_or_else(String::new, |b| match &b.cond {
                Some(c) => format!("Else if {}", algo::plain(c)),
                None => "Else".into(),
            }),
            Part::End => format!("End of {}", l.kind.name()),
        });
        text.unwrap_or_default().trim().to_string()
    }

    fn rows_ui(&mut self, ui: &mut Ui, c: &Canvas, to_screen: &dyn Fn(f64, f64) -> Pos2, k: f64, out: &mut Outcome) {
        let t = Tokens::get(ui.ctx());
        let em = self.size;
        let left = if self.a.numbering > 0 { TEXT * em } else { 0.0 };
        let regions: Vec<&Region> = c.regions.iter().filter(|r| r.part == RegionPart::Row).collect();
        let pointer = ui.ctx().pointer_interact_pos();
        let mut drop_at: Option<(usize, bool)> = None;
        for (i, r) in regions.iter().enumerate() {
            let rect = Rect::from_min_max(to_screen(r.x, r.y), to_screen(r.x + r.w, r.y + r.h));
            let Some(row) = c.rows.get(i).cloned() else { continue };
            let selected = self.sel.contains(&i);
            let label = format!("Line {}: {}", row.number.map_or_else(|| "–".to_string(), |n| n.to_string()), self.row_label(&row));
            let resp = ui.interact(rect, Id::unique(("algo-row", i)), Sense::click_and_drag());
            resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, selected, &label));
            if selected {
                ui.painter().rect_filled(rect, 2.0, Color32::from_rgba_unmultiplied(t.accent.r(), t.accent.g(), t.accent.b(), 28));
            } else if resp.hovered() {
                ui.painter().rect_filled(rect, 2.0, Color32::from_rgba_unmultiplied(0, 0, 0, 10));
            }
            if resp.clicked() {
                let shift = ui.input(|inp| inp.modifiers.shift);
                if shift && !self.sel.is_empty() {
                    let (a, b) = (self.sel.iter().copied().min().unwrap_or(i).min(i), self.sel.iter().copied().max().unwrap_or(i).max(i));
                    self.sel = (a..=b).collect();
                } else {
                    self.sel = vec![i];
                }
                if self.editing != Some(i) {
                    self.stop_editing();
                }
            }
            if resp.double_clicked() {
                if self.field(i).is_some() {
                    self.edit_row(i, false);
                } else {
                    self.add_below(Some(i), Line::state(""));
                }
            }
            if resp.drag_started() && row.part == Part::Line {
                self.drag = Some(i);
            }
            if self.drag.is_some()
                && let Some(p) = pointer
                && rect.contains(p)
            {
                drop_at = Some((i, p.y > rect.center().y));
            }
            // + above and + below, beside the hovered (or selected) row.
            if (resp.hovered() || selected) && self.drag.is_none() && self.editing != Some(i) {
                for (dy, below, tip) in [(-1.0f32, false, "Add a line above"), (1.0, true, "Add a line below")] {
                    let at = pos2(rect.left() - 13.0, if below { rect.bottom() } else { rect.top() } + dy * 2.0);
                    let b = egui::Button::new(RichText::new("+").strong().color(t.accent_text))
                        .fill(t.accent)
                        .corner_radius(8.0)
                        .min_size(vec2(16.0, 16.0));
                    let br = ui.put(Rect::from_center_size(at, vec2(16.0, 16.0)), b).on_hover_text(tip);
                    br.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, format!("{tip} line {}", i + 1)));
                    if br.clicked() {
                        if below {
                            self.add_below(Some(i), Line::state(""));
                        } else {
                            self.add_above(i, Line::state(""));
                        }
                    }
                }
            }
            if self.editing == Some(i) {
                let indent = left + INDENT * em * row.depth as f64;
                // The fields may stand out past a narrow algorithm's right edge, to have room.
                let x = to_screen(indent, 0.0).x;
                // Both stay inside the visible pane: the text gives way to the comment.
                let limit = ui.clip_rect().right() - 4.0;
                let end = rect.right().max(x + 470.0).min(limit);
                let right = (rect.right() - rect.width() * 0.3).max(x + 300.0).min(end - 150.0).max(x + 80.0);
                let text_rect = Rect::from_min_max(pos2(x, rect.top()), pos2(right, rect.bottom()));
                let comment_rect = Rect::from_min_max(pos2(right + 6.0, rect.top()), pos2(end.max(right + 40.0), rect.bottom()));
                self.inline_editor(ui, i, text_rect, comment_rect, k, out);
            }
        }
        if let Some(from) = self.drag {
            if let Some((to, below)) = drop_at
                && let Some(target) = regions.get(to)
            {
                let y = if below { target.y + target.h } else { target.y };
                let a = to_screen(target.x, y);
                ui.painter().line_segment([a, pos2(a.x + (target.w * k) as f32, a.y)], Stroke::new(3.0, t.accent));
            }
            if ui.input(|inp| inp.pointer.any_released()) {
                if let (Some((to, below)), Some(src)) = (drop_at, c.rows.get(from).cloned())
                    && let Some(target) = c.rows.get(to)
                {
                    let slot = if below { edit::slot_below(&self.a, target) } else { edit::slot_above(&self.a, target) };
                    if let Some(slot) = slot
                        && to != from
                    {
                        self.apply(|a| edit::move_to(a, &src.path, &slot).map(|p| Some((p, Part::Line))));
                    }
                }
                self.drag = None;
            }
        }
    }

    /// The fields over row `i` while it is typed in: its text, and its comment at the right.
    fn inline_editor(&mut self, ui: &mut Ui, i: usize, text_rect: Rect, comment_rect: Rect, k: f64, out: &mut Outcome) {
        let ctx = ui.ctx().clone();
        let focused = ctx.memory(|m| m.has_focus(field_id()));
        let mut moved = false;
        // egui lets go of the field's focus at Esc itself, so this doesn't wait for focus.
        if ctx.input_mut(|inp| inp.consume_key(Modifiers::NONE, Key::Escape)) {
            if let Some(b) = self.backup.take() {
                // Changed: its undo step goes too, as the line is as it was.
                if self.pending_undo.is_none() {
                    self.undo.pop();
                }
                self.a = b;
            }
            self.dirty = !self.undo.is_empty();
            self.pending_undo = None;
            self.editing = None;
            ctx.memory_mut(|m| m.surrender_focus(field_id()));
            return;
        }
        if focused {
            let consume = |m: Modifiers, key: Key| ctx.input_mut(|inp| inp.consume_key(m, key));
            let text = self.typing.clone();
            if consume(Modifiers::SHIFT, Key::Tab) {
                self.stop_editing();
                self.restructure(Action::Outdent);
                if let Some(&j) = self.sel.first() {
                    self.edit_row(j, false);
                }
                return;
            }
            if consume(Modifiers::NONE, Key::Tab) {
                self.stop_editing();
                self.restructure(Action::Indent);
                if let Some(&j) = self.sel.first() {
                    self.edit_row(j, false);
                }
                return;
            }
            for (key, down) in [(Key::ArrowUp, false), (Key::ArrowDown, true)] {
                if consume(Modifiers::NONE, key) {
                    let n = self.rows().len();
                    let mut j = i;
                    loop {
                        j = if down {
                            j + 1
                        } else {
                            match j.checked_sub(1) {
                                Some(x) => x,
                                None => break,
                            }
                        };
                        if j >= n {
                            break;
                        }
                        if self.field(j).is_some() {
                            self.stop_editing();
                            self.edit_row(j, false);
                            moved = true;
                            break;
                        }
                    }
                    if moved {
                        return;
                    }
                }
            }
            if text.is_empty() && consume(Modifiers::NONE, Key::Backspace) {
                let row = self.row(i);
                if row.as_ref().is_some_and(|r| r.part == Part::Line && edit::line(&self.a, &r.path).is_some_and(|l| l.kind == Kind::State)) {
                    self.stop_editing();
                    if let Some(r) = row {
                        self.apply(|a| {
                            edit::remove(a, &r.path);
                            Ok(None)
                        });
                    }
                    let back = i.saturating_sub(1);
                    if back < self.rows().len() {
                        self.edit_row(back, false);
                    }
                    return;
                }
            }
            if consume(Modifiers::NONE, Key::Enter) {
                self.enter(i, &text);
                return;
            }
        }
        let mut text = self.typing.clone();
        // The cursor goes where it was asked to before the field reads its keys this frame.
        if let Some(at) = self.pending_cursor.take() {
            let mut state = egui::text_edit::TextEditState::load(&ctx, field_id()).unwrap_or_default();
            state.cursor.set_char_range(Some(CCursorRange::one(CCursor::new(at))));
            state.store(&ctx, field_id());
        }
        let font = egui::FontId::monospace(((self.size * 0.62) * k).clamp(11.0, 26.0) as f32);
        let t = Tokens::get(ui.ctx());
        ui.painter().rect_filled(text_rect.expand(2.0), 3.0, t.text_field);
        let edit = egui::TextEdit::singleline(&mut text)
            .id(field_id())
            .font(font.clone())
            .desired_width(text_rect.width())
            // Tab and Shift+Tab move the line (handled above), not the focus.
            .lock_focus(true)
            .hint_text("type: a statement, or if …, while …, for …, return …, procedure Name(x)…")
            .frame(egui::Frame::NONE);
        let resp = ui.put(text_rect, edit);
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, true, "Line (LaTeX)"));
        if self.focus {
            resp.request_focus();
            if self.select_all {
                let mut state = egui::text_edit::TextEditState::load(&ctx, field_id()).unwrap_or_default();
                state.cursor.set_char_range(Some(CCursorRange::two(CCursor::new(0), CCursor::new(text.chars().count()))));
                state.store(&ctx, field_id());
            }
            self.focus = false;
            self.select_all = false;
        }
        if resp.changed() {
            self.touch();
            self.typing = text.clone();
            if !self.smart(i, &text, false) {
                self.set_field(i, text);
            }
        }
        if let Some(mut comment) = self.comment_of(i) {
            ui.painter().rect_filled(comment_rect.expand(2.0), 3.0, t.text_field);
            let c =
                egui::TextEdit::singleline(&mut comment).id(Id::unique("algo-comment")).font(font).hint_text("▷ comment").frame(egui::Frame::NONE);
            let cr = ui.put(comment_rect, c);
            cr.widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, true, "Comment"));
            if cr.changed() {
                self.touch();
                self.set_comment(i, comment);
            }
        }
        let _ = out;
    }

    /// A keyword typed first turns the statement into its block (`whole`: at Enter).
    fn smart(&mut self, i: usize, text: &str, whole: bool) -> bool {
        let Some(r) = self.row(i) else { return false };
        let plain = r.part == Part::Line && edit::line(&self.a, &r.path).is_some_and(|l| l.kind == Kind::State);
        if !plain {
            return false;
        }
        let Some((kind, rest)) = edit::typed(text, whole) else { return false };
        let Some(l) = edit::line_mut(&mut self.a, &r.path) else { return false };
        let comment = l.comment.clone();
        let mut new = edit::from_typed(kind, &rest);
        new.comment = comment;
        *l = new;
        // Keep typing where the text goes (a Repeat's text is at its `until`).
        if kind == Kind::Repeat || kind == Kind::Loop {
            self.stop_editing();
            self.edit_row(i + 1, false);
        } else {
            // The keyword is gone from the field; what followed it stays, as typed.
            self.focus = true;
            self.pending_cursor = Some(rest.chars().count());
            self.typing = rest;
        }
        true
    }

    /// Enter: `else` and `else if …` become a part of the If; a keyword makes its block; else a
    /// new line below.
    fn enter(&mut self, i: usize, text: &str) {
        if self.typed_branch(i, text) {
            return;
        }
        if self.smart(i, text, true) {
            return;
        }
        let Some(r) = self.row(i) else { return };
        let Some(l) = edit::line(&self.a, &r.path) else { return };
        // A block's first row: into its first line when that is still empty.
        let first = match r.part {
            Part::Line if l.kind.is_block() => l.body.first(),
            Part::Branch(k) => l.branches.get(k).and_then(|b| b.body.first()),
            _ => None,
        };
        if first.is_some_and(|f| f.kind == Kind::State && f.text.is_empty() && f.body.is_empty()) {
            self.stop_editing();
            self.edit_row(i + 1, false);
            return;
        }
        // An empty last line of a block: out of the block, as when typing it on paper.
        let last = r.path.last().is_some_and(|&(list, index)| {
            let parent = r.path.get(..r.path.len().saturating_sub(1)).unwrap_or(&[]);
            let len = edit::line(&self.a, parent)
                .map_or(0, |p| if list == 0 { p.body.len() } else { p.branches.get(list - 1).map_or(0, |b| b.body.len()) });
            index + 1 == len
        });
        if r.part == Part::Line && r.depth > 0 && last && l.kind == Kind::State && text.trim().is_empty() && l.comment.is_empty() {
            self.stop_editing();
            self.restructure(Action::Outdent);
            if let Some(&j) = self.sel.first() {
                self.edit_row(j, false);
            }
            return;
        }
        self.stop_editing();
        self.add_below(Some(i), Line::state(""));
    }

    // -----------------------------------------------------------------------------------------
    // The side panel.

    fn side(&mut self, ui: &mut Ui) {
        ui.strong("Add");
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
            for (label, kind, tip) in BLOCKS {
                let wraps = kind.is_block() && self.selected_lines().len() > 1;
                let tip = if wraps { format!("{tip} (around the selected lines)") } else { tip.to_string() };
                if ui.button(*label).on_hover_text(tip).clicked() {
                    self.insert_block(*kind);
                }
            }
            let in_if = self.sel.first().is_some_and(|&i| self.enclosing_if(i).is_some());
            if ui.add_enabled(in_if, egui::Button::new("Else if")).on_hover_text("Add an else if to the If").clicked() {
                self.add_branch(Some(String::new()));
            }
            if ui.add_enabled(in_if, egui::Button::new("Else")).on_hover_text("Add an else to the If").clicked() {
                self.add_branch(None);
            }
        });
        ui.weak("Or type at the start of a line: if …, while …, for …, for all …, return …, repeat, loop, procedure Name(x), else, else if ….");
        ui.separator();
        match self.sel.as_slice() {
            [] => self.settings(ui),
            [i] => self.details(ui, *i),
            many => {
                ui.strong(format!("{} rows", many.len()));
                ui.weak("Pick a block above to put the selected lines inside it.");
            }
        }
    }

    fn details(&mut self, ui: &mut Ui, i: usize) {
        let Some(r) = self.row(i) else { return };
        let Some(l) = edit::line(&self.a, &r.path).cloned() else { return };
        ui.strong(match r.part {
            Part::Line => l.kind.name().to_string(),
            Part::Branch(k) => {
                if l.branches.get(k).is_some_and(|b| b.cond.is_some()) {
                    "Else if".into()
                } else {
                    "Else".into()
                }
            }
            Part::End => format!("End of {}", l.kind.name()),
        });
        let caption = match (r.part, l.kind) {
            (Part::Line, Kind::If | Kind::While) | (Part::Branch(_), _) | (Part::End, Kind::Repeat) => "Condition",
            (Part::Line, Kind::For | Kind::ForAll) => "Range",
            (Part::Line, Kind::Procedure | Kind::Function) => "Name(parameters)",
            (Part::Line, Kind::Raw) => "LaTeX",
            (Part::Line, Kind::Return) => "Value",
            _ => "Text",
        };
        if let Some(mut text) = self.field(i) {
            ui.label(format!("{caption} (LaTeX)"));
            let resp = ui.add(egui::TextEdit::singleline(&mut text).code_editor().desired_width(f32::INFINITY));
            if resp.gained_focus() {
                self.edit_row(i, false);
                self.focus = false;
            }
            if resp.changed() {
                self.touch();
                if self.editing == Some(i) {
                    self.typing = text.clone();
                }
                self.set_field(i, text);
            }
        }
        if let Some(mut c) = self.comment_of(i) {
            ui.label("Comment");
            if ui.add(egui::TextEdit::singleline(&mut c).code_editor().desired_width(f32::INFINITY).hint_text("shown as ▷ … at the right")).changed()
            {
                let before = self.snapshot();
                self.set_comment(i, c);
                self.remember(before);
            }
        }
        if r.part == Part::Line && l.kind != Kind::Raw {
            ui.label("Label (for \\ref)");
            let mut label = l.label.clone();
            if ui.add(egui::TextEdit::singleline(&mut label).code_editor().desired_width(f32::INFINITY)).changed() {
                let before = self.snapshot();
                if let Some(x) = edit::line_mut(&mut self.a, &r.path) {
                    x.label = label;
                }
                self.remember(before);
            }
            if matches!(l.kind, Kind::State | Kind::Return) {
                let mut numbered = l.numbered;
                if ui.checkbox(&mut numbered, "Numbered").on_hover_text("Off: \\Statex, a line without a number").changed() {
                    self.apply(|a| {
                        if let Some(x) = edit::line_mut(a, &r.path) {
                            x.numbered = numbered;
                        }
                        Ok(Some((r.path.clone(), Part::Line)))
                    });
                }
            }
        }
        ui.horizontal(|ui| {
            if l.kind.is_block() && r.part != Part::Branch(0) && ui.button("Unwrap").on_hover_text("Keep its lines, drop the block").clicked() {
                self.apply(|a| edit::unwrap(a, &r.path).map(|p| Some((p, Part::Line))));
            }
            if ui.button("Delete").clicked() {
                let ctx = ui.ctx().clone();
                self.act(if l.kind.is_block() && r.part != Part::Line { Action::Delete } else { Action::DeleteAll }, &ctx);
            }
        });
    }

    fn settings(&mut self, ui: &mut Ui) {
        ui.strong("Algorithm");
        let mut a = self.a.clone();
        ui.label("Caption");
        ui.add(egui::TextEdit::singleline(&mut a.caption).desired_width(f32::INFINITY));
        ui.horizontal(|ui| {
            ui.label("Number");
            ui.add(egui::TextEdit::singleline(&mut a.number).desired_width(50.0));
            ui.label("Label");
            ui.add(egui::TextEdit::singleline(&mut a.label).desired_width(f32::INFINITY));
        });
        ui.horizontal(|ui| {
            ui.label("Frame");
            for (f, name) in [(Frame::Ruled, "Ruled"), (Frame::Boxed, "Boxed"), (Frame::None, "None")] {
                ui.radio_value(&mut a.frame, f, name);
            }
        });
        ui.horizontal(|ui| {
            ui.label("Numbers");
            let mut every = a.numbering;
            ui.radio_value(&mut every, 0, "None");
            ui.radio_value(&mut every, 1, "Every line");
            let mut nth = every > 1;
            if ui.radio(nth, "Every").clicked() {
                nth = true;
                every = every.max(5);
            }
            if nth {
                ui.add(egui::DragValue::new(&mut every).range(2..=100));
            }
            a.numbering = every;
        });
        ui.checkbox(&mut a.show_end, "Show “end” lines");
        let io = a.keyword("require") == r"\textbf{Input:}";
        let mut want = io;
        ui.checkbox(&mut want, "Require/Ensure as Input/Output");
        if want != io {
            a.set_keyword("require", want.then(|| r"\textbf{Input:}".to_string()));
            a.set_keyword("ensure", want.then(|| r"\textbf{Output:}".to_string()));
        }
        ui.horizontal(|ui| {
            let mut wrap = a.wrap.is_some();
            ui.checkbox(&mut wrap, "Wrap lines at");
            let mut w = a.wrap.unwrap_or(30.0);
            ui.add_enabled(wrap, egui::DragValue::new(&mut w).range(10.0..=200.0).suffix(" em"));
            a.wrap = wrap.then_some(w);
        });
        if a != self.a {
            let before = self.snapshot();
            self.a = a;
            self.remember(before);
        }
        ui.add_space(6.0);
        ui.weak("Click a line to select it, double-click (or Enter) to type in it. Drag a line to move it.");
    }

    // -----------------------------------------------------------------------------------------
    // The LaTeX pane and the footer.

    fn source_pane(&mut self, ui: &mut Ui, out: &mut Outcome) {
        egui::CollapsingHeader::new("LaTeX (algpseudocode)").id_salt("algo-source").default_open(false).show(ui, |ui| {
            let id = Id::unique("algo-source-text");
            let focused = ui.memory(|m| m.has_focus(id));
            let mut text = match (&self.source, focused) {
                (Some(s), true) => s.clone(),
                _ => algo::write(&self.a),
            };
            // A long algorithm's LaTeX scrolls here; the picture keeps the room.
            let resp = ScrollArea::vertical()
                .id_salt("algo-source-scroll")
                .max_height(170.0)
                .show(ui, |ui| ui.add(egui::TextEdit::multiline(&mut text).id(id).code_editor().desired_rows(7).desired_width(f32::INFINITY)))
                .inner;
            if resp.changed() {
                self.source = Some(text.clone());
                match algo::open(&text) {
                    Ok(o) => {
                        if o.algorithm != self.a {
                            let before = self.snapshot();
                            self.remember(before);
                            self.a = o.algorithm;
                            self.sel.clear();
                            self.editing = None;
                        }
                        self.notices = o.notices;
                        self.source_error = None;
                    }
                    Err(e) => self.source_error = Some(e),
                }
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
                    ui.ctx().copy_text(algo::write(&self.a));
                }
                if ui.button("Edit as LaTeX…").clicked() {
                    *out = Outcome::EditLatex;
                }
            });
        });
    }

    fn footer(&mut self, ui: &mut Ui, out: &mut Outcome) {
        let t = Tokens::get(ui.ctx());
        let red = Color32::from_rgb(0xC0, 0x30, 0x30);
        if let Some(s) = &self.status {
            ui.colored_label(red, s);
        }
        for n in &self.notices {
            ui.weak(n);
        }
        if self.confirm_discard {
            ui.horizontal(|ui| {
                ui.label("Discard the changes to this algorithm?");
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
            "Enter: a new line · Tab / Shift+Tab: into / out of a block · ↑↓: other lines · Esc: put the line back · Delete removes · {insert} inserts. \
             The LaTeX needs {}.",
            algo::preamble(&self.a).join(" ")
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
}

#[cfg(test)]
#[path = "algo_editor_tests.rs"]
mod tests;
