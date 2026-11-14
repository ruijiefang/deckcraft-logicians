//! Proof trees read from the syntaxes of the LaTeX proof packages:
//!
//! - **ebproof**: `\hypo{A}`, `\infer2[label]{C}` (and `\infer0`…`\infer9`), `\ellipsis{label}{C}`,
//!   with the capitalised `\Hypo`, `\Infer`, `\Ellipsis` and the options `double`, `dashed`,
//!   `dotted`, `no rule`, `rule style=…`, `left label=…`, `right label=…`.
//! - **bussproofs**: `\AxiomC{…}`, `\UnaryInfC` … `\QuinaryInfC` (and the `\AXC`, `\UIC`, `\BIC`,
//!   `\TIC` abbreviations), `\RightLabel`, `\LeftLabel`, `\noLine`, `\doubleLine`, `\dashedLine`,
//!   `\dottedLine` and their `\always…` forms, `\DisplayProof`.
//! - **mathpartir**: `\inferrule{P_1 \\ P_2}{C}` and `\inferrule*[right=…, left=…]{…}{…}`.
//! - **proof.sty**: `\infer[label]{C}{P_1 & P_2}`, `\infer=` (double rule), `\infer*` and
//!   `\deduce`.
//!
//! ebproof and bussproofs build trees on a stack, bottom-up; mathpartir and proof.sty nest them.

use super::scan::{Scan, split_top};

/// How deep proof.sty and mathpartir premises may nest.
const MAX_NEST: usize = 64;
/// Most nodes in one input.
pub(crate) const MAX_NODES: usize = 2_000;

/// The line between premises and conclusion.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rule {
    /// An assumption: a statement with no line above it.
    Leaf,
    Single,
    Double,
    Dashed,
    Dotted,
    /// No line (bussproofs `\noLine`, ebproof `no rule`, proof.sty `\deduce`).
    None,
    /// Vertical dots: several steps left out (ebproof `\ellipsis`, proof.sty `\infer*`).
    Ellipsis,
}

/// A statement or label: LaTeX source, in math mode or in text mode.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stmt {
    pub src: String,
    pub math: bool,
}

impl Stmt {
    fn math(s: &str) -> Self {
        Stmt { src: s.trim().to_string(), math: true }
    }
    fn text(s: &str) -> Self {
        Stmt { src: s.trim().to_string(), math: false }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Proof {
    /// The conclusion, on one or more lines (mathpartir allows several).
    pub concl: Vec<Stmt>,
    pub premises: Vec<Proof>,
    pub rule: Rule,
    pub left: Option<Stmt>,
    pub right: Option<Stmt>,
}

impl Proof {
    fn leaf(s: Stmt) -> Self {
        Proof { concl: vec![s], premises: Vec::new(), rule: Rule::Leaf, left: None, right: None }
    }
}

/// How many steps deep a tree goes, counted without recursion.
pub fn depth(p: &Proof) -> usize {
    let mut deepest = 0;
    let mut stack = vec![(p, 1usize)];
    while let Some((q, d)) = stack.pop() {
        deepest = deepest.max(d);
        stack.extend(q.premises.iter().map(|c| (c, d + 1)));
    }
    deepest
}

/// Whether `src` is written in one of the proof syntaxes.
pub fn is_proof(src: &str) -> bool {
    ["\\hypo", "\\Hypo", "\\infer", "\\Infer", "\\AxiomC", "\\Axiom", "\\AXC", "\\inferrule", "\\deduce", "{prooftree}", "\\ellipsis", "\\Ellipsis"]
        .iter()
        .any(|k| src.contains(k))
}

/// The proof trees in `src`, left to right.
pub fn parse(src: &str) -> Result<Vec<Proof>, String> {
    parse_syntaxes(src).map(|(trees, _)| trees)
}

/// The proof trees in `src` and the syntaxes they were written in (`ebproof`, `bussproofs`,
/// `mathpartir`, `proof.sty`), each named once, in the order met.
pub fn parse_syntaxes(src: &str) -> Result<(Vec<Proof>, Vec<&'static str>), String> {
    let mut p = Parser::new(src);
    p.run(0)?;
    p.finish();
    if p.done.is_empty() {
        return Err("no proof found: start with \\hypo{…}, \\AxiomC{…}, \\inferrule{…}{…} or \\infer{…}{…}".into());
    }
    Ok((p.done, p.syntaxes))
}

/// bussproofs state: labels and line style for the next inference, and the sticky style.
#[derive(Default)]
struct Bp {
    left: Option<Stmt>,
    right: Option<Stmt>,
    once: Option<Rule>,
    always: Option<Rule>,
}

struct Parser {
    s: Scan,
    stack: Vec<Proof>,
    done: Vec<Proof>,
    nodes: usize,
    bp: Bp,
    syntaxes: Vec<&'static str>,
}

impl Parser {
    fn new(src: &str) -> Self {
        Parser { s: Scan::new(src), stack: Vec::new(), done: Vec::new(), nodes: 0, bp: Bp::default(), syntaxes: Vec::new() }
    }

    fn uses(&mut self, syntax: &'static str) {
        if !self.syntaxes.contains(&syntax) {
            self.syntaxes.push(syntax);
        }
    }

    fn count(&mut self) -> Result<(), String> {
        self.nodes += 1;
        if self.nodes > MAX_NODES { Err(format!("more than {MAX_NODES} steps in one proof")) } else { Ok(()) }
    }

    /// Moves every finished tree on the stack to the output (`\end{prooftree}`, `\DisplayProof`).
    fn finish(&mut self) {
        self.done.append(&mut self.stack);
    }

    fn pop(&mut self, n: usize, cmd: &str) -> Result<Vec<Proof>, String> {
        if self.stack.len() < n {
            return Err(format!("\\{cmd} needs {n} premise(s) above it but there are {}", self.stack.len()));
        }
        Ok(self.stack.split_off(self.stack.len() - n))
    }

    fn run(&mut self, depth: usize) -> Result<(), String> {
        while !self.s.at_end() {
            self.s.ws();
            if self.s.peek() != Some('\\') {
                // Text between commands (stray `$`, braces, punctuation) is not part of a tree.
                self.s.bump();
                continue;
            }
            let Some(name) = self.s.command() else { continue };
            self.command(&name, depth)?;
        }
        Ok(())
    }

    fn command(&mut self, name: &str, depth: usize) -> Result<(), String> {
        match name {
            "begin" => {
                let _env = self.s.group();
                let _ = self.s.opt();
            }
            "end" => {
                if self.s.group().is_some_and(|e| e.starts_with("prooftree")) {
                    self.finish();
                }
            }
            "DisplayProof" | "DP" => self.finish(),
            "hypo" | "Hypo" => {
                self.uses("ebproof");
                let opts = self.s.opt().unwrap_or_default();
                let text = self.s.arg().unwrap_or_default();
                self.count()?;
                let mut p = Proof::leaf(Stmt::math(&text));
                apply_options(&mut p, &opts);
                p.rule = Rule::Leaf;
                self.stack.push(p);
            }
            "infer" | "Infer" => self.infer(name, depth)?,
            "ellipsis" | "Ellipsis" => {
                self.uses("ebproof");
                let label = self.s.arg().unwrap_or_default();
                let text = self.s.arg().unwrap_or_default();
                self.count()?;
                let premises = self.pop(1, name)?;
                let right = (!label.trim().is_empty()).then(|| Stmt::text(&label));
                self.stack.push(Proof { concl: vec![Stmt::math(&text)], premises, rule: Rule::Ellipsis, left: None, right });
            }
            "rewrite" | "Rewrite" | "overlay" | "Overlay" => {
                let _ = self.s.arg();
            }
            "delims" | "Delims" => {
                let l = self.s.arg().unwrap_or_default();
                let r = self.s.arg().unwrap_or_default();
                if let Some(c) = self.stack.last_mut().and_then(|p| p.concl.first_mut()) {
                    c.src = format!("{l}{}{r}", c.src);
                }
            }
            "inferrule" => {
                self.uses("mathpartir");
                let p = self.inferrule(depth)?;
                self.stack.push(p);
            }
            "deduce" => {
                let p = self.proof_sty(Rule::None, depth)?;
                self.stack.push(p);
            }
            _ => self.bussproofs(name)?,
        }
        Ok(())
    }

    /// `\infer`: ebproof's when an arity follows its options, else proof.sty's.
    fn infer(&mut self, name: &str, depth: usize) -> Result<(), String> {
        let rule = if self.s.eat('=') {
            Some(Rule::Double)
        } else if self.s.eat('*') {
            Some(Rule::Ellipsis)
        } else {
            None
        };
        let save = self.s.i;
        let opts = self.s.opt();
        if let Some(n) = self.s.digit_next() {
            self.uses("ebproof");
            let label = self.s.opt();
            let text = self.s.arg().unwrap_or_default();
            self.count()?;
            let premises = self.pop(n, name)?;
            let mut p = Proof { concl: vec![Stmt::math(&text)], premises, rule: rule.unwrap_or(Rule::Single), left: None, right: None };
            apply_options(&mut p, opts.as_deref().unwrap_or(""));
            if let Some(l) = label.filter(|l| !l.trim().is_empty()) {
                // `[{a]b}]`: braces keep a `]` inside the label.
                p.right = Some(Stmt::text(unbrace(l.trim())));
            }
            self.stack.push(p);
            return Ok(());
        }
        self.s.i = save;
        let p = self.proof_sty(rule.unwrap_or(Rule::Single), depth)?;
        self.stack.push(p);
        Ok(())
    }

    /// proof.sty's `\infer[label]{conclusion}{premise & premise}` (and `\deduce`).
    fn proof_sty(&mut self, rule: Rule, depth: usize) -> Result<Proof, String> {
        self.uses("proof.sty");
        let label = self.s.opt();
        let concl = self.s.arg().unwrap_or_default();
        let premises = self.s.arg().unwrap_or_default();
        self.count()?;
        let premises = nested(&premises, &["&"], depth)?;
        // proof.sty sets the label in math mode, beside the rule.
        let right = label.filter(|l| !l.trim().is_empty()).map(|l| Stmt::math(&l));
        Ok(Proof { concl: vec![Stmt::math(&concl)], premises, rule, left: None, right })
    }

    /// mathpartir's `\inferrule[name]{premises}{conclusions}` and `\inferrule*[key=value]{…}{…}`.
    fn inferrule(&mut self, depth: usize) -> Result<Proof, String> {
        let star = self.s.eat('*');
        let opts = self.s.opt().unwrap_or_default();
        let premises = self.s.group().unwrap_or_default();
        let concl = self.s.group().unwrap_or_default();
        self.count()?;
        let premises = nested(&premises, &["\\\\", "\\and"], depth)?;
        let concl: Vec<Stmt> = split_top(&concl, &["\\\\"]).iter().filter(|l| !l.trim().is_empty()).map(|l| Stmt::math(l)).collect();
        let mut p = Proof { concl, premises, rule: Rule::Single, left: None, right: None };
        if star {
            for (k, v) in key_values(&opts) {
                match k.as_str() {
                    "right" | "Right" => p.right = Some(Stmt::text(&v)),
                    "left" | "Left" | "lab" | "Lab" => p.left = Some(Stmt::text(&v)),
                    _ => {}
                }
            }
        } else if !opts.trim().is_empty() {
            p.left = Some(Stmt::text(&opts));
        }
        Ok(p)
    }

    fn bussproofs(&mut self, name: &str) -> Result<(), String> {
        let arity = match name {
            "AxiomC" | "AXC" | "Axiom" | "AX" => 0,
            "UnaryInfC" | "UIC" | "UnaryInf" | "UI" => 1,
            "BinaryInfC" | "BIC" | "BinaryInf" | "BI" => 2,
            "TrinaryInfC" | "TIC" | "TrinaryInf" | "TI" => 3,
            "QuaternaryInfC" | "QuaternaryInf" => 4,
            "QuinaryInfC" | "QuinaryInf" => 5,
            "RightLabel" | "RL" => {
                self.bp.right = self.s.arg().map(|l| Stmt::text(&l));
                return Ok(());
            }
            "LeftLabel" | "LL" => {
                self.bp.left = self.s.arg().map(|l| Stmt::text(&l));
                return Ok(());
            }
            _ => {
                if let Some(rule) = line_command(name) {
                    if name.starts_with("always") {
                        self.bp.always = Some(rule);
                    } else {
                        self.bp.once = Some(rule);
                    }
                }
                // Anything else (`\small`, `\EnableBpAbbreviations`, `\ebproofset{…}`) is
                // formatting this layout doesn't need: its group argument is skipped.
                if line_command(name).is_none() && name.chars().all(|c| c.is_ascii_alphabetic()) && self.s.peek() == Some('{') {
                    let _ = self.s.group();
                }
                return Ok(());
            }
        };
        // `\AxiomC{text}` is text mode; `\Axiom$\fCenter A$` is math.
        let stmt = match self.s.dollar() {
            Some(m) => Stmt::math(&m.replace("\\fCenter", "")),
            None => Stmt::text(&self.s.arg().unwrap_or_default()),
        };
        self.count()?;
        self.uses("bussproofs");
        let premises = self.pop(arity, name)?;
        let rule = if arity == 0 { Rule::Leaf } else { self.bp.once.take().or(self.bp.always).unwrap_or(Rule::Single) };
        let (left, right) = if arity == 0 { (None, None) } else { (self.bp.left.take(), self.bp.right.take()) };
        self.stack.push(Proof { concl: vec![stmt], premises, rule, left, right });
        Ok(())
    }
}

/// bussproofs' line-style commands.
fn line_command(name: &str) -> Option<Rule> {
    Some(match name.trim_start_matches("always") {
        "noLine" => Rule::None,
        "singleLine" | "solidLine" => Rule::Single,
        "doubleLine" => Rule::Double,
        "dashedLine" => Rule::Dashed,
        "dottedLine" => Rule::Dotted,
        _ => return None,
    })
}

/// Premises separated by `seps`: nested proofs where they hold one, else statements.
fn nested(src: &str, seps: &[&str], depth: usize) -> Result<Vec<Proof>, String> {
    if depth >= MAX_NEST {
        return Err(format!("proofs nested more than {MAX_NEST} deep"));
    }
    let mut out = Vec::new();
    for piece in split_top(src, seps) {
        let t = piece.trim();
        if t.is_empty() {
            continue;
        }
        if is_proof(t) {
            let mut p = Parser::new(t);
            p.run(depth + 1)?;
            p.finish();
            out.append(&mut p.done);
        } else {
            out.push(Proof::leaf(Stmt::math(t)));
        }
    }
    Ok(out)
}

/// `key=value, flag` options, values unbraced.
fn key_values(opts: &str) -> Vec<(String, String)> {
    split_top(opts, &[","])
        .into_iter()
        .map(|kv| match kv.split_once('=') {
            Some((k, v)) => (k.trim().to_string(), unbrace(v.trim()).to_string()),
            None => (kv.trim().to_string(), String::new()),
        })
        .filter(|(k, _)| !k.is_empty())
        .collect()
}

fn unbrace(s: &str) -> &str {
    s.strip_prefix('{').and_then(|t| t.strip_suffix('}')).unwrap_or(s)
}

/// ebproof options on a step.
fn apply_options(p: &mut Proof, opts: &str) {
    for (k, v) in key_values(opts) {
        let style = match k.as_str() {
            "rule style" => v.as_str(),
            other => other,
        };
        match style {
            "double" => p.rule = Rule::Double,
            "dashed" => p.rule = Rule::Dashed,
            "dotted" => p.rule = Rule::Dotted,
            "no rule" => p.rule = Rule::None,
            "simple" => p.rule = Rule::Single,
            _ => {}
        }
        match k.as_str() {
            "left label" => p.left = Some(Stmt::text(&v)),
            "right label" | "label" => p.right = Some(Stmt::text(&v)),
            // `template=\inserttext` sets the statement as text; ebproof's default is
            // `$\inserttext$`, math.
            "template" => {
                let math = v.contains('$');
                for c in &mut p.concl {
                    c.math = math;
                }
            }
            _ => {}
        }
    }
}
