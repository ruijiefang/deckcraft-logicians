//! The Pseudo-code tool's changes, each a function of the algorithm. Rows are a view of the tree
//! ([`super::rows`]); [`slot_above`] and [`slot_below`] say where a row's new line goes:
//!
//! | Row | above | below |
//! |---|---|---|
//! | statement | before it, in its block | after it, in its block |
//! | block (`if …`) | before the block | first line inside |
//! | `else if` / `else` | last line of the part before | first line of its part |
//! | `end` | last line inside the block | after the block |

use super::{Algorithm, Branch, Kind, Line, MAX_DEPTH, MAX_LINES, Part, Path, Row};

/// Where a new line goes: in `parent`'s list `list` (0: its own lines, k: its k-th branch's;
/// the top level is the empty parent's list 0), at `index`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Slot {
    pub parent: Path,
    pub list: usize,
    pub index: usize,
}

fn walk<'a>(lines: &'a [Line], path: &[(usize, usize)]) -> Option<&'a Line> {
    let (&(_, i), rest) = path.split_first()?;
    let l = lines.get(i)?;
    match rest.first() {
        None => Some(l),
        Some(&(list, _)) => walk(list_of(l, list)?, rest),
    }
}

fn walk_mut<'a>(lines: &'a mut [Line], path: &[(usize, usize)]) -> Option<&'a mut Line> {
    let (&(_, i), rest) = path.split_first()?;
    let l = lines.get_mut(i)?;
    match rest.first() {
        None => Some(l),
        Some(&(list, _)) => walk_mut(list_of_mut(l, list)?, rest),
    }
}

fn list_of(l: &Line, list: usize) -> Option<&Vec<Line>> {
    if list == 0 { Some(&l.body) } else { l.branches.get(list - 1).map(|b| &b.body) }
}

fn list_of_mut(l: &mut Line, list: usize) -> Option<&mut Vec<Line>> {
    if list == 0 { Some(&mut l.body) } else { l.branches.get_mut(list - 1).map(|b| &mut b.body) }
}

pub fn line<'a>(a: &'a Algorithm, path: &[(usize, usize)]) -> Option<&'a Line> {
    walk(&a.body, path)
}

pub fn line_mut<'a>(a: &'a mut Algorithm, path: &[(usize, usize)]) -> Option<&'a mut Line> {
    walk_mut(&mut a.body, path)
}

fn list_mut<'a>(a: &'a mut Algorithm, parent: &[(usize, usize)], list: usize) -> Option<&'a mut Vec<Line>> {
    if parent.is_empty() {
        return (list == 0).then_some(&mut a.body);
    }
    list_of_mut(line_mut(a, parent)?, list)
}

fn list_len(a: &Algorithm, parent: &[(usize, usize)], list: usize) -> usize {
    if parent.is_empty() {
        return a.body.len();
    }
    line(a, parent).and_then(|l| list_of(l, list)).map_or(0, Vec::len)
}

/// A path's slot: its parent, its list, its index.
fn slot_of(path: &[(usize, usize)]) -> Option<Slot> {
    let (&(list, index), parent) = path.split_last()?;
    Some(Slot { parent: parent.to_vec(), list, index })
}

fn path_of(s: &Slot) -> Path {
    let mut p = s.parent.clone();
    p.push((s.list, s.index));
    p
}

/// The list a block's new last line goes in: its last branch's, else its own.
fn last_list(l: &Line) -> usize {
    l.branches.len()
}

pub fn slot_above(a: &Algorithm, r: &Row) -> Option<Slot> {
    match r.part {
        Part::Line => slot_of(&r.path),
        Part::Branch(k) => Some(Slot { parent: r.path.clone(), list: k, index: list_len(a, &r.path, k) }),
        Part::End => {
            let list = line(a, &r.path).map_or(0, last_list);
            Some(Slot { parent: r.path.clone(), list, index: list_len(a, &r.path, list) })
        }
    }
}

pub fn slot_below(a: &Algorithm, r: &Row) -> Option<Slot> {
    match r.part {
        Part::Line if line(a, &r.path).is_some_and(|l| l.kind.is_block()) => Some(Slot { parent: r.path.clone(), list: 0, index: 0 }),
        Part::Line | Part::End => slot_of(&r.path).map(|s| Slot { index: s.index + 1, ..s }),
        Part::Branch(k) => Some(Slot { parent: r.path.clone(), list: k + 1, index: 0 }),
    }
}

fn depth_of(l: &Line) -> usize {
    if !l.kind.is_block() {
        return 0;
    }
    1 + l.body.iter().chain(l.branches.iter().flat_map(|b| b.body.iter())).map(depth_of).max().unwrap_or(0)
}

fn size_of(l: &Line) -> usize {
    1 + l.body.iter().map(size_of).sum::<usize>() + l.branches.iter().map(|b| 1 + b.body.iter().map(size_of).sum::<usize>()).sum::<usize>()
}

/// Puts `l` at `slot`; returns its path.
pub fn insert(a: &mut Algorithm, slot: &Slot, l: Line) -> Result<Path, String> {
    if a.count() + size_of(&l) > MAX_LINES {
        return Err(format!("An algorithm has at most {MAX_LINES} lines."));
    }
    if slot.parent.len() + 1 + depth_of(&l) > MAX_DEPTH {
        return Err(format!("Blocks nest at most {MAX_DEPTH} deep."));
    }
    let list = list_mut(a, &slot.parent, slot.list).ok_or("That place isn't there any more.")?;
    let index = slot.index.min(list.len());
    list.insert(index, l);
    Ok(path_of(&Slot { index, ..slot.clone() }))
}

/// Takes the line at `path` out.
pub fn remove(a: &mut Algorithm, path: &[(usize, usize)]) -> Option<Line> {
    let s = slot_of(path)?;
    let list = list_mut(a, &s.parent, s.list)?;
    (s.index < list.len()).then(|| list.remove(s.index))
}

/// A new block with one empty line inside (an If–else: an `else` part too).
pub fn block(kind: Kind, with_else: bool) -> Line {
    let mut l = Line::new(kind, "");
    l.body.push(Line::state(""));
    if with_else && kind == Kind::If {
        l.branches.push(Branch { cond: None, comment: String::new(), body: vec![Line::state("")] });
    }
    l
}

/// Replaces the block at `path` with its lines (its own, then each branch's).
pub fn unwrap(a: &mut Algorithm, path: &[(usize, usize)]) -> Result<Path, String> {
    let l = line(a, path).ok_or("That line isn't there any more.")?;
    if !l.kind.is_block() {
        return Err("Only a block can be unwrapped.".into());
    }
    let s = slot_of(path).ok_or("That line isn't there any more.")?;
    let l = remove(a, path).ok_or("That line isn't there any more.")?;
    let mut inner: Vec<Line> = l.body;
    for b in l.branches {
        inner.extend(b.body);
    }
    let list = list_mut(a, &s.parent, s.list).ok_or("That place isn't there any more.")?;
    let at = s.index.min(list.len());
    for (k, x) in inner.into_iter().enumerate() {
        list.insert(at + k, x);
    }
    Ok(path.to_vec())
}

/// Moves the line into the block just above it, as its last line.
pub fn indent(a: &mut Algorithm, path: &[(usize, usize)]) -> Result<Path, String> {
    let s = slot_of(path).ok_or("That line isn't there any more.")?;
    let prev = s.index.checked_sub(1).ok_or("There is no block above it to move into.")?;
    let mut target = s.parent.clone();
    target.push((s.list, prev));
    let block = line(a, &target).ok_or("There is no block above it to move into.")?;
    if !block.kind.is_block() {
        return Err("The line above isn't a block.".into());
    }
    let list = last_list(block);
    let len = list_len(a, &target, list);
    let l = remove(a, path).ok_or("That line isn't there any more.")?;
    let to = Slot { parent: target, list, index: len };
    match insert(a, &to, l.clone()) {
        Ok(p) => Ok(p),
        Err(e) => {
            let _ = insert(a, &s, l);
            Err(e)
        }
    }
}

/// Moves the line out of its block, to just after the block.
pub fn outdent(a: &mut Algorithm, path: &[(usize, usize)]) -> Result<Path, String> {
    if path.len() < 2 {
        return Err("It is already at the outermost level.".into());
    }
    let parent = path.get(..path.len() - 1).unwrap_or(&[]).to_vec();
    let after = slot_of(&parent).map(|s| Slot { index: s.index + 1, ..s }).ok_or("That line isn't there any more.")?;
    let l = remove(a, path).ok_or("That line isn't there any more.")?;
    insert(a, &after, l)
}

/// Swaps the line with the one above or below it in its block.
pub fn move_by(a: &mut Algorithm, path: &[(usize, usize)], up: bool) -> Result<Path, String> {
    let s = slot_of(path).ok_or("That line isn't there any more.")?;
    let list = list_mut(a, &s.parent, s.list).ok_or("That line isn't there any more.")?;
    let other = if up { s.index.checked_sub(1) } else { Some(s.index + 1).filter(|i| *i < list.len()) };
    let other = other.ok_or(if up { "It is the first line of its block." } else { "It is the last line of its block." })?;
    list.swap(s.index, other);
    Ok(path_of(&Slot { index: other, ..s }))
}

/// Moves the line at `from` to `to` (a drag); not into itself.
pub fn move_to(a: &mut Algorithm, from: &[(usize, usize)], to: &Slot) -> Result<Path, String> {
    if to.parent.len() >= from.len() && to.parent.get(..from.len()) == Some(from) {
        return Err("A block can't move into itself.".into());
    }
    let f = slot_of(from).ok_or("That line isn't there any more.")?;
    let mut to = to.clone();
    // Taking the line out shifts what comes after it in its list.
    let d = from.len() - 1;
    if to.parent.len() == d && to.parent == f.parent && to.list == f.list && to.index > f.index {
        to.index -= 1;
    } else if to.parent.len() > d
        && to.parent.get(..d) == Some(&f.parent[..])
        && let Some(step) = to.parent.get_mut(d)
        && step.0 == f.list
        && step.1 > f.index
    {
        step.1 -= 1;
    }
    let l = remove(a, from).ok_or("That line isn't there any more.")?;
    match insert(a, &to, l.clone()) {
        Ok(p) => Ok(p),
        Err(e) => {
            let _ = insert(a, &f, l);
            Err(e)
        }
    }
}

/// Puts the lines at `paths` (next to each other in one block) inside a new block of `kind`.
pub fn wrap(a: &mut Algorithm, paths: &[Path], kind: Kind) -> Result<Path, String> {
    let mut slots: Vec<Slot> = paths.iter().filter_map(|p| slot_of(p)).collect();
    slots.sort_by_key(|s| s.index);
    let first = slots.first().cloned().ok_or("Select the lines to put inside it.")?;
    let together = slots.iter().all(|s| s.parent == first.parent && s.list == first.list) && slots.windows(2).all(|w| w[1].index == w[0].index + 1);
    if !together {
        return Err("Select lines next to each other, in one block.".into());
    }
    if !kind.is_block() {
        return Err("Only a block can hold lines.".into());
    }
    let mut taken = Vec::new();
    for s in slots.iter().rev() {
        if let Some(l) = remove(a, &path_of(s)) {
            taken.push(l);
        }
    }
    taken.reverse();
    let mut b = Line::new(kind, "");
    b.body = taken;
    insert(a, &first, b)
}

/// Adds an `else if` (`cond`) or `else` to the If at `path`, with an empty line; `else if`s go
/// before an `else`. Returns the branch's index.
pub fn add_branch(a: &mut Algorithm, path: &[(usize, usize)], cond: Option<String>) -> Result<usize, String> {
    let l = line_mut(a, path).ok_or("That line isn't there any more.")?;
    if l.kind != Kind::If {
        return Err("Only an If has else parts.".into());
    }
    let has_else = l.branches.last().is_some_and(|b| b.cond.is_none());
    if cond.is_none() && has_else {
        return Err("It has an else already.".into());
    }
    let b = Branch { cond, comment: String::new(), body: vec![Line::state("")] };
    let at = if has_else { l.branches.len() - 1 } else { l.branches.len() };
    l.branches.insert(at, b);
    Ok(at)
}

/// Removes the If's `k`-th part; its lines join the part before it.
pub fn remove_branch(a: &mut Algorithm, path: &[(usize, usize)], k: usize) -> Result<(), String> {
    let l = line_mut(a, path).ok_or("That line isn't there any more.")?;
    if k >= l.branches.len() {
        return Err("That part isn't there any more.".into());
    }
    let b = l.branches.remove(k);
    let into = if k == 0 { &mut l.body } else { &mut l.branches[k - 1].body };
    into.extend(b.body);
    Ok(())
}

/// What a line typed with a keyword first becomes: `if x` an If on x, `while`, `for`,
/// `for all`, `return`, `repeat`, `loop`, `procedure F(x)`, `function`, `require`, `ensure`.
/// `whole`: the line is complete (Enter), so `repeat` and `loop` with nothing after count too.
pub fn typed(text: &str, whole: bool) -> Option<(Kind, String)> {
    let lower = text.to_ascii_lowercase();
    let table: [(&str, Kind); 11] = [
        ("for all ", Kind::ForAll),
        ("forall ", Kind::ForAll),
        ("if ", Kind::If),
        ("while ", Kind::While),
        ("for ", Kind::For),
        ("return ", Kind::Return),
        ("procedure ", Kind::Procedure),
        ("function ", Kind::Function),
        ("require ", Kind::Require),
        ("ensure ", Kind::Ensure),
        ("repeat ", Kind::Repeat),
    ];
    for (word, kind) in table {
        if lower.starts_with(word) {
            return Some((kind, text.get(word.len()..).unwrap_or("").to_string()));
        }
    }
    if whole {
        match lower.trim() {
            "repeat" => return Some((Kind::Repeat, String::new())),
            "loop" => return Some((Kind::Loop, String::new())),
            "return" => return Some((Kind::Return, String::new())),
            _ => {}
        }
    }
    None
}

/// A procedure's header typed as `Name(args)`: its name and parameters.
pub fn split_name(s: &str) -> (String, String) {
    match s.split_once('(') {
        Some((n, rest)) => (n.trim().to_string(), rest.trim_end().trim_end_matches(')').to_string()),
        None => (s.trim().to_string(), String::new()),
    }
}

/// The row showing `path`'s `part`.
pub fn row_of(rows: &[Row], path: &[(usize, usize)], part: Part) -> Option<usize> {
    rows.iter().position(|r| r.path == path && r.part == part)
}

/// A line made from a typed keyword (see [`typed`]): a block gets one empty line inside.
pub fn from_typed(kind: Kind, rest: &str) -> Line {
    let mut l = if kind.is_block() { block(kind, false) } else { Line::new(kind, "") };
    if matches!(kind, Kind::Procedure | Kind::Function) {
        let (n, args) = split_name(rest);
        l.text = n;
        l.args = args;
    } else if kind != Kind::Repeat {
        l.text = rest.to_string();
    }
    l
}

/// Starting points: (name, algpseudocode).
pub const TEMPLATES: &[(&str, &str)] = &[
    (
        "Euclid's algorithm",
        r"\Procedure{Euclid}{$a, b$} \Comment{The gcd of $a$ and $b$}
  \State $r \gets a \bmod b$
  \While{$r \neq 0$}
    \State $a \gets b$
    \State $b \gets r$
    \State $r \gets a \bmod b$
  \EndWhile
  \State \Return $b$
\EndProcedure",
    ),
    (
        "Binary search",
        r"\Require a sorted array $A[1..n]$ and a key $x$
\Ensure an index $i$ with $A[i] = x$, or $0$
\Function{Search}{$A, x$}
  \State $lo \gets 1$, $hi \gets n$
  \While{$lo \le hi$}
    \State $mid \gets \lfloor (lo + hi) / 2 \rfloor$
    \If{$A[mid] = x$}
      \State \Return $mid$
    \ElsIf{$A[mid] < x$}
      \State $lo \gets mid + 1$
    \Else
      \State $hi \gets mid - 1$
    \EndIf
  \EndWhile
  \State \Return $0$
\EndFunction",
    ),
    (
        "Insertion sort",
        r"\Procedure{InsertionSort}{$A$}
  \For{$j \gets 2$ \textbf{to} $n$}
    \State $key \gets A[j]$
    \State $i \gets j - 1$
    \While{$i > 0$ \textbf{and} $A[i] > key$}
      \State $A[i + 1] \gets A[i]$
      \State $i \gets i - 1$
    \EndWhile
    \State $A[i + 1] \gets key$
  \EndFor
\EndProcedure",
    ),
    (
        "Breadth-first search",
        r"\Procedure{BFS}{$G, s$}
  \ForAll{$v \in V(G)$}
    \State $d(v) \gets \infty$
  \EndFor
  \State $d(s) \gets 0$; $Q \gets \langle s \rangle$
  \While{$Q$ is not empty}
    \State $u \gets$ \Call{Dequeue}{$Q$}
    \ForAll{$(u, v) \in E(G)$ with $d(v) = \infty$}
      \State $d(v) \gets d(u) + 1$
      \State \Call{Enqueue}{$Q, v$}
    \EndFor
  \EndWhile
\EndProcedure",
    ),
    (
        "Depth-first search",
        r"\Procedure{DFS}{$G, u$}
  \State mark $u$ as visited
  \ForAll{$(u, v) \in E(G)$}
    \If{$v$ is not visited}
      \State \Call{DFS}{$G, v$}
    \EndIf
  \EndFor
\EndProcedure",
    ),
    (
        "Dijkstra's algorithm",
        r"\Procedure{Dijkstra}{$G, w, s$}
  \ForAll{$v \in V(G)$}
    \State $d(v) \gets \infty$
  \EndFor
  \State $d(s) \gets 0$; $Q \gets V(G)$
  \While{$Q \neq \emptyset$}
    \State $u \gets$ the vertex of $Q$ with the least $d(u)$
    \State $Q \gets Q \setminus \{u\}$
    \ForAll{$(u, v) \in E(G)$}
      \If{$d(u) + w(u, v) < d(v)$}
        \State $d(v) \gets d(u) + w(u, v)$ \Comment{Relax the edge}
      \EndIf
    \EndFor
  \EndWhile
\EndProcedure",
    ),
    (
        "DPLL",
        r"\Require a set of clauses $\Phi$
\Ensure whether $\Phi$ is satisfiable
\Function{DPLL}{$\Phi$}
  \While{$\Phi$ has a unit clause $\{\ell\}$}
    \State $\Phi \gets \Phi|_{\ell}$ \Comment{Unit propagation}
  \EndWhile
  \If{$\Phi = \emptyset$}
    \State \Return \textbf{true}
  \ElsIf{$\Phi$ has an empty clause}
    \State \Return \textbf{false}
  \EndIf
  \State choose a literal $\ell$ in $\Phi$
  \State \Return \Call{DPLL}{$\Phi|_{\ell}$} \textbf{or} \Call{DPLL}{$\Phi|_{\neg \ell}$}
\EndFunction",
    ),
    (
        "Unification",
        r"\Require a set of equations $E$ between terms
\Ensure a most general unifier of $E$, or failure
\Function{Unify}{$E$}
  \State $\sigma \gets \emptyset$
  \While{$E \neq \emptyset$}
    \State take $s \doteq t$ out of $E$
    \If{$s = t$}
      \State \textbf{continue}
    \ElsIf{$s = f(s_1, \dots, s_n)$ and $t = f(t_1, \dots, t_n)$}
      \State $E \gets E \cup \{s_1 \doteq t_1, \dots, s_n \doteq t_n\}$
    \ElsIf{$s$ is a variable $x$ not occurring in $t$}
      \State $E \gets E[t/x]$; $\sigma \gets \sigma[t/x] \cup \{x \mapsto t\}$
    \ElsIf{$t$ is a variable}
      \State $E \gets E \cup \{t \doteq s\}$
    \Else
      \State \Return failure
    \EndIf
  \EndWhile
  \State \Return $\sigma$
\EndFunction",
    ),
    (
        "Resolution",
        r"\Require a set of clauses $S$
\Ensure whether $S$ is unsatisfiable
\Function{Refute}{$S$}
  \Repeat
    \State $N \gets \{ \mathit{res}(C, D) : C, D \in S \} \setminus S$
    \If{$N$ has the empty clause}
      \State \Return \textbf{true}
    \EndIf
    \State $S \gets S \cup N$
  \Until{$N = \emptyset$}
  \State \Return \textbf{false}
\EndFunction",
    ),
];

/// The template `name`, with `\caption` set to its name.
pub fn template(name: &str) -> Option<Algorithm> {
    let (_, src) = TEMPLATES.iter().find(|(n, _)| *n == name)?;
    let mut a = super::open(&format!("\\begin{{algorithmic}}[1]\n{src}\n\\end{{algorithmic}}")).ok()?.algorithm;
    a.frame = super::Frame::Ruled;
    a.caption = name.to_string();
    Some(a)
}
