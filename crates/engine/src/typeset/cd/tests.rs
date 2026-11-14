use deckcraft_model::text::TextBody;

use super::edit::{self, Selection, TEMPLATES};
use super::{Arrow, At, Body, Diagram, End, Frame, Head, Label, Loop, Object, Sep, Side, Tail, describe, open, write};
use crate::typeset::{Item, Kind, Part, Style, typeset};

fn d(src: &str) -> Diagram {
    open(&format!("\\begin{{tikzcd}} {src} \\end{{tikzcd}}")).unwrap_or_else(|e| panic!("{e}: {src}")).diagram
}

fn measure(b: &TextBody) -> (f64, f64, f64) {
    let size = b.paragraphs.iter().flat_map(|p| &p.runs).find_map(|r| r.props.size).unwrap_or(20.0);
    let w = b.paragraphs.iter().map(|p| p.runs.iter().map(|r| r.text.chars().count()).sum::<usize>()).max().unwrap_or(0);
    (w as f64 * size * 0.6, size * 1.15, size * 0.9)
}

fn style() -> Style {
    Style { font: None, size: 20.0 }
}

fn labels(a: &Arrow) -> Vec<&str> {
    a.labels.iter().map(|l| l.text.as_str()).collect()
}

#[test]
fn reads_cells_arrows_and_labels() {
    let x = d(r#"A \arrow[r, "f"] \arrow[d, "g"'] & B \arrow[d, "h"] \\ C \arrow[r, "k"' near start, red] & D"#);
    assert_eq!(x.size(), (2, 2));
    assert_eq!(x.objects.iter().map(|o| o.label.as_str()).collect::<Vec<_>>(), ["A", "B", "C", "D"]);
    assert_eq!(x.arrows.len(), 4);
    let f = &x.arrows[0];
    assert_eq!((f.from, f.to), (End::Cell(At::new(0, 0)), End::Cell(At::new(0, 1))));
    assert_eq!(labels(f), ["f"]);
    assert_eq!(x.arrows[1].labels[0].side, Side::Right);
    let k = &x.arrows[3];
    assert_eq!(k.labels[0].pos, 0.25);
    assert_eq!(k.color, Some([255, 0, 0]));
}

#[test]
fn reads_the_older_forms_and_shortcuts() {
    let x = d(r"A \arrow{d} \arrow{r}[near start]{\phi}[near end]{\psi} & B \arrow[red]{d}{\xi} \\ C \rar[blue]{\eta} & D");
    assert_eq!(x.arrows.len(), 4);
    assert_eq!(labels(&x.arrows[1]), [r"\phi", r"\psi"]);
    assert_eq!((x.arrows[1].labels[0].pos, x.arrows[1].labels[1].pos), (0.25, 0.75));
    assert_eq!(x.arrows[3].to, End::Cell(At::new(1, 1)));
    assert_eq!(labels(&x.arrows[3]), [r"\eta"]);
}

#[test]
fn reads_styles_into_tails_bodies_and_heads() {
    let one = |opts: &str| d(&format!(r"A \arrow[r, {opts}] & B")).arrows.remove(0);
    let look = |a: &Arrow| (a.tail, a.body, a.head);
    assert_eq!(look(&one("hook")), (Tail::Hook, Body::Solid, Head::Arrow));
    assert_eq!(look(&one("tail, two heads, dashed")), (Tail::Mono, Body::Dashed, Head::TwoHeads));
    assert_eq!(look(&one("Rightarrow")), (Tail::None, Body::Double, Head::Arrow));
    assert_eq!(look(&one("equal")), (Tail::None, Body::Double, Head::None));
    assert_eq!(look(&one("maps to")), (Tail::Bar, Body::Solid, Head::Arrow));
    assert_eq!(look(&one("harpoon'")), (Tail::None, Body::Solid, Head::HarpoonRight));
    assert_eq!(look(&one("squiggly")), (Tail::None, Body::Squiggly, Head::Arrow));
    assert_eq!(look(&one("phantom")).1, Body::Phantom);
    assert_eq!(look(&one("draw=none")).1, Body::Invisible);
    // Drawn the other way round: from B to A, label sides swapped.
    let m = d(r#"A \arrow[r, mapsfrom, "f"] & B"#).arrows.remove(0);
    assert_eq!((m.from, m.to, m.tail), (End::Cell(At::new(0, 1)), End::Cell(At::new(0, 0)), Tail::Bar));
    assert_eq!(m.labels[0].side, Side::Right);
    // A position on the arrow places its labels that don't give one.
    let corner = d(r#"A \arrow[dr, phantom, "\lrcorner", very near start] & B \\ C & D"#).arrows.remove(0);
    assert_eq!((corner.body, corner.labels[0].pos), (Body::Phantom, 0.125));
    let b = one("bend left=40, shift right=2, shorten <=3pt, shorten >=1mm, line width=0.8pt, crossing over");
    assert_eq!((b.bend, b.shift, b.width, b.crossing), (40.0, -2.0, Some(0.8), true));
    assert!((b.shorten.0 - 3.0).abs() < 1e-9 && (b.shorten.1 - 2.845).abs() < 1e-2);
    assert_eq!(one("bend right").bend, -30.0);
    assert_eq!(one("thick").width, Some(0.8));
    assert_eq!(one("draw={rgb,255:red,214;green,92;blue,92}").color, Some([214, 92, 92]));
}

#[test]
fn reads_quiver_exports() {
    let src = "% https://q.uiver.app/#q=WzAsMl0=\n\\[\\begin{tikzcd}\n\tA & B \\\\\n\tC & D\n\t\\arrow[\"f\", from=1-1, to=1-2]\n\t\\arrow[\"g\"', curve={height=-12pt}, from=1-1, to=2-1]\n\t\\arrow[\"\\lrcorner\"{anchor=center, pos=0.125}, draw=none, from=1-1, to=2-2]\n\t\\arrow[\"k\"{description}, color={rgb,255:red,92;green,92;blue,214}, from=2-1, to=2-2]\n\\end{tikzcd}\\]";
    let o = open(src).unwrap();
    let x = o.diagram;
    assert_eq!(x.objects.len(), 4);
    assert_eq!(x.arrows.len(), 4);
    assert!(x.arrows[1].bend > 10.0, "a negative height bends left: {}", x.arrows[1].bend);
    let corner = &x.arrows[2];
    assert_eq!((corner.body, corner.labels[0].side, corner.labels[0].pos), (Body::Invisible, Side::Over, 0.125));
    assert_eq!(x.arrows[3].labels[0].side, Side::On);
    assert_eq!(x.arrows[3].color, Some([92, 92, 214]));
    assert!(o.notices.iter().any(|n| n.contains("curve")), "{:?}", o.notices);
}

#[test]
fn two_cells_reach_arrows_by_name_in_any_order() {
    let x = d(r#"A \arrow[Rightarrow, from=U, to=D, "\alpha"] \arrow[r, bend left=50, ""{name=U, below}] \arrow[r, bend right=50, ""{name=D}] & B"#);
    assert_eq!(x.arrows.len(), 3);
    let two = x.arrows.iter().find(|a| a.body == Body::Double).unwrap();
    assert!(matches!((two.from, two.to), (End::Arrow(_), End::Arrow(_))));
    let i = x.arrows.iter().position(|a| a.body == Body::Double).unwrap();
    assert_eq!(x.level(i), Some(1));
    // A name nothing defines drops only that arrow.
    let y = d(r#"A \arrow[r] \arrow[Rightarrow, from=Q, to=1-2] & B"#);
    assert_eq!(y.arrows.len(), 1);
}

#[test]
fn cell_options_frames_names_and_gaps() {
    let x = d(r"|[draw]| A \arrow[to=Z] & |[draw, circle, text=blue]| B \\[2em] |[alias=Z]| C &[1em] D");
    assert_eq!(x.object(At::new(0, 0)).map(|o| o.frame), Some(Frame::Box));
    let b = x.object(At::new(0, 1)).unwrap();
    assert_eq!((b.frame, b.color), (Frame::Circle, Some([0, 0, 255])));
    assert_eq!(x.arrows[0].to, End::Cell(At::new(1, 0)), "alias");
    assert_eq!(x.row_gaps.get(&0), Some(&2.0));
    assert_eq!(x.col_gaps.get(&1), Some(&1.0));
    let y = d(r"[row sep=small, column sep=2.5em] A & B");
    assert_eq!((y.row_sep, y.col_sep), (Sep::Small, Sep::Em(2.5)));
}

#[test]
fn ampersand_replacement_and_braces() {
    let x = d(r#"[ampersand replacement=\&] A \oplus B \ar[r, "{\begin{pmatrix} e & f \\ g & h \end{pmatrix}}"] \& C \oplus D"#);
    assert_eq!(x.objects.len(), 2);
    assert!(x.arrows[0].labels[0].text.contains("pmatrix"));
    // Brackets in math don't end anything (a diagram starting with one is reading options,
    // in TeX too).
    let y = d(r#"A \arrow[r, "f"] & x \in [0,1)"#);
    assert_eq!(y.objects[1].label, r"x \in [0,1)");
    assert_eq!(y.arrows.len(), 1);
}

#[test]
fn writes_plain_tikz_cd_that_reads_back_the_same() {
    for (name, src) in TEMPLATES {
        let x = d(src);
        let w = write(&x);
        let back = open(&w).unwrap_or_else(|e| panic!("{name}: {e}\n{w}")).diagram;
        assert_eq!(strip_names(back), strip_names(x.clone()), "{name}:\n{w}");
        assert!(!w.contains("curve=") && !w.contains("between="), "plain tikz-cd: {w}");
    }
    let w = write(&d(r#"A \arrow[r, "f"] \arrow[d] & B \\ C"#));
    assert_eq!(w, "\\begin{tikzcd}\n\tA \\arrow[r, \"f\"] \\arrow[d] & B \\\\\n\tC\n\\end{tikzcd}");
}

/// Names the writer makes up don't matter.
fn strip_names(mut x: Diagram) -> Diagram {
    for a in &mut x.arrows {
        a.name = None;
    }
    x
}

#[test]
fn every_look_round_trips() {
    let tails = [Tail::None, Tail::Head, Tail::Mono, Tail::Hook, Tail::HookRight, Tail::Bar];
    let bodies = [Body::Solid, Body::Dashed, Body::Dotted, Body::Squiggly, Body::Invisible];
    let heads = [Head::Arrow, Head::TwoHeads, Head::Harpoon, Head::HarpoonRight, Head::None];
    for t in tails {
        for b in bodies {
            for h in heads {
                let mut x = d("A & B");
                let mut a = Arrow::new(End::Cell(At::new(0, 0)), End::Cell(At::new(0, 1)));
                (a.tail, a.body, a.head) = (t, b, h);
                x.arrows.push(a);
                let back = open(&write(&x)).unwrap().diagram;
                let got = &back.arrows[0];
                assert_eq!((got.tail, got.body, got.head), (t, b, h), "{}", write(&x));
            }
        }
    }
    // Double arrows: the tips tikz-cd has names for.
    for (t, h) in [(Tail::None, Head::Arrow), (Tail::None, Head::None), (Tail::Head, Head::Arrow), (Tail::Head, Head::None), (Tail::Bar, Head::Arrow)]
    {
        let mut x = d("A & B");
        let mut a = Arrow::new(End::Cell(At::new(0, 0)), End::Cell(At::new(0, 1)));
        (a.tail, a.body, a.head) = (t, Body::Double, h);
        x.arrows.push(a);
        let got = open(&write(&x)).unwrap().diagram.arrows.remove(0);
        assert_eq!((got.tail, got.body, got.head), (t, Body::Double, h), "{}", write(&x));
    }
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

fn random_diagram(r: &mut Rng) -> Diagram {
    let mut x = Diagram::default();
    let words = ["A", r"X \times Y", r"\mathcal{C}", "0", r"F(A)", r"\bullet", "B_1"];
    let (rows, cols) = (1 + r.below(3), 1 + r.below(4));
    for i in 0..rows {
        for j in 0..cols {
            if r.below(3) > 0 {
                x.objects.push(Object::new(At::new(i, j), words[r.below(words.len() as u64)]));
            }
        }
    }
    let cells: Vec<At> = x.objects.iter().map(|o| o.at).collect();
    for _ in 0..r.below(6) {
        if cells.is_empty() {
            break;
        }
        let f = cells[r.below(cells.len() as u64)];
        let t = cells[r.below(cells.len() as u64)];
        let Ok(i) = edit::add_arrow(&mut x, End::Cell(f), End::Cell(t)) else { continue };
        let a = &mut x.arrows[i];
        let looped = a.from == a.to;
        a.tail = [Tail::None, Tail::Mono, Tail::Hook, Tail::Bar, Tail::Head][r.below(5)];
        a.body = [Body::Solid, Body::Dashed, Body::Dotted, Body::Squiggly][r.below(4)];
        a.head = [Head::Arrow, Head::TwoHeads, Head::Harpoon, Head::None][r.below(4)];
        if !looped {
            a.bend = [0.0, 30.0, -45.0, 12.5][r.below(4)];
            a.shift = [0.0, 1.0, -2.0][r.below(3)];
        }
        if r.below(2) == 0 {
            let mut l = Label::new(["f", r"\alpha", "g_1", r"\pi_1"][r.below(4)]);
            l.side = [Side::Left, Side::Right, Side::On][r.below(3)];
            l.pos = [0.5, 0.25, 0.3][r.below(3)];
            a.labels.push(l);
        }
        if r.below(4) == 0 {
            a.color = Some([214, 92, 92]);
        }
        a.crossing = r.below(5) == 0;
    }
    if x.arrows.len() >= 2 && r.below(2) == 0 {
        let _ = edit::add_arrow(&mut x, End::Arrow(0), End::Arrow(1));
    }
    x
}

#[test]
fn random_diagrams_round_trip() {
    let mut r = Rng(0x9E37_79B9_7F4A_7C15);
    for _ in 0..300 {
        let x = random_diagram(&mut r);
        let w = write(&x);
        let back = open(&w).unwrap_or_else(|e| panic!("{e}\n{w}")).diagram;
        assert_eq!(strip_names(back), strip_names(x), "{w}");
    }
}

#[test]
fn layout_follows_tikz_cd_spacing() {
    let t = crate::typeset::cd::layout(&d(r"A \arrow[r] \arrow[d] & B \\ C"), &style(), &measure, 0);
    assert_eq!(t.kind, Kind::Diagram);
    let g = t.grid.clone().unwrap();
    let em = 20.0;
    // Column centres: half of A, 1ex, 2.4em, 1ex, half of B apart.
    let centre = |c: (f64, f64)| (c.0 + c.1) / 2.0;
    let dx = centre(g.cols[1]) - centre(g.cols[0]);
    let a_w = 0.6 * em;
    assert!((dx - (a_w + 2.0 * 0.4306 * em + 2.4 * em)).abs() < 0.5, "{dx}");
    // The horizontal arrow sits on the math axis and stops at B's box.
    let arrow = t.regions.iter().find(|r| r.part == Part::Arrow && r.path == [0]).unwrap();
    let (first, last) = (arrow.poly.first().unwrap(), arrow.poly.last().unwrap());
    assert!((first.1 - last.1).abs() < 1e-6, "horizontal");
    let b = t.regions.iter().find(|r| r.part == Part::Object && r.path == [0, 1]).unwrap();
    assert!((last.0 - b.x).abs() < 1e-6, "ends at B's border: {} vs {}", last.0, b.x);
    let a = t.regions.iter().find(|r| r.part == Part::Object && r.path == [0, 0]).unwrap();
    assert!(first.1 > a.y && first.1 < a.y + a.h);
}

#[test]
fn labels_sit_on_the_left_unless_swapped() {
    let t = crate::typeset::cd::layout(&d(r#"A \arrow[r, "f"] \arrow[d, "g"] \arrow[d, "h"', shift left] & B \\ C"#), &style(), &measure, 0);
    let label = |i: usize| t.regions.iter().find(|r| r.part == Part::ArrowLabel && r.path == [i, 0]).unwrap();
    let arrow = |i: usize| t.regions.iter().find(|r| r.part == Part::Arrow && r.path == [i]).unwrap();
    let f = label(0);
    assert!(f.y + f.h <= arrow(0).poly[0].1 + 1e-6, "f above its rightward arrow");
    let g = label(1);
    assert!(g.x >= arrow(1).poly[0].0 - 1e-6, "g right of its downward arrow (its left)");
    let h = label(2);
    assert!(h.x + h.w <= arrow(2).poly[0].0 + 1e-6, "h swapped: left of the page");
}

#[test]
fn diagrams_scale_with_their_size_and_draw_every_kind_of_arrow() {
    let src = r#"\begin{tikzcd} A \arrow[r, hook, "i"] \arrow[d, two heads] \arrow[dr, Rightarrow, bend left] & B \arrow[d, squiggly] \arrow[loop right, "f"] \\ C \arrow[r, maps to, dashed, crossing over] & D \arrow[ul, phantom, "\ulcorner", very near start] \end{tikzcd}"#;
    let small = typeset(src, &Style { font: None, size: 10.0 }, &measure).unwrap();
    let big = typeset(src, &Style { font: None, size: 30.0 }, &measure).unwrap();
    assert!((big.w / small.w - 3.0).abs() < 0.05, "{} {}", small.w, big.w);
    assert!(
        big.items.iter().any(|i| matches!(i, Item::Path { stroke: Some(s), .. } if s.paint == crate::typeset::Paint::Background)),
        "crossing over"
    );
    assert!(big.items.iter().all(|i| match i {
        Item::Path { path, .. } => path.elements().iter().all(|e| e.end_point().is_some_and(|p| p.x.is_finite() && p.y.is_finite())),
        _ => true,
    }));
}

#[test]
fn edits_add_move_remove_and_refuse() {
    let mut x = Diagram::default();
    let (a, _) = edit::add_object(&mut x, 0, 0, "A").unwrap();
    let (b, _) = edit::add_object(&mut x, 0, 1, "B").unwrap();
    assert!(edit::add_object(&mut x, 0, 1, "C").is_err(), "occupied");
    // A cell left of everything moves the diagram right.
    let (c, moved) = edit::add_object(&mut x, 0, -1, "Z").unwrap();
    assert_eq!((c, moved), (At::new(0, 0), (0, 1)));
    let (a, b) = (At::new(a.row, a.col + 1), At::new(b.row, b.col + 1));
    let f = edit::add_arrow(&mut x, End::Cell(a), End::Cell(b)).unwrap();
    let g = edit::add_arrow(&mut x, End::Cell(a), End::Cell(b)).unwrap();
    let two = edit::add_arrow(&mut x, End::Arrow(f), End::Arrow(g)).unwrap();
    assert_eq!(x.arrows[two].body, Body::Double);
    assert!(edit::add_arrow(&mut x, End::Arrow(two), End::Arrow(two)).is_err());
    let lp = edit::add_arrow(&mut x, End::Cell(a), End::Cell(a)).unwrap();
    assert_eq!(x.arrows[lp].looping, Some(Loop::Above));
    // Moving A down keeps its arrows on it.
    let to = edit::move_objects(&mut x, &[a], 1, 0).unwrap();
    assert_eq!(x.arrows[f].from, End::Cell(to[0]));
    assert!(edit::move_objects(&mut x, &[to[0]], -1, 1).is_err(), "onto B");
    // Removing an arrow removes the 2-cell on it.
    edit::remove_arrows(&mut x, &[f]);
    assert_eq!(x.arrows.len(), 2);
    assert!(x.arrows.iter().all(|a| a.is_cell_arrow()));
    // Removing an object removes its arrows and the empty column.
    edit::remove(&mut x, &Selection::object(At::new(0, 0)));
    assert_eq!(x.objects.len(), 2);
    assert_eq!(x.size().1, 2);
}

#[test]
fn flips_keep_labels_on_the_same_outside() {
    let mut x = d(r#"A \arrow[r, "f", bend left] & B"#);
    edit::flip(&mut x, true);
    let a = &x.arrows[0];
    assert_eq!((a.from, a.to), (End::Cell(At::new(0, 1)), End::Cell(At::new(0, 0))));
    assert_eq!((a.bend, a.labels[0].side), (-30.0, Side::Right));
    let mut y = d(r"A \arrow[r] & B");
    edit::rotate(&mut y);
    assert_eq!(y.arrows[0].to, End::Cell(At::new(1, 0)), "a quarter turn clockwise: right becomes down");
}

#[test]
fn copy_and_paste_take_what_arrows_need() {
    let x = edit::template("Natural transformation").unwrap();
    let two = x.arrows.iter().position(|a| !a.is_cell_arrow()).unwrap();
    let part = edit::extract(&x, &Selection::arrow(two));
    assert_eq!(part.arrows.len(), 3, "the 2-cell brings its two arrows");
    assert_eq!(part.objects.len(), 2);
    let mut y = Diagram::default();
    let sel = edit::paste(&mut y, &part, At::new(2, 0)).unwrap();
    assert_eq!(sel.objects.len(), 2);
    assert!(edit::paste(&mut y, &part, At::new(2, 0)).is_err(), "in the way");
}

#[test]
fn templates_open_and_describe_themselves() {
    for (name, _) in TEMPLATES {
        let x = edit::template(name).unwrap_or_else(|| panic!("{name}"));
        assert!(!x.objects.is_empty(), "{name}");
        assert!(describe(&x).starts_with("Commutative diagram:"));
    }
    let sq = edit::template("Commutative square").unwrap();
    assert!(describe(&sq).contains("f: A → B"), "{}", describe(&sq));
}

#[test]
fn hostile_input_never_crashes() {
    let long = "A & ".repeat(200);
    let rows = "A \\\\ ".repeat(200);
    for src in [
        "\\begin{tikzcd}",
        "\\begin{tikzcd}\\end{tikzcd}",
        "\\begin{tikzcd} A \\arrow[ \\end{tikzcd}",
        "\\begin{tikzcd} A \\arrow[r, \"f] & B \\end{tikzcd}",
        "\\begin{tikzcd} A \\arrow[from=U, to=U, name=U] \\end{tikzcd}",
        "\\begin{tikzcd} A \\arrow[rrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrr] \\end{tikzcd}",
        "\\begin{tikzcd} A \\arrow[bend left=1e308, shift left=nan, shorten=1e9pt, line width=-3pt] \\end{tikzcd}",
        "\\begin{tikzcd} A \\arrow[l] \\arrow[u] \\end{tikzcd}",
        "\\begin{tikzcd}[sep=hugeee, row sep=1e400em] {{{ A \\end{tikzcd}",
        "\\begin{tikzcd} |[draw| A \\end{tikzcd}",
        "\\begin{tikzcd} A \\arrow[\"\"{name=a}] \\arrow[from=a, to=b, \"\"{name=b}] \\arrow[from=b, to=a] \\end{tikzcd}",
        &format!("\\begin{{tikzcd}} {long} \\end{{tikzcd}}"),
        &format!("\\begin{{tikzcd}} {rows} \\end{{tikzcd}}"),
    ] {
        if let Ok(o) = open(src) {
            let (rows, cols) = o.diagram.size();
            assert!(rows <= super::MAX_ROWS && cols <= super::MAX_COLS);
            let _ = write(&o.diagram);
            let _ = crate::typeset::cd::layout(&o.diagram, &style(), &measure, 1);
        }
    }
    // Nesting arrows between arrows past the limit is refused.
    let mut x = d(r"A \arrow[r] \arrow[r, shift left] & B");
    let mut prev = (0, 1);
    for _ in 0..super::MAX_LEVEL + 2 {
        let Ok(i) = edit::add_arrow(&mut x, End::Arrow(prev.0), End::Arrow(prev.1)) else { break };
        let Ok(j) = edit::add_arrow(&mut x, End::Arrow(prev.0), End::Arrow(prev.1)) else { break };
        prev = (i, j);
    }
    assert!((0..x.arrows.len()).all(|i| x.level(i).is_some_and(|l| l <= super::MAX_LEVEL)));
}

/// What DeckCraft writes compiles with the real tikz-cd (skipped where TeX isn't installed, as
/// on CI).
#[test]
fn written_tikz_cd_compiles_with_pdflatex() {
    let has_tex = std::process::Command::new("kpsewhich").arg("tikz-cd.sty").output().is_ok_and(|o| o.status.success() && !o.stdout.is_empty());
    if !has_tex {
        return;
    }
    let mut r = Rng(0x2545_F491_4F6C_DD1D);
    let mut body = String::new();
    for (name, _) in TEMPLATES {
        body.push_str(&format!("\\[\n{}\n\\]\n", write(&edit::template(name).unwrap_or_default())));
    }
    for _ in 0..40 {
        body.push_str(&format!("\\[\n{}\n\\]\n", write(&random_diagram(&mut r))));
    }
    let doc = format!(
        "\\documentclass{{article}}\n\\usepackage{{amssymb,tikz-cd}}\n\\usetikzlibrary{{decorations.pathmorphing}}\n\\begin{{document}}\n{body}\\end{{document}}\n"
    );
    let dir = std::env::temp_dir().join(format!("deckcraft-tikzcd-{}", std::process::id()));
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
fn tidying_writes_each_arrow_after_its_object() {
    let mut x = d("A & B \\\\ C & D");
    let k = edit::add_arrow(&mut x, End::Cell(At::new(1, 0)), End::Cell(At::new(1, 1))).unwrap();
    let f = edit::add_arrow(&mut x, End::Cell(At::new(0, 0)), End::Cell(At::new(0, 1))).unwrap();
    let two = edit::add_arrow(&mut x, End::Arrow(k), End::Arrow(f)).unwrap();
    let g = edit::add_arrow(&mut x, End::Cell(At::new(0, 0)), End::Cell(At::new(1, 0))).unwrap();
    assert!(write(&x).contains("from=1-1"), "out of order: written by from/to");
    let _ = (two, g);
    edit::tidy(&mut x);
    let w = write(&x);
    assert!(!w.contains("from=1-1"), "{w}");
    assert!(w.contains("\tA \\arrow[r, ") && w.contains("\\arrow[d] & B \\\\\n\tC \\arrow[r, "), "{w}");
    let back = open(&w).unwrap().diagram;
    let two = back.arrows.iter().find(|a| !a.is_cell_arrow()).unwrap();
    let ends = |e: End| match e {
        End::Arrow(i) => back.arrows[i].from,
        End::Cell(at) => End::Cell(at),
    };
    assert_eq!((ends(two.from), ends(two.to)), (End::Cell(At::new(1, 0)), End::Cell(At::new(0, 0))), "the 2-cell still joins k and f");
}
