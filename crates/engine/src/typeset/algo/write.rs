//! Writing pseudo-code as `algpseudocode` inside the `algorithm` float. Hidden `end` lines are
//! written as `\algtext*{End…}` and renamed keywords as `\algrenewcommand`, so the LaTeX works with
//! `\usepackage{algorithm}` and `\usepackage{algpseudocode}` alone.

use std::fmt::Write as _;

use super::{Algorithm, Frame, Kind, Line};

/// The preamble lines the algorithm needs.
pub fn preamble(a: &Algorithm) -> Vec<&'static str> {
    let mut v = Vec::new();
    if a.frame != Frame::None {
        v.push(r"\usepackage{algorithm}");
    }
    v.push(r"\usepackage{algpseudocode}");
    v
}

fn tail(l: &Line) -> String {
    let mut s = String::new();
    if !l.comment.trim().is_empty() {
        let _ = write!(s, " \\Comment{{{}}}", l.comment.trim());
    }
    if !l.label.trim().is_empty() {
        let _ = write!(s, " \\label{{{}}}", l.label.trim());
    }
    s
}

fn line(out: &mut String, l: &Line, depth: usize) {
    let pad = "  ".repeat(depth + 1);
    let state = if l.numbered { "\\State" } else { "\\Statex" };
    let text = l.text.trim();
    let spaced = |s: &str| if s.is_empty() { String::new() } else { format!(" {s}") };
    match l.kind {
        Kind::State => {
            let _ = writeln!(out, "{pad}{state}{}{}", spaced(text), tail(l));
        }
        Kind::Return => {
            let _ = writeln!(out, "{pad}{state} \\Return{}{}", spaced(text), tail(l));
        }
        Kind::Require => {
            let _ = writeln!(out, "{pad}\\Require{}{}", spaced(text), tail(l));
        }
        Kind::Ensure => {
            let _ = writeln!(out, "{pad}\\Ensure{}{}", spaced(text), tail(l));
        }
        Kind::Raw => {
            let _ = writeln!(out, "{pad}{text}");
        }
        _ => {
            let (open, end) = match l.kind {
                Kind::Procedure => (format!("\\Procedure{{{text}}}{{{}}}", l.args.trim()), "\\EndProcedure".to_string()),
                Kind::Function => (format!("\\Function{{{text}}}{{{}}}", l.args.trim()), "\\EndFunction".to_string()),
                Kind::If => (format!("\\If{{{text}}}"), "\\EndIf".to_string()),
                Kind::For => (format!("\\For{{{text}}}"), "\\EndFor".to_string()),
                Kind::ForAll => (format!("\\ForAll{{{text}}}"), "\\EndFor".to_string()),
                Kind::While => (format!("\\While{{{text}}}"), "\\EndWhile".to_string()),
                Kind::Repeat => ("\\Repeat".to_string(), format!("\\Until{{{text}}}")),
                _ => ("\\Loop".to_string(), "\\EndLoop".to_string()),
            };
            let _ = writeln!(out, "{pad}{open}{}", tail(l));
            for c in &l.body {
                line(out, c, depth + 1);
            }
            for b in &l.branches {
                let head = match &b.cond {
                    Some(c) => format!("\\ElsIf{{{}}}", c.trim()),
                    None => "\\Else".to_string(),
                };
                let comment = if b.comment.trim().is_empty() { String::new() } else { format!(" \\Comment{{{}}}", b.comment.trim()) };
                let _ = writeln!(out, "{pad}{head}{comment}");
                for c in &b.body {
                    line(out, c, depth + 1);
                }
            }
            let _ = writeln!(out, "{pad}{end}");
        }
    }
}

/// The algorithm as LaTeX.
pub fn write(a: &Algorithm) -> String {
    let mut s = String::new();
    let _ = writeln!(s, "% {} in the preamble", preamble(a).join(" "));
    let mut settings = Vec::new();
    if a.number.trim() != "1" && !a.number.trim().is_empty() {
        settings.push(format!("number={}", a.number.trim()));
    }
    if let Some(w) = a.wrap {
        settings.push(format!("wrap={w}em"));
    }
    if !settings.is_empty() {
        let _ = writeln!(s, "% DeckCraft: {}", settings.join(", "));
    }
    let float = a.frame != Frame::None;
    if a.frame == Frame::Boxed {
        let _ = writeln!(s, "\\floatstyle{{boxed}}\\restylefloat{{algorithm}}");
    }
    if float {
        let opts = a.kept.iter().find(|k| k.starts_with('[')).cloned().unwrap_or_default();
        let _ = writeln!(s, "\\begin{{algorithm}}{opts}");
        let label = if a.label.trim().is_empty() { String::new() } else { format!("\\label{{{}}}", a.label.trim()) };
        let _ = writeln!(s, "\\caption{{{}}}{label}", a.caption.trim());
    }
    for (k, v) in &a.keywords {
        let _ = writeln!(s, "\\algrenewcommand\\algorithmic{k}{{{v}}}");
    }
    if !a.show_end {
        for e in ["EndIf", "EndFor", "EndWhile", "EndLoop", "EndProcedure", "EndFunction"] {
            let _ = writeln!(s, "\\algtext*{{{e}}}");
        }
    }
    let _ = writeln!(s, "\\begin{{algorithmic}}[{}]", a.numbering);
    for l in &a.body {
        line(&mut s, l, 0);
    }
    s.push_str("\\end{algorithmic}");
    if float {
        s.push_str("\n\\end{algorithm}");
    }
    s
}
