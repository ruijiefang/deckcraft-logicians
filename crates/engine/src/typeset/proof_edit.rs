//! Proof trees as the Proof Tree builder edits them, and written back as ebproof.
//!
//! A node is named by its path: the index of its tree in the forest, then the index of each
//! premise on the way up. Every edit is a pure function of the forest that returns where the
//! selection goes; edits that would pass the typesetter's limits are refused with a message.
//!
//! The writer emits ebproof 2 (the `prooftree` environment, lowercase `\hypo`, `\infer`,
//! `\ellipsis`), one environment per tree (ebproof wants exactly one tree in each), checked
//! against the package: `rule style=double|dashed|no rule`, `left label={…}`, a braced arity
//! from ten premises on, a braced right label when it holds `]`, and `template=\inserttext` for a
//! statement set as text. ebproof has no dotted rule; one is written dashed.

use super::layout::MAX_DEPTH;
use super::proof::{self, MAX_NODES, Proof, Rule, Stmt};
use super::runs;

/// Where a node is: its tree, then a premise index per level.
pub type Path = Vec<usize>;

/// The most premises one step may have.
pub const MAX_PREMISES: usize = 99;

/// What a step is, as the builder shows it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// An assumption: no premises, no rule.
    Hypothesis,
    /// A rule with no premises above it (`\infer0`).
    Axiom,
    Inference,
}

/// Where a moved branch goes relative to the node it is dropped on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Place {
    /// Above it, as its last premise.
    Premise,
    /// Beside it on the left, under the same step.
    Before,
    /// Beside it on the right.
    After,
}

/// An empty hypothesis.
pub fn blank() -> Proof {
    Proof { concl: vec![Stmt { src: String::new(), math: true }], premises: Vec::new(), rule: Rule::Leaf, left: None, right: None }
}

pub fn kind(p: &Proof) -> Kind {
    if !p.premises.is_empty() {
        Kind::Inference
    } else if p.rule == Rule::Leaf {
        Kind::Hypothesis
    } else {
        Kind::Axiom
    }
}

pub fn node<'a>(forest: &'a [Proof], path: &[usize]) -> Option<&'a Proof> {
    let (&first, rest) = path.split_first()?;
    let mut cur = forest.get(first)?;
    for &i in rest {
        cur = cur.premises.get(i)?;
    }
    Some(cur)
}

pub fn node_mut<'a>(forest: &'a mut [Proof], path: &[usize]) -> Option<&'a mut Proof> {
    let (&first, rest) = path.split_first()?;
    let mut cur = forest.get_mut(first)?;
    for &i in rest {
        cur = cur.premises.get_mut(i)?;
    }
    Some(cur)
}

/// The list holding the node at `path` (the forest, or its parent's premises) and its index.
fn slot<'a>(forest: &'a mut Vec<Proof>, path: &[usize]) -> Option<(&'a mut Vec<Proof>, usize)> {
    let (&last, parent) = path.split_last()?;
    if parent.is_empty() {
        return (last < forest.len()).then_some((forest, last));
    }
    let p = node_mut(forest, parent)?;
    (last < p.premises.len()).then_some((&mut p.premises, last))
}

/// Every node in the forest.
pub fn count(forest: &[Proof]) -> usize {
    let mut n = 0;
    let mut stack: Vec<&Proof> = forest.iter().collect();
    while let Some(p) = stack.pop() {
        n += 1;
        stack.extend(p.premises.iter());
    }
    n
}

/// How deep the deepest tree goes.
pub fn depth(forest: &[Proof]) -> usize {
    forest.iter().map(proof::depth).max().unwrap_or(0)
}

/// Refuses an edit that adds `adding` nodes or reaches `deepest` levels past the limits.
fn room(forest: &[Proof], adding: usize, deepest: usize) -> Result<(), String> {
    if count(forest) + adding > MAX_NODES {
        return Err(format!("a proof can have at most {MAX_NODES} steps"));
    }
    if deepest > MAX_DEPTH {
        return Err(format!("a proof can be at most {MAX_DEPTH} steps deep"));
    }
    Ok(())
}

fn missing() -> String {
    "that step is no longer in the proof".to_string()
}

/// `path` with its last index replaced by `i`.
fn with_last(path: &[usize], i: usize) -> Path {
    let mut p = path.to_vec();
    if let Some(last) = p.last_mut() {
        *last = i;
    }
    p
}

/// A new empty premise above the step (as its last premise). A hypothesis becomes an inference.
pub fn add_premise(forest: &mut [Proof], path: &[usize]) -> Result<Path, String> {
    room(forest, 1, path.len() + 1)?;
    let n = node_mut(forest, path).ok_or_else(missing)?;
    if n.premises.len() >= MAX_PREMISES {
        return Err(format!("a step can have at most {MAX_PREMISES} premises"));
    }
    n.premises.push(blank());
    if n.rule == Rule::Leaf {
        n.rule = Rule::Single;
    }
    let mut p = path.to_vec();
    p.push(n.premises.len() - 1);
    Ok(p)
}

/// A new empty node beside this one (`right` or left): another premise of the same step, or
/// another tree beside a root.
pub fn add_sibling(forest: &mut Vec<Proof>, path: &[usize], right: bool) -> Result<Path, String> {
    room(forest, 1, path.len())?;
    let nested = path.len() > 1;
    let (list, i) = slot(forest, path).ok_or_else(missing)?;
    if nested && list.len() >= MAX_PREMISES {
        return Err(format!("a step can have at most {MAX_PREMISES} premises"));
    }
    let at = if right { i + 1 } else { i };
    list.insert(at, blank());
    Ok(with_last(path, at))
}

/// A new step below this one, with this one as its only premise. The new step is selected.
pub fn insert_below(forest: &mut Vec<Proof>, path: &[usize]) -> Result<Path, String> {
    let height = node(forest, path).map(proof::depth).ok_or_else(missing)?;
    room(forest, 1, path.len() + height)?;
    let (list, i) = slot(forest, path).ok_or_else(missing)?;
    let at = list.get_mut(i).ok_or_else(missing)?;
    let old = std::mem::replace(at, blank());
    at.premises.push(old);
    at.rule = Rule::Single;
    Ok(path.to_vec())
}

/// Removes the step and puts its premises in its place (a removed root leaves its premises as
/// trees of their own). Returns the new selection: the first promoted premise, else the step it
/// stood on, else a neighbouring tree; `None` when the forest is empty.
pub fn remove_step(forest: &mut Vec<Proof>, path: &[usize]) -> Result<Option<Path>, String> {
    let (list, i) = slot(forest, path).ok_or_else(missing)?;
    let removed = list.remove(i);
    let promoted = removed.premises.len();
    list.splice(i..i, removed.premises);
    if promoted > 0 {
        return Ok(Some(path.to_vec()));
    }
    Ok(after_removal(forest, path))
}

/// Removes the step and everything above it.
pub fn delete_branch(forest: &mut Vec<Proof>, path: &[usize]) -> Result<Option<Path>, String> {
    let (list, i) = slot(forest, path).ok_or_else(missing)?;
    list.remove(i);
    Ok(after_removal(forest, path))
}

/// Where the selection goes once the node at `path` is gone: its parent, else a tree near it.
fn after_removal(forest: &[Proof], path: &[usize]) -> Option<Path> {
    if path.len() > 1 {
        return Some(path.get(..path.len() - 1).unwrap_or_default().to_vec());
    }
    let i = path.first().copied().unwrap_or(0);
    (!forest.is_empty()).then(|| vec![i.min(forest.len() - 1)])
}

/// Swaps the node with its neighbour on the `right` (or left). At the end, nothing moves.
pub fn move_sibling(forest: &mut Vec<Proof>, path: &[usize], right: bool) -> Result<Path, String> {
    let (list, i) = slot(forest, path).ok_or_else(missing)?;
    let j = if right { i + 1 } else { i.wrapping_sub(1) };
    if j >= list.len() {
        return Ok(path.to_vec());
    }
    list.swap(i, j);
    Ok(with_last(path, j))
}

/// Moves the branch at `from` to `place` relative to the node at `to`. A branch can't move into
/// itself. Returns the branch's new path.
pub fn move_branch(forest: &mut Vec<Proof>, from: &[usize], to: &[usize], place: Place) -> Result<Path, String> {
    if to.starts_with(from) {
        return if to == from { Ok(from.to_vec()) } else { Err("a branch can't move onto a step above it".into()) };
    }
    let mut work = forest.clone();
    let (list, i) = slot(&mut work, from).ok_or_else(missing)?;
    let branch = list.remove(i);
    // Taking the branch out shifts later siblings in that list (and everything above them).
    let mut to = to.to_vec();
    let k = from.len() - 1;
    if to.len() > k
        && to.get(..k) == from.get(..k)
        && to.get(k) > from.get(k)
        && let Some(x) = to.get_mut(k)
    {
        *x -= 1;
    }
    let new = match place {
        Place::Premise => {
            let target = node_mut(&mut work, &to).ok_or_else(missing)?;
            if target.premises.len() >= MAX_PREMISES {
                return Err(format!("a step can have at most {MAX_PREMISES} premises"));
            }
            target.premises.push(branch);
            if target.rule == Rule::Leaf {
                target.rule = Rule::Single;
            }
            let mut p = to.clone();
            p.push(target.premises.len() - 1);
            p
        }
        Place::Before | Place::After => {
            let (list, j) = slot(&mut work, &to).ok_or_else(missing)?;
            let at = if place == Place::After { j + 1 } else { j };
            list.insert(at, branch);
            with_last(&to, at)
        }
    };
    if depth(&work) > MAX_DEPTH {
        return Err(format!("a proof can be at most {MAX_DEPTH} steps deep"));
    }
    *forest = work;
    Ok(new)
}

/// Whether a node is an empty hypothesis with no labels: a slot to fill.
fn is_empty_slot(p: &Proof) -> bool {
    p.premises.is_empty() && p.rule == Rule::Leaf && p.concl.iter().all(|c| c.src.trim().is_empty()) && p.left.is_none() && p.right.is_none()
}

/// Puts `branch` (a copied branch or a template) at `path`: in place of an empty slot, else as
/// the step's last premise; with no `path`, as a tree of its own.
pub fn paste(forest: &mut Vec<Proof>, path: Option<&[usize]>, branch: Proof) -> Result<Path, String> {
    let adding = count(std::slice::from_ref(&branch));
    let height = proof::depth(&branch);
    let Some(path) = path else {
        room(forest, adding, height)?;
        forest.push(branch);
        return Ok(vec![forest.len() - 1]);
    };
    let target = node(forest, path).ok_or_else(missing)?;
    if is_empty_slot(target) {
        room(forest, adding.saturating_sub(1), path.len() - 1 + height)?;
        if let Some(n) = node_mut(forest, path) {
            *n = branch;
        }
        return Ok(path.to_vec());
    }
    room(forest, adding, path.len() + height)?;
    let n = node_mut(forest, path).ok_or_else(missing)?;
    if n.premises.len() >= MAX_PREMISES {
        return Err(format!("a step can have at most {MAX_PREMISES} premises"));
    }
    n.premises.push(branch);
    if n.rule == Rule::Leaf {
        n.rule = Rule::Single;
    }
    let mut p = path.to_vec();
    p.push(n.premises.len() - 1);
    Ok(p)
}

/// Makes the step a hypothesis or an axiom (only while it has no premises; an inference stays
/// one until its premises go) or an inference (giving a premise-less step its first premise).
/// Returns the selection: the step, or its new premise.
pub fn set_kind(forest: &mut [Proof], path: &[usize], to: Kind) -> Result<Path, String> {
    let n = node_mut(forest, path).ok_or_else(missing)?;
    match to {
        Kind::Hypothesis | Kind::Axiom if !n.premises.is_empty() => Err("remove its premises first".into()),
        Kind::Hypothesis => {
            n.rule = Rule::Leaf;
            n.left = None;
            n.right = None;
            Ok(path.to_vec())
        }
        Kind::Axiom => {
            if n.rule == Rule::Leaf {
                n.rule = Rule::Single;
            }
            Ok(path.to_vec())
        }
        Kind::Inference if n.premises.is_empty() => add_premise(forest, path),
        Kind::Inference => Ok(path.to_vec()),
    }
}

/// The step above (its first premise).
pub fn first_premise(forest: &[Proof], path: &[usize]) -> Option<Path> {
    let n = node(forest, path)?;
    (!n.premises.is_empty()).then(|| {
        let mut p = path.to_vec();
        p.push(0);
        p
    })
}

/// The step below (the one this is a premise of).
pub fn parent(path: &[usize]) -> Option<Path> {
    (path.len() > 1).then(|| path.get(..path.len() - 1).unwrap_or_default().to_vec())
}

/// The neighbour on the `right` (or left): a sibling premise, or the next tree.
pub fn neighbour(forest: &[Proof], path: &[usize], right: bool) -> Option<Path> {
    let (&last, up) = path.split_last()?;
    let len = if up.is_empty() { forest.len() } else { node(forest, up)?.premises.len() };
    let j = if right { last + 1 } else { last.checked_sub(1)? };
    (j < len).then(|| with_last(path, j))
}

/// Every node, conclusions first (pre-order), with its depth from its tree's root (0).
pub fn steps(forest: &[Proof]) -> Vec<(Path, usize)> {
    let mut out = Vec::new();
    let mut stack: Vec<(Path, &Proof)> = forest.iter().enumerate().rev().map(|(i, p)| (vec![i], p)).collect();
    while let Some((path, p)) = stack.pop() {
        for (i, q) in p.premises.iter().enumerate().rev() {
            let mut child = path.clone();
            child.push(i);
            stack.push((child, q));
        }
        let d = path.len() - 1;
        out.push((path, d));
    }
    out
}

/// A statement or label on one line of plain text, for lists and messages.
pub fn plain(s: &Stmt) -> String {
    let pieces = if s.math { runs::math_pieces(&s.src) } else { runs::text_pieces(&s.src) };
    runs::plain(&pieces).trim().to_string()
}

/// A step on one line: its statement, or "(empty)".
pub fn describe(p: &Proof) -> String {
    let t: Vec<String> = p.concl.iter().map(plain).filter(|t| !t.is_empty()).collect();
    if t.is_empty() { "(empty)".to_string() } else { t.join(" ") }
}

// ---------------------------------------------------------------------------------------------
// Writing ebproof.

/// The forest as ebproof source.
pub fn to_ebproof(forest: &[Proof]) -> String {
    write(forest, None)
}

/// The forest as ebproof source for the builder's preview: empty statements show as □ so they
/// can be seen and clicked.
pub fn to_ebproof_preview(forest: &[Proof]) -> String {
    write(forest, Some(r"\square"))
}

fn write(forest: &[Proof], placeholder: Option<&str>) -> String {
    let mut out = String::new();
    for (i, tree) in forest.iter().enumerate() {
        if i > 0 {
            out.push_str("\\qquad\n");
        }
        out.push_str("\\begin{prooftree}\n");
        tree_into(&mut out, tree, 1, placeholder);
        out.push_str("\\end{prooftree}\n");
    }
    out
}

/// A statement's source and whether it needs ebproof's text template.
fn statement(s: &Stmt, placeholder: Option<&str>) -> (String, bool) {
    let src = s.src.trim();
    if src.is_empty() {
        return (placeholder.unwrap_or("").to_string(), false);
    }
    if s.math {
        return (src.to_string(), false);
    }
    // Text that is all math (bussproofs' `$A$`) is written as math.
    match whole_math(src) {
        Some(inner) => (inner.trim().to_string(), false),
        None => (src.to_string(), true),
    }
}

/// The math inside `$…$` or `\(…\)` when that is all of `s`.
fn whole_math(s: &str) -> Option<&str> {
    if let Some(inner) = s.strip_prefix('$').and_then(|r| r.strip_suffix('$'))
        && !inner.contains('$')
        && !inner.is_empty()
    {
        return Some(inner);
    }
    s.strip_prefix("\\(").and_then(|r| r.strip_suffix("\\)")).filter(|i| !i.contains("\\(") && !i.contains("\\)"))
}

/// A label for ebproof, which sets labels as text: math labels (proof.sty's) go in `$…$`.
fn label(s: &Stmt) -> String {
    let src = s.src.trim();
    if s.math && !src.is_empty() { format!("${src}$") } else { src.to_string() }
}

fn rule_option(p: &Proof) -> Option<&'static str> {
    match p.rule {
        Rule::Double => Some("rule style=double"),
        // ebproof has no dotted rule; an ellipsis needs exactly one premise.
        Rule::Dashed | Rule::Dotted => Some("rule style=dashed"),
        Rule::Ellipsis if p.premises.len() != 1 => Some("rule style=dashed"),
        Rule::None => Some("rule style=no rule"),
        Rule::Single | Rule::Leaf | Rule::Ellipsis => None,
    }
}

fn tree_into(out: &mut String, p: &Proof, depth: usize, placeholder: Option<&str>) {
    for q in &p.premises {
        tree_into(out, q, depth + 1, placeholder);
    }
    let indent = "  ".repeat(depth);
    // Several conclusion lines (mathpartir's) become one: ebproof sets one per step.
    let lines: Vec<(String, bool)> = p.concl.iter().map(|c| statement(c, placeholder)).collect();
    let text = lines.iter().map(|(t, _)| t.as_str()).filter(|t| !t.is_empty()).collect::<Vec<_>>().join(r" \quad ");
    let text = if text.is_empty() { placeholder.unwrap_or("").to_string() } else { text };
    let as_text = lines.iter().any(|(_, t)| *t);
    let mut opts: Vec<String> = Vec::new();
    if as_text {
        opts.push(r"template=\inserttext".to_string());
    }
    let n = p.premises.len();
    if n == 0 && p.rule == Rule::Leaf {
        out.push_str(&format!("{indent}\\hypo{}{{{text}}}\n", bracket(&opts)));
        return;
    }
    let right = p.right.as_ref().map(label).filter(|l| !l.is_empty());
    if n == 1 && p.rule == Rule::Ellipsis {
        out.push_str(&format!("{indent}\\ellipsis{{{}}}{{{text}}}\n", right.unwrap_or_default()));
        return;
    }
    if let Some(r) = rule_option(p) {
        opts.push(r.to_string());
    }
    if let Some(l) = p.left.as_ref().map(label).filter(|l| !l.is_empty()) {
        opts.push(format!("left label={{{l}}}"));
    }
    let arity = if n <= 9 { n.to_string() } else { format!("{{{n}}}") };
    let right = match right {
        Some(r) if r.contains(']') => format!("[{{{r}}}]"),
        Some(r) => format!("[{r}]"),
        None => String::new(),
    };
    out.push_str(&format!("{indent}\\infer{}{arity}{right}{{{text}}}\n", bracket(&opts)));
}

fn bracket(opts: &[String]) -> String {
    if opts.is_empty() { String::new() } else { format!("[{}]", opts.join(", ")) }
}

// ---------------------------------------------------------------------------------------------
// Opening a proof in the builder.

/// A proof ready for the builder, and what writing it as ebproof changes.
#[derive(Clone, Debug, PartialEq)]
pub struct Opened {
    pub forest: Vec<Proof>,
    pub notices: Vec<String>,
}

/// Reads `src` for the builder. It opens only when writing the trees back as ebproof draws the
/// same proof (statements, labels, rules and structure); what the rewrite changes otherwise is
/// listed in the notices. Anything else is an error, and the LaTeX is edited as text instead.
pub fn open(src: &str) -> Result<Opened, String> {
    let (forest, syntaxes) = proof::parse_syntaxes(src)?;
    if depth(&forest) > MAX_DEPTH {
        return Err(format!("the proof is more than {MAX_DEPTH} steps deep"));
    }
    let mut notices = Vec::new();
    let others: Vec<&str> = syntaxes.iter().copied().filter(|s| *s != "ebproof").collect();
    if !others.is_empty() {
        notices.push(format!("Saving rewrites this {} proof in ebproof syntax.", others.join(" and ")));
    }
    let mut dropped: Vec<&str> = ["\\rewrite", "\\overlay", "\\Rewrite", "\\Overlay"].into_iter().filter(|c| src.contains(c)).collect();
    dropped.dedup();
    if !dropped.is_empty() {
        notices.push(format!("{} is left out.", dropped.join(" and ")));
    }
    if src.lines().any(|l| l.trim_start().starts_with('%') || l.contains(" %")) {
        notices.push("Comments in the LaTeX are left out.".into());
    }
    let mut dotted = false;
    let mut lines = false;
    let mut stack: Vec<&Proof> = forest.iter().collect();
    while let Some(p) = stack.pop() {
        dotted |= p.rule == Rule::Dotted;
        lines |= p.concl.len() > 1;
        stack.extend(p.premises.iter());
    }
    if dotted {
        notices.push("ebproof has no dotted rule: dotted rules become dashed.".into());
    }
    if lines {
        notices.push("A conclusion on several lines becomes one line.".into());
    }
    let written = to_ebproof(&forest);
    let again = proof::parse(&written)?;
    // The rewrite must draw what the source drew, apart from the changes named above.
    if !same_look(&normalized(&forest), &again) {
        return Err("this proof can't be edited as a tree without changing how it looks; edit its LaTeX instead".into());
    }
    Ok(Opened { forest: again, notices })
}

/// The forest with the changes the ebproof writer makes on purpose: dotted rules dashed, math
/// conclusion lines joined into one.
fn normalized(forest: &[Proof]) -> Vec<Proof> {
    let mut out = forest.to_vec();
    let mut stack: Vec<&mut Proof> = out.iter_mut().collect();
    while let Some(p) = stack.pop() {
        if p.rule == Rule::Dotted {
            p.rule = Rule::Dashed;
        }
        if p.concl.len() > 1 && p.concl.iter().all(|c| c.math) {
            let joined = p.concl.iter().map(|c| c.src.trim()).filter(|s| !s.is_empty()).collect::<Vec<_>>().join(r" \quad ");
            p.concl = vec![Stmt { src: joined, math: true }];
        }
        stack.extend(p.premises.iter_mut());
    }
    out
}

/// Whether two forests draw the same: structure, rules and every statement and label.
pub fn same_look(a: &[Proof], b: &[Proof]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| same_node(x, y))
}

fn same_node(a: &Proof, b: &Proof) -> bool {
    let rule = |p: &Proof| match p.rule {
        Rule::Ellipsis if p.premises.len() != 1 => Rule::Dashed,
        r => r,
    };
    let look = |s: &Stmt| runs::merge(if s.math { runs::math_pieces(&s.src) } else { runs::text_pieces(&s.src) });
    let opt = |x: &Option<Stmt>, y: &Option<Stmt>| match (x, y) {
        (None, None) => true,
        (Some(x), Some(y)) => look(x) == look(y),
        (Some(s), None) | (None, Some(s)) => s.src.trim().is_empty(),
    };
    rule(a) == rule(b)
        && a.concl.len() == b.concl.len()
        && a.concl.iter().zip(&b.concl).all(|(x, y)| look(x) == look(y))
        && opt(&a.left, &b.left)
        && opt(&a.right, &b.right)
        && a.premises.len() == b.premises.len()
        && a.premises.iter().zip(&b.premises).all(|(x, y)| same_node(x, y))
}

/// What would break the written LaTeX: unbalanced braces in a statement or label. Each problem
/// names its step.
pub fn problems(forest: &[Proof]) -> Vec<(Path, String)> {
    let mut out = Vec::new();
    for (path, _) in steps(forest) {
        let Some(p) = node(forest, &path) else { continue };
        let what = describe(p);
        if p.concl.iter().any(|c| !balanced(&c.src)) {
            out.push((path.clone(), format!("“{what}”: its statement has unbalanced braces")));
        }
        for (name, l) in [("left", &p.left), ("right", &p.right)] {
            if l.as_ref().is_some_and(|l| !balanced(&l.src)) {
                out.push((path.clone(), format!("“{what}”: its {name} label has unbalanced braces")));
            }
        }
    }
    out
}

/// How many statements are still empty.
pub fn empty_statements(forest: &[Proof]) -> usize {
    steps(forest).iter().filter_map(|(p, _)| node(forest, p)).filter(|p| p.concl.iter().all(|c| c.src.trim().is_empty())).count()
}

/// Whether `{` and `}` pair up (escaped `\{` and `\}` don't count).
pub fn balanced(s: &str) -> bool {
    let mut depth: i64 = 0;
    let mut cs = s.chars();
    while let Some(c) = cs.next() {
        match c {
            '\\' => {
                cs.next();
            }
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth < 0 {
                    return false;
                }
            }
            _ => {}
        }
    }
    depth == 0
}

// ---------------------------------------------------------------------------------------------
// Templates: common natural-deduction and sequent rules, schematic in Γ, A, B, C.

/// (name, ebproof source).
pub const TEMPLATES: &[(&str, &str)] = &[
    ("Axiom", r"\infer0[Ax]{\Gamma, A \vdash A}"),
    ("∧ introduction", r"\hypo{\Gamma \vdash A}\hypo{\Gamma \vdash B}\infer2[$\wedge$I]{\Gamma \vdash A \wedge B}"),
    ("∧ elimination (left)", r"\hypo{\Gamma \vdash A \wedge B}\infer1[$\wedge$E$_1$]{\Gamma \vdash A}"),
    ("∧ elimination (right)", r"\hypo{\Gamma \vdash A \wedge B}\infer1[$\wedge$E$_2$]{\Gamma \vdash B}"),
    ("∨ introduction (left)", r"\hypo{\Gamma \vdash A}\infer1[$\vee$I$_1$]{\Gamma \vdash A \vee B}"),
    ("∨ introduction (right)", r"\hypo{\Gamma \vdash B}\infer1[$\vee$I$_2$]{\Gamma \vdash A \vee B}"),
    ("∨ elimination", r"\hypo{\Gamma \vdash A \vee B}\hypo{\Gamma, A \vdash C}\hypo{\Gamma, B \vdash C}\infer3[$\vee$E]{\Gamma \vdash C}"),
    ("→ introduction", r"\hypo{\Gamma, A \vdash B}\infer1[$\to$I]{\Gamma \vdash A \to B}"),
    ("→ elimination (modus ponens)", r"\hypo{\Gamma \vdash A \to B}\hypo{\Gamma \vdash A}\infer2[$\to$E]{\Gamma \vdash B}"),
    ("¬ introduction", r"\hypo{\Gamma, A \vdash \bot}\infer1[$\neg$I]{\Gamma \vdash \neg A}"),
    ("¬ elimination", r"\hypo{\Gamma \vdash A}\hypo{\Gamma \vdash \neg A}\infer2[$\neg$E]{\Gamma \vdash \bot}"),
    ("⊥ elimination", r"\hypo{\Gamma \vdash \bot}\infer1[$\bot$E]{\Gamma \vdash A}"),
    ("∀ introduction", r"\hypo{\Gamma \vdash A}\infer1[$\forall$I]{\Gamma \vdash \forall x\, A}"),
    ("∀ elimination", r"\hypo{\Gamma \vdash \forall x\, A}\infer1[$\forall$E]{\Gamma \vdash A[t/x]}"),
    ("∃ introduction", r"\hypo{\Gamma \vdash A[t/x]}\infer1[$\exists$I]{\Gamma \vdash \exists x\, A}"),
    ("∃ elimination", r"\hypo{\Gamma \vdash \exists x\, A}\hypo{\Gamma, A \vdash C}\infer2[$\exists$E]{\Gamma \vdash C}"),
    ("Cut", r"\hypo{\Gamma \vdash A}\hypo{\Gamma, A \vdash C}\infer2[cut]{\Gamma \vdash C}"),
    ("Weakening", r"\hypo{\Gamma \vdash C}\infer1[W]{\Gamma, A \vdash C}"),
    ("Derivation (⋮)", r"\hypo{\Gamma}\ellipsis{}{\Gamma \vdash A}"),
];

/// The template called `name`, as a tree.
pub fn template(name: &str) -> Option<Proof> {
    let (_, src) = TEMPLATES.iter().find(|(n, _)| *n == name)?;
    proof::parse(src).ok()?.into_iter().next()
}

#[cfg(test)]
#[path = "proof_edit_tests.rs"]
mod tests;
