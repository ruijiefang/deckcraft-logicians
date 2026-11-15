//! Setting pseudo-code as algorithmicx does, measured from its output at 10 pt (and scaled to the
//! style's size): the `algorithm` float's rules (0.8 pt on top, 0.4 pt under the caption and at
//! the bottom), the caption "Algorithm n" in bold, line numbers "n:" in 8 pt right-aligned at
//! 1.2em, lines from 1.7em, 1.5em of indentation per level, 1.1955em from line to line, keywords
//! in bold, procedure names in small caps, and comments "▷ …" flush right.

use deckcraft_model::text::Align;
use deckcraft_model::{ColorRef, Fill};

use deckcraft_geom::{BezPath, Point};

use super::super::layout::{Boxed, Item, Paint, Part as RegionPart, Region, Stroke, Typeset, boxed, text};
use super::super::runs::{self, Piece};
use super::super::{Kind as TypesetKind, Measure, Style};
use super::{Algorithm, Frame, Kind, Line, Part, Row, rows};

const GUTTER: f64 = 1.2;
const TEXT: f64 = 1.7;
const INDENT: f64 = 1.5;
const PITCH: f64 = 1.1955;
const NUMBER: f64 = 0.8;
const TOP_RULE: f64 = 0.08;
const RULE: f64 = 0.04;
const CAPTION_BASE: f64 = 0.931;
const CAPTION_RULE: f64 = 0.413;
const FIRST_BASE: f64 = 0.966;
const LAST_RULE: f64 = 0.612;
/// Space between a line and its comment, and after "▷".
const COMMENT_GAP: f64 = 1.0;
/// "▷" is set this much smaller than the text, this far before the comment.
const MARK: f64 = 0.62;
const MARK_GAP: f64 = 0.33;
/// Where a wrapped line's next lines start, beyond the line's own indentation.
const HANG: f64 = 1.0;
/// The editor's faint rows (hidden `end` lines).
const GHOST: [u8; 3] = [170, 170, 170];

/// The macros algpseudocode adds, as DeckCraft's text setting knows them: `\Call{F}{x}` as
/// F(x) in small caps, `\Return`, `\textproc`, `\bmod`/`\mod`/`\pmod` (upright "mod"), `\not=`.
pub(crate) fn prepare(src: &str) -> String {
    let mut out = String::with_capacity(src.len() + 16);
    let mut math = false;
    let mut i = 0;
    while let Some(ch) = src.get(i..).and_then(|r| r.chars().next()) {
        match ch {
            '$' => {
                math = !math;
                out.push('$');
                i += 1;
            }
            '\\' => {
                let rest = src.get(i + 1..).unwrap_or("");
                let name: String = rest.chars().take_while(|c| c.is_ascii_alphabetic()).collect();
                let after = i + 1 + name.len();
                let group = |at: usize| -> Option<(String, usize)> {
                    let k = at + src.get(at..)?.len() - src.get(at..)?.trim_start().len();
                    if src.as_bytes().get(k) != Some(&b'{') {
                        return None;
                    }
                    let mut depth = 0usize;
                    for (off, c) in src.get(k..)?.char_indices() {
                        match c {
                            '{' => depth += 1,
                            '}' => {
                                depth -= 1;
                                if depth == 0 {
                                    return Some((src.get(k + 1..k + off)?.to_string(), k + off + 1));
                                }
                            }
                            _ => {}
                        }
                    }
                    None
                };
                match name.as_str() {
                    "Call" => {
                        if let Some((f, j)) = group(after)
                            && let Some((args, k)) = group(j)
                        {
                            out.push_str(&format!("\\textsc{{{f}}}({args})"));
                            i = k;
                            continue;
                        }
                    }
                    "textproc" => {
                        if let Some((f, j)) = group(after) {
                            out.push_str(&format!("\\textsc{{{f}}}"));
                            i = j;
                            continue;
                        }
                    }
                    "Return" => {
                        out.push_str("\\textbf{return}");
                        i = after;
                        continue;
                    }
                    "bmod" | "mod" => {
                        out.push_str(if math { "$ mod $" } else { " mod " });
                        i = after;
                        continue;
                    }
                    "pmod" => {
                        if let Some((m, j)) = group(after) {
                            out.push_str(&if math { format!("$ (mod ${m}$)$") } else { format!(" (mod {m})") });
                            i = j;
                            continue;
                        }
                    }
                    "not" if rest.get(name.len()..).is_some_and(|r| r.trim_start().starts_with('=')) => {
                        let eq = rest.get(name.len()..).map_or(0, |r| r.len() - r.trim_start().len()) + 1;
                        out.push_str("\\neq ");
                        i = after + eq;
                        continue;
                    }
                    _ => {}
                }
                // Another command, or an escaped character: kept as it is.
                let end = if name.is_empty() { i + 1 + rest.chars().next().map_or(0, char::len_utf8) } else { after };
                out.push_str(src.get(i..end).unwrap_or(""));
                i = end.max(i + 1);
            }
            c => {
                out.push(c);
                i += c.len_utf8();
            }
        }
    }
    out
}

fn kw(a: &Algorithm, k: &str) -> String {
    a.keyword(k)
}

fn join(parts: &[String]) -> String {
    parts.iter().filter(|p| !p.trim().is_empty()).cloned().collect::<Vec<_>>().join(" ")
}

fn line_at<'a>(a: &'a Algorithm, path: &[(usize, usize)]) -> Option<&'a Line> {
    super::edit::line(a, path)
}

/// A row's LaTeX (its keywords and its text), or `None` for a raw line (shown as written).
pub(crate) fn row_text(a: &Algorithm, r: &Row) -> Option<String> {
    let l = line_at(a, &r.path)?;
    let t = l.text.trim().to_string();
    Some(match r.part {
        Part::Line => match l.kind {
            Kind::State => t,
            Kind::Return => join(&[kw(a, "return"), t]),
            Kind::Require => join(&[kw(a, "require"), t]),
            Kind::Ensure => join(&[kw(a, "ensure"), t]),
            Kind::Raw => return None,
            Kind::If => join(&[kw(a, "if"), t, kw(a, "then")]),
            Kind::For => join(&[kw(a, "for"), t, kw(a, "do")]),
            Kind::ForAll => join(&[kw(a, "forall"), t, kw(a, "do")]),
            Kind::While => join(&[kw(a, "while"), t, kw(a, "do")]),
            Kind::Repeat => kw(a, "repeat"),
            Kind::Loop => kw(a, "loop"),
            Kind::Procedure | Kind::Function => {
                let k = if l.kind == Kind::Procedure { "procedure" } else { "function" };
                format!("{} \\textsc{{{}}}({})", kw(a, k), t, l.args.trim())
            }
        },
        Part::Branch(k) => match l.branches.get(k).and_then(|b| b.cond.clone()) {
            Some(c) => join(&[kw(a, "else"), kw(a, "if"), c.trim().to_string(), kw(a, "then")]),
            None => kw(a, "else"),
        },
        Part::End => match l.kind {
            Kind::Repeat => join(&[kw(a, "until"), t]),
            Kind::If => join(&[kw(a, "end"), kw(a, "if")]),
            Kind::For | Kind::ForAll => join(&[kw(a, "end"), kw(a, "for")]),
            Kind::While => join(&[kw(a, "end"), kw(a, "while")]),
            Kind::Loop => join(&[kw(a, "end"), kw(a, "loop")]),
            Kind::Procedure => join(&[kw(a, "end"), kw(a, "procedure")]),
            Kind::Function => join(&[kw(a, "end"), kw(a, "function")]),
            _ => String::new(),
        },
    })
}

fn row_comment(a: &Algorithm, r: &Row) -> String {
    let Some(l) = line_at(a, &r.path) else { return String::new() };
    match r.part {
        Part::Line => l.comment.trim().to_string(),
        Part::Branch(k) => l.branches.get(k).map(|b| b.comment.trim().to_string()).unwrap_or_default(),
        Part::End => String::new(),
    }
}

fn colored(mut b: Boxed, c: Option<[u8; 3]>) -> Boxed {
    if let Some(c) = c {
        for p in &mut b.body.paragraphs {
            for r in &mut p.runs {
                r.props.fill = Some(Fill::solid(ColorRef::rgb(deckcraft_color::Rgba::rgb(c[0], c[1], c[2]))));
            }
        }
    }
    b
}

fn set(pieces: Vec<Piece>, style: &Style, measure: Measure) -> Boxed {
    boxed(vec![pieces], style, measure, Align::Left)
}

/// `src` as pieces: LaTeX, or (raw) the characters as written.
fn pieces(src: &str, raw: bool) -> Vec<Piece> {
    if raw { vec![Piece { text: src.to_string(), look: Default::default() }] } else { runs::text_pieces(&prepare(src)) }
}

/// Breaks `src` at spaces outside braces and math so each part fits `max` wide.
fn wrap(src: &str, max: f64, style: &Style, measure: Measure) -> Vec<String> {
    let mut words: Vec<String> = Vec::new();
    let (mut depth, mut math, mut cur) = (0usize, false, String::new());
    for c in src.chars() {
        match c {
            '{' => depth += 1,
            '}' => depth = depth.saturating_sub(1),
            '$' => math = !math,
            ' ' if depth == 0 && !math => {
                if !cur.is_empty() {
                    words.push(std::mem::take(&mut cur));
                }
                continue;
            }
            _ => {}
        }
        cur.push(c);
    }
    if !cur.is_empty() {
        words.push(cur);
    }
    let mut lines: Vec<String> = Vec::new();
    let mut line = String::new();
    for w in words.into_iter().take(400) {
        let candidate = if line.is_empty() { w.clone() } else { format!("{line} {w}") };
        if !line.is_empty() && set(pieces(&candidate, false), style, measure).w > max {
            lines.push(std::mem::replace(&mut line, w));
        } else {
            line = candidate;
        }
    }
    lines.push(line);
    lines
}

/// One row measured: its text (one box per wrapped line), number and comment.
struct Set {
    row: Row,
    lines: Vec<Boxed>,
    /// "▷" (set smaller, as TeX's `\triangleright` is) and the comment's text.
    comment: Option<(Boxed, Boxed)>,
    number: Option<Boxed>,
}

/// Lays out `a`. In the editor (`editor`), hidden `end` rows are drawn faintly so they can be
/// clicked; regions name each row by its index in [`rows`]`(a, editor)`.
pub(crate) fn layout(a: &Algorithm, style: &Style, measure: Measure, editor: bool) -> Typeset {
    let em = style.size;
    let numbered = a.numbering > 0;
    let small = Style { size: style.size * NUMBER, ..style.clone() };
    let mark = Style { size: style.size * MARK, ..style.clone() };
    let left = if numbered { TEXT * em } else { 0.0 };
    let all = rows(a, editor);
    let mut sets: Vec<Set> = Vec::new();
    for r in all {
        let raw = line_at(a, &r.path).is_some_and(|l| l.kind == Kind::Raw && r.part == Part::Line);
        // Kept LaTeX (definitions such as `\algrenewcommand`) prints nothing in the paper: it is
        // shown, gray, only in the editor.
        if raw && !editor {
            continue;
        }
        let src = if raw { line_at(a, &r.path).map(|l| l.text.trim().to_string()).unwrap_or_default() } else { row_text(a, &r).unwrap_or_default() };
        let ghost = (r.ghost || raw).then_some(GHOST);
        let indent = INDENT * em * r.depth as f64;
        let parts = match a.wrap {
            Some(w) if !raw && w.is_finite() => {
                let max = (w * em - left - indent).max(4.0 * em);
                let first = wrap(&src, max, style, measure);
                // Lines after the first hang further in, and so have less room.
                let mut out = vec![first.first().cloned().unwrap_or_default()];
                let rest = first.get(1..).unwrap_or(&[]).join(" ");
                if !rest.is_empty() {
                    out.extend(wrap(&rest, (max - HANG * em).max(4.0 * em), style, measure));
                }
                out
            }
            _ => vec![src],
        };
        let lines = parts.iter().map(|p| colored(set(pieces(p, raw), style, measure), ghost)).collect();
        let c = row_comment(a, &r);
        let comment = (!c.is_empty()).then(|| {
            let tri = colored(set(vec![Piece { text: "▷".into(), look: Default::default() }], &mark, measure), ghost);
            (tri, colored(set(pieces(&c, false), style, measure), ghost))
        });
        let number = match (numbered, r.number) {
            (true, Some(n)) if n % a.numbering.max(1) as usize == 0 => {
                Some(colored(set(vec![Piece { text: format!("{n}:"), look: Default::default() }], &small, measure), ghost))
            }
            _ => None,
        };
        sets.push(Set { row: r, lines, comment, number });
    }
    let caption = (a.frame != Frame::None).then(|| {
        let src = format!("\\textbf{{Algorithm {}}} {}", a.number.trim(), a.caption.trim());
        set(pieces(src.trim(), false), style, measure)
    });
    // The frame is as wide as its widest line with its comment.
    let mut width = caption.as_ref().map_or(0.0, |c| c.w);
    for s in &sets {
        let indent = INDENT * em * s.row.depth as f64;
        for (k, b) in s.lines.iter().enumerate() {
            let hang = if k > 0 { HANG * em } else { 0.0 };
            let c = if k == 0 { s.comment.as_ref().map_or(0.0, |(t, c)| COMMENT_GAP * em + t.w + MARK_GAP * em + c.w) } else { 0.0 };
            width = width.max(left + indent + hang + b.w + c);
        }
    }
    width = width.max(8.0 * em);
    let mut items: Vec<Item> = Vec::new();
    let mut regions: Vec<Region> = Vec::new();
    let line = |x1: f64, y1: f64, x2: f64, y2: f64, t: f64| {
        let mut p = BezPath::new();
        p.move_to(Point::new(x1, y1));
        p.line_to(Point::new(x2, y2));
        Item::Path { path: p, stroke: Some(Stroke { width: t, paint: Paint::Text, dash: None, round: false }), fill: None }
    };
    let rule = |items: &mut Vec<Item>, y: f64, t: f64| items.push(line(0.0, y, width, y, t));
    let mut y: f64 = 0.0;
    let mut first_base = 0.75 * em;
    if a.frame == Frame::Ruled {
        rule(&mut items, 0.0, TOP_RULE * em);
        if let Some(c) = caption.clone() {
            let base = CAPTION_BASE * em;
            items.push(text(0.0, base - c.base, c));
            let r = base + CAPTION_RULE * em;
            rule(&mut items, r, RULE * em);
            first_base = r + FIRST_BASE * em;
        }
    } else if a.frame == Frame::Boxed {
        first_base = (0.3 + 0.75) * em;
    }
    y = y.max(first_base);
    let mut last = first_base;
    for (i, s) in sets.into_iter().enumerate() {
        let indent = INDENT * em * s.row.depth as f64;
        let top = y - 0.86 * em;
        if let Some(n) = s.number {
            items.push(text(left - (TEXT - GUTTER) * em - n.w, y - n.base, n));
        }
        let mut base = y;
        let count = s.lines.len();
        for (k, b) in s.lines.into_iter().enumerate() {
            let hang = if k > 0 { HANG * em } else { 0.0 };
            items.push(text(left + indent + hang, base - b.base, b));
            if k + 1 < count {
                base += PITCH * em;
            }
        }
        if let Some((t, c)) = s.comment {
            let cx = width - c.w;
            // The mark sits on the math axis, centred where the text's x-height is.
            let tx = cx - MARK_GAP * em - t.w;
            let axis = y - 0.25 * em;
            items.push(text(tx, axis + 0.3 * MARK * em - t.base, t));
            items.push(text(cx, y - c.base, c));
        }
        let bottom = base - 0.86 * em + PITCH * em;
        regions.push(Region { path: vec![i], part: RegionPart::Row, x: 0.0, y: top, w: width, h: bottom - top, poly: Vec::new() });
        last = base;
        y = base + PITCH * em;
    }
    let bottom = match a.frame {
        Frame::Ruled => {
            let b = last + LAST_RULE * em;
            rule(&mut items, b, RULE * em);
            b
        }
        Frame::Boxed => {
            let b = last + (0.3 + 0.36) * em;
            for (x1, y1, x2, y2) in [(0.0, 0.0, width, 0.0), (0.0, b, width, b), (0.0, 0.0, 0.0, b), (width, 0.0, width, b)] {
                items.push(line(x1, y1, x2, y2, RULE * em));
            }
            match caption {
                Some(c) => {
                    let base = b + 1.25 * em;
                    let h = base + 0.4 * em;
                    items.push(text(0.0, base - c.base, c));
                    h
                }
                None => b,
            }
        }
        Frame::None => last + 0.36 * em,
    };
    let h = bottom.max(first_base);
    Typeset { kind: TypesetKind::Algorithm, w: width, h, items, thickness: RULE * em, regions, grid: None }
}
