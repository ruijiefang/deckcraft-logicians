//! Placing measured statements: proof trees, grids and formulas, in points from the top left.

use deckcraft_model::text::{Align, AutoFit, Paragraph, TextBody};

use super::array::{Grid, GridKind};
use super::proof::{Proof, Rule, Stmt};
use super::runs::{self, Piece};
use super::scan::split_top;
use super::{Kind, Measure, Style};

/// The deepest proof tree typeset ([`super::typeset`] refuses deeper ones; each level is a
/// recursion step here, and deeper premises are left out if one gets through).
pub(crate) const MAX_DEPTH: usize = 160;
/// Space between premises (ebproof's `separation`), in ems.
const SEPARATION: f64 = 1.5;
/// Space between a statement's box and a rule, in ems (the boxes carry their line's leading).
const RULE_GAP: f64 = 0.06;
/// Space between a rule and its label, in ems.
const LABEL_GAP: f64 = 0.35;
/// Labels are set this much smaller than statements.
const LABEL_SCALE: f64 = 0.85;
/// Distance between the two lines of a double rule, in ems.
const DOUBLE_GAP: f64 = 0.12;
/// Height of the dots of an ellipsis step, in ems.
const ELLIPSIS: f64 = 1.3;
/// Space between proof trees set side by side, in ems.
const TREE_GAP: f64 = 2.5;

/// A laid-out piece, in points from the typeset's top left.
#[derive(Clone, Debug, PartialEq)]
pub enum Item {
    Text { x: f64, y: f64, w: f64, h: f64, body: TextBody },
    Line { x1: f64, y1: f64, x2: f64, y2: f64, dash: Option<&'static str> },
}

/// Typeset LaTeX: its pieces, its size and the thickness its lines are drawn with.
#[derive(Clone, Debug, PartialEq)]
pub struct Typeset {
    pub kind: Kind,
    pub w: f64,
    pub h: f64,
    pub items: Vec<Item>,
    pub thickness: f64,
}

/// A text body that is exactly as big as its text: no insets, no wrapping, no autofit.
fn body(paragraphs: Vec<Paragraph>) -> TextBody {
    let mut b = TextBody { paragraphs, ..Default::default() };
    b.body.inset_l = Some(0.0);
    b.body.inset_r = Some(0.0);
    b.body.inset_t = Some(0.0);
    b.body.inset_b = Some(0.0);
    b.body.wrap = Some(false);
    b.body.autofit = Some(AutoFit::None);
    b
}

/// A measured text body.
struct Boxed {
    body: TextBody,
    w: f64,
    h: f64,
}

fn boxed(lines: Vec<Vec<Piece>>, style: &Style, measure: Measure, align: Align) -> Boxed {
    let b = body(lines.into_iter().map(|p| runs::paragraph(p, style, align)).collect());
    let (w, h) = measure(&b);
    Boxed { body: b, w: finite(w), h: finite(h) }
}

fn finite(v: f64) -> f64 {
    if v.is_finite() { v.max(0.0) } else { 0.0 }
}

fn pieces(s: &Stmt) -> Vec<Piece> {
    if s.math { runs::math_pieces(&s.src) } else { runs::text_pieces(&s.src) }
}

fn stmt_box(lines: &[Stmt], style: &Style, measure: Measure, align: Align) -> Boxed {
    boxed(lines.iter().map(pieces).collect(), style, measure, align)
}

fn text(x: f64, y: f64, b: Boxed) -> Item {
    Item::Text { x, y, w: b.w, h: b.h, body: b.body }
}

fn hline(x1: f64, x2: f64, y: f64, dash: Option<&'static str>) -> Item {
    Item::Line { x1, y1: y, x2, y2: y, dash }
}

/// The items' bounds: (min x, min y, max x, max y).
fn bounds(items: &[Item]) -> (f64, f64, f64, f64) {
    let mut b = (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
    let mut add = |x0: f64, y0: f64, x1: f64, y1: f64| {
        b = (b.0.min(x0.min(x1)), b.1.min(y0.min(y1)), b.2.max(x0.max(x1)), b.3.max(y0.max(y1)));
    };
    for it in items {
        match it {
            Item::Text { x, y, w, h, .. } => add(*x, *y, x + w, y + h),
            Item::Line { x1, y1, x2, y2, .. } => add(*x1, *y1, *x2, *y2),
        }
    }
    if b.0.is_finite() { b } else { (0.0, 0.0, 0.0, 0.0) }
}

fn shift(items: &mut [Item], dx: f64, dy: f64) {
    for it in items {
        match it {
            Item::Text { x, y, .. } => {
                *x += dx;
                *y += dy;
            }
            Item::Line { x1, y1, x2, y2, .. } => {
                *x1 += dx;
                *x2 += dx;
                *y1 += dy;
                *y2 += dy;
            }
        }
    }
}

fn thickness(style: &Style) -> f64 {
    (style.size * 0.04).max(0.75)
}

/// A laid-out subtree: its items from its own top left, its width, and where its conclusion is
/// (left and right edges, bottom), which is what the step below lines up with.
struct Laid {
    items: Vec<Item>,
    w: f64,
    cl: f64,
    cr: f64,
    cb: f64,
}

/// Proof trees side by side, their conclusions on one line.
pub(crate) fn proofs(trees: &[Proof], style: &Style, measure: Measure) -> Typeset {
    let laid: Vec<Laid> = trees.iter().map(|t| lay(t, style, measure, 0)).collect();
    let base = laid.iter().map(|l| l.cb).fold(0.0, f64::max);
    let mut items = Vec::new();
    let mut x = 0.0;
    for mut l in laid {
        shift(&mut l.items, x, base - l.cb);
        items.append(&mut l.items);
        x += l.w + TREE_GAP * style.size;
    }
    finish(Kind::Proof, items, style)
}

/// Moves the items to start at the origin.
fn finish(kind: Kind, mut items: Vec<Item>, style: &Style) -> Typeset {
    let (x0, y0, x1, y1) = bounds(&items);
    shift(&mut items, -x0, -y0);
    Typeset { kind, w: x1 - x0, h: y1 - y0, items, thickness: thickness(style) }
}

fn lay(p: &Proof, style: &Style, measure: Measure, depth: usize) -> Laid {
    let em = style.size;
    let c = stmt_box(&p.concl, style, measure, Align::Center);
    if p.rule == Rule::Leaf && p.premises.is_empty() {
        return Laid { w: c.w, cl: 0.0, cr: c.w, cb: c.h, items: vec![text(0.0, 0.0, c)] };
    }
    let subs: Vec<Laid> = if depth >= MAX_DEPTH { Vec::new() } else { p.premises.iter().map(|q| lay(q, style, measure, depth + 1)).collect() };
    // Premises side by side, their conclusions' bottoms on one line.
    let base = subs.iter().map(|s| s.cb).fold(0.0, f64::max);
    let mut items = Vec::new();
    let mut x = 0.0;
    let mut span: Option<(f64, f64)> = None;
    for mut s in subs {
        let (l, r) = (x + s.cl, x + s.cr);
        span = Some(span.map_or((l, r), |(a, _)| (a, r)));
        shift(&mut s.items, x, base - s.cb);
        items.append(&mut s.items);
        x += s.w + SEPARATION * em;
    }
    // The rule covers the premises' conclusions and this conclusion, which is centred under them.
    let (center, span_w) = span.map_or((c.w / 2.0, 0.0), |(a, b)| ((a + b) / 2.0, b - a));
    let rule_w = span_w.max(c.w);
    let (rl, rr) = (center - rule_w / 2.0, center + rule_w / 2.0);
    let gap = RULE_GAP * em;
    let y = base + gap;
    let (mid, cy) = match p.rule {
        Rule::Leaf | Rule::None => (y, y),
        Rule::Ellipsis => {
            let end = y + ELLIPSIS * em;
            items.push(Item::Line { x1: center, y1: y + gap, x2: center, y2: end - gap, dash: Some("sysDot") });
            ((y + end) / 2.0, end)
        }
        Rule::Double => {
            let d = DOUBLE_GAP * em;
            items.push(hline(rl, rr, y, None));
            items.push(hline(rl, rr, y + d, None));
            (y + d / 2.0, y + d + gap)
        }
        Rule::Single | Rule::Dashed | Rule::Dotted => {
            let dash = match p.rule {
                Rule::Dashed => Some("dash"),
                Rule::Dotted => Some("sysDot"),
                _ => None,
            };
            items.push(hline(rl, rr, y, dash));
            (y, y + gap)
        }
    };
    let cx = center - c.w / 2.0;
    let (cw, ch) = (c.w, c.h);
    items.push(text(cx, cy, c));
    let small = Style { size: style.size * LABEL_SCALE, ..style.clone() };
    let (left_edge, right_edge) = if p.rule == Rule::Ellipsis { (center, center) } else { (rl, rr) };
    if let Some(r) = &p.right {
        let b = stmt_box(std::slice::from_ref(r), &small, measure, Align::Left);
        let by = mid - b.h / 2.0;
        items.push(text(right_edge + LABEL_GAP * em, by, b));
    }
    if let Some(l) = &p.left {
        let b = stmt_box(std::slice::from_ref(l), &small, measure, Align::Left);
        let (bx, by) = (left_edge - LABEL_GAP * em - b.w, mid - b.h / 2.0);
        items.push(text(bx, by, b));
    }
    let (x0, y0, x1, _) = bounds(&items);
    shift(&mut items, -x0, -y0);
    Laid { items, w: x1 - x0, cl: cx - x0, cr: cx + cw - x0, cb: cy + ch - y0 }
}

/// A grid: cells in columns, with the spec's rules.
pub(crate) fn grid(g: &Grid, style: &Style, measure: Measure) -> Typeset {
    let em = style.size;
    let ncols = g.rows.iter().map(|r| r.cells.iter().map(|c| c.span).sum::<usize>()).max().unwrap_or(0).max(g.cols.len()).max(1);
    let col_align = |j: usize| g.cols.get(j).copied().unwrap_or(Align::Center);
    // Measured cells: (row, first column, span, box).
    let mut cells: Vec<(usize, usize, usize, Align, Boxed)> = Vec::new();
    for (r, row) in g.rows.iter().enumerate() {
        let mut j = 0;
        for cell in &row.cells {
            if j >= ncols {
                break;
            }
            let span = cell.span.min(ncols - j).max(1);
            let p = match g.kind {
                GridKind::Tabular => runs::text_pieces(&cell.src),
                GridKind::Align if !j.is_multiple_of(2) => runs::math_pieces_lead(&cell.src),
                _ => runs::math_pieces(&cell.src),
            };
            let align = cell.align.unwrap_or_else(|| col_align(j));
            cells.push((r, j, span, align, boxed(vec![p], style, measure, Align::Left)));
            j += span;
        }
    }
    let empty = boxed(vec![Vec::new()], style, measure, Align::Left);
    // Column padding on each side, by kind.
    let pad = |j: usize| -> (f64, f64) {
        match g.kind {
            GridKind::Array | GridKind::Tabular => (0.4 * em, 0.4 * em),
            GridKind::Align if j > 0 && j.is_multiple_of(2) => (1.5 * em, 0.0),
            GridKind::Align | GridKind::Gather => (0.0, 0.0),
        }
    };
    let mut widths = vec![0.0f64; ncols];
    for (_, j, span, _, b) in &cells {
        if *span == 1
            && let Some(w) = widths.get_mut(*j)
        {
            *w = w.max(b.w);
        }
    }
    // A spanning cell wider than its columns widens the last of them.
    for (_, j, span, _, b) in &cells {
        if *span > 1 {
            let inner: f64 =
                (*j..j + span).map(|k| widths.get(k).copied().unwrap_or(0.0) + pad(k).0 + pad(k).1).sum::<f64>() - pad(*j).0 - pad(j + span - 1).1;
            if b.w > inner
                && let Some(w) = widths.get_mut(j + span - 1)
            {
                *w += b.w - inner;
            }
        }
    }
    let mut edges = Vec::with_capacity(ncols + 1);
    let mut x = 0.0;
    edges.push(x);
    for j in 0..ncols {
        let (l, r) = pad(j);
        x += l + widths.get(j).copied().unwrap_or(0.0) + r;
        edges.push(x);
    }
    let nrows = g.rows.len();
    let pad_y = if g.vrules.is_empty() && g.rows.iter().all(|r| r.rules_above.is_empty()) { 0.0 } else { 0.12 * em };
    let mut heights = vec![empty.h; nrows];
    for (r, _, _, _, b) in &cells {
        if let Some(h) = heights.get_mut(*r) {
            *h = h.max(b.h);
        }
    }
    let mut tops = Vec::with_capacity(nrows + 1);
    let mut y = 0.0;
    tops.push(y);
    for r in 0..nrows {
        y += heights.get(r).copied().unwrap_or(0.0) + 2.0 * pad_y;
        tops.push(y);
    }
    let edge = |k: usize| edges.get(k.min(ncols)).copied().unwrap_or(0.0);
    let top = |r: usize| tops.get(r.min(nrows)).copied().unwrap_or(0.0);
    let total_w = edge(ncols);
    let total_h = top(nrows);
    let mut items = Vec::new();
    for (r, j, span, align, b) in cells {
        let (x0, x1) = (edge(j) + pad(j).0, edge(j + span) - pad(j + span - 1).1);
        let x = match align {
            Align::Right => x1 - b.w,
            Align::Center => (x0 + x1 - b.w) / 2.0,
            _ => x0,
        };
        items.push(text(x, top(r) + pad_y, b));
    }
    for &k in &g.vrules {
        let x = edge(k);
        items.push(Item::Line { x1: x, y1: 0.0, x2: x, y2: total_h, dash: None });
    }
    let rule = |(a, b): (usize, usize), y: f64| hline(edge(a), edge(b + 1), y, None);
    for (r, row) in g.rows.iter().enumerate() {
        items.extend(row.rules_above.iter().map(|&ab| rule(ab, top(r))));
    }
    items.extend(g.rules_below.iter().map(|&ab| rule(ab, total_h)));
    // Rules may sit on the edges: the typeset's box is the grid's.
    Typeset { kind: Kind::Grid, w: total_w, h: total_h, items, thickness: thickness(style) }
}

/// A formula on one line (or several, split at `\\`, centred).
pub(crate) fn formula(src: &str, style: &Style, measure: Measure) -> Typeset {
    let lines: Vec<Vec<Piece>> = split_top(src, &["\\\\"]).iter().filter(|l| !l.trim().is_empty()).map(|l| runs::math_pieces(l)).collect();
    let b = boxed(if lines.is_empty() { vec![Vec::new()] } else { lines }, style, measure, Align::Center);
    let (w, h) = (b.w, b.h);
    Typeset { kind: Kind::Formula, w, h, items: vec![text(0.0, 0.0, b)], thickness: thickness(style) }
}
