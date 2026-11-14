//! Writing a diagram as plain tikz-cd: each arrow after the object it starts at, with a
//! direction (`\arrow[dr, "f"]`), arrows between arrows at the end by name. It compiles with
//! `\usepackage{tikz-cd}` (squiggly arrows also want `decorations.pathmorphing`, which a
//! comment at the top says).

use std::fmt::Write as _;

use super::parse::COLORS;
use super::{Arrow, At, Body, Diagram, End, Frame, Head, Label, Rgb, Sep, Side, Tail};

/// A number as short as it can be written (three decimals at most).
pub(crate) fn num(v: f64) -> String {
    let v = if v.is_finite() { (v * 1000.0).round() / 1000.0 } else { 0.0 };
    let s = format!("{v:.3}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" { "0".into() } else { s.to_string() }
}

fn color(c: Rgb) -> String {
    match COLORS.iter().find(|(_, v)| *v == c) {
        Some((n, _)) => (*n).to_string(),
        None => format!("{{rgb,255:red,{};green,{};blue,{}}}", c[0], c[1], c[2]),
    }
}

/// Label or object text that has to be braced inside an option list.
fn quoted(text: &str) -> String {
    let needs = text.contains([',', '=', '"', ']', '[']) || text.starts_with(' ') || text.ends_with(' ');
    if needs { format!("\"{{{text}}}\"") } else { format!("\"{text}\"") }
}

fn position(pos: f64) -> Option<String> {
    let names = [(0.0, "at start"), (0.125, "very near start"), (0.25, "near start"), (0.75, "near end"), (0.875, "very near end"), (1.0, "at end")];
    if (pos - 0.5).abs() < 1e-9 {
        return None;
    }
    Some(names.iter().find(|(p, _)| (p - pos).abs() < 1e-9).map_or_else(|| format!("pos={}", num(pos)), |(_, n)| (*n).to_string()))
}

fn label(l: &Label) -> String {
    let mut opts: Vec<String> = Vec::new();
    match l.side {
        Side::Left => {}
        Side::Right => opts.push("swap".into()),
        Side::On => opts.push("description".into()),
        Side::Over => opts.push("marking".into()),
    }
    opts.extend(position(l.pos));
    if let Some(c) = l.color {
        opts.push(format!("text={}", color(c)));
    }
    opts.extend(l.extra.iter().cloned());
    let text = quoted(&l.text);
    match opts.as_slice() {
        [] => text,
        [one] if one == "swap" => format!("{text}'"),
        _ => format!("{text}{{{}}}", opts.join(", ")),
    }
}

/// The tikz-cd styles for an arrow's tips and line, in an order where later ones only change
/// what they name (a style setting both tips comes first).
fn styles(a: &Arrow) -> Vec<&'static str> {
    let mut out = Vec::new();
    match a.body {
        Body::Phantom => return vec!["phantom"],
        Body::Double => {
            let base = match (a.tail, a.head) {
                (Tail::Head, Head::None) => "Leftarrow",
                (Tail::Head, _) => "Leftrightarrow",
                (Tail::Bar, _) => "Mapsto",
                (_, Head::None) => "equal",
                _ => "Rightarrow",
            };
            out.push(base);
            match a.tail {
                Tail::Mono => out.push("tail"),
                Tail::Hook => out.push("hook"),
                Tail::HookRight => out.push("hook'"),
                _ => {}
            }
            match a.head {
                Head::TwoHeads => out.push("two heads"),
                Head::Harpoon => out.push("harpoon"),
                Head::HarpoonRight => out.push("harpoon'"),
                Head::None if base == "Mapsto" => out.push("no head"),
                _ => {}
            }
            return out;
        }
        Body::Dashed => out.push("dashed"),
        Body::Dotted => out.push("dotted"),
        Body::Squiggly => out.push("squiggly"),
        Body::Solid | Body::Invisible => {}
    }
    match a.tail {
        Tail::None => {}
        Tail::Head => out.push("leftrightarrow"),
        Tail::Mono => out.push("tail"),
        Tail::Hook => out.push("hook"),
        Tail::HookRight => out.push("hook'"),
        Tail::Bar => out.push("maps to"),
    }
    match a.head {
        Head::Arrow => {}
        Head::TwoHeads => out.push("two heads"),
        Head::Harpoon => out.push("harpoon"),
        Head::HarpoonRight => out.push("harpoon'"),
        Head::None => out.push("no head"),
    }
    out
}

/// The direction letters from `a` to `b`: down/up, then right/left.
fn direction(a: At, b: At) -> String {
    let mut s = String::new();
    let (dr, dc) = (b.row as i64 - a.row as i64, b.col as i64 - a.col as i64);
    let rows = if dr >= 0 { "d" } else { "u" };
    let cols = if dc >= 0 { "r" } else { "l" };
    for _ in 0..dr.unsigned_abs() {
        s.push_str(rows);
    }
    for _ in 0..dc.unsigned_abs() {
        s.push_str(cols);
    }
    s
}

fn cell(at: At) -> String {
    format!("{}-{}", at.row + 1, at.col + 1)
}

/// The names arrows are reached by: each arrow some arrow reaches gets one.
fn names(d: &Diagram) -> Vec<Option<String>> {
    let mut used: Vec<String> = d.arrows.iter().filter_map(|a| a.name.clone()).collect();
    (0..d.arrows.len())
        .map(|i| {
            if !d.is_referenced(i) {
                return None;
            }
            if let Some(n) = d.arrows.get(i).and_then(|a| a.name.clone()) {
                return Some(n);
            }
            // Not only r/l/d/u letters, which `from=` would read as a direction.
            let mut k = i;
            loop {
                let n = format!("a{k}");
                if !used.contains(&n) {
                    used.push(n.clone());
                    return Some(n);
                }
                k += 1;
            }
        })
        .collect()
}

fn options(a: &Arrow, i: usize, names: &[Option<String>], placed_at: Option<At>) -> String {
    let mut o: Vec<String> = Vec::new();
    let end = |e: End| -> String {
        match e {
            End::Cell(at) => cell(at),
            End::Arrow(j) => names.get(j).cloned().flatten().unwrap_or_default(),
        }
    };
    match (placed_at, a.from, a.to) {
        (Some(_), End::Cell(f), End::Cell(t)) if f == t => {
            o.push(format!("loop {}", a.looping.unwrap_or_default().name()));
        }
        (Some(_), End::Cell(f), End::Cell(t)) => o.push(direction(f, t)),
        _ => {}
    }
    o.extend(a.labels.iter().map(label));
    if let Some(n) = names.get(i).cloned().flatten() {
        o.push(format!("\"\"{{name={n}, anchor=center, inner sep=0}}"));
    }
    o.extend(styles(a).into_iter().map(str::to_string));
    // A loop's own shape decides its curve; bending or shifting it gives TeX no direction.
    let looped = a.from == a.to;
    if a.bend.abs() > 1e-9 && !looped {
        let side = if a.bend > 0.0 { "left" } else { "right" };
        o.push(if (a.bend.abs() - 30.0).abs() < 1e-9 { format!("bend {side}") } else { format!("bend {side}={}", num(a.bend.abs())) });
    }
    if a.shift.abs() > 1e-9 && !looped {
        let side = if a.shift > 0.0 { "left" } else { "right" };
        o.push(if (a.shift.abs() - 1.0).abs() < 1e-9 { format!("shift {side}") } else { format!("shift {side}={}", num(a.shift.abs())) });
    }
    if a.shorten.0 > 1e-9 {
        o.push(format!("shorten <={}pt", num(a.shorten.0)));
    }
    if a.shorten.1 > 1e-9 {
        o.push(format!("shorten >={}pt", num(a.shorten.1)));
    }
    if let Some(w) = a.width {
        o.push(format!("line width={}pt", num(w)));
    }
    if let Some(c) = a.color {
        o.push(format!("draw={}", color(c)));
    }
    if a.body == Body::Invisible {
        o.push("draw=none".into());
    }
    if a.crossing {
        o.push("crossing over".into());
    }
    o.extend(a.extra.iter().cloned());
    if placed_at.is_none() {
        o.push(format!("from={}", end(a.from)));
        o.push(format!("to={}", end(a.to)));
    }
    o.join(", ")
}

fn object_text(d: &Diagram, at: At) -> String {
    let Some(o) = d.object(at) else { return String::new() };
    let mut opts: Vec<String> = Vec::new();
    match o.frame {
        Frame::None => {}
        Frame::Box => opts.push("draw".into()),
        Frame::Circle => opts.extend(["draw".to_string(), "circle".to_string()]),
    }
    if let Some(c) = o.color {
        opts.push(format!("text={}", color(c)));
    }
    opts.extend(o.extra.iter().cloned());
    if opts.is_empty() { o.label.clone() } else { format!("|[{}]| {}", opts.join(", "), o.label) }
}

fn sep_value(s: Sep) -> String {
    s.name().map_or_else(|| format!("{}em", num(s.row_em())), str::to_string)
}

/// The preamble lines the diagram needs besides `\usepackage{tikz-cd}`.
pub fn preamble(d: &Diagram) -> Vec<&'static str> {
    if d.arrows.iter().any(|a| a.body == Body::Squiggly) { vec![r"\usetikzlibrary{decorations.pathmorphing}"] } else { Vec::new() }
}

/// The diagram as tikz-cd.
pub fn write(d: &Diagram) -> String {
    let mut s = String::new();
    for p in preamble(d) {
        let _ = writeln!(s, "% {p} in the preamble draws the squiggly arrows");
    }
    let mut opts: Vec<String> = Vec::new();
    if d.row_sep != Sep::Normal {
        opts.push(format!("row sep={}", sep_value(d.row_sep)));
    }
    if d.col_sep != Sep::Normal {
        opts.push(format!("column sep={}", sep_value(d.col_sep)));
    }
    opts.extend(d.extra.iter().cloned());
    s.push_str("\\begin{tikzcd}");
    if !opts.is_empty() {
        let _ = write!(s, "[{}]", opts.join(", "));
    }
    s.push('\n');
    let names = names(d);
    let (rows, cols) = d.size();
    // Arrows are drawn in the order they are written (which matters for `crossing over`), so
    // they stay in order: each after the object it starts at while they come in the cells'
    // order, the rest at the very end, by `from` and `to`.
    let mut at_cell: std::collections::BTreeMap<At, Vec<usize>> = std::collections::BTreeMap::new();
    let mut late: Vec<usize> = Vec::new();
    let mut last: Option<At> = None;
    for (i, a) in d.arrows.iter().enumerate() {
        match (a.from, a.to) {
            (End::Cell(f), End::Cell(_)) if late.is_empty() && last.is_none_or(|l| f >= l) => {
                at_cell.entry(f).or_default().push(i);
                last = Some(f);
            }
            _ => late.push(i),
        }
    }
    for r in 0..rows.max(1) {
        let mut parts: Vec<String> = Vec::new();
        let last = (0..cols).rev().find(|&c| {
            let at = At::new(r, c);
            d.object(at).is_some() || at_cell.contains_key(&at)
        });
        if let Some(last) = last {
            for c in 0..=last {
                let at = At::new(r, c);
                let mut text = object_text(d, at);
                for &i in at_cell.get(&at).into_iter().flatten() {
                    if let Some(a) = d.arrows.get(i) {
                        if !text.is_empty() {
                            text.push(' ');
                        }
                        let _ = write!(text, "\\arrow[{}]", options(a, i, &names, Some(at)));
                    }
                }
                let gap = d.col_gaps.get(&c).filter(|_| c > 0).map(|g| format!("[{}em]", num(*g))).unwrap_or_default();
                parts.push(if c == 0 { text } else { format!("&{gap} {text}") });
            }
        }
        let mut line = format!("\t{}", parts.join(" ").trim_end());
        if r + 1 < rows {
            line.push_str(" \\\\");
            if let Some(g) = d.row_gaps.get(&r) {
                let _ = write!(line, "[{}em]", num(*g));
            }
        }
        // A blank line would end TeX's paragraph inside display math.
        if !line.trim().is_empty() {
            s.push_str(line.trim_end());
            s.push('\n');
        }
    }
    for i in late {
        if let Some(a) = d.arrows.get(i) {
            let _ = writeln!(s, "\t\\arrow[{}]", options(a, i, &names, None));
        }
    }
    s.push_str("\\end{tikzcd}");
    s
}
