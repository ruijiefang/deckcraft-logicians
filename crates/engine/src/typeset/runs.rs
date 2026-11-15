//! Math (and the text around it) set on one line as formatted runs: italic variables, upright
//! numbers, operators and function names, TeX's spacing between atoms (The TeXbook, ch. 18:
//! thin, medium and thick spaces by atom class), raised and lowered scripts, fractions as `a/b`,
//! radicals as `√(…)` and the math alphabets (`\mathbb{N}` → ℕ) as their Unicode letters.

use craft_math::math::{MClass, MScr, MSty, ScriptKind, math_alnum, math_class};
use craft_math::math_latex::parse_latex;
use craft_math::{MNode, MRun};
use deckcraft_model::text::{Align, Caps, Paragraph, Run, RunKind, RunProps};

use super::Style;

/// How deep the flattening follows nested structures (craft-math bounds its trees; this is a
/// second guard so the recursion here is bounded whatever the input).
const MAX_DEPTH: usize = 96;
/// Baseline offsets of scripts, as fractions of the size (PowerPoint's 30 % and −25 %).
const SUP: f64 = 0.3;
const SUB: f64 = -0.25;
const THIN: char = '\u{2009}';
const MEDIUM: char = '\u{205F}';
const THICK: char = '\u{2005}';

/// The formatting of one piece of text.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Look {
    pub italic: bool,
    pub bold: bool,
    pub small_caps: bool,
    pub baseline: f64,
}

/// Text with one formatting.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Piece {
    pub text: String,
    pub look: Look,
}

/// The spacing class of an atom ([`MClass`] plus explicit spaces, which take no automatic space).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Cls {
    Ord,
    Op,
    Bin,
    Rel,
    Open,
    Close,
    Punct,
    Space,
}

impl From<MClass> for Cls {
    fn from(c: MClass) -> Self {
        match c {
            MClass::Op => Cls::Op,
            MClass::Bin => Cls::Bin,
            MClass::Rel => Cls::Rel,
            MClass::Open => Cls::Open,
            MClass::Close => Cls::Close,
            MClass::Punct => Cls::Punct,
            MClass::Ord | MClass::Inner => Cls::Ord,
        }
    }
}

/// One atom: its pieces, its class, and whether it hangs on the atom before (a script), so no
/// space goes in front of it and it doesn't change the class spacing is measured from.
#[derive(Clone, Debug)]
struct Atom {
    pieces: Vec<Piece>,
    cls: Cls,
    attached: bool,
}

/// Flattens a math tree into atoms.
struct Flat {
    atoms: Vec<Atom>,
    /// Script level: TeX leaves out the medium and thick spaces in scripts.
    script: bool,
    depth: usize,
}

impl Flat {
    fn new(script: bool, depth: usize) -> Self {
        Flat { atoms: Vec::new(), script, depth }
    }

    fn push(&mut self, text: impl Into<String>, cls: Cls, look: Look) {
        self.atoms.push(Atom { pieces: vec![Piece { text: text.into(), look }], cls, attached: false });
    }

    /// Append to the last piece (a combining mark after its base).
    fn append(&mut self, c: char, look: Look) {
        match self.atoms.last_mut().and_then(|a| a.pieces.last_mut()) {
            Some(p) => p.text.push(c),
            None => self.push(c.to_string(), Cls::Ord, look),
        }
    }

    fn arg(&mut self, a: &[MNode], look: Look) {
        if self.depth >= MAX_DEPTH {
            return;
        }
        self.depth += 1;
        for n in a {
            self.node(n, look);
        }
        self.depth -= 1;
    }

    /// `a` flattened apart, at `look`, spaced as a script or not.
    fn apart(&self, a: &[MNode], look: Look, script: bool) -> Vec<Atom> {
        let mut f = Flat::new(script, self.depth + 1);
        f.arg(a, look);
        f.atoms
    }

    /// `a` in parentheses when it has more than one atom (`(a+b)/2`).
    fn grouped(&mut self, a: &[MNode], look: Look) {
        let atoms = self.apart(a, look, self.script);
        let many = atoms.iter().filter(|x| x.cls != Cls::Space).count() > 1;
        if many {
            self.push("(", Cls::Open, look);
        }
        self.atoms.extend(atoms);
        if many {
            self.push(")", Cls::Close, look);
        }
    }

    /// A script hung on the atom before, raised or lowered by `shift`.
    fn attach(&mut self, a: &[MNode], look: Look, shift: f64) {
        if a.is_empty() {
            return;
        }
        let inner = Look { baseline: look.baseline + shift, ..look };
        let pieces = spaced(self.apart(a, inner, true), true);
        self.atoms.push(Atom { pieces, cls: Cls::Ord, attached: true });
    }

    fn node(&mut self, n: &MNode, look: Look) {
        match n {
            MNode::Run(r) => self.run(r, look),
            MNode::Frac { num, den, .. } => {
                self.grouped(num, look);
                self.push("/", Cls::Ord, look);
                self.grouped(den, look);
            }
            MNode::Script { kind, base, sub, sup } => {
                if *kind == ScriptKind::Pre {
                    self.attach(sub, look, SUB);
                    self.attach(sup, look, SUP);
                    self.arg(base, look);
                } else {
                    self.arg(base, look);
                    self.attach(sub, look, SUB);
                    self.attach(sup, look, SUP);
                }
            }
            MNode::Rad { deg, deg_hide, e } => self.radical(deg, *deg_hide, e, look),
            MNode::Nary { chr, sub, sup, e, .. } => {
                self.push(chr.to_string(), Cls::Op, look);
                self.attach(sub, look, SUB);
                self.attach(sup, look, SUP);
                self.arg(e, look);
            }
            MNode::Delim { beg, end, sep, elems, .. } => {
                if let Some(b) = beg {
                    self.push(b.to_string(), Cls::Open, look);
                }
                for (i, e) in elems.iter().enumerate() {
                    if i > 0
                        && let Some(s) = sep
                    {
                        self.push(s.to_string(), Cls::from(math_class(*s)), look);
                    }
                    self.arg(e, look);
                }
                if let Some(e) = end {
                    self.push(e.to_string(), Cls::Close, look);
                }
            }
            MNode::Func { name, e } => {
                let pieces = spaced(self.apart(name, look, self.script), self.script);
                self.atoms.push(Atom { pieces, cls: Cls::Op, attached: false });
                self.arg(e, look);
            }
            MNode::Lim { upper, e, lim } => {
                // `\underset{x}{\min}` is one operator, spaced like `\min`.
                if matches!(&e[..], [MNode::Run(r)] if r.sty == Some(MSty::Plain) && r.text.chars().count() > 1 && r.text.chars().all(char::is_alphabetic))
                {
                    let pieces = spaced(self.apart(e, look, self.script), self.script);
                    self.atoms.push(Atom { pieces, cls: Cls::Op, attached: false });
                } else {
                    self.arg(e, look);
                }
                self.attach(lim, look, if *upper { SUP } else { SUB });
            }
            MNode::Acc { chr, e } => {
                self.arg(e, look);
                self.append(*chr, look);
            }
            MNode::Bar { top, e } => {
                let mark = if *top { '\u{305}' } else { '\u{332}' };
                for mut a in self.apart(e, look, self.script) {
                    for p in &mut a.pieces {
                        p.text = p.text.chars().flat_map(|c| if c.is_whitespace() { vec![c] } else { vec![c, mark] }).collect();
                    }
                    self.atoms.push(a);
                }
            }
            MNode::BorderBox { e, .. } | MNode::Boxed { e } | MNode::GroupChr { e, .. } => self.arg(e, look),
            MNode::Phant { show, e, .. } => {
                if *show {
                    self.arg(e, look);
                }
            }
            MNode::Matrix { rows, .. } => {
                for (i, row) in rows.iter().enumerate() {
                    if i > 0 {
                        self.push(";", Cls::Punct, look);
                    }
                    for (j, cell) in row.iter().enumerate() {
                        if j > 0 {
                            self.push("\u{2003}", Cls::Space, look);
                        }
                        self.arg(cell, look);
                    }
                }
            }
            MNode::EqArr { rows } => {
                for (i, row) in rows.iter().enumerate() {
                    if i > 0 {
                        self.push(";", Cls::Punct, look);
                    }
                    let row: Vec<MNode> = row.iter().filter(|n| !matches!(n, MNode::Run(r) if r.text == "&")).cloned().collect();
                    self.arg(&row, look);
                }
            }
        }
    }

    fn radical(&mut self, deg: &[MNode], hidden: bool, e: &[MNode], look: Look) {
        let degree = craft_math::math::to_linear(deg);
        let sign = match degree.trim() {
            _ if hidden || deg.is_empty() => "√",
            "3" => "∛",
            "4" => "∜",
            _ => {
                self.attach(deg, look, SUP);
                "√"
            }
        };
        self.push(sign, Cls::Ord, look);
        self.grouped(e, look);
    }

    fn run(&mut self, r: &MRun, look: Look) {
        if r.nor {
            // `\text{…}`: upright text, as written.
            let look = Look {
                italic: matches!(r.sty, Some(MSty::Italic | MSty::BoldItalic)),
                bold: matches!(r.sty, Some(MSty::Bold | MSty::BoldItalic)),
                ..look
            };
            self.push(r.text.clone(), Cls::Ord, look);
            return;
        }
        let mut cs = r.text.chars().peekable();
        while let Some(c) = cs.next() {
            if c == '\\' {
                // An unknown command kept as text: shown as written, upright.
                let mut name = String::from(c);
                while let Some(&n) = cs.peek().filter(|n| n.is_ascii_alphabetic()) {
                    name.push(n);
                    cs.next();
                }
                self.push(name, Cls::Ord, Look { italic: false, ..look });
            } else if is_combining(c) {
                self.append(c, look);
            } else if is_space(c) {
                self.push(c.to_string(), Cls::Space, look);
            } else if c == '-' {
                // TeX sets a hyphen in math as the minus sign.
                self.push("\u{2212}".to_string(), Cls::from(math_class(c)), look);
            } else {
                let (ch, l) = letter(c, r.sty, r.scr, look);
                self.push(ch.to_string(), Cls::from(math_class(c)), l);
            }
        }
    }
}

fn is_combining(c: char) -> bool {
    matches!(c as u32, 0x300..=0x36F | 0x20D0..=0x20FF)
}

fn is_space(c: char) -> bool {
    matches!(c, ' ' | '\u{A0}' | '\u{2000}'..='\u{200B}' | '\u{205F}')
}

/// Lowercase Greek, which TeX sets italic like Latin letters (capitals stay upright).
fn is_lower_greek(c: char) -> bool {
    matches!(c, 'α'..='ω' | 'ϑ' | 'ϕ' | 'ϖ' | 'ϰ' | 'ϱ' | 'ϵ')
}

/// The character and look of one math character in a run's style and alphabet.
fn letter(c: char, sty: Option<MSty>, scr: MScr, look: Look) -> (char, Look) {
    let bold = matches!(sty, Some(MSty::Bold | MSty::BoldItalic));
    if scr != MScr::Roman {
        // `\mathbb`, `\mathcal`…: the Unicode mathematical alphanumerics (with ℕ, ℝ, ℬ… from the
        // Letterlike Symbols block), set upright.
        return (math_alnum(c, bold, false, scr), Look { italic: false, bold: false, ..look });
    }
    let variable = c.is_ascii_alphabetic() || is_lower_greek(c);
    let italic = match sty {
        Some(MSty::Plain | MSty::Bold) => false,
        Some(MSty::Italic | MSty::BoldItalic) => true,
        None => variable,
    };
    (c, Look { italic, bold: bold || look.bold, ..look })
}

/// TeX's space between atoms of classes `l` and `r`, or `None`. In scripts only the thin spaces
/// stay (the entries The TeXbook puts in parentheses are left out).
fn gap(l: Cls, r: Cls, script: bool) -> Option<char> {
    use Cls::{Bin, Close, Op, Open, Ord, Punct, Rel};
    let (space, display_only) = match (l, r) {
        (Ord | Close, Op) | (Op, Ord | Op) => (THIN, false),
        (Ord | Close, Bin) | (Bin, Ord | Op | Open) => (MEDIUM, true),
        (Ord | Op | Close, Rel) | (Rel, Ord | Op | Open) => (THICK, true),
        (Punct, Ord | Op | Rel | Open | Close | Punct) => (THIN, true),
        _ => return None,
    };
    if script && display_only { None } else { Some(space) }
}

/// A binary operator with nothing to combine on its left or right is an ordinary atom (the
/// minus of `-x`, `(-1)`, `a=-b`). So is ⊥ there: `\bot` (falsum, `\Gamma \vdash \bot`) and
/// `\perp` (a relation, `u \perp v`) are one character, told apart by where it stands.
/// `lead`: an ordinary atom comes before the first.
fn demote_binaries(atoms: &mut [Atom], lead: bool) {
    let core: Vec<usize> = (0..atoms.len()).filter(|&i| atoms.get(i).is_some_and(|a| !a.attached && a.cls != Cls::Space)).collect();
    for (k, &i) in core.iter().enumerate() {
        let before = if lead { Some(Cls::Ord) } else { None };
        let prev = k.checked_sub(1).and_then(|p| core.get(p)).and_then(|&p| atoms.get(p)).map(|a| a.cls).or(before);
        let next = core.get(k + 1).and_then(|&n| atoms.get(n)).map(|a| a.cls);
        if let Some(a) = atoms.get_mut(i)
            && (a.cls == Cls::Bin || (a.cls == Cls::Rel && a.pieces.first().is_some_and(|p| p.text == "⊥")))
            && (matches!(prev, None | Some(Cls::Bin | Cls::Op | Cls::Rel | Cls::Open | Cls::Punct))
                || matches!(next, None | Some(Cls::Rel | Cls::Close | Cls::Punct)))
        {
            a.cls = Cls::Ord;
        }
    }
}

/// The atoms with TeX's spaces between them, as pieces. `lead` sets the first atom as if an
/// ordinary atom came before it (the right column of an `align`, which starts at its `=`).
fn spaced_from(mut atoms: Vec<Atom>, script: bool, lead: bool) -> Vec<Piece> {
    demote_binaries(&mut atoms, lead);
    let mut out: Vec<Piece> = Vec::new();
    let mut prev = if lead { Some(Cls::Ord) } else { None };
    for a in atoms {
        if !a.attached && a.cls != Cls::Space {
            let cls = a.cls;
            if let Some(p) = prev
                && let Some(space) = gap(p, cls, script)
            {
                let look = a.pieces.first().map(|p| Look { italic: false, ..p.look }).unwrap_or_default();
                out.push(Piece { text: space.to_string(), look });
            }
            prev = Some(cls);
        }
        out.extend(a.pieces);
    }
    out
}

fn spaced(atoms: Vec<Atom>, script: bool) -> Vec<Piece> {
    spaced_from(atoms, script, false)
}

/// LaTeX math set on one line.
pub(crate) fn math_pieces(src: &str) -> Vec<Piece> {
    tree_pieces(&parse_latex(src), false)
}

/// LaTeX math set on one line as the right-hand column of an `align` (spaced after a `{}`).
pub(crate) fn math_pieces_lead(src: &str) -> Vec<Piece> {
    let mut f = Flat::new(false, 0);
    f.arg(&parse_latex(src), Look::default());
    spaced_from(f.atoms, false, true)
}

pub(crate) fn tree_pieces(tree: &[MNode], script: bool) -> Vec<Piece> {
    let mut f = Flat::new(script, 0);
    f.arg(tree, Look::default());
    spaced(f.atoms, script)
}

/// Text-mode LaTeX (a rule's label, a `bussproofs` statement, a `tabular` cell): upright text with
/// `$…$` and `\(…\)` math inside, `\textbf`/`\textit`/`\emph`/`\textsc` and the usual escapes.
pub(crate) fn text_pieces(src: &str) -> Vec<Piece> {
    let cs: Vec<char> = src.chars().take(super::MAX_SOURCE).collect();
    let mut out = Vec::new();
    let mut i = 0;
    text_into(&cs, &mut i, Look::default(), &mut out, 0, false);
    merge(out)
}

/// Reads text from `cs[*i]` into `out` up to the end or, in a group, its closing `}`.
fn text_into(cs: &[char], i: &mut usize, look: Look, out: &mut Vec<Piece>, depth: usize, group: bool) {
    let push = |out: &mut Vec<Piece>, s: &str, look: Look| out.push(Piece { text: s.to_string(), look });
    while let Some(&c) = cs.get(*i) {
        *i += 1;
        match c {
            '}' if group => return,
            '}' => {}
            '{' => {
                if depth < MAX_DEPTH {
                    text_into(cs, i, look, out, depth + 1, true);
                }
            }
            '$' => {
                let display = cs.get(*i) == Some(&'$');
                if display {
                    *i += 1;
                }
                let math = until(cs, i, |cs, j| cs.get(j) == Some(&'$'));
                *i += if display { 2 } else { 1 };
                out.extend(math_pieces(&math).into_iter().map(|p| Piece { look: Look { baseline: p.look.baseline + look.baseline, ..p.look }, ..p }));
            }
            '~' => push(out, "\u{A0}", look),
            '-' if cs.get(*i) == Some(&'-') => {
                let em = cs.get(*i + 1) == Some(&'-');
                *i += if em { 2 } else { 1 };
                push(out, if em { "—" } else { "–" }, look);
            }
            c if c.is_whitespace() => {
                if !out.last().is_some_and(|p| p.text.ends_with(' ')) {
                    push(out, " ", look);
                }
                while cs.get(*i).is_some_and(|c| c.is_whitespace()) {
                    *i += 1;
                }
            }
            '\\' => text_command(cs, i, look, out, depth),
            c => push(out, &c.to_string(), look),
        }
    }
}

/// A command in text mode, after its backslash.
fn text_command(cs: &[char], i: &mut usize, look: Look, out: &mut Vec<Piece>, depth: usize) {
    let start = *i;
    while cs.get(*i).is_some_and(char::is_ascii_alphabetic) {
        *i += 1;
    }
    let name: String = cs.get(start..*i).unwrap_or(&[]).iter().collect();
    if name.is_empty() {
        let Some(&c) = cs.get(*i) else { return };
        *i += 1;
        let text = match c {
            '(' => {
                let math = until(cs, i, |cs, j| cs.get(j) == Some(&'\\') && cs.get(j + 1) == Some(&')'));
                *i += 2;
                out.extend(math_pieces(&math));
                return;
            }
            ',' => "\u{2009}".to_string(),
            ' ' | '\\' => " ".to_string(),
            c => c.to_string(),
        };
        out.push(Piece { text, look });
        return;
    }
    // A space after a control word ends it.
    while cs.get(*i).is_some_and(|c| *c == ' ') {
        *i += 1;
    }
    let styled = match name.as_str() {
        "textbf" | "mathbf" | "bfseries" => Some(Look { bold: true, ..look }),
        "textit" | "emph" | "textsl" | "mathit" | "itshape" => Some(Look { italic: true, ..look }),
        "textsc" | "scshape" => Some(Look { small_caps: true, ..look }),
        "textrm" | "textup" | "textnormal" | "textsf" | "texttt" | "text" | "mbox" | "mathrm" | "mathsf" | "mathtt" | "rm" | "upshape" => Some(look),
        _ => None,
    };
    if let Some(l) = styled {
        if cs.get(*i) == Some(&'{') && depth < MAX_DEPTH {
            *i += 1;
            text_into(cs, i, l, out, depth + 1, true);
        }
        return;
    }
    let text = match name.as_str() {
        "quad" => "\u{2003}".to_string(),
        "qquad" => "\u{2003}\u{2003}".to_string(),
        "ldots" | "dots" | "textellipsis" => "…".to_string(),
        "textbackslash" => "\\".to_string(),
        "S" => "§".to_string(),
        n => match craft_math::math_symbols::symbol(n) {
            Some(c) => c.to_string(),
            None => format!("\\{n}"),
        },
    };
    out.push(Piece { text, look });
}

/// The characters from `*i` up to where `end` holds (or the end), leaving `*i` there.
fn until(cs: &[char], i: &mut usize, end: impl Fn(&[char], usize) -> bool) -> String {
    let start = *i;
    while *i < cs.len() && !end(cs, *i) {
        // An escaped delimiter (`\$`) doesn't end it.
        if cs.get(*i) == Some(&'\\') {
            *i += 1;
        }
        *i += 1;
    }
    let stop = (*i).min(cs.len());
    cs.get(start..stop).unwrap_or(&[]).iter().collect()
}

/// Adjacent pieces with the same look joined, except after a space: the text layout keeps a
/// space, digit or symbol in the face of the character before it within a run, so a `¬` or `2`
/// after a space that followed `∨` or `≤` (both drawn by a fallback font) would come out in that
/// fallback font. Starting a run there lets the next atom pick its own face.
pub(crate) fn merge(pieces: Vec<Piece>) -> Vec<Piece> {
    let mut out: Vec<Piece> = Vec::with_capacity(pieces.len());
    for p in pieces {
        if p.text.is_empty() {
            continue;
        }
        match out.last_mut() {
            Some(last) if last.look == p.look && !last.text.ends_with(char::is_whitespace) => last.text.push_str(&p.text),
            _ => out.push(p),
        }
    }
    out
}

/// A paragraph of the pieces in `style`, aligned `align`.
pub(crate) fn paragraph(pieces: Vec<Piece>, style: &Style, align: Align) -> Paragraph {
    let runs = merge(pieces)
        .into_iter()
        .map(|p| Run {
            text: p.text,
            props: RunProps {
                font: style.font.clone(),
                size: Some(style.size),
                italic: Some(p.look.italic),
                bold: p.look.bold.then_some(true),
                caps: p.look.small_caps.then_some(Caps::Small),
                baseline: (p.look.baseline != 0.0).then_some(p.look.baseline),
                // Math isn't prose: the spelling check skips it (PowerPoint's `noProof`).
                no_proof: Some(true),
                ..Default::default()
            },
            kind: RunKind::Text,
        })
        .collect();
    let mut para = Paragraph { runs, ..Default::default() };
    para.props.align = Some(align);
    para.end_props = RunProps { font: style.font.clone(), size: Some(style.size), ..Default::default() };
    para
}

/// One line of plain text for the pieces, for an equation's linear form: scripts as Unicode
/// superscript and subscript characters where all of them have one, else `^(…)` and `_(…)`.
pub(crate) fn plain(pieces: &[Piece]) -> String {
    let mut s = String::new();
    let mut i = 0;
    while let Some(p) = pieces.get(i) {
        let b = p.look.baseline;
        if b == 0.0 {
            s.push_str(&p.text);
            i += 1;
            continue;
        }
        // The run of pieces on the same side of the baseline.
        let mut text = String::new();
        while let Some(q) = pieces.get(i).filter(|q| (q.look.baseline > 0.0) == (b > 0.0) && q.look.baseline != 0.0) {
            text.push_str(&q.text);
            i += 1;
        }
        let mapped: Option<String> = text.chars().filter(|c| !is_space(*c)).map(|c| if b > 0.0 { superscript(c) } else { subscript(c) }).collect();
        match mapped {
            Some(m) => s.push_str(&m),
            None => {
                s.push(if b > 0.0 { '^' } else { '_' });
                let one = text.chars().filter(|c| !is_space(*c)).count() == 1;
                if one {
                    s.push_str(text.trim());
                } else {
                    s.push('(');
                    s.push_str(text.trim());
                    s.push(')');
                }
            }
        }
    }
    s
}

fn superscript(c: char) -> Option<char> {
    let digits = "⁰¹²³⁴⁵⁶⁷⁸⁹";
    if let Some(d) = c.to_digit(10) {
        return digits.chars().nth(d as usize);
    }
    Some(match c {
        '+' => '⁺',
        '-' | '−' => '⁻',
        '=' => '⁼',
        '(' => '⁽',
        ')' => '⁾',
        'n' => 'ⁿ',
        'i' => 'ⁱ',
        '′' | '″' | '‴' | '*' | '∗' | '†' => c,
        _ => return None,
    })
}

fn subscript(c: char) -> Option<char> {
    let digits = "₀₁₂₃₄₅₆₇₈₉";
    if let Some(d) = c.to_digit(10) {
        return digits.chars().nth(d as usize);
    }
    Some(match c {
        '+' => '₊',
        '-' | '−' => '₋',
        '=' => '₌',
        '(' => '₍',
        ')' => '₎',
        'a' => 'ₐ',
        'e' => 'ₑ',
        'o' => 'ₒ',
        'x' => 'ₓ',
        'h' => 'ₕ',
        'k' => 'ₖ',
        'l' => 'ₗ',
        'm' => 'ₘ',
        'n' => 'ₙ',
        'p' => 'ₚ',
        's' => 'ₛ',
        't' => 'ₜ',
        'i' => 'ᵢ',
        'j' => 'ⱼ',
        'r' => 'ᵣ',
        'u' => 'ᵤ',
        'v' => 'ᵥ',
        _ => return None,
    })
}
