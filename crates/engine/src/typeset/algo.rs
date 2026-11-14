//! Pseudo-code in the style of algorithmicx's `algpseudocode`: an algorithm is a tree of lines
//! (statements, and blocks such as `\If` or `\While` holding lines), shown as numbered rows.
//!
//! [`open`] reads `algpseudocode` (and the older `algorithmic` package's uppercase commands), with
//! or without an `algorithm` float around it; [`write`] writes `algpseudocode` inside the float;
//! the layout follows algorithmicx's look as measured from its output (rules, caption, a numbered
//! gutter, 1.5em of indentation per level, comments flush right); [`edit`] holds the Pseudo-code
//! tool's changes. Commands DeckCraft doesn't know are kept as raw lines and written back.

pub mod edit;
mod layout;
mod parse;
#[cfg(test)]
mod tests;
mod write;

pub(crate) use layout::layout;
pub use parse::{Opened, is_algorithm, open};
pub use write::{preamble, write};

/// The most lines (of every depth) an algorithm has.
pub const MAX_LINES: usize = 2_000;
/// How deep blocks nest.
pub const MAX_DEPTH: usize = 64;
/// The longest text of a line, in characters.
pub const MAX_TEXT: usize = 2_000;

/// What a line is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Kind {
    /// `\State text` (numbered) or `\Statex text` (not numbered).
    #[default]
    State,
    /// `\State \Return text`.
    Return,
    /// `\Require text`.
    Require,
    /// `\Ensure text`.
    Ensure,
    /// LaTeX kept as written.
    Raw,
    Procedure,
    Function,
    If,
    For,
    ForAll,
    While,
    /// `\Repeat … \Until{text}`.
    Repeat,
    Loop,
}

impl Kind {
    /// Whether it holds lines.
    pub fn is_block(self) -> bool {
        matches!(self, Kind::Procedure | Kind::Function | Kind::If | Kind::For | Kind::ForAll | Kind::While | Kind::Repeat | Kind::Loop)
    }

    /// Its name in the interface.
    pub fn name(self) -> &'static str {
        match self {
            Kind::State => "Statement",
            Kind::Return => "Return",
            Kind::Require => "Require",
            Kind::Ensure => "Ensure",
            Kind::Raw => "LaTeX",
            Kind::Procedure => "Procedure",
            Kind::Function => "Function",
            Kind::If => "If",
            Kind::For => "For",
            Kind::ForAll => "For all",
            Kind::While => "While",
            Kind::Repeat => "Repeat–until",
            Kind::Loop => "Loop",
        }
    }

    /// Whether it has a text to type: a statement's, a condition, a range, a name.
    pub fn has_text(self) -> bool {
        !matches!(self, Kind::Loop)
    }
}

/// An `\ElsIf{cond}` (or an `\Else`: no condition) of an If, with its lines.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Branch {
    pub cond: Option<String>,
    pub comment: String,
    pub body: Vec<Line>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Line {
    pub kind: Kind,
    /// The statement's text, a condition (If, While, Repeat's `until`), a range (For), the
    /// value returned, or a procedure's name. Text-mode LaTeX with math in `$…$`.
    pub text: String,
    /// A procedure's or function's parameters.
    pub args: String,
    pub comment: String,
    pub label: String,
    /// `false`: `\Statex`, a line without a number.
    pub numbered: bool,
    pub body: Vec<Line>,
    /// An If's `else if` and `else` parts.
    pub branches: Vec<Branch>,
}

impl Line {
    pub fn new(kind: Kind, text: &str) -> Self {
        Line { kind, text: text.to_string(), numbered: true, ..Default::default() }
    }

    pub fn state(text: &str) -> Self {
        Line::new(Kind::State, text)
    }
}

/// The frame around the algorithm.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Frame {
    /// The `algorithm` float's rules, caption on top.
    #[default]
    Ruled,
    /// A box, caption below (`\floatstyle{boxed}`).
    Boxed,
    /// No float: the lines alone.
    None,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Algorithm {
    pub caption: String,
    pub label: String,
    /// The number after "Algorithm" (a slide has no counter).
    pub number: String,
    pub frame: Frame,
    /// 0: no numbers; 1: every line; n: every nth line.
    pub numbering: u32,
    /// Draw the `end …` lines.
    pub show_end: bool,
    /// Keywords renamed with `\algrenewcommand`: (keyword, LaTeX), e.g. ("require", "\textbf{Input:}").
    pub keywords: Vec<(String, String)>,
    /// Lines wider than this many ems wrap (`None`: never).
    pub wrap: Option<f64>,
    pub body: Vec<Line>,
    /// Float options (`[htb]`) and other settings DeckCraft keeps as written.
    pub kept: Vec<String>,
}

impl Default for Algorithm {
    fn default() -> Self {
        Algorithm {
            caption: String::new(),
            label: String::new(),
            number: "1".into(),
            frame: Frame::Ruled,
            numbering: 1,
            show_end: true,
            keywords: Vec::new(),
            wrap: None,
            body: Vec::new(),
            kept: Vec::new(),
        }
    }
}

/// algpseudocode's keywords and how it sets them.
pub const KEYWORDS: &[(&str, &str)] = &[
    ("require", r"\textbf{Require:}"),
    ("ensure", r"\textbf{Ensure:}"),
    ("procedure", r"\textbf{procedure}"),
    ("function", r"\textbf{function}"),
    ("if", r"\textbf{if}"),
    ("then", r"\textbf{then}"),
    ("else", r"\textbf{else}"),
    ("for", r"\textbf{for}"),
    ("forall", r"\textbf{for all}"),
    ("do", r"\textbf{do}"),
    ("while", r"\textbf{while}"),
    ("repeat", r"\textbf{repeat}"),
    ("until", r"\textbf{until}"),
    ("loop", r"\textbf{loop}"),
    ("return", r"\textbf{return}"),
    ("end", r"\textbf{end}"),
];

impl Algorithm {
    /// A keyword as set: renamed, else algpseudocode's.
    pub fn keyword(&self, k: &str) -> String {
        self.keywords
            .iter()
            .find(|(n, _)| n == k)
            .map(|(_, v)| v.clone())
            .or_else(|| KEYWORDS.iter().find(|(n, _)| *n == k).map(|(_, v)| v.to_string()))
            .unwrap_or_default()
    }

    pub fn set_keyword(&mut self, k: &str, v: Option<String>) {
        self.keywords.retain(|(n, _)| n != k);
        if let Some(v) = v
            && KEYWORDS.iter().find(|(n, _)| *n == k).is_none_or(|(_, d)| *d != v)
        {
            self.keywords.push((k.to_string(), v));
        }
    }

    /// How many lines, of every depth.
    pub fn count(&self) -> usize {
        fn n(lines: &[Line]) -> usize {
            lines.iter().map(|l| 1 + n(&l.body) + l.branches.iter().map(|b| 1 + n(&b.body)).sum::<usize>()).sum()
        }
        n(&self.body)
    }
}

/// Where a line is: for each level, (which list: 0 the block's own lines, k the k-th branch's),
/// then its index in that list. The top level's list is 0.
pub type Path = Vec<(usize, usize)>;

/// What a row shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    /// The line itself (a statement, or a block's first row).
    Line,
    /// The k-th branch's row of an If (`else if …` or `else`).
    Branch(usize),
    /// A block's last row (`end while`, `until …`).
    End,
}

/// One row of the algorithm as shown.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub path: Path,
    pub part: Part,
    pub depth: usize,
    /// Its line number (when numbered).
    pub number: Option<usize>,
    /// Drawn only in the editor (an `end` row the algorithm hides).
    pub ghost: bool,
}

fn next(n: &mut usize, numbered: bool) -> Option<usize> {
    numbered.then(|| {
        *n += 1;
        *n
    })
}

/// The rows, in order. `ghosts`: hidden `end` rows too, marked (the editor's).
pub fn rows(a: &Algorithm, ghosts: bool) -> Vec<Row> {
    let mut out = Vec::new();
    let mut n = 0usize;
    #[allow(clippy::too_many_arguments)]
    fn walk(lines: &[Line], prefix: &Path, list: usize, depth: usize, a: &Algorithm, ghosts: bool, n: &mut usize, out: &mut Vec<Row>) {
        if depth > MAX_DEPTH {
            return;
        }
        for (i, l) in lines.iter().enumerate() {
            let mut path = prefix.clone();
            path.push((list, i));
            let numbered = l.numbered && !matches!(l.kind, Kind::Require | Kind::Ensure);
            out.push(Row { path: path.clone(), part: Part::Line, depth, number: next(n, numbered), ghost: false });
            if l.kind.is_block() {
                walk(&l.body, &path, 0, depth + 1, a, ghosts, n, out);
                for (k, b) in l.branches.iter().enumerate() {
                    out.push(Row { path: path.clone(), part: Part::Branch(k), depth, number: next(n, true), ghost: false });
                    walk(&b.body, &path, k + 1, depth + 1, a, ghosts, n, out);
                }
                // A Repeat's `until` is always drawn.
                let shown = a.show_end || l.kind == Kind::Repeat;
                if shown || ghosts {
                    out.push(Row { path: path.clone(), part: Part::End, depth, number: next(n, shown), ghost: !shown });
                }
            }
        }
    }
    walk(&a.body, &Vec::new(), 0, 0, a, ghosts, &mut n, &mut out);
    out
}

/// Plain text of a line's LaTeX, for lists and alt text.
pub fn plain(src: &str) -> String {
    let p = super::runs::text_pieces(&layout::prepare(src));
    p.iter().map(|x| x.text.as_str()).collect::<String>().split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The algorithm in words, for screen readers.
pub fn describe(a: &Algorithm) -> String {
    let head = if a.caption.trim().is_empty() { format!("Algorithm {}", a.number) } else { format!("Algorithm {}: {}", a.number, plain(&a.caption)) };
    let lines: Vec<String> = rows(a, false).iter().take(400).filter_map(|r| layout::row_text(a, r).map(|t| plain(&t))).collect();
    format!("{head}. {}", lines.join("; "))
}
