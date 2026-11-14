//! Placing a diagram as tikz-cd does, from its manual: rows and columns as wide as their widest
//! cell, cells padded by 1ex × 0.85ex, rows 1.8em and columns 2.4em apart (by default), arrows
//! from border to border aimed at the math axis, labels in script size 0.5ex from their arrow on
//! its left. The diagram is a 10 pt TeX diagram scaled to the style's size.

use deckcraft_geom::arrow::{self, Tip};
use deckcraft_geom::{BezPath, CubicBez, KurboShape as _, Point, Rect, Vec2};
use deckcraft_model::text::Align;
use deckcraft_model::{ColorRef, Fill};

use super::super::layout::{Boxed, Item, Paint, Part, Region, Stroke, Typeset, bounds, boxed, shift, shift_regions, text, thickness};
use super::super::{Kind, Measure, Style, runs};
use super::{Arrow, At, Body, Diagram, End, Frame, Head, Label, MAX_LEVEL, Rgb, Side, Tail};

/// tikz-cd's cell padding: 1ex across, 0.85ex up and down (ex = 0.4306 em).
const XSEP: f64 = 0.4306;
const YSEP: f64 = 0.366;
/// The math axis, where arrows aim, above the baseline.
const AXIS: f64 = 0.25;
/// A label's distance from its arrow (0.5ex).
const INNER: f64 = 0.215;
/// Labels are set in script size.
const SCRIPT: f64 = 0.7;
/// Above the baseline and below it, the ink of ordinary math (a capital's height, a little
/// depth), as fractions of its size.
const INK_UP: f64 = 0.683;
const INK_DOWN: f64 = 0.05;
/// The math font's rule thickness, which arrows are drawn with.
const RULE: f64 = 0.0398;
/// One step of `shift left` (0.56ex).
const SHIFT: f64 = 0.2411;
/// Each line of a double arrow, either side of the middle.
const DOUBLE: f64 = 0.0965;
/// The background under a crossing arrow (1.5ex wide).
const CLEARANCE: f64 = 0.6458;
/// Squiggles: straight lead at each end, tooth length, amplitude.
const SQUIGGLE: (f64, f64, f64) = (0.24, 0.184, 0.0757);
/// Loops leave 15° either side of their direction and reach at least 5mm out.
const LOOP_SPREAD: f64 = 15.0;
const LOOP_MIN: f64 = 1.4226;
const LOOP_LOOSENESS: f64 = 8.0;
/// Dashes and dots, in line widths: on, off.
const DASHED: (f64, f64) = (7.0, 4.0);
const DOTTED: (f64, f64) = (1.0, 5.0);

/// A diagram's rows (top, bottom) and columns (left, right), in points from the typeset's top
/// left, as drawn (empty rows and columns are thin).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Grid {
    pub rows: Vec<(f64, f64)>,
    pub cols: Vec<(f64, f64)>,
}

/// Where an arrow can start or end: a cell's box (its centre at the math axis), or a point on
/// another arrow.
#[derive(Clone, Copy)]
struct Node {
    rect: Rect,
    centre: Point,
    circle: Option<(Point, f64)>,
}

impl Node {
    fn exit(&self, dir: Vec2) -> Point {
        match self.circle {
            Some((c, r)) => arrow::exit_ellipse(c, r, r, dir),
            None => arrow::exit_rect(self.rect, self.centre, dir),
        }
    }
}

fn paint(c: Option<Rgb>) -> Paint {
    c.map_or(Paint::Text, Paint::Rgb)
}

/// Text set in `color`.
fn colored(mut b: Boxed, color: Option<Rgb>) -> Boxed {
    if let Some(c) = color {
        for p in &mut b.body.paragraphs {
            for r in &mut p.runs {
                r.props.fill = Some(Fill::solid(ColorRef::rgb(deckcraft_color::Rgba::rgb(c[0], c[1], c[2]))));
            }
        }
    }
    b
}

/// How far a measured line's ink reaches above and below its baseline: ordinary math's, plus
/// whatever a raised or lowered script adds to the line's own ascent or descent.
fn ink(b: &Boxed, reference: &Boxed, size: f64) -> (f64, f64) {
    let up = INK_UP * size + (b.base - reference.base).max(0.0);
    let down = INK_DOWN * size + ((b.h - b.base) - (reference.h - reference.base)).max(0.0);
    (up, down)
}

fn math(src: &str, style: &Style, measure: Measure) -> Boxed {
    boxed(vec![runs::math_pieces(src)], style, measure, Align::Left)
}

/// The eight directions TikZ anchors a label by, as signs of x and y.
fn snap(n: Vec2) -> (f64, f64) {
    let a = n.y.atan2(n.x);
    if !a.is_finite() {
        return (0.0, -1.0);
    }
    let k = (a / std::f64::consts::FRAC_PI_4).round();
    let snapped = k * std::f64::consts::FRAC_PI_4;
    let (s, c) = snapped.sin_cos();
    (c.round(), s.round())
}

fn stroked(path: BezPath, width: f64, paint: Paint, dash: Option<(f64, f64)>, round: bool) -> Item {
    Item::Path { path, stroke: Some(Stroke { width, paint, dash, round }), fill: None }
}

/// Where an arrow's ends land and what it draws.
struct Laid {
    /// The middle of the arrow (where 2-cells reach it).
    middle: Point,
    items: Vec<Item>,
    regions: Vec<Region>,
}

/// The arrow `a` (index `i`) between `from` and `to`.
#[allow(clippy::too_many_arguments)]
fn lay_arrow(a: &Arrow, i: usize, from: Node, to: Node, looped: bool, style: &Style, reference: &Boxed, measure: Measure) -> Laid {
    let em = style.size;
    let pt = em / 10.0;
    let x_axis = Vec2::new(1.0, 0.0);
    let base = if looped {
        let deg = a.looping.unwrap_or_default().degrees();
        let out = arrow::turn(x_axis, deg + LOOP_SPREAD);
        let back = arrow::turn(x_axis, deg - LOOP_SPREAD);
        let (p, q) = (from.exit(out), from.exit(back));
        let reach = (LOOP_MIN * em).max(LOOP_LOOSENESS * arrow::BEND_ARM * (q - p).hypot());
        CubicBez::new(p, p + out * reach, q + back * reach, q)
    } else {
        let dir = to.centre - from.centre;
        let dir = arrow::unit(dir).unwrap_or(x_axis);
        let bend = if a.bend.is_finite() { a.bend.clamp(-179.0, 179.0) } else { 0.0 };
        let out = arrow::turn(dir, bend);
        let back = arrow::turn(-dir, -bend);
        let (p, q) = (from.exit(out), to.exit(back));
        if bend.abs() < 1e-9 { arrow::line(p, q) } else { arrow::curve(p, out, q, back, arrow::BEND_ARM) }
    };
    let chord = arrow::unit(base.p3 - base.p0).unwrap_or(x_axis);
    let shift = if a.shift.is_finite() { a.shift.clamp(-100.0, 100.0) } else { 0.0 };
    let off = arrow::left_of(chord) * (shift * SHIFT * em);
    let shifted = CubicBez::new(base.p0 + off, base.p1 + off, base.p2 + off, base.p3 + off);
    let (middle, _) = arrow::at(&shifted, 0.5);
    let shorten = |v: f64| if v.is_finite() { (v * pt).clamp(0.0, 1000.0 * pt) } else { 0.0 };
    let curve = arrow::trim(&shifted, shorten(a.shorten.0), shorten(a.shorten.1));
    let mut items = Vec::new();
    let mut regions = Vec::new();
    let lw = a.width.filter(|w| w.is_finite() && *w > 0.0).map_or(RULE * em, |w| w.min(20.0) * pt);
    let ink = paint(a.color);
    let Some(c) = curve else { return Laid { middle, items, regions } };
    if a.crossing && a.body.drawn() {
        items.push(stroked(arrow::path_of(&c), CLEARANCE * em, Paint::Background, None, false));
    }
    if a.body.drawn() {
        let double = a.body == Body::Double;
        let (_, d0) = arrow::at(&c, 0.0);
        let (_, d1) = arrow::at(&c, 1.0);
        let head = match (a.head, double) {
            (Head::None, _) => None,
            (Head::Arrow, true) => Some(Tip::Implies),
            (Head::Arrow, false) => Some(Tip::Head),
            (Head::TwoHeads, _) => Some(Tip::TwoHeads),
            (Head::Harpoon, _) => Some(Tip::Harpoon(true)),
            (Head::HarpoonRight, _) => Some(Tip::Harpoon(false)),
        };
        let (tail, tail_dir) = match (a.tail, double) {
            (Tail::None, _) => (None, d0),
            (Tail::Head, true) => (Some(Tip::Implies), -d0),
            (Tail::Head, false) => (Some(Tip::Head), -d0),
            (Tail::Mono, _) => (Some(Tip::Mono), d0),
            (Tail::Hook, _) => (Some(Tip::Hook(true)), d0),
            (Tail::HookRight, _) => (Some(Tip::Hook(false)), d0),
            (Tail::Bar, true) => (Some(Tip::WideBar), d0),
            (Tail::Bar, false) => (Some(Tip::Bar), d0),
        };
        let head = head.map(|k| arrow::tip(k, c.p3, d1, em, lw));
        let tail = tail.map(|k| arrow::tip(k, c.p0, tail_dir, em, lw));
        let trim = |d: &Option<arrow::Drawn>| d.as_ref().map_or(0.0, |t| t.trim);
        if let Some(shaft) = arrow::trim(&c, trim(&tail), trim(&head)) {
            match a.body {
                Body::Double => {
                    let mut p = arrow::path_of(&arrow::offset(&shaft, DOUBLE * em));
                    p.extend(arrow::path_of(&arrow::offset(&shaft, -DOUBLE * em)));
                    items.push(stroked(p, lw, ink, None, false));
                }
                Body::Squiggly => {
                    let (lead, half, amp) = SQUIGGLE;
                    items.push(stroked(arrow::squiggle(&shaft, lead * em, half * em, amp * em), lw, ink, None, false));
                }
                Body::Dashed => items.push(stroked(arrow::path_of(&shaft), lw, ink, Some(DASHED), false)),
                Body::Dotted => items.push(stroked(arrow::path_of(&shaft), lw, ink, Some(DOTTED), false)),
                _ => items.push(stroked(arrow::path_of(&shaft), lw, ink, None, false)),
            }
        }
        let mut tips = BezPath::new();
        for d in [tail, head].into_iter().flatten() {
            tips.extend(d.path);
        }
        if !tips.elements().is_empty() {
            items.push(stroked(tips, lw, ink, None, true));
        }
    }
    regions.push(Region {
        path: vec![i],
        part: Part::Arrow,
        x: c.p0.x.min(c.p3.x),
        y: c.p0.y.min(c.p3.y),
        w: (c.p3.x - c.p0.x).abs(),
        h: (c.p3.y - c.p0.y).abs(),
        poly: arrow::samples(&c, 24).into_iter().map(|p| (p.x, p.y)).collect(),
    });
    for (k, l) in a.labels.iter().enumerate() {
        if l.text.trim().is_empty() {
            continue;
        }
        let (bx, item_bg, region) = lay_label(l, &c, a.body, style, reference, measure);
        if let Some(bg) = item_bg {
            items.push(bg);
        }
        let mut r = region;
        r.path = vec![i, k];
        regions.push(r);
        items.push(bx);
    }
    Laid { middle, items, regions }
}

/// A label on curve `c`: its text, a patch of background behind it (`description`) and where it is.
fn lay_label(l: &Label, c: &CubicBez, body: Body, style: &Style, reference: &Boxed, measure: Measure) -> (Item, Option<Item>, Region) {
    let em = style.size;
    // A phantom arrow's labels are full size; the others script size.
    let full = body == Body::Phantom;
    let size = if full { style.size } else { style.size * SCRIPT };
    let lstyle = Style { size, ..style.clone() };
    let small_ref = (!full).then(|| math("A", &lstyle, measure));
    let b = colored(math(&l.text, &lstyle, measure), l.color);
    let (up, down) = ink(&b, small_ref.as_ref().unwrap_or(reference), size);
    let inner = INNER * em;
    let (w, h) = (b.w + 2.0 * inner, up + down + 2.0 * inner);
    let pos = if l.pos.is_finite() { l.pos.clamp(0.0, 1.0) } else { 0.5 };
    let (p, t) = arrow::at(c, pos);
    let centred = full || matches!(l.side, Side::On | Side::Over);
    let centre = if centred {
        p
    } else {
        let n = if l.side == Side::Right { -arrow::left_of(t) } else { arrow::left_of(t) };
        let (sx, sy) = snap(n);
        p + Vec2::new(sx * w / 2.0, sy * h / 2.0)
    };
    let top = centre.y - h / 2.0;
    let baseline = top + inner + up;
    let (x, y) = (centre.x - b.w / 2.0, baseline - b.base);
    let bg = (l.side == Side::On && !full).then(|| {
        let r = Rect::new(centre.x - w / 2.0, top, centre.x + w / 2.0, top + h);
        Item::Path { path: r.to_path(0.1), stroke: None, fill: Some(Paint::Background) }
    });
    let region = Region { path: Vec::new(), part: Part::ArrowLabel, x: centre.x - w / 2.0, y: top, w, h, poly: Vec::new() };
    (text(x, y, b), bg, region)
}

/// Lays out `d`. `ring` empty rows and columns are added on every side (the editor's room to
/// grow); the regions and grid then count them too.
pub(crate) fn layout(d: &Diagram, style: &Style, measure: Measure, ring: usize) -> Typeset {
    let em = style.size;
    let (rows0, cols0) = d.size();
    let ring = ring.min(2);
    let (rows, cols) = (rows0.max(1) + 2 * ring, cols0.max(1) + 2 * ring);
    let reference = math("A", style, measure);
    let at = |o: At| At::new(o.row + ring, o.col + ring);
    // Each object: its box, ink above and below its baseline.
    struct Cell {
        b: Boxed,
        up: f64,
        down: f64,
        frame: Frame,
    }
    let mut cells: std::collections::BTreeMap<At, Cell> = std::collections::BTreeMap::new();
    for o in &d.objects {
        let b = colored(math(&o.label, style, measure), o.color);
        let (up, down) = if o.label.trim().is_empty() { (0.0, 0.0) } else { ink(&b, &reference, em) };
        cells.insert(at(o.at), Cell { up, down, frame: o.frame, b });
    }
    let (xsep, ysep) = (XSEP * em, YSEP * em);
    let node_w = |c: Option<&Cell>| c.map_or(0.0, |c| if c.b.w > 0.0 { c.b.w } else { 0.0 }) + 2.0 * xsep;
    let mut col_w = vec![2.0 * xsep; cols];
    let mut up = vec![ysep; rows];
    let mut down = vec![ysep; rows];
    for (a, c) in &cells {
        if let Some(w) = col_w.get_mut(a.col) {
            *w = w.max(node_w(Some(c)));
        }
        if let Some(u) = up.get_mut(a.row) {
            *u = u.max(c.up + ysep);
        }
        if let Some(v) = down.get_mut(a.row) {
            *v = v.max(c.down + ysep);
        }
    }
    let gap = |m: &std::collections::BTreeMap<usize, f64>, k: usize| m.get(&k).copied().filter(|g| g.is_finite()).unwrap_or(0.0);
    let col_sep = d.col_sep.col_em().clamp(0.0, 50.0);
    let row_sep = d.row_sep.row_em().clamp(0.0, 50.0);
    let mut cx = Vec::with_capacity(cols);
    let mut x = col_w.first().copied().unwrap_or(0.0) / 2.0;
    for j in 0..cols {
        if j > 0 {
            let extra = if j >= ring { gap(&d.col_gaps, j - ring) } else { 0.0 };
            x += col_w.get(j - 1).copied().unwrap_or(0.0) / 2.0 + (col_sep + extra) * em + col_w.get(j).copied().unwrap_or(0.0) / 2.0;
        }
        cx.push(x);
    }
    let mut base = Vec::with_capacity(rows);
    let mut y = up.first().copied().unwrap_or(0.0);
    for i in 0..rows {
        if i > 0 {
            let extra = if i > ring { gap(&d.row_gaps, i - 1 - ring) } else { 0.0 };
            y += down.get(i - 1).copied().unwrap_or(0.0) + (row_sep + extra) * em + up.get(i).copied().unwrap_or(0.0);
        }
        base.push(y);
    }
    let centre_x = |c: usize| cx.get(c).copied().unwrap_or(0.0);
    let baseline = |r: usize| base.get(r).copied().unwrap_or(0.0);
    let node = |a: At| -> Node {
        let c = cells.get(&a);
        let w = node_w(c);
        let (u, dn) = c.map_or((0.0, 0.0), |c| (c.up, c.down));
        let (x, y) = (centre_x(a.col), baseline(a.row));
        let rect = Rect::new(x - w / 2.0, y - u - ysep, x + w / 2.0, y + dn + ysep);
        let circle = c.filter(|c| c.frame == Frame::Circle).map(|_| {
            let r = ((rect.width() / 2.0).powi(2) + (rect.height() / 2.0).powi(2)).sqrt();
            (rect.center(), r)
        });
        Node { rect, centre: Point::new(x, y - AXIS * em), circle }
    };
    let mut items: Vec<Item> = Vec::new();
    let mut regions: Vec<Region> = Vec::new();
    for (a, c) in &cells {
        let n = node(*a);
        let (x, y) = (centre_x(a.col) - c.b.w / 2.0, baseline(a.row) - c.b.base);
        match c.frame {
            Frame::None => {}
            Frame::Box => items.push(stroked(n.rect.to_path(0.1), RULE * em, Paint::Text, None, false)),
            Frame::Circle => {
                if let Some((ctr, r)) = n.circle {
                    items.push(stroked(arrow::circle(ctr, r), RULE * em, Paint::Text, None, false));
                }
            }
        }
        regions.push(Region {
            path: vec![a.row, a.col],
            part: Part::Object,
            x: n.rect.x0,
            y: n.rect.y0,
            w: n.rect.width(),
            h: n.rect.height(),
            poly: Vec::new(),
        });
        items.push(text(x, y, Boxed { body: c.b.body.clone(), w: c.b.w, h: c.b.h, base: c.b.base }));
    }
    // Arrows: laid out in the order they nest (an arrow between arrows needs their middles),
    // drawn in the order they are written.
    let n = d.arrows.len();
    let mut laid: Vec<Option<Laid>> = (0..n).map(|_| None).collect();
    let mut order: Vec<(usize, usize)> = (0..n).filter_map(|i| d.level(i).map(|l| (l, i))).collect();
    order.sort();
    for (level, i) in order {
        if level > MAX_LEVEL {
            continue;
        }
        let Some(a) = d.arrows.get(i) else { continue };
        let end = |e: End| -> Option<Node> {
            match e {
                End::Cell(c) => Some(node(at(c))),
                End::Arrow(j) => {
                    laid.get(j).and_then(Option::as_ref).map(|l| Node { rect: Rect::from_points(l.middle, l.middle), centre: l.middle, circle: None })
                }
            }
        };
        let (Some(f), Some(t)) = (end(a.from), end(a.to)) else { continue };
        let looped = a.from == a.to;
        let l = lay_arrow(a, i, f, t, looped, style, &reference, measure);
        if let Some(slot) = laid.get_mut(i) {
            *slot = Some(l);
        }
    }
    for l in laid.into_iter().flatten() {
        items.extend(l.items);
        regions.extend(l.regions);
    }
    let grid = Grid {
        rows: (0..rows).map(|r| (baseline(r) - up.get(r).copied().unwrap_or(0.0), baseline(r) + down.get(r).copied().unwrap_or(0.0))).collect(),
        cols: (0..cols)
            .map(|c| (centre_x(c) - col_w.get(c).copied().unwrap_or(0.0) / 2.0, centre_x(c) + col_w.get(c).copied().unwrap_or(0.0) / 2.0))
            .collect(),
    };
    // The typeset starts at its ink; in the editor, at its grid (empty cells included).
    let (mut x0, mut y0, mut x1, mut y1) = bounds(&items);
    if ring > 0 || items.is_empty() {
        let gx0 = grid.cols.first().map_or(0.0, |c| c.0);
        let gy0 = grid.rows.first().map_or(0.0, |r| r.0);
        let gx1 = grid.cols.last().map_or(0.0, |c| c.1);
        let gy1 = grid.rows.last().map_or(0.0, |r| r.1);
        if items.is_empty() {
            (x0, y0, x1, y1) = (gx0, gy0, gx1, gy1);
        } else {
            (x0, y0, x1, y1) = (x0.min(gx0), y0.min(gy0), x1.max(gx1), y1.max(gy1));
        }
    }
    shift(&mut items, -x0, -y0);
    shift_regions(&mut regions, -x0, -y0);
    let grid = Grid {
        rows: grid.rows.into_iter().map(|(a, b)| (a - y0, b - y0)).collect(),
        cols: grid.cols.into_iter().map(|(a, b)| (a - x0, b - x0)).collect(),
    };
    Typeset { kind: Kind::Diagram, w: (x1 - x0).max(0.0), h: (y1 - y0).max(0.0), items, thickness: thickness(style), regions, grid: Some(grid) }
}
