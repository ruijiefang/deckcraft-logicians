use deckcraft_engine::typeset::cd::{At, Body, End, Side};
use egui::accesskit::Role;
use egui::{Event, Key, Modifiers, PointerButton, Pos2, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::{By, Queryable as _, by};
use serde_json::json;

use crate::SlideApp;

fn harness(session: deckcraft_engine::Session) -> Harness<'static, SlideApp> {
    let mut h = Harness::builder().with_size(vec2(1500.0, 1000.0)).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        SlideApp::new(session, crate::Services::default()),
    );
    h.run_steps(2);
    h
}

fn label_field() -> By<'static> {
    by().role(Role::TextInput).label_contains("Label")
}

/// Test labels live for the test run.
fn text(s: String) -> &'static str {
    Box::leak(s.into_boxed_str())
}

fn cell(r: usize, c: usize) -> By<'static> {
    by().label_contains(text(format!("cell {r}, {c}")))
}

fn object(name: &str) -> By<'static> {
    by().label(text(format!("Object: {name}")))
}

fn editor<'a>(h: &'a Harness<'_, SlideApp>) -> &'a super::Editor {
    h.state().cd_editor.as_ref().expect("the editor is open")
}

/// The LaTeX of every diagram on the first slide.
fn diagrams(h: &Harness<'_, SlideApp>) -> Vec<String> {
    h.state().session.doc().unwrap().doc.slides[0]
        .shapes
        .iter()
        .flat_map(|s| s.tags.iter().filter(|(n, _)| n == "DECKCRAFT_LATEX").map(|(_, v)| v.clone()))
        .collect()
}

fn centre(h: &Harness<'_, SlideApp>, by: By<'_>) -> Pos2 {
    h.get(by).rect().center()
}

/// A press at `from`, moves, and a release at `to`, with `mods` held.
fn drag(h: &mut Harness<'_, SlideApp>, from: Pos2, to: Pos2, mods: Modifiers) {
    h.input_mut().events.push(Event::PointerMoved(from));
    h.run_steps(2);
    h.event(Event::ModifiersChanged(mods));
    h.input_mut().events.push(Event::PointerButton { pos: from, button: PointerButton::Primary, pressed: true, modifiers: mods });
    h.run_steps(2);
    for k in 1..=6 {
        let p = from + (to - from) * (k as f32 / 6.0);
        h.input_mut().events.push(Event::PointerMoved(p));
        h.run_steps(1);
    }
    h.input_mut().events.push(Event::PointerButton { pos: to, button: PointerButton::Primary, pressed: false, modifiers: mods });
    h.run_steps(3);
    h.event(Event::ModifiersChanged(Modifiers::NONE));
    h.run_steps(2);
}

fn type_label(h: &mut Harness<'_, SlideApp>, text: &str) {
    h.get(label_field()).type_text(text);
    h.run_steps(2);
}

/// Insert ▸ Commutative Diagram opens the editor; a click on a cell adds an object with its
/// label ready to type, a drag from one object to another an arrow; a style picked in the side
/// panel and Ctrl+Enter insert the diagram as tikz-cd.
#[test]
fn draws_a_diagram_with_clicks_and_a_drag() {
    let mut h = harness(deckcraft_engine::Session::with_new());
    h.state_mut().ui.tab = "Insert".into();
    h.run_steps(2);
    h.get(by().role(Role::Button).label_contains("Commutative")).click();
    h.run_steps(3);
    assert_eq!(h.state().dialog.as_ref().map(|d| d.id.as_str()), Some("commutativeDiagram"));

    h.get(cell(1, 1)).click();
    h.run_steps(3);
    assert!(h.get(label_field()).is_focused(), "a new object's label is ready to type");
    // Its placeholder is selected: typing replaces it.
    type_label(&mut h, "A");
    h.get(cell(1, 2)).click();
    h.run_steps(3);
    type_label(&mut h, "B");
    assert_eq!(editor(&h).d.objects.iter().map(|o| o.label.as_str()).collect::<Vec<_>>(), ["A", "B"]);

    let (a, b) = (centre(&h, object("A")), centre(&h, object("B")));
    drag(&mut h, a, b, Modifiers::NONE);
    let d = &editor(&h).d;
    assert_eq!(d.arrows.len(), 1);
    assert_eq!((d.arrows[0].from, d.arrows[0].to), (End::Cell(At::new(0, 0)), End::Cell(At::new(0, 1))));
    assert_eq!(editor(&h).sel.arrows, [0], "the new arrow is selected");

    type_label(&mut h, "f");
    h.get(by().role(Role::Button).label("Dashed")).click();
    h.run_steps(2);
    assert_eq!(editor(&h).d.arrows[0].body, Body::Dashed);
    h.get(by().role(Role::Button).label("On")).click();
    h.run_steps(2);
    assert_eq!(editor(&h).d.arrows[0].labels[0].side, Side::On);

    h.key_press_modifiers(Modifiers::COMMAND, Key::Enter);
    h.run_steps(3);
    let got = diagrams(&h);
    assert_eq!(got.len(), 1, "{got:?}");
    assert!(got[0].contains(r#"A \arrow[r, "f"{description}, dashed] & B"#), "{}", got[0]);
    assert!(h.state().dialog.is_none() && h.state().cd_editor.is_none(), "inserting closes the editor");
}

/// An arrow dropped on an empty cell adds the object there too; one dropped on an arrow makes a
/// 2-cell; Alt-dragging an object moves it.
#[test]
fn drags_make_objects_two_cells_and_moves() {
    let mut s = deckcraft_engine::Session::with_new();
    s.execute("insert.latex", &json!({"latex": r"\begin{tikzcd} A \arrow[r, bend left] \arrow[r, bend right] & B \end{tikzcd}"})).unwrap();
    let mut h = harness(s);
    h.state_mut().run("app.dialog", json!({"id": "commutativeDiagram"})).unwrap();
    h.run_steps(4);
    assert_eq!(editor(&h).d.arrows.len(), 2, "opened on the selected diagram");

    let a = centre(&h, object("A"));
    let empty = centre(&h, cell(2, 1));
    drag(&mut h, a, empty, Modifiers::NONE);
    let d = &editor(&h).d;
    assert_eq!(d.objects.len(), 3);
    assert_eq!(d.arrows.len(), 3);
    assert_eq!(d.arrows[2].to, End::Cell(At::new(1, 0)));

    // From the upper arrow's handle to the lower one.
    let mut ab: Vec<Pos2> = h.get_all(by().label("Arrow: A → B")).map(|n| n.rect().center()).collect();
    assert_eq!(ab.len(), 2);
    ab.sort_by(|p, q| p.y.total_cmp(&q.y));
    let (upper, lower) = (ab[0], ab[1]);
    drag(&mut h, upper, lower, Modifiers::NONE);
    let d = &editor(&h).d;
    assert!(d.arrows.iter().any(|x| matches!((x.from, x.to), (End::Arrow(_), End::Arrow(_))) && x.body == Body::Double), "{:?}", d.arrows);

    let b = centre(&h, object("B"));
    let below_b = centre(&h, cell(2, 2));
    drag(&mut h, b, below_b, Modifiers::ALT);
    assert!(editor(&h).d.object(At::new(1, 1)).is_some_and(|o| o.label == "B"), "{:?}", editor(&h).d.objects);
}

/// Double-clicking an inserted diagram opens it in the editor; Update changes it in place, and
/// undo and redo step through the editor's changes.
#[test]
fn edits_a_diagram_in_place_with_undo() {
    let mut s = deckcraft_engine::Session::with_new();
    let id = s.execute("insert.latex", &json!({"latex": r#"\begin{tikzcd} A \arrow[r, "f"] & B \end{tikzcd}"#})).unwrap()["id"].clone();
    let mut h = harness(s);
    h.state_mut().run("app.dialog", json!({"id": "commutativeDiagram"})).unwrap();
    h.run_steps(4);
    h.get(object("B")).click();
    h.run_steps(2);
    h.key_press(Key::Delete);
    h.run_steps(2);
    assert_eq!(editor(&h).d.objects.len(), 1);
    assert!(editor(&h).d.arrows.is_empty(), "the arrow to B went with it");
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run_steps(2);
    assert_eq!(editor(&h).d.objects.len(), 2);
    assert_eq!(editor(&h).d.arrows.len(), 1);

    // Templates add beside what is there.
    h.get(by().label("Templates")).click();
    h.run_steps(2);
    h.get(by().role(Role::Button).label("Pullback")).click();
    h.run_steps(3);
    assert_eq!(editor(&h).d.objects.len(), 6);
    assert!(editor(&h).d.arrows.iter().any(|a| a.body == Body::Phantom));

    h.get(by().role(Role::Button).label("Update")).click();
    h.run_steps(3);
    let all = diagrams(&h);
    assert_eq!(all.len(), 1, "updated, not added");
    assert!(all[0].contains(r"\lrcorner"), "{}", all[0]);
    let shapes = &h.state().session.doc().unwrap().doc.slides[0].shapes;
    assert!(shapes.iter().any(|sh| sh.id.0 as u64 == id.as_u64().unwrap()));
}

/// Esc in a label puts it back; Esc with something selected lets it go; Esc with nothing selected
/// closes, asking first when there are changes.
#[test]
fn escape_reverts_then_deselects_then_closes() {
    let mut h = harness(deckcraft_engine::Session::with_new());
    h.state_mut().run("app.dialog", json!({"id": "commutativeDiagram"})).unwrap();
    h.run_steps(3);
    h.get(cell(1, 1)).click();
    h.run_steps(3);
    type_label(&mut h, "X");
    assert_eq!(editor(&h).d.objects[0].label, "X");
    h.key_press(Key::Escape);
    h.run_steps(2);
    assert_eq!(editor(&h).d.objects[0].label, r"\bullet", "the label as it was");
    h.key_press(Key::Escape);
    h.run_steps(2);
    assert!(editor(&h).sel.is_empty());
    h.key_press(Key::Escape);
    h.run_steps(2);
    assert!(h.state().dialog.is_some(), "a changed diagram asks first");
    h.get(by().role(Role::Button).label("Discard")).click();
    h.run_steps(2);
    assert!(h.state().dialog.is_none() && h.state().cd_editor.is_none());
    assert!(diagrams(&h).is_empty());
}
