//! LaTeX on a slide, two ways:
//!
//! - `insert.latex`: typeset by DeckCraft ([`crate::typeset`]) as text boxes and lines: formulas,
//!   arrays and tables with their rules, aligned lines and proof trees (ebproof, bussproofs,
//!   mathpartir, proof.sty). It looks the same in DeckCraft, in PDF and in PowerPoint.
//! - `insert.equation`: an Office Math equation (OMML) PowerPoint edits as an equation;
//!   DeckCraft shows its linear form.
//!
//! Both keep their source in the `DECKCRAFT_LATEX` tag (a `p:custDataLst` tag, which PowerPoint
//! keeps), so `latex.get` reads it back and `latex.set` typesets it again in place.

use craft_math::{Math, math_latex, omml};
use deckcraft_color::{Rgba, SchemeSlot};
use deckcraft_geom::{BezPath, KurboShape as _, PathEl, Rect, Xfrm};
use deckcraft_model::style::{Dash, LineCap, LineJoin};
use deckcraft_model::text::{Align, AutoFit, Paragraph, Run, RunKind, RunProps, TextBody};
use deckcraft_model::{ColorRef, CustomPath, Fill, Geom, Line, Presentation, Shape, ShapeId, ShapeKind, ShapeStyle};
use serde_json::{Value, json};

use super::{CommandSpec, NO_ID_SPACE, bad, cmd, current_shapes, f64_param, has_selection, has_slide, rect_param, shape_of, str_param, targets};
use crate::typeset::{self, Item, Kind, Paint, Stroke, Style, Typeset};
use crate::{Result, Session};

/// The tag holding a LaTeX shape's source.
pub const SOURCE_TAG: &str = "DECKCRAFT_LATEX";
/// The tag holding the size and font it was typeset in (`size=24;font=Liberation Serif`).
pub const STYLE_TAG: &str = "DECKCRAFT_LATEX_STYLE";
const DEFAULT_SIZE: f64 = 24.0;
/// What an algorithm is set in when no size is given.
pub const ALGORITHM_SIZE: f64 = 16.0;
/// An OFL serif with italics (from craft-fonts); symbols it lacks come from font fallback.
pub const DEFAULT_FONT: &str = "Liberation Serif";
const MATH_NS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/math";
/// PowerPoint's equation font; DeckCraft draws it with its substitute.
const EQUATION_FONT: &str = "Cambria Math";

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            "insert.latex",
            "LaTeX",
            ["Insert", "Symbols"],
            None,
            "{latex: string (a formula; an array, tabular, align or gather environment; proof trees in ebproof, bussproofs, mathpartir or proof.sty syntax; a tikz-cd commutative diagram; or an algpseudocode algorithm), rect?: [x,y,w,h] (x and y place it; the size comes from the text), size?: pt (24; 16 for an algorithm), font?: family (Liberation Serif; \"\" for the theme's)} → {id, kind: formula|grid|proof|diagram|algorithm}",
            has_slide,
            insert_latex
        ),
        cmd!(
            "insert.equation",
            "Equation",
            ["Insert", "Symbols"],
            None,
            "{latex: string, rect?: [x,y,w,h], size?: pt (24)} → {id} | {inline: true} (an Office Math equation PowerPoint edits as an equation; DeckCraft draws its linear form. While text is being edited it goes in at the caret, in the text's size unless size is given)",
            has_slide,
            insert_equation
        ),
        cmd!(
            query "latex.get",
            "LaTeX Source",
            [],
            None,
            "{id?} → {id, latex, kind: typeset|equation, size?, font?} (the selected LaTeX shape or equation; size and font: what a typeset one was set in)",
            has_selection,
            get
        ),
        cmd!(
            "latex.set",
            "Edit LaTeX",
            ["Insert", "Symbols"],
            None,
            "{latex: string, id?, size?: pt, font?: family} (typesets a LaTeX shape again where it is, or rewrites an equation)",
            has_selection,
            set
        ),
    ]
}

fn style_param(p: &Value, fallback: Option<Style>) -> Style {
    let base = fallback.unwrap_or(Style { font: Some(DEFAULT_FONT.into()), size: DEFAULT_SIZE });
    let size = f64_param(p, "size").unwrap_or(base.size);
    let font = match str_param(p, "font") {
        Some("") => None,
        Some(f) => Some(f.to_string()),
        None => base.font,
    };
    Style { font, size }
}

fn style_tag(s: &Style) -> String {
    format!("size={};font={}", s.size, s.font.as_deref().unwrap_or(""))
}

fn parse_style_tag(t: &str) -> Option<Style> {
    let mut style = Style { font: None, size: DEFAULT_SIZE };
    for part in t.split(';') {
        match part.split_once('=') {
            Some(("size", v)) => style.size = v.trim().parse().ok().filter(|v: &f64| v.is_finite())?,
            Some(("font", v)) => style.font = (!v.is_empty()).then(|| v.to_string()),
            _ => {}
        }
    }
    Some(style)
}

fn tag<'a>(sh: &'a Shape, name: &str) -> Option<&'a str> {
    sh.tags.iter().find(|(n, _)| n == name).map(|(_, v)| v.as_str())
}

/// Typesets `src` with the current slide's fonts doing the measuring.
fn typeset_here(s: &Session, src: &str, style: &Style) -> std::result::Result<Typeset, String> {
    typeset_on(s.doc().map_err(|e| e.to_string())?, src, style)
}

/// Typesets `src` with `st`'s current slide's fonts doing the measuring.
fn typeset_on(st: &crate::DocState, src: &str, style: &Style) -> std::result::Result<Typeset, String> {
    let ctx = super::text::ctx_for(&st.doc, &st.selection).ok_or("no slide")?;
    let fields = deckcraft_text::NoFields;
    let rect = Rect::new(0.0, 0.0, 10_000.0, 10_000.0);
    let measure = |b: &TextBody| {
        let sh = Shape { text_box: true, text: Some(b.clone()), geom: Geom::preset("rect"), ..Default::default() };
        let opts = deckcraft_text::Opts { rect, fields: &fields, prompt_color: None, no_shrink: true };
        let base = deckcraft_text::layout(&ctx, &sh, b, &opts).lines.first().map_or(0.0, |l| l.baseline - rect.y0);
        (deckcraft_text::fit_width(&ctx, &sh, b, rect, &fields), deckcraft_text::fit_height(&ctx, &sh, b, rect, &fields), base)
    };
    typeset::typeset(src, style, &measure)
}

fn rule_shape(x1: f64, y1: f64, x2: f64, y2: f64, dash: Option<&str>, width: f64) -> Shape {
    Shape {
        name: "Rule".into(),
        xfrm: Some(Xfrm::new(x1.min(x2), y1.min(y2), (x2 - x1).abs(), (y2 - y1).abs())),
        geom: Geom::preset("line"),
        line: Some(Line {
            fill: Some(Fill::solid(ColorRef::scheme(SchemeSlot::Tx1))),
            width: Some(width),
            dash: dash.map(|d| Dash::Preset { name: d.into() }),
            ..Default::default()
        }),
        style: Some(ShapeStyle::line(SchemeSlot::Tx1)),
        ..Default::default()
    }
}

fn paint(p: Paint) -> ColorRef {
    match p {
        Paint::Text => ColorRef::scheme(SchemeSlot::Tx1),
        Paint::Background => ColorRef::scheme(SchemeSlot::Bg1),
        Paint::Rgb([r, g, b]) => ColorRef::rgb(Rgba::rgb(r, g, b)),
    }
}

/// `path`'s drawing as custom geometry's path text, relative to (`x0`, `y0`).
fn path_text(path: &BezPath, x0: f64, y0: f64) -> String {
    use std::fmt::Write as _;
    let mut d = String::new();
    let pt = |d: &mut String, p: deckcraft_geom::Point| {
        let _ = write!(d, " {:.3} {:.3}", p.x - x0, p.y - y0);
    };
    for el in path.elements() {
        match *el {
            PathEl::MoveTo(p) => {
                d.push_str(" M");
                pt(&mut d, p);
            }
            PathEl::LineTo(p) => {
                d.push_str(" L");
                pt(&mut d, p);
            }
            PathEl::QuadTo(a, p) => {
                d.push_str(" Q");
                pt(&mut d, a);
                pt(&mut d, p);
            }
            PathEl::CurveTo(a, b, p) => {
                d.push_str(" C");
                pt(&mut d, a);
                pt(&mut d, b);
                pt(&mut d, p);
            }
            PathEl::ClosePath => d.push_str(" Z"),
        }
    }
    d.trim().to_string()
}

/// A diagram's path as a shape of custom geometry: an arrow, its tips, a frame, or background.
fn path_shape(path: BezPath, stroke: Option<Stroke>, fill: Option<Paint>) -> Option<Shape> {
    let r = path.bounding_box();
    if !(r.x0.is_finite() && r.y0.is_finite() && r.x1.is_finite() && r.y1.is_finite()) || path.elements().is_empty() {
        return None;
    }
    // A straight arrow has no height (or width): its box keeps a hair of one.
    let (w, h) = (r.width().max(0.01), r.height().max(0.01));
    let d = path_text(&path, r.x0, r.y0);
    let line = match &stroke {
        Some(s) => Line {
            fill: Some(Fill::solid(paint(s.paint))),
            width: Some(s.width),
            dash: s.dash.map(|(on, off)| Dash::Custom { pattern: vec![(on, off)] }),
            cap: Some(if s.round { LineCap::Round } else { LineCap::Flat }),
            join: Some(LineJoin::Round),
            ..Default::default()
        },
        None => Line { fill: Some(Fill::None), ..Default::default() },
    };
    let fill_mode = if fill.is_some() { deckcraft_geom::preset::FillMode::Norm } else { deckcraft_geom::preset::FillMode::None };
    Some(Shape {
        name: if fill == Some(Paint::Background) { "Background".into() } else { "Arrow".into() },
        xfrm: Some(Xfrm::new(r.x0, r.y0, w, h)),
        geom: Geom::Custom { paths: vec![CustomPath { w, h, d, fill: fill_mode, stroke: stroke.is_some() }] },
        fill: Some(fill.map_or(Fill::None, |p| Fill::solid(paint(p)))),
        line: Some(line),
        ..Default::default()
    })
}

/// The typeset as shapes: a text box for a formula, else a group of text boxes and lines.
fn build(t: Typeset, at: (f64, f64), src: &str, style: &Style) -> Shape {
    let thickness = t.thickness;
    let mut kids: Vec<Shape> = t
        .items
        .into_iter()
        .filter_map(|it| match it {
            Item::Text { x, y, w, h, body } => Some(Shape {
                name: "Text".into(),
                xfrm: Some(Xfrm::new(x, y, w, h)),
                text_box: true,
                geom: Geom::preset("rect"),
                text: Some(body),
                ..Default::default()
            }),
            Item::Line { x1, y1, x2, y2, dash } => Some(rule_shape(x1, y1, x2, y2, dash, thickness)),
            Item::Path { path, stroke, fill } => path_shape(path, stroke, fill),
        })
        .collect();
    let tags = vec![(SOURCE_TAG.to_string(), src.to_string()), (STYLE_TAG.to_string(), style_tag(style))];
    let descr = if t.kind == Kind::Algorithm {
        typeset::algo::open(src).map(|o| typeset::algo::describe(&o.algorithm)).unwrap_or_default().chars().take(1_000).collect()
    } else if t.kind == Kind::Diagram {
        typeset::cd::open(src).map(|o| typeset::cd::describe(&o.diagram)).unwrap_or_default().chars().take(1_000).collect()
    } else {
        src.chars().take(1_000).collect::<String>()
    };
    if t.kind == Kind::Formula
        && kids.len() == 1
        && let Some(mut sh) = kids.pop()
    {
        if let Some(x) = sh.xfrm.as_mut() {
            x.x = at.0;
            x.y = at.1;
        }
        sh.name = String::new();
        sh.tags = tags;
        sh.descr = descr;
        return sh;
    }
    let (w, h) = (t.w.max(1.0), t.h.max(1.0));
    Shape {
        xfrm: Some(Xfrm::new(at.0, at.1, w, h)),
        kind: ShapeKind::Group { children: kids, child: Xfrm::new(0.0, 0.0, w, h) },
        tags,
        descr,
        ..Default::default()
    }
}

/// A preview of LaTeX as `insert.latex` would place it: the shape at the origin (ids from
/// 10 000, not the document's) and, for a proof, where each step's parts are.
#[derive(Clone, Debug)]
pub struct Preview {
    pub shape: Shape,
    pub regions: Vec<typeset::Region>,
    pub w: f64,
    pub h: f64,
}

/// Typesets `src` for a preview (the Proof Tree builder's canvas), measured with the current
/// slide's fonts. `font` `None` is the default font.
pub fn preview(st: &crate::DocState, src: &str, size: f64, font: Option<&str>) -> std::result::Result<Preview, String> {
    let style = Style { font: Some(font.unwrap_or(DEFAULT_FONT).to_string()).filter(|f| !f.is_empty()), size };
    let t = typeset_on(st, src, &style)?;
    let (w, h, regions) = (t.w, t.h, t.regions.clone());
    let mut shape = build(t, (0.0, 0.0), src, &style);
    shape.id = ShapeId(9_999);
    if let ShapeKind::Group { children, .. } = &mut shape.kind {
        for (k, c) in children.iter_mut().enumerate() {
            c.id = ShapeId(10_000 + k as u32);
        }
    }
    Ok(Preview { shape, regions, w, h })
}

/// A diagram laid out for the Commutative Diagram editor: with `ring` empty rows and columns
/// around it, its grid and where its objects, arrows and labels are.
#[derive(Clone, Debug)]
pub struct DiagramPreview {
    pub preview: Preview,
    pub grid: typeset::cd::Grid,
}

pub fn preview_diagram(
    st: &crate::DocState,
    d: &typeset::cd::Diagram,
    size: f64,
    font: Option<&str>,
    ring: usize,
) -> std::result::Result<DiagramPreview, String> {
    let style = Style { font: Some(font.unwrap_or(DEFAULT_FONT).to_string()).filter(|f| !f.is_empty()), size };
    if !(size.is_finite() && (1.0..=400.0).contains(&size)) {
        return Err("size must be between 1 and 400 pt".into());
    }
    let ctx = super::text::ctx_for(&st.doc, &st.selection).ok_or("no slide")?;
    let fields = deckcraft_text::NoFields;
    let rect = Rect::new(0.0, 0.0, 10_000.0, 10_000.0);
    let measure = |b: &TextBody| {
        let sh = Shape { text_box: true, text: Some(b.clone()), geom: Geom::preset("rect"), ..Default::default() };
        let opts = deckcraft_text::Opts { rect, fields: &fields, prompt_color: None, no_shrink: true };
        let base = deckcraft_text::layout(&ctx, &sh, b, &opts).lines.first().map_or(0.0, |l| l.baseline - rect.y0);
        (deckcraft_text::fit_width(&ctx, &sh, b, rect, &fields), deckcraft_text::fit_height(&ctx, &sh, b, rect, &fields), base)
    };
    let t = typeset::cd::layout(d, &style, &measure, ring);
    let grid = t.grid.clone().unwrap_or_default();
    let (w, h, regions) = (t.w, t.h, t.regions.clone());
    let src = typeset::cd::write(d);
    let mut shape = build(t, (0.0, 0.0), &src, &style);
    shape.id = ShapeId(9_999);
    if let ShapeKind::Group { children, .. } = &mut shape.kind {
        for (k, c) in children.iter_mut().enumerate() {
            c.id = ShapeId(10_000 + k as u32);
        }
    }
    Ok(DiagramPreview { preview: Preview { shape, regions, w, h }, grid })
}

/// An algorithm laid out for the Pseudo-code tool (with hidden `end` rows drawn faintly), and
/// its rows (what each region's index names).
pub fn preview_algorithm(
    st: &crate::DocState,
    a: &typeset::algo::Algorithm,
    size: f64,
    font: Option<&str>,
) -> std::result::Result<(Preview, Vec<typeset::algo::Row>), String> {
    let style = Style { font: Some(font.unwrap_or(DEFAULT_FONT).to_string()).filter(|f| !f.is_empty()), size };
    if !(size.is_finite() && (1.0..=400.0).contains(&size)) {
        return Err("size must be between 1 and 400 pt".into());
    }
    let ctx = super::text::ctx_for(&st.doc, &st.selection).ok_or("no slide")?;
    let fields = deckcraft_text::NoFields;
    let rect = Rect::new(0.0, 0.0, 10_000.0, 10_000.0);
    let measure = |b: &TextBody| {
        let sh = Shape { text_box: true, text: Some(b.clone()), geom: Geom::preset("rect"), ..Default::default() };
        let opts = deckcraft_text::Opts { rect, fields: &fields, prompt_color: None, no_shrink: true };
        let base = deckcraft_text::layout(&ctx, &sh, b, &opts).lines.first().map_or(0.0, |l| l.baseline - rect.y0);
        (deckcraft_text::fit_width(&ctx, &sh, b, rect, &fields), deckcraft_text::fit_height(&ctx, &sh, b, rect, &fields), base)
    };
    let t = typeset::algo::layout(a, &style, &measure, true);
    let (w, h, regions) = (t.w, t.h, t.regions.clone());
    let src = typeset::algo::write(a);
    let mut shape = build(t, (0.0, 0.0), &src, &style);
    shape.id = ShapeId(9_999);
    if let ShapeKind::Group { children, .. } = &mut shape.kind {
        for (k, c) in children.iter_mut().enumerate() {
            c.id = ShapeId(10_000 + k as u32);
        }
    }
    Ok((Preview { shape, regions, w, h }, typeset::algo::rows(a, true)))
}

/// The size and font a LaTeX shape was typeset in, from its style tag.
fn style_of(sh: &Shape) -> Option<Style> {
    tag(sh, STYLE_TAG).and_then(parse_style_tag)
}

/// Which dialog edits the LaTeX shape `id`: the Commutative Diagram editor for a diagram, the
/// Proof Tree builder for a proof it can open as a tree, else the Equation dialog.
pub(crate) fn dialog_for(st: &crate::DocState, id: ShapeId) -> &'static str {
    let src = st.shape(id).and_then(|sh| tag(sh, SOURCE_TAG));
    if src.is_some_and(|s| typeset::algo::is_algorithm(s) && typeset::algo::open(s).is_ok()) {
        return "pseudocode";
    }
    if src.is_some_and(|s| typeset::cd::is_diagram(s) && typeset::cd::open(s).is_ok()) {
        return "commutativeDiagram";
    }
    let proof = src.is_some_and(|src| typeset::is_proof(src) && typeset::proof_edit::open(src).is_ok());
    if proof { "proofTree" } else { "equation" }
}

fn base_name(kind: Kind) -> &'static str {
    match kind {
        Kind::Formula => "LaTeX",
        Kind::Grid => "LaTeX Array",
        Kind::Proof => "Proof Tree",
        Kind::Diagram => "Commutative Diagram",
        Kind::Algorithm => "Algorithm",
    }
}

/// Where a new shape of size `w` × `h` goes: `rect`'s corner, else the slide's centre.
fn place(s: &Session, p: &Value, w: f64, h: f64) -> (f64, f64) {
    if let Some(r) = rect_param(p, "rect") {
        return (r.x, r.y);
    }
    let size = s.active().map(|d| d.doc.slide_size).unwrap_or(deckcraft_model::defaults::WIDE);
    (((size.width - w) / 2.0).max(0.0), ((size.height - h) / 2.0).max(0.0))
}

fn assign_ids(doc: &mut Presentation, sh: &mut Shape, cmd: &str) -> Result<()> {
    sh.id = ShapeId(doc.alloc_id().ok_or_else(|| bad(cmd, NO_ID_SPACE))?);
    // Typeset groups are one level deep.
    if let ShapeKind::Group { children, .. } = &mut sh.kind {
        for c in children {
            c.id = ShapeId(doc.alloc_id().ok_or_else(|| bad(cmd, NO_ID_SPACE))?);
        }
    }
    Ok(())
}

fn insert_latex(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "insert.latex";
    let src = str_param(p, "latex").ok_or_else(|| bad(CMD, "`latex` is required"))?.to_string();
    let mut style = style_param(p, None);
    // An algorithm of twenty lines at 24 pt would be taller than a slide.
    if f64_param(p, "size").is_none() && typeset::algo::is_algorithm(&src) {
        style.size = ALGORITHM_SIZE;
    }
    let t = typeset_here(s, &src, &style).map_err(|e| bad(CMD, e))?;
    let kind = t.kind;
    let at = place(s, p, t.w, t.h);
    let mut shape = build(t, at, &src, &style);
    let id = s.edit(|doc, sel| {
        assign_ids(doc, &mut shape, CMD)?;
        let (taken, count) = super::current_names(doc, sel);
        shape.name = super::insert::first_free_name(base_name(kind), &taken, count);
        let id = shape.id;
        crate::shapes_mut(doc, sel).ok_or_else(|| bad(CMD, "no slide"))?.push(shape);
        sel.shapes = vec![id];
        sel.text = None;
        Ok(id)
    })?;
    Ok(json!({"id": id, "kind": kind.as_str()}))
}

/// craft-math writes Word's OMML: run properties as `w:rPr` (a namespace a presentation doesn't
/// declare) and no `xmlns:m` (a Word document declares it at its root). An equation in a slide
/// sits in `a14:m` and declares the math namespace itself.
fn presentation_omml(xml: &str) -> String {
    let mut out = strip_element(xml, "w:rPr");
    if let Some(end) = out.find('>') {
        let at = if out.get(..end).is_some_and(|t| t.ends_with('/')) { end - 1 } else { end };
        out.insert_str(at, &format!(r#" xmlns:m="{MATH_NS}""#));
    }
    out
}

/// `xml` without any `<name …>…</name>` or `<name …/>` element.
fn strip_element(xml: &str, name: &str) -> String {
    let open = format!("<{name}");
    let close = format!("</{name}>");
    let mut out = String::with_capacity(xml.len());
    let mut rest = xml;
    while let Some(i) = rest.find(&open) {
        let tail = rest.get(i..).unwrap_or("");
        let named = matches!(tail.get(open.len()..).and_then(|t| t.chars().next()), Some('>' | ' ' | '/'));
        if !named {
            out.push_str(rest.get(..i + open.len()).unwrap_or(""));
            rest = rest.get(i + open.len()..).unwrap_or("");
            continue;
        }
        out.push_str(rest.get(..i).unwrap_or(""));
        let tag_end = tail.find('>').map_or(tail.len(), |k| k + 1);
        rest = if tail.get(..tag_end).is_some_and(|t| t.ends_with("/>")) {
            tail.get(tag_end..).unwrap_or("")
        } else {
            tail.find(&close).and_then(|k| tail.get(k + close.len()..)).unwrap_or("")
        };
    }
    out.push_str(rest);
    out
}

/// An Office Math run for `src`: OMML for PowerPoint, its linear form for DeckCraft to draw.
fn equation_run(src: &str, size: f64) -> Run {
    let tree = math_latex::parse_latex(typeset::strip_math_delims(src));
    let text = typeset::linear_text(&tree);
    let math = Math { nodes: tree, ..Default::default() };
    let omml = presentation_omml(&omml::write_str("", true, &math, None));
    Run {
        text,
        props: RunProps { font: Some(EQUATION_FONT.into()), size: Some(size), no_proof: Some(true), ..Default::default() },
        kind: RunKind::Math { omml },
    }
}

fn insert_equation(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "insert.equation";
    let src = str_param(p, "latex").ok_or_else(|| bad(CMD, "`latex` is required"))?;
    if src.trim().is_empty() {
        return Err(bad(CMD, "the LaTeX is empty"));
    }
    let size_param = f64_param(p, "size").filter(|v| v.is_finite() && (1.0..=400.0).contains(v));
    // While text is being edited, the equation goes in the text at the caret.
    if s.doc()?.selection.text.as_ref().is_some_and(|t| !t.notes) {
        let mut run = equation_run(src, size_param.unwrap_or(DEFAULT_SIZE));
        run.props.size = size_param;
        super::text::insert_math(s, run)?;
        return Ok(json!({"inline": true}));
    }
    let size = size_param.unwrap_or(DEFAULT_SIZE);
    let mut para = Paragraph { runs: vec![equation_run(src, size)], ..Default::default() };
    para.props.align = Some(Align::Center);
    let mut body = TextBody { paragraphs: vec![para], ..Default::default() };
    body.body.wrap = Some(false);
    body.body.autofit = Some(AutoFit::Shape);
    let (x, y) = place(s, p, 300.0, size * 2.0);
    let shape = Shape {
        xfrm: Some(Xfrm::new(x, y, 300.0, size * 2.0)),
        text_box: true,
        geom: Geom::preset("rect"),
        text: Some(body),
        tags: vec![(SOURCE_TAG.to_string(), src.to_string())],
        ..Default::default()
    };
    let id = super::insert::add_shape(s, CMD, shape, true)?;
    super::text::fit_text_box(s, id)?;
    Ok(json!({"id": id}))
}

/// After a LaTeX shape's box was resized by hand (it was `orig`), its text is set again to fill
/// the new box: a typeset shape typeset at its size times the scale, an equation with its runs'
/// sizes scaled and its box fitted. The scale is the axis that changed more (the box keeps its
/// aspect while dragged). The document changes in place, inside the resize's undo step.
/// `false` when `id` isn't a LaTeX shape (it is fitted as ordinary text instead).
pub(crate) fn rescale_after_resize(s: &mut Session, id: ShapeId, orig: Xfrm) -> Result<bool> {
    const CMD: &str = "resize";
    let st = s.doc()?;
    if latex_target(st, id) != Some(id) {
        return Ok(false);
    }
    let Some(sh) = st.shape(id).cloned() else { return Ok(false) };
    let now = super::xfrm_of(&st.doc, &st.selection, &sh);
    let ratio = |a: f64, b: f64| if b > 1e-9 { a / b } else { 1.0 };
    let (rw, rh) = (ratio(now.w, orig.w), ratio(now.h, orig.h));
    let scale = if (rw - 1.0).abs() >= (rh - 1.0).abs() { rw } else { rh };
    if !(scale.is_finite() && scale > 0.0) || (scale - 1.0).abs() < 1e-3 {
        return Ok(true);
    }
    let scaled = |size: f64| ((size * scale * 10.0).round() / 10.0).clamp(1.0, 400.0);
    if math_run(&sh).is_some() {
        // An equation: its text grows or shrinks, and the box fits it again.
        let mut new = sh.clone();
        if let Some(t) = new.text.as_mut() {
            for p in &mut t.paragraphs {
                for r in &mut p.runs {
                    r.props.size = Some(scaled(r.props.size.unwrap_or(18.0)));
                }
                p.end_props.size = p.end_props.size.map(scaled);
            }
        }
        replace_in_place(s, id, new)?;
        super::text::fit_text_box(s, id)?;
        return Ok(true);
    }
    let Some(src) = tag(&sh, SOURCE_TAG).map(str::to_string) else { return Ok(false) };
    let mut style = style_of(&sh).unwrap_or(Style { font: Some(DEFAULT_FONT.into()), size: DEFAULT_SIZE });
    style.size = scaled(style.size);
    let t = typeset_here(s, &src, &style).map_err(|e| bad(CMD, e))?;
    let mut new = build(t, (now.x, now.y), &src, &style);
    if let Some(x) = new.xfrm.as_mut() {
        x.rot = now.rot;
        x.flip_h = now.flip_h;
        x.flip_v = now.flip_v;
    }
    new.id = id;
    new.name = sh.name.clone();
    replace_in_place(s, id, new)?;
    Ok(true)
}

/// The LaTeX shapes a size command will change (its `ids`, else the selection) and their boxes
/// now, for [`rescaled_after`].
pub(crate) fn latex_boxes(s: &Session, p: &Value) -> Result<Vec<(ShapeId, Xfrm)>> {
    let ids = targets(s, p)?;
    let st = s.doc()?;
    Ok(ids
        .into_iter()
        .filter(|id| latex_target(st, *id) == Some(*id))
        .filter_map(|id| st.shape(id).map(|sh| (id, super::xfrm_of(&st.doc, &st.selection, sh))))
        .collect())
}

/// After a size command: the LaTeX shapes in `before` set again to fill their new boxes.
pub(crate) fn rescaled_after(s: &mut Session, before: Vec<(ShapeId, Xfrm)>, out: Value) -> Result<Value> {
    for (id, orig) in before {
        rescale_after_resize(s, id, orig)?;
    }
    Ok(out)
}

/// Puts `new` in place of shape `id` without an undo step of its own (the gesture it belongs
/// to has one), giving a group's members fresh ids.
fn replace_in_place(s: &mut Session, id: ShapeId, mut new: Shape) -> Result<()> {
    let st = s.doc_mut()?;
    let mut d = (*st.doc).clone();
    if let ShapeKind::Group { children, .. } = &mut new.kind {
        for c in children {
            c.id = ShapeId(d.alloc_id().ok_or_else(|| bad("resize", NO_ID_SPACE))?);
        }
    }
    let list = crate::shapes_mut(&mut d, &st.selection).ok_or_else(|| bad("resize", "no slide"))?;
    if let Some(target) = deckcraft_model::find_shape_mut(list, id) {
        *target = new;
    }
    st.doc = std::sync::Arc::new(d);
    st.revision += 1;
    Ok(())
}

/// The first equation run of a shape: its OMML and size.
fn math_run(sh: &Shape) -> Option<(&str, Option<f64>)> {
    sh.text.as_ref()?.paragraphs.iter().flat_map(|p| &p.runs).find_map(|r| match &r.kind {
        RunKind::Math { omml } => Some((omml.as_str(), r.props.size)),
        _ => None,
    })
}

/// The LaTeX shape `id` is, or is inside: a typeset shape (tagged) or a text box with an equation.
fn latex_root(shapes: &[Shape], id: ShapeId, depth: usize) -> Option<&Shape> {
    if depth > 64 {
        return None;
    }
    shapes.iter().find_map(|sh| {
        let hit = sh.id == id || deckcraft_model::find_shape(sh.children(), id).is_some();
        if hit && (tag(sh, SOURCE_TAG).is_some() || math_run(sh).is_some()) {
            return Some(sh);
        }
        latex_root(sh.children(), id, depth + 1)
    })
}

/// The LaTeX shape a double-click on `id` edits through Insert ▸ Equation: a shape typeset from
/// LaTeX (or a member of one), or a text box holding nothing but an equation. Their text is drawn
/// from the source, so typing into it would be lost the next time it is typeset. Text with an
/// equation among other words stays editable in place.
pub(crate) fn latex_target(st: &crate::DocState, id: ShapeId) -> Option<ShapeId> {
    let shapes = current_shapes(&st.doc, &st.selection)?;
    let root = latex_root(shapes, id, 0)?;
    let only_math = root.text.as_ref().is_some_and(|t| {
        let mut runs = t.paragraphs.iter().flat_map(|p| &p.runs);
        runs.clone().any(|r| matches!(r.kind, RunKind::Math { .. }))
            && runs.all(|r| matches!(r.kind, RunKind::Math { .. }) || (matches!(r.kind, RunKind::Text | RunKind::Break) && r.text.trim().is_empty()))
    });
    (tag(root, SOURCE_TAG).is_some() || only_math).then_some(root.id)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Found {
    Typeset,
    Equation,
}

/// The selected (or `id`) LaTeX shape: its id, kind and source.
fn source(s: &Session, p: &Value, cmd: &str) -> Result<(ShapeId, Found, String)> {
    let ids = targets(s, p)?;
    let st = s.doc()?;
    let shapes = current_shapes(&st.doc, &st.selection).ok_or_else(|| bad(cmd, "no slide"))?;
    let root = ids.iter().find_map(|id| latex_root(shapes, *id, 0)).ok_or_else(|| bad(cmd, "select a LaTeX shape or an equation"))?;
    if let Some((omml_xml, size)) = math_run(root) {
        // The kept source while the equation is the one it made (PowerPoint may have edited it).
        let kept = tag(root, SOURCE_TAG)
            .filter(|src| matches!(&equation_run(src, size.unwrap_or(DEFAULT_SIZE)).kind, RunKind::Math { omml } if omml == omml_xml));
        let latex = match kept {
            Some(k) => k.to_string(),
            None => omml::read_str(omml_xml).map(|m| math_latex::to_latex(&m.nodes)).unwrap_or_default(),
        };
        return Ok((root.id, Found::Equation, latex));
    }
    Ok((root.id, Found::Typeset, tag(root, SOURCE_TAG).unwrap_or_default().to_string()))
}

fn get(s: &mut Session, p: &Value) -> Result<Value> {
    let (id, found, latex) = source(s, p, "latex.get")?;
    let kind = if found == Found::Equation { "equation" } else { "typeset" };
    let mut v = json!({"id": id, "latex": latex, "kind": kind});
    if let Some(style) = s.doc()?.shape(id).and_then(style_of) {
        v["size"] = json!(style.size);
        v["font"] = json!(style.font.unwrap_or_default());
    }
    Ok(v)
}

fn set(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "latex.set";
    let src = str_param(p, "latex").ok_or_else(|| bad(CMD, "`latex` is required"))?.to_string();
    let (id, found, _) = source(s, p, CMD)?;
    let old = shape_of(s, id)?;
    if found == Found::Equation {
        let size = f64_param(p, "size").or_else(|| math_run(&old).and_then(|(_, sz)| sz)).unwrap_or(DEFAULT_SIZE);
        let run = equation_run(&src, size);
        s.edit(|doc, sel| {
            let list = crate::shapes_mut(doc, sel).ok_or_else(|| bad(CMD, "no slide"))?;
            let sh = deckcraft_model::find_shape_mut(list, id).ok_or_else(|| bad(CMD, "no such shape"))?;
            if let Some(r) =
                sh.text.as_mut().and_then(|t| t.paragraphs.iter_mut().flat_map(|p| &mut p.runs).find(|r| matches!(r.kind, RunKind::Math { .. })))
            {
                r.text = run.text;
                r.kind = run.kind;
                r.props.size = Some(size);
            }
            sh.tags.retain(|(n, _)| n != SOURCE_TAG);
            sh.tags.push((SOURCE_TAG.to_string(), src.clone()));
            sel.shapes = vec![id];
            Ok(())
        })?;
        super::text::fit_text_box(s, id)?;
        return Ok(json!({"id": id}));
    }
    let style = style_param(p, style_of(&old));
    let t = typeset_here(s, &src, &style).map_err(|e| bad(CMD, e))?;
    let kind = t.kind;
    let at = old.xfrm.map_or((0.0, 0.0), |x| (x.x, x.y));
    let mut shape = build(t, at, &src, &style);
    s.edit(|doc, sel| {
        assign_ids(doc, &mut shape, CMD)?;
        shape.id = id;
        shape.name = old.name.clone();
        let list = crate::shapes_mut(doc, sel).ok_or_else(|| bad(CMD, "no slide"))?;
        let sh = deckcraft_model::find_shape_mut(list, id).ok_or_else(|| bad(CMD, "no such shape"))?;
        *sh = shape;
        sel.shapes = vec![id];
        sel.text = None;
        Ok(())
    })?;
    Ok(json!({"id": id, "kind": kind.as_str()}))
}

#[cfg(test)]
#[path = "latex_tests.rs"]
mod tests;
