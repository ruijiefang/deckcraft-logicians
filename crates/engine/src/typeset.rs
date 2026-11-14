//! LaTeX typeset by DeckCraft itself as text boxes and lines, so it looks the same in DeckCraft,
//! in PDF and in PowerPoint:
//!
//! - **proof trees** in the syntaxes of ebproof, bussproofs, mathpartir and proof.sty
//!   ([`proof`]);
//! - **grids**: `array` and `tabular` with their `|` and `\hline` rules (truth tables), `align`
//!   and `gather` ([`array`]);
//! - **formulas** on one line: italic variables, upright operators and names, TeX's spacing
//!   between atoms, raised and lowered scripts ([`runs`]).
//!
//! Math is read by craft-math, so every named symbol it knows (`\forall`, `\vDash`, `\multimap`…)
//! is available. Office Math equations ([`crate::cmd::latex`]'s `insert.equation`) keep the
//! equation's structure for PowerPoint instead.

mod array;
mod layout;
mod proof;
pub mod proof_edit;
mod runs;
mod scan;
#[cfg(test)]
mod tests;

pub use array::{Grid, GridKind};
pub use layout::{Item, Part, Region, Typeset};
pub use proof::{Proof, Rule, Stmt, is_proof};

use deckcraft_model::text::TextBody;

/// The most source characters typeset at once.
pub const MAX_SOURCE: usize = 20_000;

/// What the source was typeset as.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Formula,
    Grid,
    Proof,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Formula => "formula",
            Kind::Grid => "grid",
            Kind::Proof => "proof",
        }
    }
}

/// The font and size statements are set in.
#[derive(Clone, Debug, PartialEq)]
pub struct Style {
    /// `None`: the theme's font.
    pub font: Option<String>,
    pub size: f64,
}

/// Measures a text body: its width and height in points.
pub type Measure<'a> = &'a dyn Fn(&TextBody) -> (f64, f64);

/// What `src` would be typeset as, without typesetting it.
pub fn kind_of(src: &str) -> Kind {
    let src = src.trim();
    if is_proof(src) {
        Kind::Proof
    } else if array::parse(src).is_some() {
        Kind::Grid
    } else {
        Kind::Formula
    }
}

/// Typesets `src`.
pub fn typeset(src: &str, style: &Style, measure: Measure) -> Result<Typeset, String> {
    let src = src.trim();
    if src.is_empty() {
        return Err("the LaTeX is empty".into());
    }
    if src.chars().count() > MAX_SOURCE {
        return Err(format!("the LaTeX is longer than {MAX_SOURCE} characters"));
    }
    if !(style.size.is_finite() && (1.0..=400.0).contains(&style.size)) {
        return Err("size must be between 1 and 400 pt".into());
    }
    if is_proof(src) {
        let trees = proof::parse(src)?;
        if let Some(d) = trees.iter().map(proof::depth).max().filter(|d| *d > layout::MAX_DEPTH) {
            return Err(format!("the proof is {d} steps deep; at most {} are typeset", layout::MAX_DEPTH));
        }
        return Ok(layout::proofs(&trees, style, measure));
    }
    if let Some(g) = array::parse(src) {
        return Ok(layout::grid(&g, style, measure));
    }
    Ok(layout::formula(strip_math_delims(src), style, measure))
}

/// `src` without one pair of `$$…$$`, `$…$`, `\[…\]`, `\(…\)`, or an `equation`/`displaymath`
/// environment around it.
pub(crate) fn strip_math_delims(src: &str) -> &str {
    let t = src.trim();
    for (open, close) in [("$$", "$$"), ("\\[", "\\]"), ("\\(", "\\)"), ("$", "$")] {
        if let Some(inner) = t.strip_prefix(open).and_then(|r| r.strip_suffix(close))
            && t.len() >= open.len() + close.len()
            // `$a$ and $b$` is two pieces of math, not one.
            && !(open.starts_with('$') && inner.contains('$'))
        {
            return inner.trim();
        }
    }
    for env in ["equation*", "equation", "displaymath", "math"] {
        let (b, e) = (format!("\\begin{{{env}}}"), format!("\\end{{{env}}}"));
        if let Some(inner) = t.strip_prefix(b.as_str()).and_then(|r| r.strip_suffix(e.as_str())) {
            return inner.trim();
        }
    }
    t
}

/// An equation's text on one line (its linear form), for an Office Math equation's run: TeX's
/// spacing, scripts as superscript and subscript characters where Unicode has them.
pub fn linear_text(tree: &[craft_math::MNode]) -> String {
    runs::plain(&runs::tree_pieces(tree, false))
}
