use egui::accesskit::Role;
use egui::{Key, Modifiers, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::{By, Queryable as _, by};
use serde_json::json;

use crate::SlideApp;

fn harness(session: deckcraft_engine::Session) -> Harness<'static, SlideApp> {
    let mut h = Harness::builder().with_size(vec2(1500.0, 950.0)).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        SlideApp::new(session, crate::Services::default()),
    );
    h.run_steps(2);
    h
}

fn button(label: &'static str) -> By<'static> {
    by().role(Role::Button).label(label)
}

fn statement() -> By<'static> {
    by().role(Role::TextInput).label_contains("Statement")
}

/// The LaTeX of every proof on the first slide.
fn proofs(h: &Harness<'_, SlideApp>) -> Vec<String> {
    h.state().session.doc().unwrap().doc.slides[0]
        .shapes
        .iter()
        .flat_map(|s| s.tags.iter().filter(|(n, _)| n == "DECKCRAFT_LATEX").map(|(_, v)| v.clone()))
        .collect()
}

fn open(h: &mut Harness<'_, SlideApp>) {
    h.state_mut().run("app.dialog", json!({"id": "proofTree"})).unwrap();
    h.run_steps(3);
}

fn type_statement(h: &mut Harness<'_, SlideApp>, text: &str) {
    h.get(statement()).type_text(text);
    h.run_steps(2);
}

/// The Insert tab's Proof Tree button opens the builder, and a proof is built from the keyboard:
/// type the conclusion, Ctrl+↑ for a premise, Tab for the next one, Ctrl+Enter to insert.
#[test]
fn builds_a_proof_from_the_keyboard() {
    let mut h = harness(deckcraft_engine::Session::with_new());
    h.state_mut().ui.tab = "Insert".into();
    h.run_steps(2);
    h.get(by().role(Role::Button).label_contains("Proof")).click();
    h.run_steps(3);
    assert_eq!(h.state().dialog.as_ref().map(|d| d.id.as_str()), Some("proofTree"));
    assert!(h.get(statement()).is_focused(), "a new proof starts in its conclusion's statement");

    type_statement(&mut h, "C");
    h.key_press_modifiers(Modifiers::COMMAND, Key::ArrowUp);
    h.run_steps(3);
    type_statement(&mut h, "A");
    h.key_press(Key::Tab);
    h.run_steps(3);
    type_statement(&mut h, "B");
    h.key_press_modifiers(Modifiers::COMMAND, Key::Enter);
    h.run_steps(3);
    assert_eq!(proofs(&h), ["\\begin{prooftree}\n    \\hypo{A}\n    \\hypo{B}\n  \\infer2{C}\n\\end{prooftree}\n"]);
    assert!(h.state().dialog.is_none() && h.state().proof_builder.is_none(), "inserting closes the builder and drops its state");
}

/// Double-clicking a proof opens it in the builder; a step clicked and deleted, an undo, a redo,
/// and Update changes that proof in place.
#[test]
fn double_click_edits_a_proof_in_place() {
    let mut s = deckcraft_engine::Session::with_new();
    let id = s.execute("insert.latex", &json!({"latex": r"\hypo{A}\hypo{B}\infer2[R]{C}"})).unwrap()["id"].clone();
    let mut h = harness(s);
    // Double-click its first premise, as the canvas does.
    let (gx, gy, a) = {
        let st = h.state().session.doc().unwrap();
        let g = st.shape(deckcraft_model::ShapeId(id.as_u64().unwrap() as u32)).unwrap();
        let a = g.children().iter().find(|c| c.text.as_ref().is_some_and(|t| t.text() == "A")).and_then(|c| c.xfrm).unwrap();
        (g.xfrm.unwrap().x, g.xfrm.unwrap().y, a)
    };
    let ev = deckcraft_engine::PointerEvent {
        kind: deckcraft_engine::PointerKind::DoubleClick,
        x: gx + a.x + a.w / 2.0,
        y: gy + a.y + a.h / 2.0,
        mods: Default::default(),
        tol: 3.0,
        pressure: None,
    };
    h.state_mut().session.pointer(ev).unwrap();
    h.run_steps(3);
    assert_eq!(h.state().dialog.as_ref().map(|d| d.id.as_str()), Some("proofTree"));
    assert!(h.query(button("Update")).is_some(), "it edits the proof it was opened on");

    h.get(by().role(Role::Button).label("Step: A")).click();
    h.run_steps(2);
    h.key_press(Key::Delete);
    h.run_steps(2);
    assert!(h.query(by().label("Step: A")).is_none());
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run_steps(2);
    assert!(h.query(by().label("Step: A")).is_some(), "undo puts it back");
    h.key_press_modifiers(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z);
    h.run_steps(2);
    h.get(button("Update")).click();
    h.run_steps(3);
    assert_eq!(proofs(&h), ["\\begin{prooftree}\n    \\hypo{B}\n  \\infer1[R]{C}\n\\end{prooftree}\n"]);
    assert_eq!(h.state().session.doc().unwrap().doc.slides[0].shapes.iter().filter(|s| s.is_group()).count(), 1, "updated, not added");
}

/// A template fills the empty conclusion; a step dragged onto the right of another moves there;
/// LaTeX pasted from the clipboard comes in as a branch; Esc asks before discarding.
#[test]
fn templates_drag_paste_and_discard() {
    let mut h = harness(deckcraft_engine::Session::with_new());
    open(&mut h);
    // Leave the statement field so the picture has the keyboard.
    h.key_press(Key::Enter);
    h.run_steps(2);
    h.get(by().role(Role::Button).label("Templates")).click();
    h.run_steps(2);
    h.get(by().role(Role::Button).label_contains("modus ponens")).click();
    h.run_steps(3);
    let b = h.state().proof_builder.as_ref().unwrap();
    assert_eq!(b.forest[0].premises.len(), 2);
    assert!(b.forest[0].right.as_ref().is_some_and(|r| r.src.contains(r"\to")));

    // Drag the first premise onto the right quarter of the second: they swap.
    let steps: Vec<egui::Rect> = h.get_all(by().role(Role::Button).label_contains("Step: Γ")).map(|n| n.rect()).collect();
    let (first, second) = {
        let mut premises: Vec<egui::Rect> = steps.into_iter().filter(|r| r.top() < 9_999.0).collect();
        premises.sort_by(|a, b| a.top().total_cmp(&b.top()).then(a.left().total_cmp(&b.left())));
        (premises[0], premises[1])
    };
    let before = h.state().proof_builder.as_ref().unwrap().forest[0].premises.clone();
    let to = egui::pos2(second.right() - second.width() * 0.1, second.center().y);
    h.hover_at(first.center());
    h.run_steps(1);
    h.drag_at(first.center());
    h.run_steps(1);
    h.hover_at(first.center() + vec2(12.0, 0.0));
    h.run_steps(1);
    h.hover_at(to);
    h.run_steps(1);
    h.drop_at(to);
    h.run_steps(3);
    let after = h.state().proof_builder.as_ref().unwrap().forest[0].premises.clone();
    assert_eq!((after[0].clone(), after[1].clone()), (before[1].clone(), before[0].clone()), "the dragged premise moved to the right");

    // Paste a branch as LaTeX onto the selected step.
    h.event(egui::Event::Paste(r"\hypo{X}\infer1{Y}".into()));
    h.run_steps(2);
    assert!(h.query(by().label("Step: X")).is_some());

    h.key_press(Key::Escape);
    h.run_steps(2);
    assert!(h.query(by().label_contains("Discard the changes")).is_some(), "Esc asks first");
    h.get(button("Discard")).click();
    h.run_steps(3);
    assert!(h.state().dialog.is_none() && proofs(&h).is_empty());
}

/// A symbol goes in at the statement's cursor; Esc in the field puts the statement back.
#[test]
fn symbols_and_esc_in_the_statement() {
    let mut h = harness(deckcraft_engine::Session::with_new());
    open(&mut h);
    type_statement(&mut h, "A B");
    // Cursor between A and B.
    h.key_press(Key::ArrowLeft);
    h.key_press(Key::ArrowLeft);
    h.run_steps(1);
    h.get(by().role(Role::Button).label(r"Insert \wedge")).click();
    h.run_steps(3);
    assert_eq!(h.get(statement()).value().as_deref(), Some(r"A\wedge  B"));
    h.key_press(Key::Escape);
    h.run_steps(2);
    assert_eq!(h.get(statement()).value().as_deref(), Some(""), "Esc in the field restores the statement it started with");
    assert_eq!(h.state().dialog.as_ref().map(|d| d.id.as_str()), Some("proofTree"), "and doesn't close the builder");
}

/// The Equation dialog opens a proof in the builder, and the builder hands its LaTeX back.
#[test]
fn routes_between_the_equation_dialog_and_the_builder() {
    let mut h = harness(deckcraft_engine::Session::with_new());
    h.state_mut().run("app.dialog", json!({"id": "equation"})).unwrap();
    h.run_steps(2);
    let field = by().role(Role::MultilineTextInput).label_contains("Type LaTeX");
    h.get(field.clone()).focus();
    h.run_steps(1);
    h.get(field).type_text(r"\hypo{A}\infer1[R]{B}");
    h.run_steps(2);
    h.get(button("Open in Proof Tree builder")).click();
    h.run_steps(3);
    assert_eq!(h.state().dialog.as_ref().map(|d| d.id.as_str()), Some("proofTree"));
    let b = h.state().proof_builder.as_ref().unwrap();
    assert_eq!((b.forest.len(), b.forest[0].premises.len()), (1, 1));

    h.get(by().label("LaTeX (ebproof)")).click();
    h.run_steps(2);
    h.get(button("Edit as LaTeX…")).click();
    h.run_steps(3);
    let d = h.state().dialog.as_ref().unwrap();
    assert_eq!(d.id, "equation");
    assert_eq!(d.fields.get("eq").map(String::as_str), Some("\\begin{prooftree}\n    \\hypo{A}\n  \\infer1[R]{B}\n\\end{prooftree}\n"));
    assert!(h.state().proof_builder.is_none(), "leaving the builder drops its state");
}
