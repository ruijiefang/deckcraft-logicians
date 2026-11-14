//! Reading pseudo-code: `algpseudocode` (`\State`, `\If{…}`, `\EndIf`…), the older `algorithmic`
//! package's uppercase commands (`\STATE`, `\IF{…}`, `\ENDIF`, `\AND`…), the `algorithm` float's
//! caption and label, keyword renames and hidden `end` lines. Anything else is kept as raw LaTeX.

use super::{Algorithm, Branch, Frame, KEYWORDS, Kind, Line, MAX_DEPTH, MAX_LINES, MAX_TEXT};

/// An algorithm read from LaTeX, and what reading it changed or left out.
#[derive(Clone, Debug, PartialEq)]
pub struct Opened {
    pub algorithm: Algorithm,
    pub notices: Vec<String>,
}

/// Whether `src` is pseudo-code.
pub fn is_algorithm(src: &str) -> bool {
    let s = strip_comments(src);
    s.contains("\\begin{algorithmic}")
}

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

/// The index of the `}` closing the `{` at byte `open`.
fn matching(s: &str, open: usize, o: u8, c: u8) -> Option<usize> {
    let b = s.as_bytes();
    let mut depth = 0usize;
    let mut i = open;
    while let Some(&ch) = b.get(i) {
        if ch == b'\\' {
            i += 2;
            continue;
        }
        if ch == o {
            depth += 1;
        } else if ch == c {
            depth = depth.checked_sub(1)?;
            if depth == 0 {
                return Some(i);
            }
        }
        i += 1;
    }
    None
}

fn skip_space(s: &str, mut i: usize) -> usize {
    while s.as_bytes().get(i).is_some_and(|c| c.is_ascii_whitespace()) {
        i += 1;
    }
    i
}

/// A `{group}` at byte `i` (after spaces): its text and the byte after it.
fn group(s: &str, i: usize) -> Option<(String, usize)> {
    let k = skip_space(s, i);
    if s.as_bytes().get(k) != Some(&b'{') {
        return None;
    }
    let close = matching(s, k, b'{', b'}')?;
    Some((s.get(k + 1..close).unwrap_or("").to_string(), close + 1))
}

/// An `[option]` at byte `i` (after spaces).
fn option(s: &str, i: usize) -> Option<(String, usize)> {
    let k = skip_space(s, i);
    if s.as_bytes().get(k) != Some(&b'[') {
        return None;
    }
    let close = matching(s, k, b'[', b']')?;
    Some((s.get(k + 1..close).unwrap_or("").to_string(), close + 1))
}

/// The command name after the `\` at byte `i`: letters (and a trailing `*`).
fn name_at(s: &str, i: usize) -> (String, usize) {
    let rest = s.get(i + 1..).unwrap_or("");
    let mut n: String = rest.chars().take_while(|c| c.is_ascii_alphabetic()).collect();
    if n.is_empty() {
        n = rest.chars().take(1).collect();
    } else if rest.get(n.len()..).is_some_and(|t| t.starts_with('*')) {
        n.push('*');
    }
    let end = i + 1 + n.len();
    (n, end)
}

/// The commands that start a line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Cmd {
    State,
    Statex,
    Return,
    Print,
    Require,
    Ensure,
    If,
    ElsIf,
    Else,
    EndIf,
    For,
    ForAll,
    EndFor,
    While,
    EndWhile,
    Repeat,
    Until,
    Loop,
    EndLoop,
    Procedure,
    EndProcedure,
    Function,
    EndFunction,
    /// A line-level command kept as written (`\algstore`, `\algtext*`, `\algrenewcommand`…).
    Raw,
}

fn command(name: &str) -> Option<Cmd> {
    use Cmd::*;
    Some(match name {
        "State" | "STATE" => State,
        "Statex" => Statex,
        "RETURN" => Return,
        "PRINT" => Print,
        "Require" | "REQUIRE" => Require,
        "Ensure" | "ENSURE" => Ensure,
        "If" | "IF" => If,
        "ElsIf" | "ELSIF" => ElsIf,
        "Else" | "ELSE" => Else,
        "EndIf" | "ENDIF" => EndIf,
        "For" | "FOR" => For,
        "ForAll" | "FORALL" => ForAll,
        "EndFor" | "ENDFOR" => EndFor,
        "While" | "WHILE" => While,
        "EndWhile" | "ENDWHILE" => EndWhile,
        "Repeat" | "REPEAT" => Repeat,
        "Until" | "UNTIL" => Until,
        "Loop" | "LOOP" => Loop,
        "EndLoop" | "ENDLOOP" => EndLoop,
        "Procedure" => Procedure,
        "EndProcedure" => EndProcedure,
        "Function" => Function,
        "EndFunction" => EndFunction,
        "algstore" | "algstore*" | "algrestore" | "algtext" | "algtext*" | "algrenewcommand" | "algnewcommand" | "algdef" | "algblock"
        | "algblockdefx" | "algrenewtext" | "algnewtext" | "renewcommand" | "newcommand" | "setcounter" | "Statex*" => Raw,
        _ => return None,
    })
}

/// One line of the source: its command, its arguments and the text after them.
#[derive(Debug)]
struct Item {
    cmd: Cmd,
    args: Vec<String>,
    /// The comment of an old `\IF[comment]{…}`.
    opt: Option<String>,
    text: String,
    raw: String,
}

/// How many braced arguments a command takes.
fn arity(c: Cmd) -> usize {
    match c {
        Cmd::If | Cmd::ElsIf | Cmd::For | Cmd::ForAll | Cmd::While | Cmd::Until => 1,
        Cmd::Procedure | Cmd::Function => 2,
        _ => 0,
    }
}

/// Splits the body into lines at the commands that start one.
fn items(body: &str) -> Vec<Item> {
    let b = body.as_bytes();
    let mut out: Vec<Item> = Vec::new();
    let mut i = 0;
    let mut depth = 0usize;
    let mut cur: Option<Item> = None;
    let mut start_text = 0;
    let flush = |cur: &mut Option<Item>, out: &mut Vec<Item>, text: &str| {
        if let Some(mut it) = cur.take() {
            it.text = text.trim().to_string();
            out.push(it);
        } else if !text.trim().is_empty() {
            // Text before any command: a statement (as algorithmic would set it, a line).
            out.push(Item { cmd: Cmd::State, args: Vec::new(), opt: None, text: text.trim().to_string(), raw: String::new() });
        }
    };
    while let Some(&c) = b.get(i) {
        match c {
            b'{' => depth += 1,
            b'}' => depth = depth.saturating_sub(1),
            b'\\' if depth == 0 => {
                let (name, after) = name_at(body, i);
                if let Some(cmd) = command(&name) {
                    flush(&mut cur, &mut out, body.get(start_text..i).unwrap_or(""));
                    let mut j = after;
                    let mut args = Vec::new();
                    let mut opt = None;
                    if cmd == Cmd::Raw {
                        // Its arguments, as written: `{…}` and `[…]` groups and a command name.
                        loop {
                            let k = skip_space(body, j);
                            match b.get(k) {
                                Some(b'{') | Some(b'[') if args.len() < 6 => {
                                    let (o, cl) = if b.get(k) == Some(&b'{') { (b'{', b'}') } else { (b'[', b']') };
                                    let Some(close) = matching(body, k, o, cl) else { break };
                                    args.push(String::new());
                                    j = close + 1;
                                }
                                Some(b'\\') if args.is_empty() && (name.starts_with("alg") || name.ends_with("command")) => {
                                    let (_, a) = name_at(body, k);
                                    if a == k + 1 {
                                        break;
                                    }
                                    j = a;
                                    args.push(String::new());
                                }
                                _ => break,
                            }
                        }
                        let raw = body.get(i..j).unwrap_or("").trim().to_string();
                        out.push(Item { cmd, args: Vec::new(), opt: None, text: String::new(), raw });
                        i = j;
                        start_text = j;
                        continue;
                    }
                    if let Some((o, k)) = option(body, j).filter(|_| matches!(cmd, Cmd::If | Cmd::ElsIf | Cmd::For | Cmd::ForAll | Cmd::While)) {
                        opt = Some(o);
                        j = k;
                    }
                    for _ in 0..arity(cmd) {
                        match group(body, j) {
                            Some((g, k)) => {
                                args.push(g);
                                j = k;
                            }
                            None => args.push(String::new()),
                        }
                    }
                    cur = Some(Item { cmd, args, opt, text: String::new(), raw: String::new() });
                    i = j;
                    start_text = j;
                    continue;
                }
                i = after;
                continue;
            }
            _ => {}
        }
        i += 1;
    }
    flush(&mut cur, &mut out, body.get(start_text..).unwrap_or(""));
    out
}

/// The old `algorithmic` package's words, as algpseudocode writes them.
fn modernize(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while let Some(ch) = text.get(i..).and_then(|r| r.chars().next()) {
        if ch == '\\' {
            let (n, end) = name_at(text, i);
            let word = match n.as_str() {
                "AND" => Some("and"),
                "OR" => Some("or"),
                "XOR" => Some("xor"),
                "NOT" => Some("not"),
                "TRUE" => Some("true"),
                "FALSE" => Some("false"),
                "TO" => Some("to"),
                _ => None,
            };
            match word {
                Some(w) => out.push_str(&format!("\\textbf{{{w}}}")),
                None => out.push_str(text.get(i..end).unwrap_or("")),
            }
            i = end.max(i + 1);
            continue;
        }
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

/// Takes `\Comment{…}` (or the old `\COMMENT{…}`) and `\label{…}` out of a line's text.
fn take(text: &str) -> (String, String, String) {
    let mut t = text.to_string();
    let mut comment = String::new();
    let mut label = String::new();
    for (cmd, slot) in [("\\Comment", 0), ("\\COMMENT", 0), ("\\label", 1)] {
        while let Some(k) = t.find(cmd) {
            let after = k + cmd.len();
            if t.get(after..).and_then(|r| r.chars().next()).is_some_and(|c| c.is_ascii_alphabetic()) {
                break;
            }
            let Some((g, end)) = group(&t, after) else { break };
            if slot == 0 {
                if !comment.is_empty() {
                    comment.push(' ');
                }
                comment.push_str(g.trim());
            } else {
                label = g.trim().to_string();
            }
            t.replace_range(k..end, "");
        }
    }
    (modernize(t.trim()), modernize(&comment), label)
}

fn cut(s: String, notices: &mut Vec<String>) -> String {
    if s.chars().count() > MAX_TEXT {
        notices.push(format!("A line longer than {MAX_TEXT} characters is cut short."));
        s.chars().take(MAX_TEXT).collect()
    } else {
        s
    }
}

/// An open block while the lines are read.
struct Open {
    line: Line,
    /// Where new lines go: 0 the block's own, k its k-th branch.
    into: usize,
}

fn push(stack: &mut [Open], top: &mut Vec<Line>, line: Line) {
    match stack.last_mut() {
        Some(o) => {
            if o.into == 0 {
                o.line.body.push(line);
            } else if let Some(b) = o.line.branches.get_mut(o.into - 1) {
                b.body.push(line);
            }
        }
        None => top.push(line),
    }
}

fn close(stack: &mut Vec<Open>, top: &mut Vec<Line>) {
    if let Some(o) = stack.pop() {
        push(stack, top, o.line);
    }
}

/// The lines of an `algorithmic` body.
fn lines(body: &str, a: &mut Algorithm, notices: &mut Vec<String>) -> Vec<Line> {
    let mut top: Vec<Line> = Vec::new();
    let mut stack: Vec<Open> = Vec::new();
    let mut stray = 0usize;
    for (count, it) in items(body).into_iter().enumerate() {
        if count >= MAX_LINES {
            notices.push(format!("Only the first {MAX_LINES} lines are kept."));
            break;
        }
        let (text, comment, label) = take(&it.text);
        let text = cut(text, notices);
        let comment = match (&it.opt, comment.is_empty()) {
            (Some(o), true) => modernize(o.trim()),
            _ => comment,
        };
        let arg = |k: usize| modernize(it.args.get(k).map(|s| s.trim()).unwrap_or(""));
        let simple = |kind: Kind, text: String, numbered: bool| Line {
            kind,
            text,
            comment: comment.clone(),
            label: label.clone(),
            numbered,
            ..Default::default()
        };
        let block = |kind: Kind, text: String, args: String| Line {
            kind,
            text,
            args,
            comment: comment.clone(),
            label: label.clone(),
            numbered: true,
            ..Default::default()
        };
        match it.cmd {
            Cmd::State | Cmd::Statex => {
                let numbered = it.cmd == Cmd::State;
                let line = match text.strip_prefix("\\Return").filter(|r| !r.starts_with(|c: char| c.is_ascii_alphabetic())) {
                    Some(rest) => simple(Kind::Return, rest.trim().to_string(), numbered),
                    None => simple(Kind::State, text, numbered),
                };
                push(&mut stack, &mut top, line);
            }
            Cmd::Return => push(&mut stack, &mut top, simple(Kind::Return, text, true)),
            Cmd::Print => push(&mut stack, &mut top, simple(Kind::State, format!("\\textbf{{print}} {text}").trim().to_string(), true)),
            Cmd::Require => push(&mut stack, &mut top, simple(Kind::Require, text, true)),
            Cmd::Ensure => push(&mut stack, &mut top, simple(Kind::Ensure, text, true)),
            Cmd::Raw => {
                let raw = it.raw.clone();
                if raw.strip_prefix("\\algtext*").is_some_and(|rest| rest.contains("End")) {
                    a.show_end = false;
                    continue;
                }
                if let Some((k, v)) = rename(&raw) {
                    a.set_keyword(&k, Some(v));
                    continue;
                }
                push(&mut stack, &mut top, Line { kind: Kind::Raw, text: raw, numbered: false, ..Default::default() });
            }
            Cmd::If | Cmd::For | Cmd::ForAll | Cmd::While | Cmd::Repeat | Cmd::Loop | Cmd::Procedure | Cmd::Function => {
                if stack.len() >= MAX_DEPTH {
                    notices.push(format!("Blocks nested more than {MAX_DEPTH} deep are flattened."));
                    push(&mut stack, &mut top, simple(Kind::State, text, true));
                    continue;
                }
                let line = match it.cmd {
                    Cmd::If => block(Kind::If, arg(0), String::new()),
                    Cmd::For => block(Kind::For, arg(0), String::new()),
                    Cmd::ForAll => block(Kind::ForAll, arg(0), String::new()),
                    Cmd::While => block(Kind::While, arg(0), String::new()),
                    Cmd::Repeat => block(Kind::Repeat, String::new(), String::new()),
                    Cmd::Loop => block(Kind::Loop, String::new(), String::new()),
                    Cmd::Procedure => block(Kind::Procedure, arg(0), arg(1)),
                    _ => block(Kind::Function, arg(0), arg(1)),
                };
                stack.push(Open { line, into: 0 });
                // Text after the header (`\While{c} \State x` on one source line) is its first line.
                if !text.is_empty() && it.cmd != Cmd::Repeat && it.cmd != Cmd::Loop {
                    push(&mut stack, &mut top, Line::state(&text));
                }
            }
            Cmd::ElsIf | Cmd::Else => match stack.last_mut().filter(|o| o.line.kind == Kind::If) {
                Some(o) => {
                    let cond = (it.cmd == Cmd::ElsIf).then(|| arg(0));
                    o.line.branches.push(Branch { cond, comment, body: Vec::new() });
                    o.into = o.line.branches.len();
                    if !text.is_empty() {
                        push(&mut stack, &mut top, Line::state(&text));
                    }
                }
                None => stray += 1,
            },
            Cmd::Until => match stack.last_mut().filter(|o| o.line.kind == Kind::Repeat) {
                Some(o) => {
                    o.line.text = arg(0);
                    close(&mut stack, &mut top);
                }
                None => stray += 1,
            },
            Cmd::EndIf | Cmd::EndFor | Cmd::EndWhile | Cmd::EndLoop | Cmd::EndProcedure | Cmd::EndFunction => {
                let want: &[Kind] = match it.cmd {
                    Cmd::EndIf => &[Kind::If],
                    Cmd::EndFor => &[Kind::For, Kind::ForAll],
                    Cmd::EndWhile => &[Kind::While],
                    Cmd::EndLoop => &[Kind::Loop],
                    Cmd::EndProcedure => &[Kind::Procedure],
                    _ => &[Kind::Function],
                };
                // Close blocks left open inside it first.
                match stack.iter().rposition(|o| want.contains(&o.line.kind)) {
                    Some(k) => {
                        let unclosed = stack.len() - 1 - k;
                        if unclosed > 0 {
                            notices.push(format!("{unclosed} block(s) without their end are closed where their outer block ends."));
                        }
                        while stack.len() > k {
                            close(&mut stack, &mut top);
                        }
                    }
                    None => stray += 1,
                }
                if !text.is_empty() {
                    push(&mut stack, &mut top, Line::state(&text));
                }
            }
        }
    }
    if !stack.is_empty() {
        notices.push(format!("{} block(s) without their end are closed at the end.", stack.len()));
        while !stack.is_empty() {
            close(&mut stack, &mut top);
        }
    }
    if stray > 0 {
        notices.push(format!("{stray} end or else line(s) without their block are left out."));
    }
    top
}

/// `\algrenewcommand\algorithmicX{…}` (or `\renewcommand{\algorithmicX}{…}`): the keyword and
/// its LaTeX.
fn rename(raw: &str) -> Option<(String, String)> {
    let rest = raw.strip_prefix("\\algrenewcommand").or_else(|| raw.strip_prefix("\\renewcommand"))?;
    let rest = rest.trim_start();
    let (name, after) = if rest.starts_with('{') {
        let close = matching(rest, 0, b'{', b'}')?;
        (rest.get(1..close)?.trim().to_string(), rest.get(close + 1..)?)
    } else {
        let (n, end) = name_at(rest, 0);
        (format!("\\{n}"), rest.get(end..)?)
    };
    let key = name.strip_prefix("\\algorithmic")?.to_string();
    if !KEYWORDS.iter().any(|(k, _)| *k == key) {
        return None;
    }
    let (value, _) = group(after, 0)?;
    Some((key, value.trim().to_string()))
}

/// `% DeckCraft: number=3, wrap=30` settings that LaTeX doesn't hold.
fn settings(src: &str, a: &mut Algorithm) {
    for line in src.lines() {
        let Some(rest) = line.trim().strip_prefix("% DeckCraft:") else { continue };
        for part in rest.split(',') {
            match part.trim().split_once('=') {
                Some(("number", v)) => a.number = v.trim().chars().take(20).collect(),
                Some(("wrap", v)) => {
                    a.wrap = v.trim().trim_end_matches("em").parse::<f64>().ok().filter(|w| w.is_finite() && *w >= 5.0 && *w <= 400.0)
                }
                _ => {}
            }
        }
    }
}

/// Reads pseudo-code.
pub fn open(src: &str) -> Result<Opened, String> {
    if src.chars().count() > super::super::MAX_SOURCE {
        return Err(format!("the LaTeX is longer than {} characters", super::super::MAX_SOURCE));
    }
    let mut a = Algorithm::default();
    let mut notices = Vec::new();
    settings(src, &mut a);
    let clean = strip_comments(src);
    let begin = "\\begin{algorithmic}";
    let start = clean.find(begin).ok_or("there is no \\begin{algorithmic}")?;
    let after = start + begin.len();
    let end =
        clean.get(after..).and_then(|t| t.find("\\end{algorithmic}")).map(|k| after + k).ok_or("\\begin{algorithmic} has no \\end{algorithmic}")?;
    let mut body_start = after;
    if let Some((n, k)) = option(&clean, after) {
        a.numbering = n.trim().parse::<u32>().unwrap_or(0).min(1000);
        body_start = k;
    } else {
        a.numbering = 0;
    }
    // The float around it: caption, label, options, its style; renames anywhere outside.
    let float = clean.find("\\begin{algorithm}").filter(|&k| k < start);
    a.frame = if float.is_some() { Frame::Ruled } else { Frame::None };
    let outside = format!("{}{}", clean.get(..start).unwrap_or(""), clean.get(end..).unwrap_or(""));
    if let Some(f) = float {
        if let Some((o, _)) = option(&clean, f + "\\begin{algorithm}".len()) {
            a.kept.push(format!("[{o}]"));
        }
        if clean.get(..f).is_some_and(|t| t.contains("\\floatstyle{boxed}")) {
            a.frame = Frame::Boxed;
        }
    }
    if let Some((c, _)) = outside.find("\\caption").and_then(|k| group(&outside, k + "\\caption".len())) {
        a.caption = cut(c.trim().to_string(), &mut notices);
    }
    if let Some((l, _)) = outside.find("\\label").and_then(|k| group(&outside, k + "\\label".len())) {
        a.label = l.trim().to_string();
    }
    let mut k = 0;
    while let Some(off) = outside.get(k..).and_then(|t| t.find("\\algrenewcommand").or_else(|| t.find("\\renewcommand"))) {
        let at = k + off;
        let tail = outside.get(at..).unwrap_or("");
        if let Some((key, v)) = rename(tail) {
            a.set_keyword(&key, Some(v));
        }
        k = at + 2;
    }
    if outside.contains("\\algtext*{End") {
        a.show_end = false;
    }
    let body = clean.get(body_start..end).unwrap_or("");
    let body = lines(body, &mut a, &mut notices);
    a.body = body;
    if clean.get(end + 1..).is_some_and(|t| t.contains("\\begin{algorithmic}")) {
        notices.push("Only the first algorithm is read.".into());
    }
    notices.dedup();
    Ok(Opened { algorithm: a, notices })
}
