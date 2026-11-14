//! Commutative diagrams in tikz-cd's syntax: a grid of objects and the arrows between them (or
//! between arrows: 2-cells).
//!
//! [`parse`] reads `\begin{tikzcd} … \end{tikzcd}` as people write it and as quiver exports it;
//! [`write`] writes plain tikz-cd that compiles with `\usepackage{tikz-cd}`; the layout follows
//! tikz-cd's documented spacing (cells padded by 1ex × 0.85ex, rows 1.8em and columns 2.4em
//! apart, labels in script size 0.5ex from their arrow); [`edit`] has the Commutative Diagram
//! editor's changes. Options DeckCraft doesn't draw are kept (`extra`) and written back.

pub mod edit;
mod layout;
mod parse;
#[cfg(test)]
mod tests;
mod write;

pub use layout::Grid;
pub(crate) use layout::layout;
pub use parse::{Opened, is_diagram, open};
pub use write::{preamble, write};

/// The most rows and columns a diagram has.
pub const MAX_ROWS: usize = 64;
pub const MAX_COLS: usize = 64;
/// The most arrows a diagram has.
pub const MAX_ARROWS: usize = 1_000;
/// How deep arrows between arrows nest (a 2-cell is level 1).
pub const MAX_LEVEL: usize = 8;
/// The longest label or object, in characters.
pub const MAX_TEXT: usize = 2_000;

/// A cell of the grid.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct At {
    pub row: usize,
    pub col: usize,
}

impl At {
    pub fn new(row: usize, col: usize) -> Self {
        At { row, col }
    }
}

/// Where an arrow starts or ends: a cell, or the middle of another arrow (its index).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum End {
    Cell(At),
    Arrow(usize),
}

/// A colour as 8-bit RGB.
pub type Rgb = [u8; 3];

/// A frame drawn around an object.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Frame {
    #[default]
    None,
    Box,
    Circle,
}

/// An object: what a cell holds.
#[derive(Clone, Debug, PartialEq)]
pub struct Object {
    pub at: At,
    /// Math, as LaTeX.
    pub label: String,
    pub color: Option<Rgb>,
    pub frame: Frame,
    /// Cell options DeckCraft doesn't draw, written back as they were.
    pub extra: Vec<String>,
}

impl Object {
    pub fn new(at: At, label: &str) -> Self {
        Object { at, label: label.to_string(), color: None, frame: Frame::None, extra: Vec::new() }
    }
}

/// Which side of its arrow a label is on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Side {
    /// Left of the arrow's direction (tikz-cd's default).
    #[default]
    Left,
    /// Right of it (`'`).
    Right,
    /// On the arrow over a patch of background (`description`).
    On,
    /// On the arrow, nothing behind it (`marking`).
    Over,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Label {
    /// Math, as LaTeX.
    pub text: String,
    pub side: Side,
    /// Where along the arrow, from 0 (its start) to 1.
    pub pos: f64,
    pub color: Option<Rgb>,
    pub extra: Vec<String>,
}

impl Label {
    pub fn new(text: &str) -> Self {
        Label { text: text.to_string(), side: Side::Left, pos: 0.5, color: None, extra: Vec::new() }
    }
}

/// What an arrow starts with.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tail {
    #[default]
    None,
    /// An arrowhead pointing back (`←`, `↔`).
    Head,
    /// A monomorphism's tail (`↣`).
    Mono,
    /// A hook curling to the left of the arrow (`↪` for a left-to-right arrow).
    Hook,
    /// A hook curling to the right.
    HookRight,
    /// A bar (`↦`).
    Bar,
}

/// What an arrow's line is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Body {
    #[default]
    Solid,
    Dashed,
    Dotted,
    Squiggly,
    /// Two lines (`⇒`, `=`).
    Double,
    /// No line; its labels are full size, centred on it (`phantom`: ⌟ corners, ⊣ between
    /// adjoints).
    Phantom,
    /// No line; its labels as on a drawn arrow (`draw=none`).
    Invisible,
}

impl Body {
    /// Whether the line is drawn.
    pub fn drawn(self) -> bool {
        !matches!(self, Body::Phantom | Body::Invisible)
    }
}

/// What an arrow ends with.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Head {
    #[default]
    Arrow,
    /// An epimorphism's two heads (`↠`).
    TwoHeads,
    /// Half a head, on the left of the arrow (`⇀`).
    Harpoon,
    /// Half a head, on the right (`⇁`).
    HarpoonRight,
    None,
}

/// Which way an arrow from an object to itself loops.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Loop {
    #[default]
    Above,
    Right,
    Below,
    Left,
}

impl Loop {
    /// The direction it leaves in, in degrees counterclockwise from the page's right.
    pub fn degrees(self) -> f64 {
        match self {
            Loop::Right => 0.0,
            Loop::Above => 90.0,
            Loop::Left => 180.0,
            Loop::Below => 270.0,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Loop::Above => "above",
            Loop::Right => "right",
            Loop::Below => "below",
            Loop::Left => "left",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Arrow {
    pub from: End,
    pub to: End,
    pub labels: Vec<Label>,
    pub tail: Tail,
    pub body: Body,
    pub head: Head,
    /// Degrees, to the left (`bend left`) when positive.
    pub bend: f64,
    /// To the left, in tikz-cd's steps for parallel arrows (0.56ex each).
    pub shift: f64,
    /// How much shorter at its start and its end, in TeX points.
    pub shorten: (f64, f64),
    /// Line width in TeX points; `None` is the diagram's (the math font's rule thickness).
    pub width: Option<f64>,
    pub color: Option<Rgb>,
    /// Drawn over a gap in the arrows it crosses (`crossing over`).
    pub crossing: bool,
    /// For an arrow from an object to itself: which way it loops.
    pub looping: Option<Loop>,
    /// The name 2-cells refer to it by, as written.
    pub name: Option<String>,
    pub extra: Vec<String>,
}

impl Arrow {
    pub fn new(from: End, to: End) -> Self {
        Arrow {
            from,
            to,
            labels: Vec::new(),
            tail: Tail::None,
            body: Body::Solid,
            head: Head::Arrow,
            bend: 0.0,
            shift: 0.0,
            shorten: (0.0, 0.0),
            width: None,
            color: None,
            crossing: false,
            looping: None,
            name: None,
            extra: Vec::new(),
        }
    }

    /// Whether it joins two cells (not an arrow between arrows).
    pub fn is_cell_arrow(&self) -> bool {
        matches!((self.from, self.to), (End::Cell(_), End::Cell(_)))
    }
}

/// tikz-cd's named separations, or a length in ems.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Sep {
    Tiny,
    Small,
    Scriptsize,
    #[default]
    Normal,
    Large,
    Huge,
    Em(f64),
}

impl Sep {
    pub const NAMED: [Sep; 6] = [Sep::Tiny, Sep::Small, Sep::Scriptsize, Sep::Normal, Sep::Large, Sep::Huge];

    /// Space between rows, in ems (tikz-cd's manual).
    pub fn row_em(self) -> f64 {
        match self {
            Sep::Tiny => 0.45,
            Sep::Small => 0.9,
            Sep::Scriptsize => 1.35,
            Sep::Normal => 1.8,
            Sep::Large => 2.7,
            Sep::Huge => 3.6,
            Sep::Em(v) => v,
        }
    }

    /// Space between columns, in ems.
    pub fn col_em(self) -> f64 {
        match self {
            Sep::Tiny => 0.6,
            Sep::Small => 1.2,
            Sep::Scriptsize => 1.8,
            Sep::Normal => 2.4,
            Sep::Large => 3.6,
            Sep::Huge => 4.8,
            Sep::Em(v) => v,
        }
    }

    pub fn name(self) -> Option<&'static str> {
        Some(match self {
            Sep::Tiny => "tiny",
            Sep::Small => "small",
            Sep::Scriptsize => "scriptsize",
            Sep::Normal => "normal",
            Sep::Large => "large",
            Sep::Huge => "huge",
            Sep::Em(_) => return None,
        })
    }

    pub fn named(name: &str) -> Option<Sep> {
        Sep::NAMED.into_iter().find(|s| s.name() == Some(name))
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Diagram {
    pub objects: Vec<Object>,
    pub arrows: Vec<Arrow>,
    pub row_sep: Sep,
    pub col_sep: Sep,
    /// Extra space after a row (`\\[2em]`) or before a column (`&[1em]`), in ems.
    pub row_gaps: std::collections::BTreeMap<usize, f64>,
    pub col_gaps: std::collections::BTreeMap<usize, f64>,
    /// Diagram options DeckCraft doesn't draw.
    pub extra: Vec<String>,
}

impl Diagram {
    /// Rows and columns: up to the last object or cell an arrow reaches.
    pub fn size(&self) -> (usize, usize) {
        let cells = self.objects.iter().map(|o| o.at).chain(self.arrows.iter().flat_map(|a| [a.from, a.to]).filter_map(|e| match e {
            End::Cell(at) => Some(at),
            End::Arrow(_) => None,
        }));
        cells.fold((0, 0), |(r, c), at| (r.max(at.row.saturating_add(1)), c.max(at.col.saturating_add(1))))
    }

    pub fn object(&self, at: At) -> Option<&Object> {
        self.objects.iter().find(|o| o.at == at)
    }

    /// The arrow's level: 0 between cells, one more than the deeper arrow it joins. `None` for a
    /// broken reference or a loop of arrows.
    pub fn level(&self, i: usize) -> Option<usize> {
        let mut seen = 0;
        self.level_inner(i, &mut seen)
    }

    fn level_inner(&self, i: usize, steps: &mut usize) -> Option<usize> {
        *steps += 1;
        if *steps > MAX_ARROWS * (MAX_LEVEL + 1) {
            return None;
        }
        let a = self.arrows.get(i)?;
        let mut level = 0;
        for e in [a.from, a.to] {
            if let End::Arrow(j) = e {
                if j == i {
                    return None;
                }
                let l = self.level_inner(j, steps)?;
                if l >= MAX_LEVEL {
                    return None;
                }
                level = level.max(l + 1);
            }
        }
        Some(level)
    }

    /// Whether some arrow reaches the arrow `i`.
    pub fn is_referenced(&self, i: usize) -> bool {
        self.arrows.iter().any(|a| a.from == End::Arrow(i) || a.to == End::Arrow(i))
    }
}

/// Plain text for an object or label (its LaTeX read as math), for lists and alt text.
pub fn plain(src: &str) -> String {
    let t = super::linear_text(&craft_math::math_latex::parse_latex(src));
    t.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The diagram in words, for screen readers: each arrow as `f: A → B`.
pub fn describe(d: &Diagram) -> String {
    let end = |e: End| match e {
        End::Cell(at) => d.object(at).map(|o| plain(&o.label)).filter(|s| !s.is_empty()).unwrap_or_else(|| "·".into()),
        End::Arrow(i) => d.arrows.get(i).and_then(|a| a.labels.first()).map(|l| plain(&l.text)).unwrap_or_else(|| "an arrow".into()),
    };
    let mut parts: Vec<String> = Vec::new();
    for a in d.arrows.iter().take(200) {
        if !a.body.drawn() {
            continue;
        }
        let name = a.labels.iter().map(|l| plain(&l.text)).find(|s| !s.is_empty());
        let arrow = if a.body == Body::Double { "⇒" } else { "→" };
        let line = format!("{} {arrow} {}", end(a.from), end(a.to));
        parts.push(match name {
            Some(n) => format!("{n}: {line}"),
            None => line,
        });
    }
    if parts.is_empty() {
        let objects: Vec<String> = d.objects.iter().take(200).map(|o| plain(&o.label)).collect();
        return format!("Commutative diagram: {}", objects.join(", "));
    }
    format!("Commutative diagram: {}", parts.join("; "))
}
