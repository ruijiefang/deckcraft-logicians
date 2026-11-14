//! The Commutative Diagram editor's changes. Each is a function of the diagram, so the editor's
//! window only shows the diagram and calls these; refusals come back as a reason to show.

use super::parse::reverse_look;
use super::{Arrow, At, Body, Diagram, End, Head, Label, Loop, MAX_ARROWS, MAX_COLS, MAX_LEVEL, MAX_ROWS, Object, Side, Tail};

/// What is selected: objects by cell, arrows by index.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Selection {
    pub objects: Vec<At>,
    pub arrows: Vec<usize>,
}

impl Selection {
    pub fn is_empty(&self) -> bool {
        self.objects.is_empty() && self.arrows.is_empty()
    }
    pub fn object(at: At) -> Self {
        Selection { objects: vec![at], arrows: Vec::new() }
    }
    pub fn arrow(i: usize) -> Self {
        Selection { objects: Vec::new(), arrows: vec![i] }
    }
}

fn renumber(e: &mut End, map: &[Option<usize>]) {
    if let End::Arrow(i) = *e {
        *e = End::Arrow(map.get(i).copied().flatten().unwrap_or(usize::MAX));
    }
}

/// Removes the arrows `ids` and every arrow that reaches one of them; the rest are renumbered.
pub fn remove_arrows(d: &mut Diagram, ids: &[usize]) {
    let n = d.arrows.len();
    let mut gone = vec![false; n];
    for &i in ids {
        if let Some(g) = gone.get_mut(i) {
            *g = true;
        }
    }
    // Arrows reaching removed (or missing) arrows go too, to any depth.
    for _ in 0..=n {
        let mut changed = false;
        for (i, a) in d.arrows.iter().enumerate() {
            if gone.get(i).copied().unwrap_or(true) {
                continue;
            }
            let lost = [a.from, a.to].iter().any(|e| matches!(e, End::Arrow(j) if gone.get(*j).copied().unwrap_or(true)));
            if lost {
                if let Some(g) = gone.get_mut(i) {
                    *g = true;
                }
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    let mut map = vec![None; n];
    let mut next = 0;
    for (i, g) in gone.iter().enumerate() {
        if !g {
            if let Some(m) = map.get_mut(i) {
                *m = Some(next);
            }
            next += 1;
        }
    }
    let mut k = 0;
    d.arrows.retain(|_| {
        let keep = !gone.get(k).copied().unwrap_or(true);
        k += 1;
        keep
    });
    for a in &mut d.arrows {
        renumber(&mut a.from, &map);
        renumber(&mut a.to, &map);
    }
}

/// Moves every object and cell end `dr` rows down and `dc` columns right.
fn shift_all(d: &mut Diagram, dr: usize, dc: usize) {
    for o in &mut d.objects {
        o.at = At::new(o.at.row + dr, o.at.col + dc);
    }
    for a in &mut d.arrows {
        for e in [&mut a.from, &mut a.to] {
            if let End::Cell(at) = e {
                *at = At::new(at.row + dr, at.col + dc);
            }
        }
    }
    d.row_gaps = std::mem::take(&mut d.row_gaps).into_iter().map(|(r, g)| (r + dr, g)).collect();
    d.col_gaps = std::mem::take(&mut d.col_gaps).into_iter().map(|(c, g)| (c + dc, g)).collect();
}

/// The cells in use: objects and the cells arrows reach.
fn used(d: &Diagram) -> impl Iterator<Item = At> + '_ {
    d.objects.iter().map(|o| o.at).chain(d.arrows.iter().flat_map(|a| [a.from, a.to]).filter_map(|e| match e {
        End::Cell(at) => Some(at),
        End::Arrow(_) => None,
    }))
}

/// Drops empty rows above and columns left of everything; returns how many of each.
pub fn normalize(d: &mut Diagram) -> (usize, usize) {
    let (r0, c0) = used(d).fold((usize::MAX, usize::MAX), |(r, c), at| (r.min(at.row), c.min(at.col)));
    if r0 == usize::MAX || (r0 == 0 && c0 == 0) {
        return (0, 0);
    }
    for o in &mut d.objects {
        o.at = At::new(o.at.row - r0, o.at.col - c0);
    }
    for a in &mut d.arrows {
        for e in [&mut a.from, &mut a.to] {
            if let End::Cell(at) = e {
                *at = At::new(at.row - r0, at.col - c0);
            }
        }
    }
    d.row_gaps = std::mem::take(&mut d.row_gaps).into_iter().filter(|(r, _)| *r >= r0).map(|(r, g)| (r - r0, g)).collect();
    d.col_gaps = std::mem::take(&mut d.col_gaps).into_iter().filter(|(c, _)| *c >= c0).map(|(c, g)| (c - c0, g)).collect();
    (r0, c0)
}

/// A cell given as signed row and column, made room for: negative ones move everything down or
/// right first. Returns the cell and how far everything moved.
fn make_room(d: &mut Diagram, row: i64, col: i64) -> Result<(At, (usize, usize)), String> {
    let dr = if row < 0 { row.unsigned_abs() as usize } else { 0 };
    let dc = if col < 0 { col.unsigned_abs() as usize } else { 0 };
    let (rows, cols) = d.size();
    let (r, c) = ((row + dr as i64) as usize, (col + dc as i64) as usize);
    if r >= MAX_ROWS || c >= MAX_COLS || rows + dr > MAX_ROWS || cols + dc > MAX_COLS {
        return Err(format!("A diagram has at most {MAX_ROWS} rows and {MAX_COLS} columns."));
    }
    if dr > 0 || dc > 0 {
        shift_all(d, dr, dc);
    }
    Ok((At::new(r, c), (dr, dc)))
}

/// Adds an object at `row`, `col` (a row or column outside the diagram is added). Returns where
/// it is and how far the diagram moved to make room.
pub fn add_object(d: &mut Diagram, row: i64, col: i64, label: &str) -> Result<(At, (usize, usize)), String> {
    let mut trial = d.clone();
    let (at, moved) = make_room(&mut trial, row, col)?;
    if trial.object(at).is_some() {
        return Err("There is an object there already.".into());
    }
    trial.objects.push(Object::new(at, label));
    *d = trial;
    Ok((at, moved))
}

/// Moves the objects `ats` (and the arrows at them) by `dr` rows and `dc` columns. Refuses when
/// one would land on an object that stays, or off the grid's top or left by more than a row.
pub fn move_objects(d: &mut Diagram, ats: &[At], dr: i64, dc: i64) -> Result<Vec<At>, String> {
    if ats.is_empty() || (dr == 0 && dc == 0) {
        return Ok(ats.to_vec());
    }
    let moving: Vec<At> = ats.iter().copied().filter(|at| d.object(*at).is_some()).collect();
    let target = |at: At| (at.row as i64 + dr, at.col as i64 + dc);
    let min_r = moving.iter().map(|a| target(*a).0).min().unwrap_or(0);
    let min_c = moving.iter().map(|a| target(*a).1).min().unwrap_or(0);
    let mut trial = d.clone();
    let (_, (sr, sc)) = make_room(&mut trial, min_r.min(0), min_c.min(0))?;
    let shifted = |at: At| At::new(at.row + sr, at.col + sc);
    let from: Vec<At> = moving.iter().map(|a| shifted(*a)).collect();
    let mut to = Vec::new();
    for f in &from {
        let (r, c) = (f.row as i64 + dr, f.col as i64 + dc);
        if r < 0 || c < 0 || r as usize >= MAX_ROWS || c as usize >= MAX_COLS {
            return Err(format!("A diagram has at most {MAX_ROWS} rows and {MAX_COLS} columns."));
        }
        let t = At::new(r as usize, c as usize);
        if !from.contains(&t) && trial.object(t).is_some() {
            return Err("There is an object there already.".into());
        }
        to.push(t);
    }
    let map = |at: At| from.iter().position(|f| *f == at).and_then(|k| to.get(k).copied());
    for o in &mut trial.objects {
        if let Some(t) = map(o.at) {
            o.at = t;
        }
    }
    for a in &mut trial.arrows {
        for e in [&mut a.from, &mut a.to] {
            if let End::Cell(at) = e
                && let Some(t) = map(*at)
            {
                *at = t;
            }
        }
    }
    let (nr, nc) = normalize(&mut trial);
    *d = trial;
    Ok(to.into_iter().map(|t| At::new(t.row - nr, t.col - nc)).collect())
}

/// Removes the objects at `ats` and the arrows reaching them.
pub fn remove_objects(d: &mut Diagram, ats: &[At]) {
    d.objects.retain(|o| !ats.contains(&o.at));
    let touching: Vec<usize> = d
        .arrows
        .iter()
        .enumerate()
        .filter(|(_, a)| [a.from, a.to].iter().any(|e| matches!(e, End::Cell(at) if ats.contains(at))))
        .map(|(i, _)| i)
        .collect();
    remove_arrows(d, &touching);
    normalize(d);
}

/// Removes what is selected.
pub fn remove(d: &mut Diagram, sel: &Selection) {
    remove_arrows(d, &sel.arrows);
    remove_objects(d, &sel.objects);
}

/// Adds an arrow between two ends: cells (an arrow from an object to itself loops), or arrows
/// (a 2-cell, drawn double). Returns its index.
pub fn add_arrow(d: &mut Diagram, from: End, to: End) -> Result<usize, String> {
    if d.arrows.len() >= MAX_ARROWS {
        return Err(format!("A diagram has at most {MAX_ARROWS} arrows."));
    }
    for e in [from, to] {
        match e {
            End::Arrow(i) if i >= d.arrows.len() => return Err("That arrow isn't there any more.".into()),
            End::Cell(at) if at.row >= MAX_ROWS || at.col >= MAX_COLS => {
                return Err(format!("A diagram has at most {MAX_ROWS} rows and {MAX_COLS} columns."));
            }
            _ => {}
        }
    }
    if matches!((from, to), (End::Arrow(a), End::Arrow(b)) if a == b) {
        return Err("An arrow can't go from an arrow to itself.".into());
    }
    let mut a = Arrow::new(from, to);
    if from == to {
        a.looping = Some(Loop::Above);
    }
    if !a.is_cell_arrow() {
        a.body = Body::Double;
        a.shorten = (2.0, 2.0);
    }
    d.arrows.push(a);
    let i = d.arrows.len() - 1;
    if d.level(i).is_none() {
        d.arrows.pop();
        return Err(format!("Arrows between arrows nest at most {MAX_LEVEL} deep."));
    }
    Ok(i)
}

/// Turns arrow `i` round, keeping how it looks.
pub fn reverse(d: &mut Diagram, i: usize) {
    if let Some(a) = d.arrows.get_mut(i) {
        std::mem::swap(&mut a.from, &mut a.to);
        reverse_look(a);
    }
}

/// Mirrors the diagram left to right (`horizontal`) or top to bottom.
pub fn flip(d: &mut Diagram, horizontal: bool) {
    let (rows, cols) = d.size();
    let m = |at: At| {
        if horizontal {
            At::new(at.row, cols.saturating_sub(1).saturating_sub(at.col))
        } else {
            At::new(rows.saturating_sub(1).saturating_sub(at.row), at.col)
        }
    };
    for o in &mut d.objects {
        o.at = m(o.at);
    }
    for a in &mut d.arrows {
        for e in [&mut a.from, &mut a.to] {
            if let End::Cell(at) = e {
                *at = m(*at);
            }
        }
        // A mirror swaps left and right.
        a.bend = -a.bend;
        a.shift = -a.shift;
        a.tail = match a.tail {
            Tail::Hook => Tail::HookRight,
            Tail::HookRight => Tail::Hook,
            t => t,
        };
        a.head = match a.head {
            Head::Harpoon => Head::HarpoonRight,
            Head::HarpoonRight => Head::Harpoon,
            h => h,
        };
        for l in &mut a.labels {
            l.side = match l.side {
                Side::Left => Side::Right,
                Side::Right => Side::Left,
                s => s,
            };
        }
        a.looping = a.looping.map(|l| match (l, horizontal) {
            (Loop::Left, true) => Loop::Right,
            (Loop::Right, true) => Loop::Left,
            (Loop::Above, false) => Loop::Below,
            (Loop::Below, false) => Loop::Above,
            (l, _) => l,
        });
    }
    d.row_gaps.clear();
    d.col_gaps.clear();
}

/// Swaps rows and columns (with a flip, a quarter turn).
pub fn transpose(d: &mut Diagram) {
    let t = |at: At| At::new(at.col, at.row);
    for o in &mut d.objects {
        o.at = t(o.at);
    }
    for a in &mut d.arrows {
        for e in [&mut a.from, &mut a.to] {
            if let End::Cell(at) = e {
                *at = t(*at);
            }
        }
        // Transposing mirrors across the diagonal: left and right swap.
        a.bend = -a.bend;
        a.shift = -a.shift;
        for l in &mut a.labels {
            l.side = match l.side {
                Side::Left => Side::Right,
                Side::Right => Side::Left,
                s => s,
            };
        }
        a.looping = a.looping.map(|l| match l {
            Loop::Above => Loop::Left,
            Loop::Left => Loop::Above,
            Loop::Below => Loop::Right,
            Loop::Right => Loop::Below,
        });
    }
    std::mem::swap(&mut d.row_sep, &mut d.col_sep);
    d.row_gaps.clear();
    d.col_gaps.clear();
}

/// A quarter turn clockwise.
pub fn rotate(d: &mut Diagram) {
    transpose(d);
    flip(d, true);
}

/// The selected objects and arrows as a diagram of their own (arrows whose ends aren't all
/// selected come with the objects at their ends), moved to the top left.
pub fn extract(d: &Diagram, sel: &Selection) -> Diagram {
    let mut objects: Vec<At> = sel.objects.clone();
    let mut arrows: Vec<usize> = sel.arrows.clone();
    // Arrows between selected objects come along.
    for (i, a) in d.arrows.iter().enumerate() {
        let inside = |e: End| matches!(e, End::Cell(at) if objects.contains(&at));
        if a.is_cell_arrow() && inside(a.from) && inside(a.to) && !arrows.contains(&i) {
            arrows.push(i);
        }
    }
    // Everything a selected arrow needs: its cells' objects, and arrows it joins.
    let mut k = 0;
    while let Some(&i) = arrows.get(k) {
        k += 1;
        if k > MAX_ARROWS {
            break;
        }
        let Some(a) = d.arrows.get(i) else { continue };
        for e in [a.from, a.to] {
            match e {
                End::Cell(at) if !objects.contains(&at) => objects.push(at),
                End::Arrow(j) if !arrows.contains(&j) => arrows.push(j),
                _ => {}
            }
        }
    }
    arrows.sort_unstable();
    arrows.dedup();
    let map: Vec<Option<usize>> = (0..d.arrows.len()).map(|i| arrows.iter().position(|&a| a == i)).collect();
    let mut out = Diagram { row_sep: d.row_sep, col_sep: d.col_sep, ..Default::default() };
    out.objects = d.objects.iter().filter(|o| objects.contains(&o.at)).cloned().collect();
    out.arrows = arrows
        .iter()
        .filter_map(|&i| d.arrows.get(i).cloned())
        .map(|mut a| {
            renumber(&mut a.from, &map);
            renumber(&mut a.to, &map);
            a
        })
        .collect();
    normalize(&mut out);
    out
}

/// Pastes `part` with its top left at `at`. Refuses when an object would land on one already
/// there. Returns what was pasted, as a selection.
pub fn paste(d: &mut Diagram, part: &Diagram, at: At) -> Result<Selection, String> {
    if d.arrows.len() + part.arrows.len() > MAX_ARROWS {
        return Err(format!("A diagram has at most {MAX_ARROWS} arrows."));
    }
    let moved = |p: At| At::new(p.row + at.row, p.col + at.col);
    let mut trial = d.clone();
    let mut sel = Selection::default();
    for o in &part.objects {
        let t = moved(o.at);
        if t.row >= MAX_ROWS || t.col >= MAX_COLS {
            return Err(format!("A diagram has at most {MAX_ROWS} rows and {MAX_COLS} columns."));
        }
        if trial.object(t).is_some() {
            return Err("There are objects in the way: paste into an empty part of the grid.".into());
        }
        trial.objects.push(Object { at: t, ..o.clone() });
        sel.objects.push(t);
    }
    let base = trial.arrows.len();
    for a in &part.arrows {
        let mut a = a.clone();
        for e in [&mut a.from, &mut a.to] {
            *e = match *e {
                End::Cell(p) => End::Cell(moved(p)),
                End::Arrow(j) => End::Arrow(base + j),
            };
        }
        a.name = None;
        trial.arrows.push(a);
        sel.arrows.push(trial.arrows.len() - 1);
    }
    if (base..trial.arrows.len()).any(|i| trial.level(i).is_none()) {
        return Err(format!("Arrows between arrows nest at most {MAX_LEVEL} deep."));
    }
    *d = trial;
    Ok(sel)
}

/// Puts arrows between cells in the order of the cells they start at, arrows between arrows
/// after them, so the LaTeX writes each arrow after its object. Arrows are drawn in order, so a
/// diagram with `crossing over` arrows keeps its order.
pub fn tidy(d: &mut Diagram) {
    if d.arrows.iter().any(|a| a.crossing) {
        return;
    }
    let key = |i: usize| {
        let a = d.arrows.get(i);
        let from = match a.map(|a| (a.from, a.is_cell_arrow())) {
            Some((End::Cell(at), true)) => (at.row, at.col),
            _ => (usize::MAX, usize::MAX),
        };
        (d.level(i).unwrap_or(MAX_LEVEL), from, i)
    };
    let mut order: Vec<usize> = (0..d.arrows.len()).collect();
    order.sort_by_key(|&i| key(i));
    if order.iter().enumerate().all(|(k, &i)| k == i) {
        return;
    }
    let mut map = vec![None; order.len()];
    for (new, &old) in order.iter().enumerate() {
        if let Some(m) = map.get_mut(old) {
            *m = Some(new);
        }
    }
    let mut arrows: Vec<Arrow> = order.iter().filter_map(|&i| d.arrows.get(i).cloned()).collect();
    for a in &mut arrows {
        renumber(&mut a.from, &map);
        renumber(&mut a.to, &map);
    }
    d.arrows = arrows;
}

/// Starting points: (name, tikz-cd).
pub const TEMPLATES: &[(&str, &str)] = &[
    ("Commutative square", r#"A \arrow[r, "f"] \arrow[d, "g"'] & B \arrow[d, "h"] \\ C \arrow[r, "k"'] & D"#),
    ("Triangle", r#"A \arrow[r, "f"] \arrow[dr, "h"'] & B \arrow[d, "g"] \\ & C"#),
    ("Pullback", r#"P \arrow[r] \arrow[d] \arrow[dr, phantom, "\lrcorner", very near start] & X \arrow[d, "f"] \\ Y \arrow[r, "g"'] & Z"#),
    ("Pushout", r#"Z \arrow[r, "f"] \arrow[d, "g"'] & X \arrow[d] \\ Y \arrow[r] & P \arrow[ul, phantom, "\ulcorner", very near start]"#),
    (
        "Product",
        r#"& Z \arrow[dl, "f"'] \arrow[d, dashed, "\exists!"] \arrow[dr, "g"] & \\ A & A \times B \arrow[l, "\pi_1"] \arrow[r, "\pi_2"'] & B"#,
    ),
    (
        "Coproduct",
        r#"A \arrow[r, "\iota_1"] \arrow[dr, "f"'] & A + B \arrow[d, dashed, "\exists!"] & B \arrow[l, "\iota_2"'] \arrow[dl, "g"] \\ & Z &"#,
    ),
    ("Equalizer", r#"E \arrow[r, "e"] & A \arrow[r, shift left, "f"] \arrow[r, shift right, "g"'] & B"#),
    ("Span", r#"& C \arrow[dl, "f"'] \arrow[dr, "g"] & \\ A & & B"#),
    ("Cospan", r#"A \arrow[dr, "f"'] & & B \arrow[dl, "g"] \\ & C &"#),
    ("Adjunction", r#"\mathcal{C} \arrow[r, shift left=2, "F"] \arrow[r, phantom, "\bot"] & \mathcal{D} \arrow[l, shift left=2, "G"]"#),
    (
        "Natural transformation",
        r#"\mathcal{C} \arrow[r, bend left=50, "F", ""{name=U, anchor=center, inner sep=0}] \arrow[r, bend right=50, "G"', ""{name=D, anchor=center, inner sep=0}] & \mathcal{D} \arrow[Rightarrow, from=U, to=D, "\alpha", shorten <=2pt, shorten >=2pt]"#,
    ),
    ("Short exact sequence", r#"0 \arrow[r] & A \arrow[r, tail, "f"] & B \arrow[r, two heads, "g"] & C \arrow[r] & 0"#),
    ("Isomorphism", r#"A \arrow[r, "\sim"] & B"#),
    ("Endomorphism", r#"A \arrow[loop right, "f"]"#),
];

/// The template `name` as a diagram.
pub fn template(name: &str) -> Option<Diagram> {
    let (_, src) = TEMPLATES.iter().find(|(n, _)| *n == name)?;
    super::open(&format!("\\begin{{tikzcd}} {src} \\end{{tikzcd}}")).ok().map(|o| o.diagram)
}

/// A new label on arrow `i` (its first, if it has none), for typing into.
pub fn ensure_label(d: &mut Diagram, i: usize) -> Option<&mut Label> {
    let a = d.arrows.get_mut(i)?;
    if a.labels.is_empty() {
        a.labels.push(Label::new(""));
    }
    a.labels.first_mut()
}

/// Labels typed empty are dropped (an empty label draws nothing).
pub fn drop_empty_labels(d: &mut Diagram) {
    for a in &mut d.arrows {
        a.labels.retain(|l| !l.text.trim().is_empty());
    }
}
