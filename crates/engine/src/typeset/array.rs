//! Grids read from LaTeX environments: `array` and `tabular` with their column spec (`l`, `c`,
//! `r`, `p{…}`, `|` rules, `*{n}{…}`), `\hline`, `\cline{a-b}` and `\multicolumn`; `align`,
//! `aligned`, `alignat`, `flalign` and `eqnarray` (right/left column pairs); `gather` and
//! `gathered` (centred lines).

use deckcraft_model::text::Align;

use super::scan::{Scan, split_top};

/// Most rows and columns in one grid.
const MAX_ROWS: usize = 400;
const MAX_COLS: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GridKind {
    /// `array`: math cells.
    Array,
    /// `tabular`: text cells.
    Tabular,
    /// `align` and friends: right/left pairs, the left column spaced as if after `{}`.
    Align,
    /// `gather`: one centred math column.
    Gather,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Cell {
    pub src: String,
    pub span: usize,
    /// From `\multicolumn`, else the column's.
    pub align: Option<Align>,
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct Row {
    pub cells: Vec<Cell>,
    /// Rules above the row: `(first column, last column)` inclusive (`\hline` is all columns).
    pub rules_above: Vec<(usize, usize)>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Grid {
    pub kind: GridKind,
    pub cols: Vec<Align>,
    /// Column boundaries with a vertical rule: 0 is the left edge, `cols.len()` the right.
    pub vrules: Vec<usize>,
    pub rows: Vec<Row>,
    /// Rules under the last row.
    pub rules_below: Vec<(usize, usize)>,
}

/// The grid when all of `src` (inside `$$`/`\[ \]` or not) is one grid environment.
pub fn parse(src: &str) -> Option<Grid> {
    let t = super::strip_math_delims(src);
    let mut s = Scan::new(t);
    if s.command()? != "begin" {
        return None;
    }
    let env = s.group()?;
    let base = env.trim().trim_end_matches('*');
    let kind = match base {
        "array" => GridKind::Array,
        "tabular" | "tabularx" => GridKind::Tabular,
        "align" | "aligned" | "alignat" | "alignedat" | "flalign" | "eqnarray" | "split" => GridKind::Align,
        "gather" | "gathered" | "multline" => GridKind::Gather,
        _ => return None,
    };
    let end = format!("\\end{{{env}}}");
    let close = t.rfind(&end)?;
    if !t.get(close + end.len()..).unwrap_or("").trim().is_empty() {
        return None;
    }
    let (cols, vrules) = match kind {
        GridKind::Array | GridKind::Tabular => {
            let _ = s.opt();
            if base == "tabularx" {
                let _ = s.group();
            }
            if base == "alignat" || base == "alignedat" {
                let _ = s.group();
            }
            spec(&s.group()?)
        }
        _ => {
            if base == "alignat" || base == "alignedat" {
                let _ = s.group();
            }
            let _ = s.opt();
            (Vec::new(), Vec::new())
        }
    };
    let body = t.get(s.byte_offset().min(close)..close).unwrap_or("");
    let mut grid = Grid { kind, cols, vrules, rows: Vec::new(), rules_below: Vec::new() };
    rows(&mut grid, body);
    if matches!(kind, GridKind::Align | GridKind::Gather) {
        let n = grid.rows.iter().map(|r| r.cells.len()).max().unwrap_or(1).clamp(1, MAX_COLS);
        grid.cols = (0..n)
            .map(|i| match kind {
                GridKind::Gather => Align::Center,
                _ if i.is_multiple_of(2) => Align::Right,
                _ => Align::Left,
            })
            .collect();
    }
    Some(grid)
}

/// A column spec: the alignments and where the vertical rules are.
fn spec(s: &str) -> (Vec<Align>, Vec<usize>) {
    let mut cols = Vec::new();
    let mut vrules = Vec::new();
    spec_into(s, &mut cols, &mut vrules, 0);
    (cols, vrules)
}

fn spec_into(s: &str, cols: &mut Vec<Align>, vrules: &mut Vec<usize>, depth: usize) {
    let mut sc = Scan::new(s);
    while let Some(c) = {
        sc.ws();
        sc.bump()
    } {
        if cols.len() >= MAX_COLS {
            return;
        }
        match c {
            'l' => cols.push(Align::Left),
            'c' => cols.push(Align::Center),
            'r' => cols.push(Align::Right),
            'p' | 'm' | 'b' | 'X' => {
                if c != 'X' {
                    let _ = sc.group();
                }
                cols.push(Align::Left);
            }
            '|' => {
                if vrules.last() != Some(&cols.len()) {
                    vrules.push(cols.len());
                }
            }
            '@' | '!' | '>' | '<' => {
                let _ = sc.group();
            }
            '*' => {
                let n: usize = sc.group().and_then(|n| n.trim().parse().ok()).unwrap_or(1).min(MAX_COLS);
                let inner = sc.group().unwrap_or_default();
                if depth < 4 {
                    for _ in 0..n {
                        spec_into(&inner, cols, vrules, depth + 1);
                    }
                }
            }
            _ => {}
        }
    }
}

/// Rows split at `\\`, cells at `&`, with their rules.
fn rows(grid: &mut Grid, body: &str) {
    let lines = split_top(body, &["\\\\"]);
    let last = lines.len().saturating_sub(1);
    for (i, line) in lines.into_iter().enumerate() {
        if grid.rows.len() >= MAX_ROWS {
            break;
        }
        let (rules, rest) = leading_rules(&line, grid.cols.len().max(1));
        let rest = strip_row_options(rest);
        if i == last && rest.trim().is_empty() {
            grid.rules_below = rules;
            break;
        }
        let cells = split_top(rest, &["&"]).into_iter().take(MAX_COLS).map(|c| cell(&c)).collect();
        grid.rows.push(Row { cells, rules_above: rules });
    }
}

/// `\\[2pt]` leaves `[2pt]` at the start of the next row.
fn strip_row_options(s: &str) -> &str {
    let t = s.trim_start();
    if t.starts_with('[')
        && let Some(end) = t.find(']')
    {
        return t.get(end + 1..).unwrap_or("");
    }
    s
}

/// The `\hline`s and `\cline`s at the start of a row, and the rest.
fn leading_rules(line: &str, ncols: usize) -> (Vec<(usize, usize)>, &str) {
    let mut rules = Vec::new();
    let mut rest = line;
    loop {
        let t = rest.trim_start();
        if let Some(r) = t
            .strip_prefix("\\hline")
            .or_else(|| t.strip_prefix("\\hdashline"))
            .or_else(|| t.strip_prefix("\\toprule"))
            .or_else(|| t.strip_prefix("\\midrule"))
            .or_else(|| t.strip_prefix("\\bottomrule"))
        {
            rules.push((0, ncols - 1));
            rest = r;
        } else if let Some(r) = t.strip_prefix("\\cline") {
            let mut s = Scan::new(r);
            let range = s.group().unwrap_or_default();
            let (a, b) = range.split_once('-').unwrap_or((&range, &range));
            let a = a.trim().parse::<usize>().unwrap_or(1).clamp(1, ncols);
            let b = b.trim().parse::<usize>().unwrap_or(a).clamp(a, ncols);
            rules.push((a - 1, b - 1));
            rest = r.get(s.byte_offset().min(r.len())..).unwrap_or("");
        } else {
            return (rules, rest);
        }
    }
}

fn cell(src: &str) -> Cell {
    let t = src.trim();
    if let Some(r) = t.strip_prefix("\\multicolumn") {
        let mut s = Scan::new(r);
        let span = s.group().and_then(|n| n.trim().parse::<usize>().ok()).unwrap_or(1).clamp(1, MAX_COLS);
        let (cols, _) = spec(&s.group().unwrap_or_default());
        let content = s.group().unwrap_or_default();
        return Cell { src: content, span, align: cols.first().copied() };
    }
    Cell { src: t.to_string(), span: 1, align: None }
}
