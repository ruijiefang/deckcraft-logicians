use craft_math::{MNode, MRun};
use deckcraft_model::text::{Align, TextBody};

use super::array::{self, GridKind};
use super::proof::{self, Rule};
use super::runs::{self, Piece};
use super::{Item, Kind, Style, Typeset, strip_math_delims, typeset};

const THIN: &str = "\u{2009}";
const MED: &str = "\u{205F}";
const THICK: &str = "\u{2005}";

fn style() -> Style {
    Style { font: Some("Liberation Serif".into()), size: 20.0 }
}

/// Half an em per character, 1.2 em per line: enough to check where things go.
fn measure(b: &TextBody) -> (f64, f64, f64) {
    let size = b.paragraphs.iter().flat_map(|p| &p.runs).find_map(|r| r.props.size).unwrap_or(20.0);
    let w = b.paragraphs.iter().map(|p| p.runs.iter().map(|r| r.text.chars().count()).sum::<usize>()).max().unwrap_or(0);
    (w as f64 * size * 0.5, b.paragraphs.len().max(1) as f64 * size * 1.2, size * 0.95)
}

fn set(src: &str) -> Typeset {
    typeset(src, &style(), &measure).unwrap_or_else(|e| panic!("{src}: {e}"))
}

fn plain(src: &str) -> String {
    runs::plain(&runs::math_pieces(src))
}

fn texts(t: &Typeset) -> Vec<(f64, f64, f64, f64, String)> {
    t.items
        .iter()
        .filter_map(|i| match i {
            Item::Text { x, y, w, h, body } => Some((*x, *y, *w, *h, body.text())),
            Item::Line { .. } | Item::Path { .. } => None,
        })
        .collect()
}

fn lines(t: &Typeset) -> Vec<(f64, f64, f64, f64, Option<&'static str>)> {
    t.items
        .iter()
        .filter_map(|i| match i {
            Item::Line { x1, y1, x2, y2, dash } => Some((*x1, *y1, *x2, *y2, *dash)),
            Item::Text { .. } | Item::Path { .. } => None,
        })
        .collect()
}

fn piece<'a>(p: &'a [Piece], text: &str) -> &'a Piece {
    p.iter().find(|x| x.text.contains(text)).unwrap_or_else(|| panic!("no {text:?} in {p:#?}"))
}

#[test]
fn logic_is_spaced_like_tex() {
    let want = format!("∀x{THIN}∃y{THIN}(P(x){MED}∧{MED}Q(y)){THICK}⊢{THICK}φ");
    assert_eq!(plain(r"\forall x\,\exists y\,(P(x)\wedge Q(y))\vdash\varphi"), want);
    assert_eq!(plain(r"\Gamma\models\neg\neg A"), format!("Γ{THICK}⊨{THICK}¬¬A"));
    // (`\lor` and `\land` come with the craft-math that has amssymb's names.)
    assert_eq!(plain(r"A\vee\neg A"), format!("A{MED}∨{MED}¬A"));
    // ⊥ is falsum where an operand goes, the relation \perp between operands.
    assert_eq!(plain(r"\Gamma\vdash\bot"), format!("Γ{THICK}⊢{THICK}⊥"));
    assert_eq!(plain(r"(\bot\to A)"), format!("(⊥{THICK}→{THICK}A)"));
    assert_eq!(plain(r"u\perp v"), format!("u{THICK}⊥{THICK}v"));
    // A leading or bracketed minus is not a binary operator.
    assert_eq!(plain("-x+1"), format!("−x{MED}+{MED}1"));
    assert_eq!(plain("(-1)"), "(−1)");
    assert_eq!(plain(r"a=-b"), format!("a{THICK}={THICK}−b"));
}

#[test]
fn variables_are_italic_and_the_rest_upright() {
    let p = runs::merge(runs::math_pieces(r"\forall x\,P(x)\wedge\sin\theta=\Gamma_1"));
    assert!(piece(&p, "x").look.italic);
    assert!(piece(&p, "θ").look.italic);
    assert!(!piece(&p, "∀").look.italic);
    assert!(!piece(&p, "∧").look.italic);
    assert!(!piece(&p, "sin").look.italic);
    assert!(!piece(&p, "Γ").look.italic);
    assert!(!piece(&p, "1").look.italic);
}

#[test]
fn an_atom_after_a_space_starts_its_own_run() {
    // In one run `¬` and `2` would take the fallback face that drew `∨` and `≤` before them.
    let p = runs::merge(runs::math_pieces(r"\neg A\vee\neg B\leq 2"));
    let texts: Vec<&str> = p.iter().map(|x| x.text.as_str()).collect();
    assert!(texts.contains(&"¬"), "{texts:?}");
    assert!(texts.contains(&"2"), "{texts:?}");
    assert!(p.iter().all(|x| !x.text.trim_end().contains(char::is_whitespace)), "{texts:?}");
    assert_eq!(texts.concat(), format!("¬A{MED}∨{MED}¬B{THICK}≤{THICK}2"));
}

#[test]
fn an_underset_operator_name_is_spaced_as_an_operator() {
    // `\underset{x}{\min} f(x)`, as craft-math parses it (built here so the test doesn't depend
    // on the craft-math version).
    let tree =
        [MNode::Lim { upper: false, e: vec![MNode::Run(MRun::plain("min"))], lim: vec![MNode::Run(MRun::new("x"))] }, MNode::Run(MRun::new("f(x)"))];
    assert_eq!(runs::plain(&runs::tree_pieces(&tree, false)), format!("minₓ{THIN}f(x)"));
    assert_eq!(plain(r"A \overset{\text{def}}{=} B"), format!("A{THICK}=^(def){THICK}B"));
}

#[test]
fn scripts_alphabets_and_structures() {
    let p = runs::math_pieces("x_i^2");
    assert_eq!(piece(&p, "i").look.baseline, -0.25);
    assert_eq!(piece(&p, "2").look.baseline, 0.3);
    assert_eq!(runs::plain(&p), "xᵢ²");
    assert_eq!(plain(r"\mathbb{N}\subseteq\mathbb{R}"), format!("ℕ{THICK}⊆{THICK}ℝ"));
    assert_eq!(plain(r"\mathcal{L}\mathfrak{g}"), "ℒ𝔤");
    assert_eq!(plain(r"\frac{a+b}{2}"), format!("(a{MED}+{MED}b)/2"));
    assert_eq!(plain(r"\sqrt{x}+\sqrt[3]{y}"), format!("√x{MED}+{MED}∛y"));
    assert_eq!(plain(r"\sum_{i=1}^{n} i"), format!("∑ᵢ₌₁ⁿ{THIN}i"));
    assert_eq!(plain(r"\hat{x}\bar{y}"), "x\u{302}y\u{305}");
    // \overset, \underset and \stackrel hang their label on the symbol.
    let p = runs::math_pieces(r"A\overset{\text{def}}{\iff}B");
    assert_eq!(piece(&p, "def").look.baseline, 0.3);
    assert_eq!(runs::plain(&p), format!("A{THICK}⟺^(def){THICK}B"));
    let p = runs::math_pieces(r"a\underset{n}{\to}b");
    assert_eq!(piece(&p, "n").look.baseline, -0.25);
    assert_eq!(plain(r"x\stackrel{?}{=}y"), format!("x{THICK}=^?{THICK}y"));
}

#[test]
fn text_mode_with_math_inside() {
    let p = runs::text_pieces(r"$\wedge$I");
    assert_eq!(p.iter().map(|x| x.text.as_str()).collect::<String>(), "∧I");
    assert!(p.iter().all(|x| !x.look.italic));
    let p = runs::text_pieces(r"\textsc{mp} and \emph{cut} -- \textbf{x}");
    assert!(piece(&p, "mp").look.small_caps);
    assert!(piece(&p, "cut").look.italic);
    assert!(piece(&p, "x").look.bold);
    assert!(p.iter().any(|x| x.text.contains('–')));
    let p = runs::text_pieces(r"\(\to\)E");
    assert_eq!(p.iter().map(|x| x.text.as_str()).collect::<String>(), "→E");
}

#[test]
fn ebproof_trees() {
    let t = proof::parse(
        r"\begin{prooftree}
            \hypo{\Gamma \vdash A}
            \hypo{\Gamma \vdash B}
            \infer2[$\wedge$I]{\Gamma \vdash A \wedge B}
            \infer1[$\wedge$E]{\Gamma \vdash A}
          \end{prooftree}",
    )
    .unwrap();
    assert_eq!(t.len(), 1);
    let root = &t[0];
    assert_eq!(root.rule, Rule::Single);
    assert_eq!(root.right.as_ref().map(|r| r.src.as_str()), Some(r"$\wedge$E"));
    assert_eq!(root.premises.len(), 1);
    assert_eq!(root.premises[0].premises.len(), 2);
    assert_eq!(root.premises[0].premises[0].rule, Rule::Leaf);
    // Capitalised commands, axioms, options and ellipses.
    let t = proof::parse(r"\Hypo{A}\Infer[double]1{B}\Infer0[ax]{C}\Infer[left label=L, dashed]2{D}\ellipsis{many}{E}\infer[no rule]1{F}").unwrap();
    let f = &t[0];
    assert_eq!(f.rule, Rule::None);
    let e = &f.premises[0];
    assert_eq!((e.rule, e.right.as_ref().map(|r| r.src.as_str())), (Rule::Ellipsis, Some("many")));
    let d = &e.premises[0];
    assert_eq!((d.rule, d.left.as_ref().map(|r| r.src.as_str())), (Rule::Dashed, Some("L")));
    assert_eq!(d.premises[0].rule, Rule::Double);
    assert_eq!((d.premises[1].rule, d.premises[1].premises.len()), (Rule::Single, 0));
    // A step with too few premises is an error, not a crash.
    let err = proof::parse(r"\hypo{A}\infer2{B}").unwrap_err();
    assert!(err.contains("needs 2"), "{err}");
}

#[test]
fn bussproofs_trees() {
    let t = proof::parse(
        r"\begin{prooftree}
            \AxiomC{$A$}
            \AxiomC{$B$}
            \RightLabel{$\wedge$I}
            \BinaryInfC{$A \wedge B$}
            \LeftLabel{\scriptsize L}\doubleLine
            \UnaryInfC{$A$}
            \noLine
            \UnaryInfC{$\vdots$}
          \end{prooftree}",
    )
    .unwrap();
    let top = &t[0];
    assert_eq!(top.rule, Rule::None);
    let a = &top.premises[0];
    assert_eq!(a.rule, Rule::Double);
    assert!(a.left.is_some());
    let and = &a.premises[0];
    assert_eq!((and.premises.len(), and.right.as_ref().map(|r| r.src.as_str())), (2, Some(r"$\wedge$I")));
    assert!(!and.concl[0].math, "bussproofs statements are text mode");
    // Two proofs separated by \DisplayProof.
    assert_eq!(proof::parse(r"\AXC{A}\UIC{B}\DP \AXC{C}\UIC{D}\DisplayProof").unwrap().len(), 2);
}

#[test]
fn mathpartir_and_proof_sty_trees() {
    let t = proof::parse(r"\inferrule*[right=App]{\Gamma \vdash e_1 : A \to B \\ \Gamma \vdash e_2 : A}{\Gamma \vdash e_1\,e_2 : B}").unwrap();
    assert_eq!(t[0].premises.len(), 2);
    assert_eq!(t[0].right.as_ref().map(|r| r.src.as_str()), Some("App"));
    let t = proof::parse(r"\inferrule{\inferrule{A}{B} \and C}{D \\ E}").unwrap();
    assert_eq!((t[0].premises.len(), t[0].premises[0].premises.len(), t[0].concl.len()), (2, 1, 2));
    let t = proof::parse(r"\infer[\wedge I]{A \wedge B}{A & \infer={B}{C}}").unwrap();
    assert_eq!(t[0].right.as_ref().map(|r| (r.src.as_str(), r.math)), Some((r"\wedge I", true)));
    assert_eq!(t[0].premises.len(), 2);
    assert_eq!(t[0].premises[1].rule, Rule::Double);
    assert_eq!(proof::parse(r"\deduce{B}{A}").unwrap()[0].rule, Rule::None);
}

#[test]
fn proof_layout_puts_the_rule_between_premises_and_conclusion() {
    let t = set(r"\hypo{A \to B}\hypo{A}\infer2[MP]{B}");
    assert_eq!(t.kind, Kind::Proof);
    let tx = texts(&t);
    let ln = lines(&t);
    assert_eq!((tx.len(), ln.len()), (4, 1));
    let find = |s: &str| tx.iter().find(|x| x.4 == s).cloned().unwrap_or_else(|| panic!("{s} in {tx:?}"));
    let (p1, p2, c, label) = (find(&format!("A{THICK}→{THICK}B")), find("A"), find("B"), find("MP"));
    let (x1, y, x2, _, dash) = ln[0];
    assert_eq!(dash, None);
    // Premises side by side on one line, above the rule; the conclusion under it, centred.
    assert_eq!(p1.1, p2.1);
    assert!(p1.1 + p1.3 <= y && y <= c.1, "{tx:?} {ln:?}");
    assert!((x1 - p1.0).abs() < 1e-9 && (x2 - (p2.0 + p2.2)).abs() < 1e-9, "the rule spans the premises");
    assert!(((c.0 + c.2 / 2.0) - (x1 + x2) / 2.0).abs() < 1e-9);
    // The label right of the rule, centred on it, and smaller.
    assert!(label.0 > x2 && label.1 < y && label.1 + label.3 > y);
    assert!(label.3 < c.3);
    // Everything inside the typeset's box.
    for (x, y, w, h, s) in &tx {
        assert!(*x >= -1e-9 && *y >= -1e-9 && x + w <= t.w + 1e-9 && y + h <= t.h + 1e-9, "{s} outside");
    }
}

#[test]
fn a_wide_conclusion_centres_its_premises() {
    let t = set(r"\hypo{A}\infer1{A \wedge A \wedge A \wedge A}");
    let tx = texts(&t);
    let (a, c) = (&tx[0], &tx[1]);
    let (x1, _, x2, _, _) = lines(&t)[0];
    assert!((x1 - c.0).abs() < 1e-9 && (x2 - (c.0 + c.2)).abs() < 1e-9, "the rule spans the wider conclusion");
    assert!(((a.0 + a.2 / 2.0) - (c.0 + c.2 / 2.0)).abs() < 1e-9);
    // Double rules have two lines; ellipses a vertical dotted one.
    assert_eq!(lines(&set(r"\hypo{A}\infer[double]1{B}")).len(), 2);
    let e = lines(&set(r"\hypo{A}\ellipsis{}{B}"));
    assert!(e.len() == 1 && e[0].0 == e[0].2 && e[0].4 == Some("sysDot"));
}

#[test]
fn truth_table_grid() {
    let src = r"\begin{array}{cc|c}
        p & q & p \wedge q \\ \hline
        T & T & T \\
        T & F & F
      \end{array}";
    let g = array::parse(src).unwrap();
    assert_eq!((g.kind, g.cols.len(), g.vrules.clone(), g.rows.len()), (GridKind::Array, 3, vec![2], 3));
    assert_eq!(g.rows[1].rules_above, vec![(0, 2)]);
    let t = set(src);
    assert_eq!(t.kind, Kind::Grid);
    assert_eq!(texts(&t).len(), 9);
    let ln = lines(&t);
    let v = ln.iter().find(|l| l.0 == l.2).unwrap();
    assert!(v.1 == 0.0 && v.3 == t.h, "the column rule runs the full height");
    let h = ln.iter().find(|l| l.1 == l.3).unwrap();
    assert!(h.0 == 0.0 && h.2 == t.w, "\\hline runs the full width");
    // The rule sits between the second and third columns.
    let third = texts(&t).iter().find(|x| x.4 == format!("p{MED}∧{MED}q")).cloned().unwrap();
    assert!(v.0 < third.0);
}

#[test]
fn tabular_multicolumn_cline_and_align() {
    let g =
        array::parse(r"\begin{tabular}{|l|*{2}{c}|} \hline \multicolumn{2}{|c|}{Head} & x \\ \cline{2-3} a & b & c \\ \hline \end{tabular}").unwrap();
    assert_eq!((g.kind, g.cols.len(), g.vrules.clone()), (GridKind::Tabular, 3, vec![0, 1, 3]));
    assert_eq!((g.rows[0].cells[0].span, g.rows[0].cells[0].align), (2, Some(Align::Center)));
    assert_eq!((g.rows[1].rules_above.clone(), g.rules_below.clone()), (vec![(1, 2)], vec![(0, 2)]));
    let t = set(r"\begin{tabular}{|l|*{2}{c}|} \hline \multicolumn{2}{|c|}{Head} & x \\ \cline{2-3} a & b & c \\ \hline \end{tabular}");
    assert_eq!(texts(&t).len(), 5);
    // align: right/left pairs, the right-hand side spaced as after `{}`.
    let g = array::parse(r"\begin{align*} \neg\neg A &\equiv A \\ &\to \top \end{align*}").unwrap();
    assert_eq!((g.kind, g.cols.clone()), (GridKind::Align, vec![Align::Right, Align::Left]));
    assert_eq!(runs::plain(&runs::math_pieces_lead(r"\equiv A")), format!("{THICK}≡{THICK}A"));
    assert!(set(r"\[ \begin{gather} a \\ b \end{gather} \]").kind == Kind::Grid);
}

#[test]
fn formulas_and_delimiters() {
    assert_eq!(strip_math_delims(r"$$ x $$"), "x");
    assert_eq!(strip_math_delims(r"\[x\]"), "x");
    assert_eq!(strip_math_delims(r"\begin{equation*}x\end{equation*}"), "x");
    assert_eq!(strip_math_delims(r"$a$ and $b$"), r"$a$ and $b$");
    let t = set(r"$\forall x\,\exists y\; R(x,y)$");
    assert_eq!(t.kind, Kind::Formula);
    assert_eq!(texts(&t).len(), 1);
    assert_eq!(texts(&set(r"a \\ b")).len(), 1, "lines of one formula share a text box");
    assert!(typeset("  ", &style(), &measure).is_err());
    assert!(typeset("x", &Style { size: f64::NAN, ..style() }, &measure).is_err());
}

#[test]
fn hostile_input_never_crashes() {
    let deep_chain = String::from(r"\hypo{A}") + &r"\infer1{A}".repeat(3_000);
    let nested_sty = r"\infer{a}{".repeat(500) + &"}".repeat(500);
    let nested_rule = r"\inferrule{".repeat(300) + &"}{x}".repeat(300);
    let wide = r"\hypo{A}".repeat(1_500) + r"\infer9{B}";
    let cases = [
        deep_chain.as_str(),
        nested_sty.as_str(),
        nested_rule.as_str(),
        wide.as_str(),
        r"\infer9",
        r"\infer{",
        r"\hypo{{{{",
        r"\begin{array}{",
        r"\begin{array}{*{999999}{c|}} a \end{array}",
        r"\begin{tabular}{l} \multicolumn{99999}{c}{x} \end{tabular}",
        r"\begin{array}{c} \cline{5-2} a \\ \cline{-} \end{array}",
        r"\AxiomC{$A",
        r"\BinaryInfC{B}",
        r"\inferrule*[right=]{}{}",
        "$$$$$$",
        r"\(\(",
        "\\",
        "}{}{][",
        r"\frac{\frac{\frac{a}{b}}{c}}{d}^{^{^{}}}",
    ];
    for src in cases {
        let _ = typeset(src, &style(), &measure);
        let _ = runs::text_pieces(src);
    }
    // Near the node cap (1,901 steps): refused as too deep to lay out, not cut off silently.
    let near_cap = String::from(r"\hypo{A}") + &r"\infer1{A}".repeat(1_900);
    let err = typeset(&near_cap, &style(), &measure).unwrap_err();
    assert!(err.contains("1901 steps deep"), "{err}");
    // At the layout depth limit every step is drawn.
    let at_limit = String::from(r"\hypo{A}") + &r"\infer1{A}".repeat(super::layout::MAX_DEPTH - 1);
    let t = typeset(&at_limit, &style(), &measure).unwrap();
    assert_eq!(lines(&t).len(), super::layout::MAX_DEPTH - 1);
    let long = "x".repeat(super::MAX_SOURCE + 1);
    assert!(typeset(&long, &style(), &measure).is_err());
}

#[test]
fn a_hyphen_in_math_is_a_minus_sign() {
    let text = |p: Vec<super::runs::Piece>| p.iter().map(|x| x.text.as_str()).collect::<String>();
    assert!(text(super::runs::math_pieces("a - 1")).contains('\u{2212}'));
    assert!(!text(super::runs::math_pieces("a - 1")).contains('-'));
    let t = text(super::runs::text_pieces("well-known $x-1$"));
    assert!(t.contains("well-known") && t.contains('\u{2212}'), "{t}");
}
