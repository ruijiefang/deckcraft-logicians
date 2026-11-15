use deckcraft_engine::typeset::algo::{self, Kind, Part, edit};
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

fn editor<'a>(h: &'a Harness<'_, SlideApp>) -> &'a super::Editor {
    h.state().algo_editor.as_ref().expect("the tool is open")
}

fn field() -> By<'static> {
    by().role(Role::TextInput).label("Line (LaTeX)")
}

/// Test labels live for the test run.
fn text(s: String) -> &'static str {
    Box::leak(s.into_boxed_str())
}

fn row(n: usize) -> By<'static> {
    by().role(Role::Button).label_contains(text(format!("Line {n}: ")))
}

/// The LaTeX of every typeset object on the first slide.
fn latex(h: &Harness<'_, SlideApp>) -> Vec<String> {
    h.state().session.doc().unwrap().doc.slides[0]
        .shapes
        .iter()
        .flat_map(|s| s.tags.iter().filter(|(n, _)| n == "DECKCRAFT_LATEX").map(|(_, v)| v.clone()))
        .collect()
}

fn open(h: &mut Harness<'_, SlideApp>) {
    h.state_mut().run("app.dialog", json!({"id": "pseudocode"})).unwrap();
    h.run_steps(4);
}

/// Types into the row being edited one key at a time, as a person does, then presses Enter.
fn line(h: &mut Harness<'_, SlideApp>, s: &str) {
    for c in s.chars() {
        h.get(field()).type_text(&c.to_string());
        h.run_steps(1);
    }
    h.key_press(Key::Enter);
    h.run_steps(3);
}

/// Insert ▸ Pseudo-code opens the tool on an empty line; an algorithm typed with the keyboard
/// alone (keywords make blocks, Enter on an empty last line steps out of one, Backspace on an
/// empty line drops it) and Ctrl+Enter insert it as algpseudocode, numbered like algorithmicx.
#[test]
fn types_an_algorithm_with_the_keyboard_alone() {
    let mut h = harness(deckcraft_engine::Session::with_new());
    h.state_mut().ui.tab = "Insert".into();
    h.run_steps(2);
    h.get(by().role(Role::Button).label_contains("Pseudo-")).click();
    h.run_steps(4);
    assert_eq!(h.state().dialog.as_ref().map(|d| d.id.as_str()), Some("pseudocode"));
    assert_eq!(editor(&h).editing, Some(0), "typing starts at once");

    line(&mut h, "procedure Euclid(a, b)");
    line(&mut h, r"for all $v \in V$");
    line(&mut h, r"$x \gets v$");
    line(&mut h, "");
    h.key_press(Key::Escape);
    h.run_steps(2);
    let p = edit::line(&editor(&h).a, &[(0, 0), (0, 0)]).cloned().unwrap();
    assert_eq!((p.kind, p.text.as_str()), (Kind::ForAll, r"$v \in V$"));
    h.state_mut().algo_editor.as_mut().unwrap().a.body[0].body.clear();
    h.state_mut().algo_editor.as_mut().unwrap().a.body[0].body.push(algo::Line::state(""));
    h.state_mut().algo_editor.as_mut().unwrap().sel = vec![1];
    h.run_steps(1);
    h.key_press(Key::Enter);
    h.run_steps(3);
    line(&mut h, r"$r \gets a \bmod b$");
    line(&mut h, r"while $r \neq 0$");
    line(&mut h, r"$a \gets b$");
    line(&mut h, r"$b \gets r$");
    line(&mut h, r"$r \gets a \bmod b$");
    line(&mut h, ""); // out of the while
    line(&mut h, "return $b$");
    h.key_press(Key::Backspace); // the empty line after it
    h.run_steps(3);

    let a = &editor(&h).a;
    assert_eq!(a.body.len(), 1, "{:#?}", a.body);
    let p = &a.body[0];
    assert_eq!((p.kind, p.text.as_str(), p.args.as_str()), (Kind::Procedure, "Euclid", "a, b"));
    let kinds: Vec<Kind> = p.body.iter().map(|l| l.kind).collect();
    assert_eq!(kinds, [Kind::State, Kind::While, Kind::Return]);
    assert_eq!(p.body[1].body.len(), 3);
    assert_eq!(p.body[2].text, "$b$");
    // procedure, 1 line, while, 3 lines, end while, return, end procedure.
    let numbered = algo::rows(a, false).iter().filter(|r| r.number.is_some()).count();
    assert_eq!(numbered, 9);

    h.key_press_modifiers(Modifiers::COMMAND, Key::Enter);
    h.run_steps(4);
    assert!(h.state().dialog.is_none() && h.state().algo_editor.is_none());
    let all = latex(&h);
    assert_eq!(all.len(), 1);
    let src = &all[0];
    for want in [r"\begin{algorithm}", r"\Procedure{Euclid}{a, b}", r"\While{$r \neq 0$}", r"\State \Return $b$", r"\EndWhile", r"\EndProcedure"] {
        assert!(src.contains(want), "{want} in\n{src}");
    }
}

/// The "+ below" button adds a line under the hovered row; the side panel adds a While around a
/// selected range; Delete on a block keeps its lines; undo goes back.
#[test]
fn builds_with_buttons_and_restructures() {
    let mut h = harness(deckcraft_engine::Session::with_new());
    open(&mut h);
    line(&mut h, "$x \\gets 0$");
    line(&mut h, "$y \\gets 1$");
    h.get(field()).type_text("$z \\gets 2$");
    h.run_steps(2);
    h.key_press(Key::Escape); // keeps nothing typed in this row: it was empty before
    h.run_steps(2);
    assert_eq!(editor(&h).a.body.len(), 3);
    assert_eq!(editor(&h).a.body[2].text, "", "Esc put the line back");

    // Rows 2 and 3 into a While.
    h.get(row(2)).click();
    h.run_steps(2);
    h.event(Event::ModifiersChanged(Modifiers::SHIFT));
    h.get(row(3)).click();
    h.run_steps(2);
    h.event(Event::ModifiersChanged(Modifiers::NONE));
    assert_eq!(editor(&h).sel, [1, 2]);
    h.get(by().role(Role::Button).label("While")).click();
    h.run_steps(3);
    let a = &editor(&h).a;
    assert_eq!(a.body.len(), 2);
    assert_eq!((a.body[1].kind, a.body[1].body.len()), (Kind::While, 2));
    h.get(field()).type_text("$y < 9$");
    h.run_steps(2);
    assert_eq!(editor(&h).a.body[1].text, "$y < 9$");
    h.key_press(Key::Escape);
    h.run_steps(2);

    // + below line 1, then Tab: into nothing (no block above), so it stays.
    h.get(row(1)).hover();
    h.run_steps(2);
    h.get(by().role(Role::Button).label("Add a line below line 1")).click();
    h.run_steps(3);
    assert_eq!(editor(&h).a.body.len(), 3);
    assert_eq!(editor(&h).editing, Some(1));
    h.get(field()).type_text("$s \\gets 0$");
    h.run_steps(2);
    h.key_press(Key::Escape);
    h.run_steps(2);

    // Delete the While: its lines stay, outdented.
    let w = editor(&h).rows().iter().position(|r| r.path == [(0, 2)] && r.part == Part::Line).unwrap();
    h.state_mut().algo_editor.as_mut().unwrap().sel = vec![w];
    h.run_steps(1);
    h.key_press(Key::Delete);
    h.run_steps(3);
    let kinds: Vec<Kind> = editor(&h).a.body.iter().map(|l| l.kind).collect();
    assert_eq!(kinds, [Kind::State; 4]);
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run_steps(3);
    assert_eq!(editor(&h).a.body[2].kind, Kind::While);
}

/// A selected algorithm opens in the tool; Templates, a drag that moves a line, and Update
/// change it in place.
#[test]
fn edits_an_algorithm_in_place() {
    let mut s = deckcraft_engine::Session::with_new();
    let src = "\\begin{algorithmic}[1]\n\\State $a$\n\\State $b$\n\\State $c$\n\\end{algorithmic}";
    s.execute("insert.latex", &json!({"latex": src})).unwrap();
    let mut h = harness(s);
    open(&mut h);
    assert!(editor(&h).target.is_some());
    assert_eq!(editor(&h).a.body.len(), 3);
    assert!(editor(&h).editing.is_none());

    // Drag line 3 above line 1.
    let from = h.get(row(3)).rect().center();
    let to = h.get(row(1)).rect().center() - vec2(0.0, 4.0);
    drag(&mut h, from, to);
    let texts: Vec<&str> = editor(&h).a.body.iter().map(|l| l.text.as_str()).collect();
    assert_eq!(texts, ["$c$", "$a$", "$b$"]);

    h.get(by().role(Role::Button).label("Update")).click();
    h.run_steps(4);
    let all = latex(&h);
    assert_eq!(all.len(), 1, "updated, not added");
    let c = all[0].find("$c$").unwrap();
    let a = all[0].find("$a$").unwrap();
    assert!(c < a, "{}", all[0]);
}

/// Templates fill an empty tool; Esc with nothing selected asks before discarding.
#[test]
fn templates_and_discard() {
    let mut h = harness(deckcraft_engine::Session::with_new());
    open(&mut h);
    h.key_press(Key::Escape);
    h.run_steps(2);
    h.get(by().label("Templates")).click();
    h.run_steps(2);
    h.get(by().role(Role::Button).label("Binary search")).click();
    h.run_steps(4);
    assert!(editor(&h).a.count() > 8);
    assert!(editor(&h).a.caption.to_lowercase().contains("search") || !editor(&h).a.body.is_empty());
    h.state_mut().algo_editor.as_mut().unwrap().sel.clear();
    h.key_press(Key::Escape);
    h.run_steps(2);
    assert!(h.state().dialog.is_some(), "a changed algorithm asks first");
    h.get(by().role(Role::Button).label("Discard")).click();
    h.run_steps(2);
    assert!(h.state().dialog.is_none() && h.state().algo_editor.is_none());
    assert!(latex(&h).is_empty());
}

/// Typing `else` in an If's body splits it; `else if x` adds an else-if.
#[test]
fn else_typed_in_an_if_makes_a_branch() {
    let mut h = harness(deckcraft_engine::Session::with_new());
    open(&mut h);
    line(&mut h, "if $x > 0$");
    line(&mut h, "$y \\gets 1$");
    line(&mut h, "else if $x < 0$");
    line(&mut h, "$y \\gets -1$");
    line(&mut h, "else");
    h.get(field()).type_text("$y \\gets 0$");
    h.run_steps(2);
    let l = &editor(&h).a.body[0];
    assert_eq!(l.kind, Kind::If);
    assert_eq!(l.body.len(), 1);
    assert_eq!(l.branches.len(), 2, "{:#?}", l.branches);
    assert_eq!(l.branches[0].cond.as_deref(), Some("$x < 0$"));
    assert_eq!(l.branches[1].cond, None);
    assert_eq!(l.branches[1].body[0].text, "$y \\gets 0$");
    let src = algo::write(&editor(&h).a);
    assert!(src.contains(r"\ElsIf{$x < 0$}") && src.contains(r"\Else"), "{src}");
}

fn drag(h: &mut Harness<'_, SlideApp>, from: Pos2, to: Pos2) {
    h.input_mut().events.push(Event::PointerMoved(from));
    h.run_steps(2);
    h.input_mut().events.push(Event::PointerButton { pos: from, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
    h.run_steps(2);
    for k in 1..=6 {
        let p = from + (to - from) * (k as f32 / 6.0);
        h.input_mut().events.push(Event::PointerMoved(p));
        h.run_steps(1);
    }
    h.input_mut().events.push(Event::PointerButton { pos: to, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
    h.run_steps(3);
}

/// Shift+Tab while typing takes the line out of its block, Tab puts it back in as the block's
/// last line; the typing carries on in the moved line.
#[test]
fn tab_and_shift_tab_while_typing() {
    let mut h = harness(deckcraft_engine::Session::with_new());
    open(&mut h);
    line(&mut h, "while $x$");
    line(&mut h, "$a$");
    h.get(field()).type_text("$b$");
    h.run_steps(2);
    h.key_press_modifiers(Modifiers::SHIFT, Key::Tab);
    h.run_steps(3);
    let a = &editor(&h).a;
    assert_eq!((a.body.len(), a.body[0].body.len()), (2, 1), "{:#?}", a.body);
    assert_eq!(a.body[1].text, "$b$");
    assert!(editor(&h).editing.is_some(), "still typing");
    h.key_press(Key::Tab);
    h.run_steps(3);
    let a = &editor(&h).a;
    assert_eq!((a.body.len(), a.body[0].body.len()), (1, 2));
    assert_eq!(a.body[0].body[1].text, "$b$");
}

/// Entering a line and leaving it unchanged is no change: no undo step, and closing doesn't ask.
/// Esc puts back even the first line of a new algorithm.
#[test]
fn looking_at_a_line_changes_nothing() {
    let mut s = deckcraft_engine::Session::with_new();
    s.execute("insert.latex", &json!({"latex": "\\begin{algorithmic}[1]\n\\State $a$\n\\end{algorithmic}"})).unwrap();
    let mut h = harness(s);
    open(&mut h);
    h.get(row(1)).click();
    h.run_steps(2);
    h.key_press(Key::Enter);
    h.run_steps(3);
    assert_eq!(editor(&h).editing, Some(0));
    h.key_press(Key::Escape);
    h.run_steps(2);
    assert!(editor(&h).undo.is_empty(), "no undo step");
    h.key_press(Key::Escape); // deselect
    h.run_steps(2);
    h.key_press(Key::Escape); // close
    h.run_steps(2);
    assert!(h.state().dialog.is_none(), "nothing changed, so it closes without asking");

    let mut h = harness(deckcraft_engine::Session::with_new());
    open(&mut h);
    h.get(field()).type_text("x");
    h.run_steps(2);
    assert_eq!(editor(&h).undo.len(), 1);
    h.key_press(Key::Escape);
    h.run_steps(2);
    assert_eq!(editor(&h).a.body[0].text, "");
    assert!(editor(&h).undo.is_empty());
}
