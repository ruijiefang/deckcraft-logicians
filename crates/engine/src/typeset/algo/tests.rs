use deckcraft_model::text::TextBody;

use super::edit::{self, Slot, TEMPLATES};
use super::{Algorithm, Frame, Kind, Line, describe, open, rows, write};
use crate::typeset::{Kind as TypesetKind, Part as RegionPart, Style, typeset};

fn alg(body: &str) -> Algorithm {
    open(&format!("\\begin{{algorithmic}}[1]\n{body}\n\\end{{algorithmic}}")).unwrap_or_else(|e| panic!("{e}")).algorithm
}

fn measure(b: &TextBody) -> (f64, f64, f64) {
    let size = b.paragraphs.iter().flat_map(|p| &p.runs).find_map(|r| r.props.size).unwrap_or(20.0);
    let w = b.paragraphs.iter().map(|p| p.runs.iter().map(|r| r.text.chars().count()).sum::<usize>()).max().unwrap_or(0);
    (w as f64 * size * 0.5, size * 1.15, size * 0.9)
}

fn style() -> Style {
    Style { font: None, size: 10.0 }
}

const EUCLID: &str = r#"\begin{algorithm}[htb]
\caption{Euclid's algorithm}\label{euclid}
\begin{algorithmic}[1]
\Require $a, b > 0$
\Procedure{Euclid}{$a,b$}\Comment{The g.c.d. of a and b}
  \State $r\gets a\bmod b$
  \While{$r\not=0$}\Comment{We have the answer if r is 0}
    \State $a\gets b$ \label{line:a}
    \State $b\gets r$
  \EndWhile
  \Statex continued
  \State \Return $b$
\EndProcedure
\end{algorithmic}
\end{algorithm}"#;

#[test]
fn reads_algpseudocode_with_its_float() {
    let a = open(EUCLID).unwrap().algorithm;
    assert_eq!((a.caption.as_str(), a.label.as_str(), a.frame, a.numbering), ("Euclid's algorithm", "euclid", Frame::Ruled, 1));
    assert_eq!(a.kept, ["[htb]"]);
    assert_eq!(a.body.len(), 2);
    assert_eq!(a.body[0].kind, Kind::Require);
    let p = &a.body[1];
    assert_eq!((p.kind, p.text.as_str(), p.args.as_str(), p.comment.as_str()), (Kind::Procedure, "Euclid", "$a,b$", "The g.c.d. of a and b"));
    let w = &p.body[1];
    assert_eq!((w.kind, w.text.as_str(), w.comment.as_str()), (Kind::While, "$r\\not=0$", "We have the answer if r is 0"));
    assert_eq!(w.body[0].label, "line:a");
    assert!(!p.body[2].numbered, "\\Statex");
    assert_eq!(p.body[3].kind, Kind::Return);
}

#[test]
fn reads_the_older_uppercase_commands() {
    let a = alg(r"\REQUIRE $n \geq 0$
\STATE $y \gets 1$
\IF{$n < 0 \AND x \neq 0$}
  \STATE $X \gets 1 / x$
\ELSIF{$n = 0$}
  \RETURN 1
\ELSE
  \STATE $X \gets x$
\ENDIF
\WHILE{$N \neq 0$} \STATE skip \ENDWHILE");
    let i = &a.body[2];
    assert_eq!(i.kind, Kind::If);
    assert!(i.text.contains("\\textbf{and}"), "{}", i.text);
    assert_eq!(i.branches.len(), 2);
    assert_eq!(i.branches[0].body[0].kind, Kind::Return);
    assert!(i.branches[1].cond.is_none());
    assert_eq!(a.body[3].body[0].text, "skip");
}

#[test]
fn keeps_what_it_does_not_know_and_closes_open_blocks() {
    let o = open(
        r"\algrenewcommand\algorithmicrequire{\textbf{Input:}}
\begin{algorithmic}[0]
\algstore{part}
\State $x$
\While{$x$}
  \If{$y$}
    \State z
\EndWhile
\EndIf
\end{algorithmic}",
    )
    .unwrap();
    let a = o.algorithm;
    assert_eq!(a.keyword("require"), r"\textbf{Input:}");
    assert_eq!((a.frame, a.numbering), (Frame::None, 0));
    assert_eq!(a.body[0].kind, Kind::Raw);
    assert!(o.notices.iter().any(|n| n.contains("closed")), "{:?}", o.notices);
    assert!(o.notices.iter().any(|n| n.contains("without their block")), "{:?}", o.notices);
}

#[test]
fn writes_what_it_reads() {
    for (name, _) in TEMPLATES {
        let a = edit::template(name).unwrap();
        let back = open(&write(&a)).unwrap().algorithm;
        assert_eq!(back, a, "{name}:\n{}", write(&a));
    }
    let mut a = open(EUCLID).unwrap().algorithm;
    a.show_end = false;
    a.number = "3".into();
    a.frame = Frame::Boxed;
    a.set_keyword("ensure", Some(r"\textbf{Output:}".into()));
    a.wrap = Some(30.0);
    let w = write(&a);
    assert!(w.contains("\\algtext*{EndWhile}") && w.contains("\\floatstyle{boxed}") && w.contains("% DeckCraft: number=3, wrap=30em"), "{w}");
    assert_eq!(open(&w).unwrap().algorithm, a);
}

/// A small deterministic generator (xorshift).
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: u64) -> usize {
        (self.next() % n) as usize
    }
}

fn random_lines(r: &mut Rng, depth: usize) -> Vec<Line> {
    let words = [r"$x \gets x + 1$", "visit $v$", r"\Call{Merge}{$A, B$}", r"$S \gets S \cup \{v\}$", ""];
    let kinds =
        [Kind::State, Kind::Return, Kind::If, Kind::For, Kind::ForAll, Kind::While, Kind::Repeat, Kind::Loop, Kind::Procedure, Kind::Function];
    (0..1 + r.below(3))
        .map(|_| {
            let kind = if depth >= 3 { Kind::State } else { kinds[r.below(kinds.len() as u64)] };
            let mut l = Line::new(kind, words[r.below(words.len() as u64)]);
            if matches!(kind, Kind::Procedure | Kind::Function) {
                l.text = "Name".into();
                l.args = "$x$".into();
            }
            if kind == Kind::Loop {
                l.text.clear();
            }
            if r.below(4) == 0 {
                l.comment = "why".into();
            }
            if kind == Kind::State && r.below(5) == 0 {
                l.numbered = false;
            }
            if kind.is_block() {
                l.body = random_lines(r, depth + 1);
                if kind == Kind::If {
                    for _ in 0..r.below(3) {
                        l.branches.push(super::Branch { cond: Some("$y$".into()), comment: String::new(), body: random_lines(r, depth + 1) });
                    }
                    if r.below(2) == 0 {
                        l.branches.push(super::Branch { cond: None, comment: String::new(), body: random_lines(r, depth + 1) });
                    }
                }
            }
            l
        })
        .collect()
}

#[test]
fn random_algorithms_round_trip() {
    let mut r = Rng(0x9E37_79B9_7F4A_7C15);
    for _ in 0..200 {
        let a = Algorithm {
            body: random_lines(&mut r, 0),
            caption: "A test".into(),
            numbering: [0, 1, 2][r.below(3)],
            show_end: r.below(2) == 0,
            ..Default::default()
        };
        let w = write(&a);
        assert_eq!(open(&w).unwrap().algorithm, a, "{w}");
    }
}

#[test]
fn rows_number_lines_and_hide_end_rows_unless_asked() {
    let mut a = alg(r"\While{$c$} \State x \EndWhile \Statex y \State z");
    let r = rows(&a, false);
    assert_eq!(r.iter().map(|x| x.number).collect::<Vec<_>>(), [Some(1), Some(2), Some(3), None, Some(4)]);
    a.show_end = false;
    assert_eq!(rows(&a, false).len(), 4);
    let ghosts = rows(&a, true);
    assert_eq!(ghosts.len(), 5);
    assert!(ghosts[2].ghost && ghosts[2].number.is_none());
}

#[test]
fn rows_say_where_new_lines_go() {
    let a = alg(r"\If{$c$} \State a \Else \State b \EndIf \State c");
    let r = rows(&a, true);
    let at = |i: usize, below: bool| if below { edit::slot_below(&a, &r[i]) } else { edit::slot_above(&a, &r[i]) }.unwrap();
    let s = |parent: Vec<(usize, usize)>, list, index| Slot { parent, list, index };
    // if / a / else / b / end if / c
    assert_eq!(at(0, false), s(vec![], 0, 0), "above a block: before it");
    assert_eq!(at(0, true), s(vec![(0, 0)], 0, 0), "below its header: first inside");
    assert_eq!(at(1, true), s(vec![(0, 0)], 0, 1), "below a statement: after it");
    assert_eq!(at(2, false), s(vec![(0, 0)], 0, 1), "above else: end of the part before");
    assert_eq!(at(2, true), s(vec![(0, 0)], 1, 0), "below else: first of its part");
    assert_eq!(at(4, false), s(vec![(0, 0)], 1, 1), "above end: last inside");
    assert_eq!(at(4, true), s(vec![], 0, 1), "below end: after the block");
}

#[test]
fn indent_outdent_move_wrap_and_unwrap() {
    let mut a = alg(r"\While{$c$} \State a \EndWhile \State b \State c");
    let p = edit::indent(&mut a, &[(0, 1)]).unwrap();
    assert_eq!(p, vec![(0, 0), (0, 1)], "b went into the while");
    assert!(edit::indent(&mut a, &[(0, 0)]).is_err(), "nothing above the while");
    let p = edit::outdent(&mut a, &p).unwrap();
    assert_eq!(p, vec![(0, 1)]);
    let p = edit::move_by(&mut a, &p, false).unwrap();
    assert_eq!(a.body[1].text, "c");
    assert_eq!(p, vec![(0, 2)]);
    let w = edit::wrap(&mut a, &[vec![(0, 1)], vec![(0, 2)]], Kind::If).unwrap();
    assert_eq!(a.body.len(), 2);
    assert_eq!(edit::line(&a, &w).map(|l| l.body.len()), Some(2));
    assert!(edit::move_to(&mut a, &w, &Slot { parent: w.clone(), list: 0, index: 0 }).is_err(), "not into itself");
    // Dragged before the while.
    let moved = edit::move_to(&mut a, &w, &Slot { parent: vec![], list: 0, index: 0 }).unwrap();
    assert_eq!(moved, vec![(0, 0)]);
    assert_eq!(a.body[1].kind, Kind::While);
    edit::unwrap(&mut a, &[(0, 0)]).unwrap();
    assert_eq!(a.body.iter().map(|l| l.text.as_str()).collect::<Vec<_>>(), ["c", "b", "$c$"]);
}

#[test]
fn branches_add_in_order_and_merge_back() {
    let mut a = alg(r"\If{$c$} \State a \EndIf");
    assert_eq!(edit::add_branch(&mut a, &[(0, 0)], None), Ok(0));
    assert_eq!(edit::add_branch(&mut a, &[(0, 0)], Some("$d$".into())), Ok(0), "else if goes before else");
    assert!(edit::add_branch(&mut a, &[(0, 0)], None).is_err());
    edit::remove_branch(&mut a, &[(0, 0)], 0).unwrap();
    assert_eq!(a.body[0].body.len(), 2, "its line joined the if's own");
}

#[test]
fn keywords_typed_first_make_blocks() {
    assert_eq!(edit::typed("if $x > 0$", false), Some((Kind::If, "$x > 0$".into())));
    assert_eq!(edit::typed("For all $v$", false), Some((Kind::ForAll, "$v$".into())));
    // Typed a key at a time, `for a`, `for al` and `for all` wait: they may become `for all`.
    for partial in ["for ", "for a", "for al", "for all"] {
        assert_eq!(edit::typed(partial, false), None, "{partial}");
    }
    assert_eq!(edit::typed("for all ", false), Some((Kind::ForAll, String::new())));
    assert_eq!(edit::typed("for alpha", false), Some((Kind::For, "alpha".into())));
    assert_eq!(edit::typed("for $i$", false), Some((Kind::For, "$i$".into())));
    assert_eq!(edit::typed("for all", true), Some((Kind::ForAll, String::new())));
    assert_eq!(edit::typed("loop", false), None);
    assert_eq!(edit::typed("loop", true), Some((Kind::Loop, String::new())));
    assert_eq!(edit::typed("iffy", true), None);
    let p = edit::from_typed(Kind::Procedure, "Merge($A, B$)");
    assert_eq!((p.text.as_str(), p.args.as_str(), p.body.len()), ("Merge", "$A, B$", 1));
}

#[test]
fn layout_numbers_indents_and_puts_comments_flush_right() {
    let a = open(EUCLID).unwrap().algorithm;
    let t = super::layout(&a, &style(), &measure, false);
    assert_eq!(t.kind, TypesetKind::Algorithm);
    let rows: Vec<_> = t.regions.iter().filter(|r| r.part == RegionPart::Row).collect();
    assert_eq!(rows.len(), super::rows(&a, false).len());
    assert!(rows.windows(2).all(|w| w[1].y > w[0].y), "rows go down the page");
    assert!(rows.iter().all(|r| (r.w - t.w).abs() < 1e-9));
    let w = t.w;
    let texts: Vec<(f64, f64, String)> = t
        .items
        .iter()
        .filter_map(|i| match i {
            crate::typeset::Item::Text { x, w, body, .. } => Some((*x, *x + *w, body.text())),
            _ => None,
        })
        .map(|(x, r, s)| (x, r, s.trim().to_string()))
        .collect();
    let find = |s: &str| texts.iter().find(|t| t.2.contains(s)).unwrap_or_else(|| panic!("{s} in {texts:?}")).clone();
    assert!((find("The g.c.d.").1 - w).abs() < 1e-6, "comment flush right");
    let state = find("mod");
    let proc = find("procedure");
    assert!((state.0 - proc.0 - 15.0).abs() < 1e-6, "one level is 1.5em: {} {}", state.0, proc.0);
    assert!(texts.iter().any(|t| t.2 == "1:"));
    assert!(find("Algorithm 1").0.abs() < 1e-9);
    // Twice the size, twice as big.
    let big = super::layout(&a, &Style { size: 20.0, ..style() }, &measure, false);
    assert!((big.w / t.w - 2.0).abs() < 0.05 && (big.h / t.h - 2.0).abs() < 0.05);
}

#[test]
fn long_lines_wrap_at_the_width() {
    let mut a = alg(r"\State one two three four five six seven eight nine ten eleven twelve");
    let one = typeset(&write(&a), &style(), &measure).unwrap();
    a.wrap = Some(14.0);
    let wrapped = super::layout(&a, &style(), &measure, false);
    assert!(wrapped.h > one.h + 10.0, "{} {}", wrapped.h, one.h);
}

#[test]
fn the_prepared_text_knows_algorithmic_macros() {
    let p = super::layout::prepare(r"$r \gets a \bmod b$, \Call{Merge}{$A$}, \Return x, $a \not= b$");
    assert!(p.contains("$ mod $") && p.contains("\\textsc{Merge}($A$)") && p.contains("\\textbf{return}") && p.contains("\\neq"), "{p}");
    assert!(describe(&open(EUCLID).unwrap().algorithm).starts_with("Algorithm 1: Euclid's algorithm. Require: a, b > 0;"));
}

#[test]
fn hostile_input_never_crashes() {
    let deep = format!("{}{}", r"\If{x}".repeat(200), r"\EndIf".repeat(200));
    let many = r"\State x ".repeat(3000);
    for src in [
        "\\begin{algorithmic}",
        "\\begin{algorithmic}[x]\\end{algorithmic}",
        "\\begin{algorithmic}\\If{\\end{algorithmic}",
        "\\begin{algorithmic}\\EndIf\\Else\\Until{x}\\end{algorithmic}",
        "\\begin{algorithmic}\\State \\Call{ \\Comment{ $ \\bmod \\end{algorithmic}",
        "\\begin{algorithmic}\\algrenewcommand{\\end{algorithmic}",
        &format!("\\begin{{algorithmic}}{deep}\\end{{algorithmic}}"),
        &format!("\\begin{{algorithmic}}{many}\\end{{algorithmic}}"),
    ] {
        if let Ok(o) = open(src) {
            assert!(o.algorithm.count() <= super::MAX_LINES);
            let _ = write(&o.algorithm);
            let _ = super::layout(&o.algorithm, &style(), &measure, true);
        }
    }
}

/// What DeckCraft writes compiles with the real algorithm and algpseudocode packages (skipped
/// where TeX isn't installed, as on CI).
#[test]
fn written_algorithms_compile_with_pdflatex() {
    let has_tex = std::process::Command::new("kpsewhich").arg("algpseudocode.sty").output().is_ok_and(|o| o.status.success() && !o.stdout.is_empty());
    if !has_tex {
        return;
    }
    let mut body = String::new();
    for (name, _) in TEMPLATES {
        body.push_str(&write(&edit::template(name).unwrap_or_default()));
        body.push_str("\n\n");
    }
    let mut r = Rng(0x2545_F491_4F6C_DD1D);
    for k in 0..30 {
        let a = Algorithm {
            body: random_lines(&mut r, 0),
            caption: format!("Random {k}"),
            show_end: k % 2 == 0,
            numbering: (k % 3) as u32,
            ..Default::default()
        };
        body.push_str(&write(&a));
        body.push_str("\n\n");
    }
    let doc = format!("\\documentclass{{article}}\n\\usepackage{{amssymb,algorithm,algpseudocode}}\n\\begin{{document}}\n{body}\\end{{document}}\n");
    let dir = std::env::temp_dir().join(format!("deckcraft-algo-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("t.tex"), &doc).unwrap();
    let out =
        std::process::Command::new("pdflatex").args(["-interaction=nonstopmode", "-halt-on-error", "t.tex"]).current_dir(&dir).output().unwrap();
    let log = std::fs::read_to_string(dir.join("t.log")).unwrap_or_default();
    let _ = std::fs::remove_dir_all(&dir);
    let at: usize =
        log.lines().find_map(|l| l.strip_prefix("l.").and_then(|r| r.split_whitespace().next()).and_then(|n| n.parse().ok())).unwrap_or(0);
    let near: Vec<&str> = doc.lines().skip(at.saturating_sub(8)).take(9).collect();
    assert!(
        out.status.success(),
        "{}\n--- near line {at}:\n{}",
        log.lines().filter(|l| l.starts_with('!')).collect::<Vec<_>>().join("\n"),
        near.join("\n")
    );
}

#[test]
fn kept_latex_lines_show_only_in_the_editor() {
    let a = alg("\\algstore{part}\n\\State $x$");
    assert_eq!(a.body[0].kind, Kind::Raw);
    let text = |editor: bool| -> Vec<String> {
        super::layout(&a, &style(), &measure, editor)
            .items
            .iter()
            .filter_map(|i| match i {
                crate::typeset::Item::Text { body, .. } => Some(body.text()),
                _ => None,
            })
            .collect()
    };
    assert!(text(true).iter().any(|t| t.contains("algstore")), "the editor shows it");
    assert!(!text(false).iter().any(|t| t.contains("algstore")), "the paper prints nothing for it");
    let rows = super::layout(&a, &style(), &measure, true).regions.iter().filter(|r| r.part == RegionPart::Row).count();
    assert_eq!(rows, super::rows(&a, true).len(), "the editor's rows line up with its regions");
}

#[test]
fn templates_use_only_known_commands() {
    for (name, _) in TEMPLATES {
        let a = edit::template(name).unwrap();
        for t in super::layout(&a, &style(), &measure, false).items.iter().filter_map(|i| match i {
            crate::typeset::Item::Text { body, .. } => Some(body.text()),
            _ => None,
        }) {
            assert!(!t.contains('\\'), "{name}: {t}");
        }
    }
}
