use deckcraft_model::text::TextBody;

use super::super::layout::MAX_DEPTH;
use super::super::proof::{self, MAX_NODES, Proof, Rule, Stmt};
use super::super::{Part, Style, typeset};
use super::*;

fn forest(src: &str) -> Vec<Proof> {
    proof::parse(src).unwrap_or_else(|e| panic!("{src}: {e}"))
}

fn text_at(f: &[Proof], path: &[usize]) -> String {
    node(f, path).map(|p| p.concl.iter().map(|c| c.src.clone()).collect::<Vec<_>>().join("|")).unwrap_or_else(|| "<none>".into())
}

/// `\hypo{A}\hypo{B}\infer2[R]{C}`: [0] is C, [0,0] A, [0,1] B.
fn abc() -> Vec<Proof> {
    forest(r"\hypo{A}\hypo{B}\infer2[R]{C}")
}

#[test]
fn adding_premises_siblings_and_steps() {
    let mut f = abc();
    let p = add_premise(&mut f, &[0, 0]).unwrap();
    assert_eq!(p, vec![0, 0, 0]);
    assert_eq!(kind(node(&f, &[0, 0]).unwrap()), Kind::Inference, "a hypothesis with a premise is an inference");
    assert_eq!(node(&f, &[0, 0]).unwrap().rule, Rule::Single);

    let mut f = abc();
    assert_eq!(add_sibling(&mut f, &[0, 1], true).unwrap(), vec![0, 2]);
    assert_eq!(add_sibling(&mut f, &[0, 0], false).unwrap(), vec![0, 0]);
    assert_eq!(
        (text_at(&f, &[0, 0]), text_at(&f, &[0, 1]), text_at(&f, &[0, 2]), text_at(&f, &[0, 3])),
        ("".into(), "A".into(), "B".into(), "".into())
    );
    // Beside a root: another tree.
    assert_eq!(add_sibling(&mut f, &[0], true).unwrap(), vec![1]);
    assert_eq!(f.len(), 2);

    let mut f = abc();
    assert_eq!(insert_below(&mut f, &[0]).unwrap(), vec![0]);
    assert_eq!(text_at(&f, &[0]), "");
    assert_eq!(text_at(&f, &[0, 0]), "C");
    assert_eq!(text_at(&f, &[0, 0, 1]), "B");
    // Mid-tree: a step between B and C.
    let mut f = abc();
    assert_eq!(insert_below(&mut f, &[0, 1]).unwrap(), vec![0, 1]);
    assert_eq!((text_at(&f, &[0, 1]), text_at(&f, &[0, 1, 0])), ("".into(), "B".into()));
}

#[test]
fn removing_steps_and_branches() {
    // A leaf: gone, its step selected; a step left without premises keeps its rule (an axiom).
    let mut f = forest(r"\hypo{A}\infer1{C}");
    assert_eq!(remove_step(&mut f, &[0, 0]).unwrap(), Some(vec![0]));
    assert_eq!(kind(node(&f, &[0]).unwrap()), Kind::Axiom);

    // An inner step: its premises take its place.
    let mut f = forest(r"\hypo{A}\hypo{B}\infer2{C}\hypo{D}\infer2{E}");
    assert_eq!(remove_step(&mut f, &[0, 0]).unwrap(), Some(vec![0, 0]));
    assert_eq!((text_at(&f, &[0, 0]), text_at(&f, &[0, 1]), text_at(&f, &[0, 2])), ("A".into(), "B".into(), "D".into()));

    // A root: its premises become trees.
    let mut f = abc();
    assert_eq!(remove_step(&mut f, &[0]).unwrap(), Some(vec![0]));
    assert_eq!((f.len(), text_at(&f, &[0]), text_at(&f, &[1])), (2, "A".into(), "B".into()));

    // The last node: nothing left to select.
    let mut f = forest(r"\hypo{A}");
    assert_eq!(remove_step(&mut f, &[0]).unwrap(), None);
    assert!(f.is_empty());

    let mut f = abc();
    assert_eq!(delete_branch(&mut f, &[0, 1]).unwrap(), Some(vec![0]));
    assert_eq!(node(&f, &[0]).unwrap().premises.len(), 1);
    assert_eq!(delete_branch(&mut f, &[0]).unwrap(), None);
    assert!(remove_step(&mut f, &[0]).is_err(), "a stale path is an error, not a crash");
}

#[test]
fn moving_and_pasting_branches() {
    let mut f = abc();
    assert_eq!(move_sibling(&mut f, &[0, 0], true).unwrap(), vec![0, 1]);
    assert_eq!((text_at(&f, &[0, 0]), text_at(&f, &[0, 1])), ("B".into(), "A".into()));
    assert_eq!(move_sibling(&mut f, &[0, 1], true).unwrap(), vec![0, 1], "at the end nothing moves");

    // A onto B: taking A out shifts B to [0,0]; A becomes its premise.
    let mut f = abc();
    assert_eq!(move_branch(&mut f, &[0, 0], &[0, 1], Place::Premise).unwrap(), vec![0, 0, 0]);
    assert_eq!((text_at(&f, &[0, 0]), text_at(&f, &[0, 0, 0])), ("B".into(), "A".into()));
    assert_eq!(node(&f, &[0, 0]).unwrap().rule, Rule::Single);
    let mut f = abc();
    assert_eq!(move_branch(&mut f, &[0, 1], &[0, 0], Place::Before).unwrap(), vec![0, 0]);
    assert_eq!(text_at(&f, &[0, 0]), "B");
    // Not into itself.
    let mut f = forest(r"\hypo{A}\infer1{B}\infer1{C}");
    let before = f.clone();
    assert!(move_branch(&mut f, &[0, 0], &[0, 0, 0], Place::Premise).is_err());
    assert_eq!(f, before);

    // Paste fills an empty slot, else becomes a premise, else a tree of its own.
    let mut f = abc();
    let p = add_premise(&mut f, &[0, 1]).unwrap();
    let branch = forest(r"\hypo{X}\infer1{Y}").remove(0);
    assert_eq!(paste(&mut f, Some(&p), branch.clone()).unwrap(), p);
    assert_eq!((text_at(&f, &p), text_at(&f, &[0, 1, 0, 0])), ("Y".into(), "X".into()));
    assert_eq!(paste(&mut f, Some(&[0]), branch.clone()).unwrap(), vec![0, 2]);
    assert_eq!(paste(&mut f, None, branch).unwrap(), vec![1]);
}

#[test]
fn kinds_and_navigation() {
    let mut f = forest(r"\hypo{A}");
    assert_eq!(kind(&f[0]), Kind::Hypothesis);
    set_kind(&mut f, &[0], Kind::Axiom).unwrap();
    assert_eq!((kind(&f[0]), f[0].rule), (Kind::Axiom, Rule::Single));
    assert_eq!(set_kind(&mut f, &[0], Kind::Inference).unwrap(), vec![0, 0], "an inference gets a premise");
    assert!(set_kind(&mut f, &[0], Kind::Hypothesis).is_err());

    let f = forest(r"\hypo{A}\hypo{B}\infer2{C}\hypo{D}\infer2{E}");
    assert_eq!(first_premise(&f, &[0]), Some(vec![0, 0]));
    assert_eq!(first_premise(&f, &[0, 1]), None);
    assert_eq!(parent(&[0, 0, 1]), Some(vec![0, 0]));
    assert_eq!(parent(&[0]), None);
    assert_eq!(neighbour(&f, &[0, 0], true), Some(vec![0, 1]));
    assert_eq!(neighbour(&f, &[0, 0], false), None);
    let order: Vec<String> = steps(&f).iter().map(|(p, d)| format!("{}{}", text_at(&f, p), d)).collect();
    assert_eq!(order, ["E0", "C1", "A2", "B2", "D1"]);
}

#[test]
fn limits_are_refused_not_exceeded() {
    // Depth: a chain at the layout limit takes no more steps above or below.
    let chain = String::from(r"\hypo{A}") + &r"\infer1{A}".repeat(MAX_DEPTH - 1);
    let mut f = forest(&chain);
    let top: Vec<usize> = std::iter::once(0).chain(std::iter::repeat_n(0, MAX_DEPTH - 1)).collect();
    assert!(add_premise(&mut f, &top).is_err());
    assert!(insert_below(&mut f, &[0]).is_err());
    // Width: a step takes at most MAX_PREMISES premises.
    let mut f = forest(r"\hypo{A}");
    for _ in 0..MAX_PREMISES {
        add_premise(&mut f, &[0]).unwrap();
    }
    assert!(add_premise(&mut f, &[0]).is_err());
    assert!(add_sibling(&mut f, &[0, 0], true).is_err());
    // Nodes.
    let mut f: Vec<Proof> = (0..MAX_NODES).map(|_| blank()).collect();
    assert!(add_sibling(&mut f, &[0], true).is_err());
}

#[test]
fn writes_ebproof() {
    assert_eq!(to_ebproof(&abc()), "\\begin{prooftree}\n    \\hypo{A}\n    \\hypo{B}\n  \\infer2[R]{C}\n\\end{prooftree}\n");
    let f = forest(
        r"\infer0[Ax]{\Gamma, A \vdash A}\hypo{B}\infer[rule style=double, left label=MP]2[{a]b}]{C}\ellipsis{w}{D}\infer[dashed]1{E}\infer[no rule]1{F}",
    );
    let want = "\\begin{prooftree}\n          \\infer0[Ax]{\\Gamma, A \\vdash A}\n          \\hypo{B}\n        \\infer[rule style=double, left label={MP}]2[{a]b}]{C}\n      \\ellipsis{w}{D}\n    \\infer[rule style=dashed]1{E}\n  \\infer[rule style=no rule]1{F}\n\\end{prooftree}\n";
    assert_eq!(to_ebproof(&f), want);
    // Ten premises: a braced arity. A text statement: ebproof's text template; all-math text
    // (bussproofs' `$A$`) is written as math. Two trees: two environments.
    let ten = (0..10).map(|i| format!(r"\hypo{{A_{i}}}")).collect::<String>() + r"\infer{10}{B}";
    assert!(to_ebproof(&forest(&ten)).contains(r"\infer{10}{B}"));
    let text = forest(r"\AxiomC{if $x$ then}\AxiomC{$A$}\BinaryInfC{done}\DisplayProof \AxiomC{$Z$}\DisplayProof");
    let w = to_ebproof(&text);
    assert!(
        w.contains(r"\hypo[template=\inserttext]{if $x$ then}") && w.contains(r"\hypo{A}") && w.contains(r"\infer[template=\inserttext]2{done}"),
        "{w}"
    );
    assert_eq!(w.matches(r"\begin{prooftree}").count(), 2);
    // proof.sty's math label is put in $…$ (ebproof sets labels as text).
    assert!(to_ebproof(&forest(r"\infer[\wedge I]{C}{A & B}")).contains(r"[$\wedge I$]"));
    // The preview shows empty statements.
    let mut e = abc();
    add_premise(&mut e, &[0, 0]).unwrap();
    assert!(to_ebproof_preview(&e).contains(r"\hypo{\square}"));
    assert!(to_ebproof(&e).contains(r"\hypo{}"));
}

/// A small deterministic generator (xorshift), so the property test needs no extra crate.
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

fn random_tree(r: &mut Rng, depth: usize) -> Proof {
    const STATEMENTS: [(&str, bool); 8] = [
        (r"\Gamma \vdash A", true),
        (r"A \wedge B", true),
        (r"\forall x\, P(x)", true),
        ("", true),
        (r"\neg\neg A \vDash \bot", true),
        (r"if $x$ then", false),
        (r"$A \to B$", false),
        (r"x_{i}^{2}", true),
    ];
    const LABELS: [&str; 5] = [r"$\wedge$I", "MP", "a]b", r"$\to$E$_1$", "cut"];
    let (src, math) = STATEMENTS[r.below(STATEMENTS.len() as u64)];
    let n = if depth >= 5 { 0 } else { r.below(4) };
    let premises: Vec<Proof> = (0..n).map(|_| random_tree(r, depth + 1)).collect();
    let rule = if n == 0 {
        [Rule::Leaf, Rule::Single, Rule::Double][r.below(3)]
    } else {
        let mut rules = vec![Rule::Single, Rule::Double, Rule::Dashed, Rule::None];
        if n == 1 {
            rules.push(Rule::Ellipsis);
        }
        rules[r.below(rules.len() as u64)]
    };
    let label = |r: &mut Rng| (rule != Rule::Leaf && r.below(2) == 0).then(|| Stmt { src: LABELS[r.below(5)].into(), math: false });
    let right = label(r);
    let left = if rule == Rule::Ellipsis { None } else { label(r) };
    // An ellipsis step is set as math (it takes no template option).
    let math = math || rule == Rule::Ellipsis;
    let src = if math && !src.is_empty() && src.contains('$') { "C" } else { src };
    Proof { concl: vec![Stmt { src: src.into(), math }], premises, rule, left, right }
}

#[test]
fn written_trees_read_back_the_same() {
    let mut r = Rng(0x9E37_79B9_7F4A_7C15);
    for i in 0..300 {
        let f: Vec<Proof> = (0..1 + r.below(2)).map(|_| random_tree(&mut r, 0)).collect();
        let written = to_ebproof(&f);
        let back = proof::parse(&written).unwrap_or_else(|e| panic!("#{i}: {e}\n{written}"));
        assert!(same_look(&f, &back), "#{i}: reads back differently\n{written}\n{f:#?}\n{back:#?}");
        // Writing what was read back gives the same source.
        assert_eq!(to_ebproof(&back), written, "#{i}");
    }
}

#[test]
fn opening_names_what_the_rewrite_changes() {
    let o = open(r"\begin{prooftree}\hypo{A}\infer1[R]{B}\end{prooftree}").unwrap();
    assert!(o.notices.is_empty(), "{:?}", o.notices);
    let o = open(r"\AxiomC{$A$}\RightLabel{R}\UnaryInfC{$B$}\DisplayProof").unwrap();
    assert!(o.notices.iter().any(|n| n.contains("bussproofs") && n.contains("ebproof")), "{:?}", o.notices);
    assert_eq!(o.forest[0].concl[0].src, "B");
    let o = open(r"\inferrule{A \\ B}{C \\ D}").unwrap();
    assert!(o.notices.iter().any(|n| n.contains("mathpartir")) && o.notices.iter().any(|n| n.contains("several lines")), "{:?}", o.notices);
    let o = open("\\AxiomC{A}\\dottedLine\\UnaryInfC{B} % why\n").unwrap();
    assert!(o.notices.iter().any(|n| n.contains("dotted")) && o.notices.iter().any(|n| n.contains("Comments")), "{:?}", o.notices);
    assert_eq!(o.forest[0].rule, Rule::Dashed);
    let o = open(r"\hypo{A}\rewrite{\color{red}}\infer1{B}").unwrap();
    assert!(o.notices.iter().any(|n| n.contains(r"\rewrite")), "{:?}", o.notices);
    assert!(open(r"\infer2{B}").is_err());
    assert!(open(r"x^2").is_err());
}

#[test]
fn problems_and_empty_statements() {
    let mut f = forest(r"\hypo{A}\infer1[R]{B}");
    assert!(problems(&f).is_empty());
    if let Some(n) = node_mut(&mut f, &[0, 0]) {
        n.concl[0].src = r"\frac{a}{b".into();
    }
    if let Some(n) = node_mut(&mut f, &[0]) {
        n.right = Some(Stmt { src: "}".into(), math: false });
    }
    let p = problems(&f);
    assert_eq!(p.len(), 2, "{p:?}");
    assert_eq!(p[0].0, vec![0]);
    assert!(balanced(r"\{ a \}") && balanced("{a}{b}") && !balanced("}{"));
    add_premise(&mut f, &[0]).unwrap();
    assert_eq!(empty_statements(&f), 1);
}

#[test]
fn every_template_reads_back_the_same() {
    for (name, src) in TEMPLATES {
        let t = template(name).unwrap_or_else(|| panic!("{name}"));
        let again = proof::parse(&to_ebproof(std::slice::from_ref(&t))).unwrap();
        assert!(same_look(std::slice::from_ref(&t), &again), "{name}: {src}");
    }
    assert!(template("no such rule").is_none());
}

fn measure(b: &TextBody) -> (f64, f64, f64) {
    let w = b.paragraphs.iter().map(|p| p.runs.iter().map(|r| r.text.chars().count()).sum::<usize>()).max().unwrap_or(0);
    (w as f64 * 10.0, b.paragraphs.len().max(1) as f64 * 24.0, 19.0)
}

#[test]
fn regions_name_every_step_where_it_is_drawn() {
    // F stands on D (with both labels) and E; D on A and B ∧ C.
    let f = forest(r"\hypo{A}\hypo{B \wedge C}\infer[left label=L]2[R]{D}\hypo{E}\infer2{F}");
    let src = to_ebproof(&f);
    let t = typeset(&src, &Style { font: None, size: 20.0 }, &measure).unwrap();
    for (path, _) in steps(&f) {
        let statements: Vec<_> = t.regions.iter().filter(|r| r.path == path && r.part == Part::Statement).collect();
        assert_eq!(statements.len(), 1, "{path:?}");
        let r = statements[0];
        // The region is the statement's text box.
        let drawn = t.items.iter().any(|i| match i {
            super::super::Item::Text { x, y, w, h, body } => {
                (x - r.x).abs() < 1e-9 && (y - r.y).abs() < 1e-9 && (w - r.w).abs() < 1e-9 && (h - r.h).abs() < 1e-9 && !body.text().is_empty()
            }
            super::super::Item::Line { .. } | super::super::Item::Path { .. } => false,
        });
        assert!(drawn, "{path:?}: {r:?}");
        assert!(r.x >= -1e-9 && r.y >= -1e-9 && r.x + r.w <= t.w + 1e-9 && r.y + r.h <= t.h + 1e-9);
    }
    let parts = |path: &[usize]| t.regions.iter().filter(|r| r.path == path).map(|r| r.part).collect::<Vec<_>>();
    assert_eq!(parts(&[0, 0]), [Part::Statement, Part::Rule, Part::RightLabel, Part::LeftLabel]);
    assert_eq!(parts(&[0, 0, 0]), [Part::Statement], "a hypothesis has no rule");
    // A second tree's paths start with 1.
    let two =
        typeset(&to_ebproof(&forest(r"\hypo{A}\begin{prooftree}\hypo{B}\end{prooftree}")), &Style { font: None, size: 20.0 }, &measure).unwrap();
    assert!(two.regions.iter().any(|r| r.path == [1]));
}

#[test]
fn the_reader_takes_what_the_writer_writes() {
    let f = forest(&((0..12).map(|i| format!(r"\hypo{{A_{{{i}}}}}")).collect::<String>() + r"\infer{12}[R]{B}"));
    assert_eq!(f[0].premises.len(), 12);
    let f = forest(r"\hypo[template=\inserttext]{if $x$}\infer[template=$\inserttext$]1[{a]b}]{B}");
    assert!(!f[0].premises[0].concl[0].math);
    assert!(f[0].concl[0].math);
    assert_eq!(f[0].right.as_ref().map(|r| r.src.as_str()), Some("a]b"));
    let (_, syntaxes) = proof::parse_syntaxes(r"\AxiomC{A}\UnaryInfC{B}\DisplayProof \hypo{C}\infer[\wedge]{D}{E}").unwrap();
    assert_eq!(syntaxes, ["bussproofs", "ebproof", "proof.sty"]);
}

/// The written LaTeX compiles with the real ebproof package (skipped where TeX isn't installed,
/// as on CI).
#[test]
fn written_ebproof_compiles_with_pdflatex() {
    let has_tex = std::process::Command::new("kpsewhich").arg("ebproof.sty").output().is_ok_and(|o| o.status.success() && !o.stdout.is_empty());
    if !has_tex {
        return;
    }
    let mut r = Rng(0x2545_F491_4F6C_DD1D);
    let mut body = String::new();
    for (_, src) in TEMPLATES {
        body.push_str(&to_ebproof(&forest(src)));
        body.push_str("\n\n");
    }
    for _ in 0..20 {
        body.push_str(&to_ebproof(&[random_tree(&mut r, 0)]));
        body.push_str("\n\n");
    }
    let doc = format!("\\documentclass{{article}}\n\\usepackage{{amssymb,ebproof}}\n\\begin{{document}}\n{body}\\end{{document}}\n");
    let dir = std::env::temp_dir().join(format!("deckcraft-ebproof-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("t.tex"), doc).unwrap();
    let out =
        std::process::Command::new("pdflatex").args(["-interaction=nonstopmode", "-halt-on-error", "t.tex"]).current_dir(&dir).output().unwrap();
    let log = std::fs::read_to_string(dir.join("t.log")).unwrap_or_default();
    let _ = std::fs::remove_dir_all(&dir);
    assert!(out.status.success(), "{}", log.lines().filter(|l| l.starts_with('!')).collect::<Vec<_>>().join("\n"));
}
