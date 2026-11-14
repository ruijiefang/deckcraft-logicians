//! Reading tikz-cd: `\begin{tikzcd}[options] cells & … \\ … \end{tikzcd}` with `\arrow[…]`
//! (or `\ar`, the older `\arrow{dr}{label}` form and `\rar`-style shortcuts), as written by hand
//! and as quiver exports it.

use std::collections::HashMap;

use super::{Arrow, At, Body, Diagram, End, Frame, Head, Label, Loop, MAX_ARROWS, MAX_COLS, MAX_ROWS, MAX_TEXT, Object, Rgb, Sep, Side, Tail};

/// A diagram read from LaTeX, and what reading it left out or changed.
#[derive(Clone, Debug, PartialEq)]
pub struct Opened {
    pub diagram: Diagram,
    pub notices: Vec<String>,
}

/// Whether `src` is a tikz-cd diagram.
pub fn is_diagram(src: &str) -> bool {
    strip_comments(src).contains("\\begin{tikzcd}")
}

/// TeX's points per unit, at the 10 pt the diagram is set as (it is scaled to its size).
fn unit_pt(unit: &str) -> Option<f64> {
    Some(match unit {
        "pt" => 1.0,
        "em" => 10.0,
        "ex" => 4.305_54,
        "mm" => 2.845_27,
        "cm" => 28.452_7,
        "in" => 72.27,
        "bp" => 1.003_75,
        "pc" => 12.0,
        "sp" => 1.0 / 65_536.0,
        _ => return None,
    })
}

/// A TeX length in points (at 10 pt): `2pt`, `1.5ex`, `+2em`; `None` without a unit.
pub(crate) fn dim_pt(s: &str) -> Option<f64> {
    let s = s.trim().trim_start_matches('+');
    let split = s.find(|c: char| c.is_ascii_alphabetic())?;
    let (num, unit) = s.split_at(split);
    let v: f64 = num.trim().parse().ok()?;
    let k = unit_pt(unit.trim())?;
    let pt = v * k;
    (pt.is_finite() && pt.abs() <= 10_000.0).then_some(pt)
}

/// A plain number.
fn number(s: &str) -> Option<f64> {
    let v: f64 = s.trim().trim_start_matches('+').parse().ok()?;
    (v.is_finite() && v.abs() <= 1e6).then_some(v)
}

/// xcolor's base colours, as 8-bit RGB.
pub(crate) const COLORS: &[(&str, Rgb)] = &[
    ("black", [0, 0, 0]),
    ("white", [255, 255, 255]),
    ("red", [255, 0, 0]),
    ("green", [0, 255, 0]),
    ("blue", [0, 0, 255]),
    ("cyan", [0, 255, 255]),
    ("magenta", [255, 0, 255]),
    ("yellow", [255, 255, 0]),
    ("gray", [128, 128, 128]),
    ("darkgray", [64, 64, 64]),
    ("lightgray", [191, 191, 191]),
    ("brown", [191, 128, 64]),
    ("lime", [191, 255, 0]),
    ("olive", [128, 128, 0]),
    ("orange", [255, 128, 0]),
    ("pink", [255, 191, 191]),
    ("purple", [191, 0, 64]),
    ("teal", [0, 128, 128]),
    ("violet", [128, 0, 128]),
];

/// A colour: a base name, or `rgb,255:red,R;green,G;blue,B` (braced or not).
fn color(s: &str) -> Option<Rgb> {
    let s = unbrace(s.trim());
    if let Some((_, c)) = COLORS.iter().find(|(n, _)| *n == s) {
        return Some(*c);
    }
    let rest = s.strip_prefix("rgb,255:")?;
    let mut out = [0u8; 3];
    for (k, part) in rest.split(';').enumerate() {
        let (name, v) = part.split_once(',')?;
        let i = ["red", "green", "blue"].iter().position(|n| *n == name.trim())?;
        let v: f64 = v.trim().parse().ok()?;
        if k > 2 || !(0.0..=255.0).contains(&v) {
            return None;
        }
        if let Some(slot) = out.get_mut(i) {
            *slot = v.round() as u8;
        }
    }
    Some(out)
}

/// `s` without one pair of braces around all of it.
fn unbrace(s: &str) -> &str {
    let t = s.trim();
    if t.starts_with('{') && t.ends_with('}') && t.len() >= 2 && matching_brace(t, 0) == Some(t.len() - 1) {
        t.get(1..t.len() - 1).unwrap_or(t).trim()
    } else {
        t
    }
}

/// The index of the `}` closing the `{` at byte `open`.
fn matching_brace(s: &str, open: usize) -> Option<usize> {
    let b = s.as_bytes();
    let mut depth = 0usize;
    let mut i = open;
    while let Some(&c) = b.get(i) {
        match c {
            b'\\' => i += 1,
            b'{' => depth += 1,
            b'}' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

/// The index of the `]` closing the `[` at byte `open` (brackets inside braces don't count).
fn matching_bracket(s: &str, open: usize) -> Option<usize> {
    let b = s.as_bytes();
    let mut braces = 0usize;
    let mut i = open + 1;
    while let Some(&c) = b.get(i) {
        match c {
            b'\\' => i += 1,
            b'{' => braces += 1,
            b'}' => braces = braces.saturating_sub(1),
            b']' if braces == 0 => return Some(i),
            _ => {}
        }
        i += 1;
    }
    None
}

/// `src` without `%` comments (an escaped `\%` stays).
fn strip_comments(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    for line in src.lines() {
        let b = line.as_bytes();
        let mut cut = line.len();
        let mut i = 0;
        while let Some(&c) = b.get(i) {
            if c == b'\\' {
                i += 2;
                continue;
            }
            if c == b'%' {
                cut = i;
                break;
            }
            i += 1;
        }
        out.push_str(line.get(..cut).unwrap_or(line));
        out.push('\n');
    }
    out
}

/// Splits at commas outside braces.
pub(crate) fn split_options(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut depth = 0usize;
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                cur.push(c);
                if let Some(n) = chars.next() {
                    cur.push(n);
                }
                continue;
            }
            '{' => depth += 1,
            '}' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                out.push(std::mem::take(&mut cur).trim().to_string());
                continue;
            }
            _ => {}
        }
        cur.push(c);
    }
    if !cur.trim().is_empty() {
        out.push(cur.trim().to_string());
    }
    out.into_iter().filter(|s| !s.is_empty()).collect()
}

/// `key=value` or a bare key, both trimmed, the key's spaces collapsed.
fn key_value(opt: &str) -> (String, Option<String>) {
    // `=` inside braces belongs to the value (`curve={height=6pt}`).
    let mut depth = 0usize;
    for (i, c) in opt.char_indices() {
        match c {
            '{' => depth += 1,
            '}' => depth = depth.saturating_sub(1),
            '=' if depth == 0 => {
                let k = opt.get(..i).unwrap_or("").split_whitespace().collect::<Vec<_>>().join(" ");
                return (k, Some(opt.get(i + 1..).unwrap_or("").trim().to_string()));
            }
            _ => {}
        }
    }
    (opt.split_whitespace().collect::<Vec<_>>().join(" "), None)
}

/// A direction string (`r`, `dl`, `uu`): the cell it reaches from `at`.
fn direction(s: &str, at: At) -> Option<At> {
    if s.is_empty() || !s.chars().all(|c| matches!(c, 'r' | 'l' | 'd' | 'u')) || s.len() > 2 * MAX_ROWS {
        return None;
    }
    let (mut r, mut c) = (at.row as i64, at.col as i64);
    for ch in s.chars() {
        match ch {
            'r' => c += 1,
            'l' => c -= 1,
            'd' => r += 1,
            _ => r -= 1,
        }
    }
    (r >= 0 && c >= 0).then(|| At::new(r as usize, c as usize))
}

/// `row-col` (1-based), as quiver and `from=2-1` write cells.
fn row_col(s: &str) -> Option<At> {
    let (r, c) = s.trim().split_once('-')?;
    let (r, c): (usize, usize) = (r.trim().parse().ok()?, c.trim().parse().ok()?);
    (r >= 1 && c >= 1 && r <= MAX_ROWS && c <= MAX_COLS).then(|| At::new(r - 1, c - 1))
}

/// An endpoint as written: a direction, a cell, or a name to look up later.
#[derive(Clone, Debug, PartialEq)]
enum Ref {
    Cell(At),
    Name(String),
}

fn endpoint(s: &str, at: At) -> Option<Ref> {
    let s = unbrace(s);
    direction(s, at).or_else(|| row_col(s)).map(Ref::Cell).or_else(|| (!s.is_empty()).then(|| Ref::Name(s.to_string())))
}

/// An arrow while its diagram is read: endpoints still as written.
struct Raw {
    arrow: Arrow,
    from: Option<Ref>,
    to: Option<Ref>,
    /// Draw it the other way round (`mapsfrom`, `hookleftarrow`…): swapped once read.
    reversed: bool,
    /// The name of the label node 2-cells reach it by.
    anchor: Option<String>,
}

/// The arrow tips and line one of tikz-cd's arrow styles sets. `None`: not a style.
#[allow(clippy::type_complexity)]
fn style(name: &str) -> Option<(Option<Tail>, Option<Body>, Option<Head>, bool)> {
    use Body::*;
    Some(match name {
        "rightarrow" | "to" => (Some(Tail::None), None, Some(Head::Arrow), false),
        "to head" => (None, None, Some(Head::Arrow), false),
        "leftarrow" => (Some(Tail::Head), None, Some(Head::None), false),
        "leftrightarrow" => (Some(Tail::Head), None, Some(Head::Arrow), false),
        "Rightarrow" | "Rrightarrow" => (Some(Tail::None), Some(Double), Some(Head::Arrow), false),
        "Leftarrow" => (Some(Tail::Head), Some(Double), Some(Head::None), false),
        "Leftrightarrow" => (Some(Tail::Head), Some(Double), Some(Head::Arrow), false),
        "equal" => (Some(Tail::None), Some(Double), Some(Head::None), false),
        "maps to" => (Some(Tail::Bar), None, None, false),
        "mapsto" => (Some(Tail::Bar), None, Some(Head::Arrow), false),
        "mapsfrom" => (Some(Tail::Bar), None, Some(Head::Arrow), true),
        "Mapsto" => (Some(Tail::Bar), Some(Double), Some(Head::Arrow), false),
        "Mapsfrom" => (Some(Tail::Bar), Some(Double), Some(Head::Arrow), true),
        "hook" => (Some(Tail::Hook), None, None, false),
        "hook'" => (Some(Tail::HookRight), None, None, false),
        "hookrightarrow" => (Some(Tail::Hook), None, Some(Head::Arrow), false),
        // ↩ is ↪ drawn the other way: its hook curls to the page's top, the reversed arrow's right.
        "hookleftarrow" => (Some(Tail::HookRight), None, Some(Head::Arrow), true),
        "tail" => (Some(Tail::Mono), None, None, false),
        "rightarrowtail" => (Some(Tail::Mono), None, Some(Head::Arrow), false),
        "leftarrowtail" => (Some(Tail::Mono), None, Some(Head::Arrow), true),
        "tail reversed" => (Some(Tail::Head), None, None, false),
        "two heads" => (None, None, Some(Head::TwoHeads), false),
        "twoheadrightarrow" => (Some(Tail::None), None, Some(Head::TwoHeads), false),
        "twoheadleftarrow" => (Some(Tail::None), None, Some(Head::TwoHeads), true),
        "harpoon" => (None, None, Some(Head::Harpoon), false),
        "harpoon'" => (None, None, Some(Head::HarpoonRight), false),
        "rightharpoonup" => (Some(Tail::None), None, Some(Head::Harpoon), false),
        "rightharpoondown" => (Some(Tail::None), None, Some(Head::HarpoonRight), false),
        "leftharpoonup" => (Some(Tail::None), None, Some(Head::HarpoonRight), true),
        "leftharpoondown" => (Some(Tail::None), None, Some(Head::Harpoon), true),
        "dashed" => (None, Some(Dashed), None, false),
        "dashrightarrow" => (Some(Tail::None), Some(Dashed), Some(Head::Arrow), false),
        "dashleftarrow" => (Some(Tail::None), Some(Dashed), Some(Head::Arrow), true),
        "dotted" => (None, Some(Dotted), None, false),
        "squiggly" => (None, Some(Squiggly), None, false),
        "rightsquigarrow" => (Some(Tail::None), Some(Squiggly), Some(Head::Arrow), false),
        "leftsquigarrow" => (Some(Tail::None), Some(Squiggly), Some(Head::Arrow), true),
        "leftrightsquigarrow" => (Some(Tail::Head), Some(Squiggly), Some(Head::Arrow), false),
        "no head" => (None, None, Some(Head::None), false),
        "no tail" => (Some(Tail::None), None, None, false),
        "dash" | "no tips" => (Some(Tail::None), None, Some(Head::None), false),
        "phantom" => (None, Some(Phantom), None, false),
        _ => return None,
    })
}

/// TikZ's line widths, in points.
fn named_width(name: &str) -> Option<f64> {
    Some(match name {
        "ultra thin" => 0.1,
        "very thin" => 0.2,
        "thin" => 0.4,
        "semithick" => 0.6,
        "thick" => 0.8,
        "very thick" => 1.2,
        "ultra thick" => 1.6,
        _ => return None,
    })
}

/// A label's `pos` from TikZ's position words.
fn position(name: &str) -> Option<f64> {
    Some(match name {
        "at start" => 0.0,
        "very near start" => 0.125,
        "near start" => 0.25,
        "midway" => 0.5,
        "near end" => 0.75,
        "very near end" => 0.875,
        "at end" => 1.0,
        _ => return None,
    })
}

/// Quiver's `curve={height=…}`: how far (in points, left when negative) the middle of the arrow
/// moves, as a bend angle for an arrow of `chord` points. quiver bends its curves a little
/// deeper than a `bend left` of the same angle at this sagitta.
fn bend_for_height(height: f64, chord: f64) -> f64 {
    let s = (1.059 * height.abs() / chord.max(1.0)).clamp(0.0, 1.0);
    let deg = s.asin().to_degrees();
    if height < 0.0 { deg } else { -deg }
}

/// A label `"text"…`: its text, the options after it, the name 2-cells reach it by, and whether
/// it says where along the arrow it goes.
fn label_item(item: &str, notices: &mut Vec<String>) -> Option<(Label, Option<String>, bool)> {
    let rest = item.strip_prefix('"')?;
    // The closing quote: the first one outside braces.
    let b = rest.as_bytes();
    let mut depth = 0usize;
    let mut close = None;
    let mut i = 0;
    while let Some(&c) = b.get(i) {
        match c {
            b'\\' => i += 1,
            b'{' => depth += 1,
            b'}' => depth = depth.saturating_sub(1),
            b'"' if depth == 0 => {
                close = Some(i);
                break;
            }
            _ => {}
        }
        i += 1;
    }
    let close = close?;
    let text = unbrace(rest.get(..close).unwrap_or("")).to_string();
    let mut tail = rest.get(close + 1..).unwrap_or("").trim();
    let mut label = Label::new(&text);
    let mut name = None;
    let mut placed = false;
    if let Some(t) = tail.strip_prefix('\'') {
        label.side = Side::Right;
        tail = t.trim();
    }
    let opts: Vec<String> = if tail.starts_with('{') {
        split_options(unbrace(tail))
    } else if tail.is_empty() {
        Vec::new()
    } else {
        vec![tail.to_string()]
    };
    for o in opts {
        let (k, v) = key_value(&o);
        match (k.as_str(), v.as_deref()) {
            ("'" | "swap", None) => label.side = Side::Right,
            ("description", None) => label.side = Side::On,
            ("marking", None) => label.side = Side::Over,
            ("anchor", Some("center")) if label.side != Side::On => label.side = Side::Over,
            ("pos", Some(v)) => match number(v) {
                Some(p) => {
                    label.pos = p.clamp(0.0, 1.0);
                    placed = true;
                }
                None => label.extra.push(o.clone()),
            },
            ("name", Some(n)) => name = Some(unbrace(n).to_string()),
            ("text" | "color", Some(c)) => match color(c) {
                Some(rgb) => label.color = Some(rgb),
                None => label.extra.push(o.clone()),
            },
            // Settings of the anchor node of a 2-cell, not of a label.
            ("inner sep", Some(_)) if text.is_empty() => {}
            (k, None) => match (position(k), color(k)) {
                (Some(p), _) => {
                    label.pos = p;
                    placed = true;
                }
                (_, Some(rgb)) => label.color = Some(rgb),
                _ => label.extra.push(o.clone()),
            },
            _ => label.extra.push(o.clone()),
        }
    }
    if label.text.chars().count() > MAX_TEXT {
        notices.push(format!("A label longer than {MAX_TEXT} characters is cut short."));
        label.text = label.text.chars().take(MAX_TEXT).collect();
    }
    Some((label, name, placed))
}

/// An arrow while its options are read.
struct Reading {
    a: Arrow,
    at: At,
    from: Option<Ref>,
    to: Option<Ref>,
    reversed: bool,
    anchor: Option<String>,
    curve_height: Option<f64>,
    /// `near start` and `pos=…` on the arrow place the labels that don't say.
    default_pos: Option<f64>,
    unplaced: Vec<usize>,
}

impl Reading {
    fn label(&mut self, o: &str, notices: &mut Vec<String>) {
        match label_item(o, notices) {
            Some((label, name, placed)) => {
                if let Some(n) = name {
                    self.anchor = Some(n);
                }
                if !label.text.is_empty() {
                    if !placed {
                        self.unplaced.push(self.a.labels.len());
                    }
                    self.a.labels.push(label);
                }
            }
            None => self.a.extra.push(o.to_string()),
        }
    }

    /// A tikz-cd arrow style (`hook`, `Rightarrow`…); `false` if `k` isn't one.
    fn style(&mut self, k: &str, notices: &mut Vec<String>) -> bool {
        let Some((tail, body, head, rev)) = style(k) else { return false };
        if let Some(t) = tail {
            self.a.tail = t;
        }
        if let Some(b) = body {
            self.a.body = b;
        }
        if let Some(h) = head {
            self.a.head = h;
        }
        self.reversed ^= rev;
        if k == "Rrightarrow" {
            notices.push("Triple arrows (Rrightarrow) are drawn as double arrows.".into());
        }
        true
    }

    /// A length option: `true` if `k` is one (read or kept as written).
    fn length(&mut self, k: &str, v: Option<&str>, o: &str) -> bool {
        let pt = v.and_then(dim_pt);
        let a = &mut self.a;
        match (k, pt) {
            ("shorten", Some(pt)) => a.shorten = (pt, pt),
            ("shorten <", Some(pt)) => a.shorten.0 = pt,
            ("shorten >", Some(pt)) => a.shorten.1 = pt,
            ("line width", Some(pt)) if pt > 0.0 => a.width = Some(pt.min(20.0)),
            ("shorten" | "shorten <" | "shorten >" | "line width", _) => a.extra.push(o.to_string()),
            _ => return false,
        }
        true
    }

    fn shift(&mut self, k: &str, v: Option<&str>, o: &str) {
        let steps = match v {
            None => Some(1.0),
            Some(v) => number(v).or_else(|| dim_pt(v).map(|pt| pt / (0.56 * 4.305_54))),
        };
        match steps {
            Some(s) => self.a.shift = if k == "shift left" { s } else { -s },
            None => self.a.extra.push(o.to_string()),
        }
    }

    fn option(&mut self, o: &str, notices: &mut Vec<String>) {
        if o.starts_with('"') {
            return self.label(o, notices);
        }
        if let Some(cell) = direction(o, self.at) {
            self.to = Some(Ref::Cell(cell));
            return;
        }
        let (k, v) = key_value(o);
        if (v.is_none() && self.style(&k, notices)) || self.length(&k, v.as_deref(), o) {
            return;
        }
        let at = self.at;
        match (k.as_str(), v.as_deref()) {
            ("from", Some(v)) => self.from = endpoint(v, at),
            ("to", Some(v)) => self.to = endpoint(v, at),
            ("bend left" | "bend right", v) => {
                let deg = v.and_then(number).unwrap_or(30.0).clamp(-180.0, 180.0);
                self.a.bend = if k == "bend left" { deg } else { -deg };
            }
            ("curve", Some(v)) => match key_value(unbrace(v)) {
                (hk, Some(h)) if hk == "height" => self.curve_height = dim_pt(&h),
                _ => self.a.extra.push(o.to_string()),
            },
            ("shift left" | "shift right", v) => self.shift(&k, v, o),
            ("draw", Some(v)) if v.trim() == "none" => self.a.body = Body::Invisible,
            ("draw" | "color", Some(v)) => match color(v) {
                Some(rgb) => self.a.color = Some(rgb),
                None => self.a.extra.push(o.to_string()),
            },
            ("crossing over", None) => self.a.crossing = true,
            ("loop", None) => self.a.looping = Some(self.a.looping.unwrap_or_default()),
            ("loop above", None) => self.a.looping = Some(Loop::Above),
            ("loop right", None) => self.a.looping = Some(Loop::Right),
            ("loop below", None) => self.a.looping = Some(Loop::Below),
            ("loop left", None) => self.a.looping = Some(Loop::Left),
            ("pos", Some(v)) if number(v).is_some() => self.default_pos = number(v).map(|p| p.clamp(0.0, 1.0)),
            (k, None) if position(k).is_some() => self.default_pos = position(k),
            (k, None) if named_width(k).is_some() => self.a.width = named_width(k),
            (k, None) if color(k).is_some() => self.a.color = color(k),
            _ => self.a.extra.push(o.to_string()),
        }
    }

    fn finish(mut self, old_labels: Vec<Label>, notices: &mut Vec<String>) -> Raw {
        if let Some(p) = self.default_pos {
            for &k in &self.unplaced {
                if let Some(l) = self.a.labels.get_mut(k) {
                    l.pos = p;
                }
            }
        }
        self.a.labels.extend(old_labels);
        if let Some(h) = self.curve_height {
            // The chord isn't known before layout: a cell apart is about 4 em across, 3.2 down.
            let span = |r: &Option<Ref>| match r {
                Some(Ref::Cell(c)) => Some(*c),
                _ => None,
            };
            let (f, t) = (span(&self.from).unwrap_or(self.at), span(&self.to).unwrap_or(self.at));
            let dx = (t.col as f64 - f.col as f64) * 40.0;
            let dy = (t.row as f64 - f.row as f64) * 32.0;
            self.a.bend = bend_for_height(h, (dx * dx + dy * dy).sqrt());
            notices.push("quiver's curve={height=…} is written as bend left/right, which looks a little different.".into());
        }
        Raw { arrow: self.a, from: self.from.or(Some(Ref::Cell(self.at))), to: self.to, reversed: self.reversed, anchor: self.anchor }
    }
}

/// Reads one `\arrow`'s options (and labels written the old way) at cell `at`.
fn arrow_from(opts: &[String], at: At, old_dir: Option<&str>, old_labels: Vec<Label>, notices: &mut Vec<String>) -> Raw {
    let mut r = Reading {
        a: Arrow::new(End::Cell(at), End::Cell(at)),
        at,
        from: None,
        to: old_dir.and_then(|d| endpoint(d, at)),
        reversed: false,
        anchor: None,
        curve_height: None,
        default_pos: None,
        unplaced: Vec::new(),
    };
    for o in opts {
        r.option(o.trim(), notices);
    }
    r.finish(old_labels, notices)
}

/// The text of a `{group}` starting at byte `i` of `s`, and the byte after it.
fn group_at(s: &str, i: usize) -> Option<(String, usize)> {
    if s.as_bytes().get(i) != Some(&b'{') {
        return None;
    }
    let close = matching_brace(s, i)?;
    Some((s.get(i + 1..close).unwrap_or("").to_string(), close + 1))
}

fn skip_space(s: &str, mut i: usize) -> usize {
    while s.as_bytes().get(i).is_some_and(|c| c.is_ascii_whitespace()) {
        i += 1;
    }
    i
}

/// The command name at byte `i` (just after a `\`): letters, or one other character.
fn command_name(s: &str, i: usize) -> (String, usize) {
    let rest = s.get(i..).unwrap_or("");
    let n: String = rest.chars().take_while(|c| c.is_ascii_alphabetic()).collect();
    if n.is_empty() {
        let c: String = rest.chars().take(1).collect();
        let l = c.len();
        (c, i + l)
    } else {
        let l = n.len();
        (n, i + l)
    }
}

/// One cell's text with its arrows taken out.
#[derive(Default)]
struct CellText {
    text: String,
    options: Option<String>,
    arrows: Vec<(Vec<String>, Option<String>, Vec<Label>)>,
}

/// Reads an `\arrow` at byte `i` (after its name); returns the byte after it.
fn read_arrow(s: &str, i: usize, shortcut: Option<&str>, cell: &mut CellText, notices: &mut Vec<String>) -> usize {
    let mut j = skip_space(s, i);
    let mut opts = Vec::new();
    if s.as_bytes().get(j) == Some(&b'[')
        && let Some(close) = matching_bracket(s, j)
    {
        opts = split_options(s.get(j + 1..close).unwrap_or(""));
        j = close + 1;
    }
    let mut dir = shortcut.map(str::to_string);
    let mut labels = Vec::new();
    // The older form: `\arrow[options]{direction}[label options]{label}…`.
    let k = skip_space(s, j);
    if dir.is_none()
        && let Some((d, after)) = group_at(s, k)
    {
        dir = Some(d.trim().to_string());
        j = after;
    }
    if dir.is_some() {
        loop {
            let k = skip_space(s, j);
            let mut label_opts = None;
            let mut after = k;
            if s.as_bytes().get(k) == Some(&b'[')
                && let Some(close) = matching_bracket(s, k)
            {
                label_opts = Some(s.get(k + 1..close).unwrap_or("").to_string());
                after = skip_space(s, close + 1);
            }
            let Some((text, next)) = group_at(s, after) else { break };
            let quoted = match &label_opts {
                Some(o) => format!("\"{{{text}}}\"{{{o}}}"),
                None => format!("\"{{{text}}}\""),
            };
            if let Some((l, _, _)) = label_item(&quoted, notices) {
                labels.push(l);
            }
            j = next;
        }
    }
    cell.arrows.push((opts, dir, labels));
    j
}

const SHORTCUTS: &[(&str, &str)] =
    &[("rar", "r"), ("lar", "l"), ("dar", "d"), ("uar", "u"), ("drar", "dr"), ("urar", "ur"), ("dlar", "dl"), ("ular", "ul")];

/// The diagram's body split into rows of cells (with their arrows), and the gaps after rows and
/// before columns.
#[allow(clippy::type_complexity)]
fn cells(body: &str, amp: &str, notices: &mut Vec<String>) -> (Vec<Vec<CellText>>, Vec<(usize, String)>, Vec<(usize, String)>) {
    let mut rows: Vec<Vec<CellText>> = vec![vec![CellText::default()]];
    let mut row_gaps = Vec::new();
    let mut col_gaps = Vec::new();
    let b = body.as_bytes();
    let mut i = 0;
    let mut too_big = false;
    while i < body.len() {
        let Some(&c) = b.get(i) else { break };
        let (r, col) = (rows.len() - 1, rows.last().map_or(0, Vec::len).saturating_sub(1));
        let Some(cell) = rows.last_mut().and_then(|row| row.last_mut()) else { break };
        // `|[options]|` opening a cell.
        if c == b'|'
            && cell.text.trim().is_empty()
            && cell.options.is_none()
            && b.get(i + 1) == Some(&b'[')
            && let Some(close) = matching_bracket(body, i + 1)
            && b.get(close + 1) == Some(&b'|')
        {
            cell.options = Some(body.get(i + 2..close).unwrap_or("").to_string());
            i = close + 2;
            continue;
        }
        if c == b'{' {
            match matching_brace(body, i) {
                Some(close) => {
                    cell.text.push_str(body.get(i..=close).unwrap_or(""));
                    i = close + 1;
                }
                None => {
                    cell.text.push_str(body.get(i..).unwrap_or(""));
                    i = body.len();
                }
            }
            continue;
        }
        let at_amp = body.get(i..).is_some_and(|t| t.starts_with(amp));
        if at_amp && (amp != "&" || c == b'&') {
            i += amp.len();
            if rows.last().is_some_and(|row| row.len() >= MAX_COLS) {
                too_big = true;
            } else if let Some(row) = rows.last_mut() {
                row.push(CellText::default());
            }
            if b.get(i) == Some(&b'[')
                && let Some(close) = matching_bracket(body, i)
            {
                col_gaps.push((col + 1, body.get(i + 1..close).unwrap_or("").to_string()));
                i = close + 1;
            }
            continue;
        }
        if c == b'\\' {
            if b.get(i + 1) == Some(&b'\\') {
                i += 2;
                let k = skip_space(body, i);
                if b.get(k) == Some(&b'[')
                    && let Some(close) = matching_bracket(body, k)
                {
                    row_gaps.push((r, body.get(k + 1..close).unwrap_or("").to_string()));
                    i = close + 1;
                }
                if rows.len() >= MAX_ROWS {
                    too_big = true;
                } else {
                    rows.push(vec![CellText::default()]);
                }
                continue;
            }
            let (name, after) = command_name(body, i + 1);
            if name == "arrow" || name == "ar" {
                i = read_arrow(body, after, None, cell, notices);
                continue;
            }
            if let Some((_, dir)) = SHORTCUTS.iter().find(|(n, _)| *n == name) {
                i = read_arrow(body, after, Some(dir), cell, notices);
                continue;
            }
            cell.text.push_str(body.get(i..after).unwrap_or(""));
            i = after;
            continue;
        }
        let ch = body.get(i..).and_then(|t| t.chars().next()).unwrap_or(' ');
        cell.text.push(ch);
        i += ch.len_utf8();
    }
    if too_big {
        notices.push(format!("Only the first {MAX_ROWS} rows and {MAX_COLS} columns are kept."));
    }
    // A trailing `\\` leaves an empty last row.
    if rows.len() > 1 && rows.last().is_some_and(|r| r.len() == 1 && r.iter().all(|c| c.text.trim().is_empty() && c.arrows.is_empty())) {
        rows.pop();
    }
    (rows, row_gaps, col_gaps)
}

/// An em length from a gap (`2em`, `5mm`), at 10 pt.
fn gap_em(s: &str) -> Option<f64> {
    dim_pt(s).map(|pt| pt / 10.0).filter(|v| v.abs() <= 100.0)
}

fn sep(v: &str) -> Option<Sep> {
    let v = unbrace(v);
    Sep::named(v).or_else(|| gap_em(v).map(Sep::Em))
}

/// The object a cell's options and text make.
fn object(at: At, c: &CellText, notices: &mut Vec<String>) -> (Option<Object>, Option<String>) {
    let label = c.text.trim();
    let mut o = Object::new(at, label);
    let mut name = None;
    if o.label.chars().count() > MAX_TEXT {
        notices.push(format!("An object longer than {MAX_TEXT} characters is cut short."));
        o.label = o.label.chars().take(MAX_TEXT).collect();
    }
    if let Some(opts) = &c.options {
        for opt in split_options(opts) {
            let (k, v) = key_value(&opt);
            match (k.as_str(), v.as_deref()) {
                ("draw", None) if o.frame == Frame::None => o.frame = Frame::Box,
                ("rectangle", None) => {}
                ("circle", None) => o.frame = Frame::Circle,
                ("alias" | "name", Some(n)) => name = Some(unbrace(n).to_string()),
                ("text" | "color", Some(v)) => match color(v) {
                    Some(rgb) => o.color = Some(rgb),
                    None => o.extra.push(opt.clone()),
                },
                (k, None) if color(k).is_some() => o.color = color(k),
                _ => o.extra.push(opt.clone()),
            }
        }
        // `circle` alone draws nothing.
        if o.frame == Frame::Circle && !split_options(opts).iter().any(|x| key_value(x).0 == "draw") {
            o.frame = Frame::None;
            o.extra.push("circle".into());
        }
    }
    let keep = !o.label.is_empty() || o.frame != Frame::None || o.color.is_some() || !o.extra.is_empty();
    (keep.then_some(o), name)
}

/// The environment's options (`[row sep=…]`) read into `d`; returns the cell separator and the
/// body after them.
fn diagram_options<'a>(d: &mut Diagram, inner: &'a str) -> (String, &'a str) {
    let mut amp = "&".to_string();
    let k = skip_space(inner, 0);
    let Some(close) = (inner.as_bytes().get(k) == Some(&b'[')).then(|| matching_bracket(inner, k)).flatten() else { return (amp, inner) };
    for o in split_options(inner.get(k + 1..close).unwrap_or("")) {
        let (key, v) = key_value(&o);
        let value = v.as_deref().and_then(sep);
        match (key.as_str(), value) {
            ("row sep", Some(s)) => d.row_sep = s,
            ("column sep", Some(s)) => d.col_sep = s,
            // One length for both; named sizes differ between rows and columns.
            ("sep", Some(s)) => {
                d.row_sep = s;
                d.col_sep = s;
            }
            ("ampersand replacement", _) if v.as_deref().is_some_and(|v| !v.trim().is_empty()) => amp = v.unwrap_or_default().trim().to_string(),
            _ => d.extra.push(o.clone()),
        }
    }
    (amp, inner.get(close + 1..).unwrap_or(""))
}

fn gaps(into: &mut std::collections::BTreeMap<usize, f64>, gaps: Vec<(usize, String)>, what: &str, notices: &mut Vec<String>) {
    for (k, g) in gaps {
        match gap_em(&g) {
            Some(em) => {
                into.insert(k, em);
            }
            None => notices.push(format!("The {what} gap [{g}] is left out.")),
        }
    }
}

/// The arrows with their endpoints found (cells by position or name, arrows by the name of
/// their label node), and how many point at nothing.
fn resolve_arrows(raws: &[Raw], cell_names: HashMap<String, At>) -> (Vec<Arrow>, usize) {
    let arrow_names: HashMap<&str, usize> = raws.iter().enumerate().filter_map(|(i, r)| r.anchor.as_deref().map(|n| (n, i))).collect();
    let resolve = |r: &Ref| -> Option<End> {
        match r {
            Ref::Cell(at) => (at.row < MAX_ROWS && at.col < MAX_COLS).then_some(End::Cell(*at)),
            Ref::Name(n) => cell_names.get(n).map(|at| End::Cell(*at)).or_else(|| arrow_names.get(n.as_str()).map(|&i| End::Arrow(i))),
        }
    };
    // Arrows that resolve, their old indices mapped to new ones.
    let mut index: Vec<Option<usize>> = vec![None; raws.len()];
    let mut kept: Vec<Arrow> = Vec::new();
    let mut unresolved = 0;
    for (i, raw) in raws.iter().enumerate() {
        let from = raw.from.as_ref().and_then(resolve);
        let to = match &raw.to {
            Some(t) => resolve(t),
            // No target: a loop when asked for, else nothing to draw.
            None if raw.arrow.looping.is_some() => from,
            None => None,
        };
        let (Some(mut from), Some(mut to)) = (from, to) else {
            unresolved += 1;
            continue;
        };
        let mut a = raw.arrow.clone();
        if raw.reversed {
            std::mem::swap(&mut from, &mut to);
            reverse_look(&mut a);
        }
        if from == to && a.looping.is_none() {
            a.looping = Some(Loop::Above);
        }
        (a.from, a.to, a.name) = (from, to, raw.anchor.clone());
        if let Some(slot) = index.get_mut(i) {
            *slot = Some(kept.len());
        }
        kept.push(a);
    }
    // References to arrows use the new indices, wherever the arrows were written.
    for a in &mut kept {
        for e in [&mut a.from, &mut a.to] {
            if let End::Arrow(old) = *e {
                *e = End::Arrow(index.get(old).copied().flatten().unwrap_or(usize::MAX));
            }
        }
    }
    (kept, unresolved)
}

/// Reads a tikz-cd diagram.
pub fn open(src: &str) -> Result<Opened, String> {
    if src.chars().count() > super::super::MAX_SOURCE {
        return Err(format!("the LaTeX is longer than {} characters", super::super::MAX_SOURCE));
    }
    let clean = strip_comments(src);
    let begin = "\\begin{tikzcd}";
    let start = clean.find(begin).ok_or("there is no \\begin{tikzcd}")?;
    let after = start + begin.len();
    let end = clean.get(after..).and_then(|t| t.find("\\end{tikzcd}")).map(|k| after + k).ok_or("\\begin{tikzcd} has no \\end{tikzcd}")?;
    let mut notices = Vec::new();
    if clean.get(end + 1..).is_some_and(|t| t.contains("\\begin{tikzcd}")) {
        notices.push("Only the first diagram is read.".into());
    }
    let mut d = Diagram::default();
    let (amp, inner) = diagram_options(&mut d, clean.get(after..end).unwrap_or(""));
    let (rows, row_gaps, col_gaps) = cells(inner, &amp, &mut notices);
    gaps(&mut d.row_gaps, row_gaps, "row", &mut notices);
    gaps(&mut d.col_gaps, col_gaps, "column", &mut notices);
    let mut cell_names: HashMap<String, At> = HashMap::new();
    let mut raws: Vec<Raw> = Vec::new();
    let mut dropped = 0usize;
    for (r, row) in rows.iter().enumerate() {
        for (c, cell) in row.iter().enumerate() {
            let at = At::new(r, c);
            let (obj, name) = object(at, cell, &mut notices);
            d.objects.extend(obj);
            if let Some(n) = name {
                cell_names.insert(n, at);
            }
            for (opts, dir, labels) in &cell.arrows {
                if raws.len() >= MAX_ARROWS {
                    dropped += 1;
                } else {
                    raws.push(arrow_from(opts, at, dir.as_deref(), labels.clone(), &mut notices));
                }
            }
        }
    }
    if dropped > 0 {
        notices.push(format!("Only the first {MAX_ARROWS} arrows are kept."));
    }
    let (arrows, mut unresolved) = resolve_arrows(&raws, cell_names);
    d.arrows = arrows;
    // An arrow reaching a dropped arrow, or a circle of arrows, goes too.
    let bad: Vec<usize> = (0..d.arrows.len()).filter(|&i| d.level(i).is_none()).collect();
    if !bad.is_empty() {
        let before = d.arrows.len();
        super::edit::remove_arrows(&mut d, &bad);
        unresolved += before - d.arrows.len();
    }
    if unresolved > 0 {
        notices.push(format!("{unresolved} arrow(s) point at nothing DeckCraft can find and are left out."));
    }
    let extra = d.objects.iter().map(|o| o.extra.len()).sum::<usize>()
        + d.arrows.iter().map(|a| a.extra.len() + a.labels.iter().map(|l| l.extra.len()).sum::<usize>()).sum::<usize>()
        + d.extra.len();
    if extra > 0 {
        notices.push(format!("{extra} option(s) DeckCraft doesn't draw are kept in the LaTeX."));
    }
    notices.dedup();
    Ok(Opened { diagram: d, notices })
}

/// An arrow read the other way round keeps its look: what was on its left is on its right.
pub(crate) fn reverse_look(a: &mut Arrow) {
    a.bend = -a.bend;
    a.shift = -a.shift;
    a.shorten = (a.shorten.1, a.shorten.0);
    for l in &mut a.labels {
        l.pos = 1.0 - l.pos;
        l.side = match l.side {
            Side::Left => Side::Right,
            Side::Right => Side::Left,
            s => s,
        };
    }
}
