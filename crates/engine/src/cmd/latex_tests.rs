use deckcraft_model::text::RunKind;
use deckcraft_model::{Shape, ShapeKind};
use serde_json::json;

use super::{SOURCE_TAG, presentation_omml, tag};
use crate::Session;

fn session() -> Session {
    Session::with_new()
}

fn shape(s: &Session, id: &serde_json::Value) -> Shape {
    let id = deckcraft_model::ShapeId(id.as_u64().unwrap() as u32);
    s.doc().unwrap().shape(id).cloned().unwrap()
}

fn count(s: &Session) -> usize {
    s.doc().unwrap().doc.slides[0].shapes.len()
}

const PROOF: &str = r"\begin{prooftree}
  \hypo{\Gamma \vdash A}
  \hypo{\Gamma \vdash B}
  \infer2[$\wedge$I]{\Gamma \vdash A \wedge B}
\end{prooftree}";

#[test]
fn a_formula_is_one_text_box_with_its_source() {
    let mut s = session();
    let before = count(&s);
    let v = s.execute("insert.latex", &json!({"latex": r"\forall x\,(P(x)\to Q(x))", "rect": [40, 50, 0, 0]})).unwrap();
    assert_eq!(v["kind"], "formula");
    let sh = shape(&s, &v["id"]);
    assert!(sh.text_box && !sh.is_group());
    assert_eq!(tag(&sh, SOURCE_TAG), Some(r"\forall x\,(P(x)\to Q(x))"));
    let x = sh.xfrm.unwrap();
    assert_eq!((x.x, x.y), (40.0, 50.0));
    assert!(x.w > 20.0 && x.h > 10.0, "sized by its text: {x:?}");
    let runs = &sh.text.as_ref().unwrap().paragraphs[0].runs;
    let text: String = runs.iter().map(|r| r.text.as_str()).collect();
    assert!(text.starts_with('∀') && text.contains('→'), "{text}");
    assert!(runs.iter().any(|r| r.text == "x" && r.props.italic == Some(true)));
    assert!(runs.iter().all(|r| r.props.no_proof == Some(true)), "math is not spell-checked");
    let got = s.execute("latex.get", &json!({})).unwrap();
    assert_eq!((got["latex"].as_str(), got["kind"].as_str()), (Some(r"\forall x\,(P(x)\to Q(x))"), Some("typeset")));
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(count(&s), before);
}

#[test]
fn a_proof_tree_is_a_group_of_statements_and_rules() {
    let mut s = session();
    let v = s.execute("insert.latex", &json!({"latex": PROOF, "size": 20})).unwrap();
    assert_eq!(v["kind"], "proof");
    let g = shape(&s, &v["id"]);
    let ShapeKind::Group { children, .. } = &g.kind else { panic!("{g:#?}") };
    let texts: Vec<String> = children.iter().filter_map(|c| c.text.as_ref().map(|t| t.text())).collect();
    let rules = children.iter().filter(|c| c.geom.preset_name() == Some("line")).count();
    assert_eq!((texts.len(), rules), (4, 1), "{texts:?}");
    assert!(texts.iter().any(|t| t.contains('⊢') && t.contains('∧')), "{texts:?}");
    // Every shape has its own id.
    let mut ids: Vec<u32> = children.iter().map(|c| c.id.0).chain([g.id.0]).collect();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), children.len() + 1);
    assert!(ids.iter().all(|i| *i > 0));
    assert!(g.name.starts_with("Proof Tree"), "{}", g.name);
    // The source comes back with a statement (inside the group) selected.
    let child = children[0].id.0;
    let got = s.execute("latex.get", &json!({"id": child})).unwrap();
    assert_eq!(got["latex"].as_str(), Some(PROOF));
}

#[test]
fn latex_set_typesets_again_in_place() {
    let mut s = session();
    let v = s.execute("insert.latex", &json!({"latex": PROOF, "rect": [30, 40, 0, 0]})).unwrap();
    let id = v["id"].clone();
    let longer = r"\hypo{A}\hypo{A \to B}\infer2[MP]{B}\infer1[$\to$I]{(A \to B) \to B}";
    let w = s.execute("latex.set", &json!({"latex": longer})).unwrap();
    assert_eq!(w["id"], id);
    let g = shape(&s, &id);
    assert_eq!(tag(&g, SOURCE_TAG), Some(longer));
    let x = g.xfrm.unwrap();
    assert_eq!((x.x, x.y), (30.0, 40.0));
    assert_eq!(g.children().iter().filter(|c| c.geom.preset_name() == Some("line")).count(), 2);
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(tag(&shape(&s, &id), SOURCE_TAG), Some(PROOF));
}

#[test]
fn grids_have_their_rules() {
    let mut s = session();
    let v = s.execute("insert.latex", &json!({"latex": r"\begin{array}{c|c} p & \neg p \\ \hline T & F \\ F & T \end{array}"})).unwrap();
    assert_eq!(v["kind"], "grid");
    let g = shape(&s, &v["id"]);
    assert_eq!(g.children().iter().filter(|c| c.geom.preset_name() == Some("line")).count(), 2);
    assert_eq!(g.children().iter().filter(|c| c.text.is_some()).count(), 6);
}

#[test]
fn bad_latex_is_an_error_and_changes_nothing() {
    let mut s = session();
    let before = count(&s);
    let e = s.execute("insert.latex", &json!({"latex": r"\hypo{A}\infer3{B}"})).unwrap_err();
    assert!(e.to_string().contains("needs 3"), "{e}");
    assert!(s.execute("insert.latex", &json!({})).is_err());
    assert!(s.execute("insert.equation", &json!({"latex": " "})).is_err());
    assert_eq!(count(&s), before);
}

#[test]
fn an_equation_is_office_math_with_its_source() {
    let mut s = session();
    let v = s.execute("insert.equation", &json!({"latex": r"\Gamma \models \neg\neg\varphi \to x^2"})).unwrap();
    let sh = shape(&s, &v["id"]);
    let run = &sh.text.as_ref().unwrap().paragraphs[0].runs[0];
    let RunKind::Math { omml } = &run.kind else { panic!("{run:#?}") };
    assert!(omml.starts_with("<m:oMathPara xmlns:m=\"http://schemas.openxmlformats.org/officeDocument/2006/math\">"), "{omml}");
    assert!(!omml.contains("w:"), "no WordprocessingML in a slide: {omml}");
    assert!(omml.contains("<m:sSup>"), "{omml}");
    assert!(run.text.contains('⊨') && run.text.contains('²'), "{}", run.text);
    assert_eq!(run.props.no_proof, Some(true));
    let got = s.execute("latex.get", &json!({})).unwrap();
    assert_eq!((got["latex"].as_str(), got["kind"].as_str()), (Some(r"\Gamma \models \neg\neg\varphi \to x^2"), Some("equation")));
    s.execute("latex.set", &json!({"latex": r"a \vdash b"})).unwrap();
    let sh = shape(&s, &v["id"]);
    assert!(sh.text.as_ref().unwrap().text().contains('⊢'));
}

#[test]
fn latex_survives_a_pptx_round_trip() {
    let mut s = session();
    s.execute("insert.latex", &json!({"latex": PROOF})).unwrap();
    s.execute("insert.equation", &json!({"latex": r"\exists x\, P(x)"})).unwrap();
    let bytes = deckcraft_pptx::export(&s.doc().unwrap().doc).unwrap();
    let back = deckcraft_pptx::import(&bytes).unwrap();
    let shapes = &back.slides[0].shapes;
    let g = shapes.iter().find(|sh| sh.is_group()).expect("the proof tree");
    assert_eq!(tag(g, SOURCE_TAG), Some(PROOF));
    assert_eq!(g.children().len(), 5);
    let eq = shapes.iter().find(|sh| super::math_run(sh).is_some()).expect("the equation");
    let (omml, _) = super::math_run(eq).unwrap();
    assert!(omml.contains("∃"), "{omml}");
    assert_eq!(tag(eq, SOURCE_TAG), Some(r"\exists x\, P(x)"));
}

#[test]
fn word_run_properties_are_removed_from_omml() {
    let xml = r#"<m:oMath><m:r><w:rPr><w:rFonts w:ascii="Cambria Math"/><w:i/></w:rPr><m:t>x</m:t></m:r><m:r><w:rPr/><m:t>y</m:t></m:r><w:rPrX/></m:oMath>"#;
    assert_eq!(
        presentation_omml(xml),
        r#"<m:oMath xmlns:m="http://schemas.openxmlformats.org/officeDocument/2006/math"><m:r><m:t>x</m:t></m:r><m:r><m:t>y</m:t></m:r><w:rPrX/></m:oMath>"#
    );
}

/// Every element an equation made from LaTeX writes is in the math namespace, which it declares:
/// fractions, scripts, radicals, big operators, delimiters, matrices, accents, bars, boxes, text,
/// functions, limits and equation arrays.
#[test]
fn equation_omml_uses_only_the_math_namespace() {
    let sources = [
        r"\frac{a}{b} + \binom{n}{k}",
        r"x_i^2 + {}^{14}C",
        r"\sqrt[3]{x} + \sqrt{y}",
        r"\sum_{i=1}^{n} i + \int_0^1 f + \prod_k a_k",
        r"\left( a \middle| b \right] + \lvert x \rvert",
        r"\begin{pmatrix} a & b \\ c & d \end{pmatrix} + \begin{array}{l|r} \top & \bot \\ \hline p & q \end{array}",
        r"\hat{x} + \vec{v} + \overline{AB} + \underline{c} + \boxed{z}",
        r"\text{if } x \mathbb{R} \mathcal{L} \mathbf{v} \operatorname{rank} M",
        r"\sin\theta + \lim_{n\to\infty} a_n + \overbrace{a+b}^{k}",
        r"A \overset{\text{def}}{\iff} B \underset{n}{\to} C \stackrel{?}{=} D",
        r"\begin{cases} 1 & x > 0 \\ 0 & \text{else} \end{cases}",
        r"\begin{aligned} a &= b \\ &= c \end{aligned}",
        r"\Gamma \not\models \neg\varphi \vdash \bot \quad \foo",
    ];
    for src in sources {
        let RunKind::Math { omml } = super::equation_run(src, 24.0).kind else { panic!("{src}") };
        assert!(omml.starts_with("<m:oMathPara xmlns:m="), "{src}: {omml}");
        let mut rest = omml.as_str();
        while let Some(i) = rest.find('<') {
            rest = &rest[i + 1..];
            let name: String = rest.trim_start_matches('/').chars().take_while(|c| !matches!(c, ' ' | '>' | '/')).collect();
            assert!(name.starts_with("m:"), "{src}: <{name}> in {omml}");
        }
    }
}
