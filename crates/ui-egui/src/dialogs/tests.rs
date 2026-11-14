use deckcraft_model::text::{Action, Hyperlink, Run, TextBody};
use deckcraft_model::{Presentation, SlideId};

use super::{
    CurrentLink, HyperlinkSeed, common_click, hyperlink_command, hyperlink_seed_of, hyperlink_should_apply, link_covering, seed_from, seed_of_link,
};

fn link(action: Action, tip: &str) -> Hyperlink {
    Hyperlink { action, tooltip: tip.into(), highlight_click: false, sound: None, stop_sound: false, zoom_return: None }
}

fn url(address: &str, tip: &str) -> Hyperlink {
    let mut h = link(Action::Url { url: address.into() }, tip);
    h.highlight_click = tip == "glow";
    h
}

/// Header & Footer ▸ Apply without picking a format leaves a slide's existing date field in
/// its own format instead of rewriting it to the default one.
#[test]
fn header_footer_apply_keeps_an_existing_date_format() {
    use std::sync::Arc;

    use egui::vec2;
    use egui_kittest::kittest::Queryable as _;
    use serde_json::json;

    use crate::SlideApp;
    let mut session = deckcraft_engine::Session::with_new();
    session.execute("design.headerFooter", &json!({"date": true, "dateText": "", "dateFormat": "datetime4", "all": true})).unwrap();
    // A file whose document-level format is unset, as an imported deck's is.
    Arc::make_mut(&mut session.doc_mut().unwrap().doc).header_footer.date_format = String::new();
    let date_fields = |app: &SlideApp| -> Vec<String> {
        app.session
            .doc()
            .unwrap()
            .doc
            .slides
            .iter()
            .flat_map(|sl| sl.shapes.iter())
            .filter(|sh| sh.ph_type() == Some(deckcraft_model::PhType::Date))
            .flat_map(|sh| sh.text.iter())
            .flat_map(|t| t.paragraphs.iter())
            .flat_map(|p| p.runs.iter())
            .filter_map(|r| match &r.kind {
                deckcraft_model::text::RunKind::Field { field } => Some(field.clone()),
                _ => None,
            })
            .collect()
    };
    let mut harness = egui_kittest::Harness::builder().with_size(vec2(1200.0, 900.0)).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        SlideApp::new(session, crate::Services::default()),
    );
    harness.run_steps(2);
    assert_eq!(date_fields(harness.state()), vec!["datetime4".to_string()]);
    harness.state_mut().run("app.dialog", json!({"id": "headerFooter"})).unwrap();
    harness.run_steps(2);
    harness.get_by_label("Apply").click_accesskit();
    harness.run_steps(3);
    assert_eq!(date_fields(harness.state()), vec!["datetime4".to_string()], "the dialog left the field's format alone");
}

/// The Export… dialog (Animated GIF) shows the GIF options and exports through `file.export` (#171).
#[test]
fn export_dialog_writes_an_animated_gif() {
    use std::cell::RefCell;

    use egui::vec2;
    use egui_kittest::kittest::Queryable as _;
    use serde_json::json;

    use crate::SlideApp;
    let dir = std::env::temp_dir().join(format!("deckcraft-ui-gif-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("deck.gif");
    let picked = path.to_string_lossy().to_string();
    let suggested = std::rc::Rc::new(RefCell::new(String::new()));
    let seen = suggested.clone();
    let services = crate::Services {
        pick_save: Some(Box::new(move |s: &str| {
            *seen.borrow_mut() = s.to_string();
            Some(picked.clone())
        })),
        ..Default::default()
    };
    let mut session = deckcraft_engine::Session::with_new();
    session.execute("slide.new", &json!({})).unwrap();
    let mut harness = egui_kittest::Harness::builder().with_size(vec2(1200.0, 900.0)).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        SlideApp::new(session, services),
    );
    harness.run_steps(2);
    harness.state_mut().run("app.exportDialog", json!({})).unwrap();
    harness.run_steps(2);
    harness.get_by_label("Animated GIF (.gif)").click_accesskit();
    harness.run_steps(2);
    assert_eq!(harness.state().dialog.as_ref().and_then(|d| d.fields.get("fmt")).map(String::as_str), Some("gif"));
    harness.get_by_label("Seconds per slide:");
    harness.get_by_label("Loop forever").click_accesskit();
    harness.run_steps(2);
    // A range outside the deck keeps the dialog open with a message, and nothing is written.
    harness.state_mut().dialog.as_mut().unwrap().fields.insert("gifTo".into(), "9".into());
    harness.run_steps(2);
    harness.key_press(egui::Key::Enter);
    harness.run_steps(2);
    assert!(harness.state().dialog.is_some());
    assert!(harness.state().status.as_ref().is_some_and(|(m, _)| m.contains("Slides")), "{:?}", harness.state().status);
    assert!(!path.exists());
    harness.state_mut().dialog.as_mut().unwrap().fields.insert("gifTo".into(), "2".into());
    harness.run_steps(2);
    harness.key_press(egui::Key::Enter);
    harness.run_steps(2);
    assert!(harness.state().dialog.is_none(), "the dialog closes after exporting");
    assert!(suggested.borrow().ends_with(".gif"), "{}", suggested.borrow());
    let bytes = std::fs::read(&path).unwrap();
    assert!(bytes.starts_with(b"GIF89a"));
    assert_eq!(u16::from_le_bytes([bytes[6], bytes[7]]), 960, "medium by default");
    assert!(!bytes.windows(11).any(|w| w == b"NETSCAPE2.0"), "Loop forever unticked: plays once");
    let _ = std::fs::remove_dir_all(&dir);
}

/// The Export… dialog offers "Transparent background" for PNG only, and the picture it
/// writes then leaves the slide background out (#784).
#[test]
fn export_dialog_writes_a_png_with_a_transparent_background() {
    use egui::vec2;
    use egui_kittest::kittest::Queryable as _;
    use serde_json::json;

    use crate::SlideApp;
    let dir = std::env::temp_dir().join(format!("deckcraft-ui-png-clear-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("slide.png");
    let picked = path.to_string_lossy().to_string();
    let services = crate::Services { pick_save: Some(Box::new(move |_: &str| Some(picked.clone()))), ..Default::default() };
    let mut harness = egui_kittest::Harness::builder().with_size(vec2(1200.0, 900.0)).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        SlideApp::new(deckcraft_engine::Session::with_new(), services),
    );
    harness.run_steps(2);
    harness.state_mut().run("app.exportDialog", json!({})).unwrap();
    harness.run_steps(2);
    assert!(harness.query_by_label("Transparent background").is_none(), "PDF has no transparent background");
    harness.get_by_label("JPEG image (current slide)").click_accesskit();
    harness.run_steps(2);
    assert!(harness.query_by_label("Transparent background").is_none(), "nor has JPEG");
    harness.get_by_label("PNG images (current slide)").click_accesskit();
    harness.run_steps(2);
    harness.get_by_label("Transparent background").click_accesskit();
    harness.run_steps(2);
    assert_eq!(harness.state().dialog.as_ref().and_then(|d| d.fields.get("pngTransparent")).map(String::as_str), Some("true"));
    harness.key_press(egui::Key::Enter);
    harness.run_steps(2);
    assert!(harness.state().dialog.is_none(), "the dialog closes after exporting");
    let img = deckcraft_render::decode_image(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(img.get_pixel(2, 2).0[3], 0, "the background is left out");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn no_link_opens_as_a_new_web_address() {
    let seed = seed_from(CurrentLink::None, &[]);
    assert_eq!(seed, HyperlinkSeed::blank());
    assert!(hyperlink_should_apply(&seed, "url", "https://", "", 0));
}

#[test]
fn web_email_and_slide_links_fill_the_dialog() {
    let ids = [SlideId(7), SlideId(9)];
    let web = seed_of_link(&url("https://example.org", "See"), &ids);
    assert_eq!(web.mode, "url");
    assert_eq!(web.url, "https://example.org");
    assert_eq!(web.tip, "See");
    assert!(!web.preserve);
    assert!(hyperlink_should_apply(&web, "url", "https://example.org", "", 0));

    let mail = seed_of_link(&url("mailto:a@b.org", "Write"), &ids);
    assert_eq!(mail.mode, "email");
    assert_eq!(mail.email, "a@b.org");
    assert_eq!(mail.tip, "Write");
    assert_eq!(hyperlink_command("email", "", "a@b.org", 0, "Write", false)["url"], "mailto:a@b.org");

    let slide = seed_of_link(&link(Action::Slide { slide: SlideId(9) }, "There"), &ids);
    assert_eq!(slide.mode, "slide");
    assert_eq!(slide.slide, 1);
    assert_eq!(slide.tip, "There");
    assert!(!slide.preserve);

    let glow = url("https://example.org", "glow");
    assert!(glow.highlight_click);
    assert!(seed_of_link(&glow, &ids).highlight);
    assert_eq!(hyperlink_command("url", "https://example.org", "", 0, "glow", true)["highlightClick"], true);
}

#[test]
fn actions_this_dialog_does_not_edit_survive_ok() {
    let next = seed_of_link(&link(Action::NextSlide, "On"), &[]);
    assert!(next.preserve);
    assert_eq!(next.tip, "On");
    assert_eq!(next.url, "");
    assert!(!hyperlink_should_apply(&next, "url", "", "", 0));
    assert!(hyperlink_should_apply(&next, "url", "https://example.org", "", 0));

    let gone = seed_of_link(&link(Action::Slide { slide: SlideId(99) }, ""), &[SlideId(1)]);
    assert!(gone.preserve, "a missing slide is not retargeted to the first slide");
    assert!(!hyperlink_should_apply(&gone, "url", "", "", 0));

    let mixed = seed_from(CurrentLink::Mixed, &[]);
    assert!(mixed.preserve);
    assert!(!hyperlink_should_apply(&mixed, "url", "", "", 0));
    assert!(hyperlink_should_apply(&mixed, "email", "", "a@b.org", 0));
}

#[test]
fn a_text_span_reports_the_link_it_covers() {
    let mut body = TextBody::from_text("Hello world");
    body.paragraphs[0].runs.clear();
    body.paragraphs[0].runs.push(Run::new("Hello "));
    let mut world = Run::new("world");
    world.props.link = Some(url("https://example.org", "tip"));
    body.paragraphs[0].runs.push(world);

    assert_eq!(link_covering(&body, (0, 6), (0, 6)), CurrentLink::None, "a caret inserts; it does not edit the run beside it");
    match link_covering(&body, (0, 6), (0, 11)) {
        CurrentLink::One(h) => assert_eq!(h.tooltip, "tip"),
        other => panic!("linked word, got {other:?}"),
    }
    assert_eq!(link_covering(&body, (0, 0), (0, 11)), CurrentLink::Mixed);

    let mut other = url("https://example.org", "tip");
    other.tooltip = "other".into();
    assert_eq!(common_click([None, None]), CurrentLink::None);
    assert!(matches!(common_click([Some(url("https://example.org", "tip"))]), CurrentLink::One(_)));
    assert_eq!(common_click([Some(url("https://example.org", "tip")), Some(other)]), CurrentLink::Mixed);
    assert_eq!(common_click(std::iter::empty()), CurrentLink::None);
}

#[test]
fn the_open_dialog_reads_the_caret_word_and_the_shape_click() {
    let mut doc = Presentation::default();
    let shape_id = {
        let slide = std::sync::Arc::make_mut(&mut doc.slides[0]);
        let mut body = TextBody::from_text("Hello world");
        body.paragraphs[0].runs.clear();
        body.paragraphs[0].runs.push(Run::new("Hello "));
        let mut world = Run::new("world");
        world.props.link = Some(url("https://example.org", "tip"));
        body.paragraphs[0].runs.push(world);
        slide.shapes[0].text = Some(body);
        slide.shapes[0].id
    };
    let mut st = deckcraft_engine::DocState::new(doc, None, "t".into());
    st.selection.text = Some(deckcraft_engine::TextSel { shape: shape_id, anchor: (0, 8), caret: (0, 8), ..Default::default() });
    let seed = hyperlink_seed_of(Some(&st));
    assert_eq!(seed.mode, "url");
    assert_eq!(seed.url, "https://example.org");
    assert_eq!(seed.tip, "tip");
    assert!(!seed.preserve);

    st.selection.text = None;
    st.selection.shapes = vec![shape_id];
    {
        let doc = std::sync::Arc::make_mut(&mut st.doc);
        let slide = std::sync::Arc::make_mut(&mut doc.slides[0]);
        let shape = slide.shapes.iter_mut().find(|sh| sh.id == shape_id).unwrap();
        let mut click = url("https://shape.example", "shape tip");
        click.highlight_click = true;
        shape.click = Some(click);
    }
    let seed = hyperlink_seed_of(Some(&st));
    assert_eq!(seed.url, "https://shape.example");
    assert_eq!(seed.tip, "shape tip");
    assert!(seed.highlight);

    st.selection.text = Some(deckcraft_engine::TextSel { shape: shape_id, anchor: (0, 11), caret: (0, 11), ..Default::default() });
    let seed = hyperlink_seed_of(Some(&st));
    assert_eq!(seed, HyperlinkSeed::blank(), "a caret at the end of the word inserts a new link");
}

/// Insert ▸ Equation typesets LaTeX (a proof tree here), keeps the dialog open on LaTeX it can't
/// read, and opened again on what it made, shows that LaTeX and updates it in place.
#[test]
fn the_equation_dialog_typesets_latex_and_edits_it_again() {
    use egui::accesskit::Role;
    use egui::{Key, Modifiers, vec2};
    use egui_kittest::kittest::Queryable as _;
    use serde_json::json;

    use crate::SlideApp;
    let session = deckcraft_engine::Session::with_new();
    let mut h = egui_kittest::Harness::builder().with_size(vec2(1400.0, 900.0)).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        SlideApp::new(session, crate::Services::default()),
    );
    h.run_steps(2);
    let groups = |app: &SlideApp| app.session.doc().unwrap().doc.slides[0].shapes.iter().filter(|s| s.is_group()).count();
    let field = || egui_kittest::kittest::by().role(Role::MultilineTextInput).label_contains("Type LaTeX");
    // Buttons named `label`: the ribbon's Insert tab, then the dialog's.
    let button = |label: &'static str| egui_kittest::kittest::by().role(Role::Button).label(label);
    let type_into_field = |h: &mut egui_kittest::Harness<'_, SlideApp>, text: &str| {
        h.get(field()).focus();
        h.run_steps(1);
        h.key_combination_modifiers(Modifiers::COMMAND, &[Key::A]);
        h.get(field()).type_text(text);
        h.run_steps(2);
    };

    h.state_mut().run("app.dialog", json!({"id": "equation"})).unwrap();
    h.run_steps(2);
    // A step with more premises than there are is reported, and the dialog stays open.
    type_into_field(&mut h, r"\hypo{A}\infer2{B}");
    h.get_all(button("Insert")).last().unwrap().click();
    h.run_steps(2);
    assert_eq!(groups(h.state()), 0);
    assert_eq!(h.get_all(button("Insert")).count(), 2, "the dialog stays open on an error");

    let proof = r"\hypo{A}\hypo{A \to B}\infer2[MP]{B}";
    type_into_field(&mut h, proof);
    h.get_all(button("Insert")).last().unwrap().click();
    h.run_steps(3);
    assert_eq!(groups(h.state()), 1);
    assert_eq!(h.get_all(button("Insert")).count(), 1, "the dialog closed");
    assert_eq!(h.state_mut().run("latex.get", json!({})).unwrap()["latex"].as_str(), Some(proof));

    // Opened on the proof: its LaTeX, and Update instead of Insert.
    h.state_mut().run("app.dialog", json!({"id": "equation"})).unwrap();
    h.run_steps(2);
    assert_eq!(h.get(field()).value().as_deref(), Some(proof));
    let longer = r"\hypo{A}\hypo{A \to B}\infer2[MP]{B}\infer1[$\to$I]{(A \to B) \to B}";
    type_into_field(&mut h, longer);
    h.get(button("Update")).click();
    h.run_steps(3);
    assert_eq!(groups(h.state()), 1);
    assert_eq!(h.state_mut().run("latex.get", json!({})).unwrap()["latex"].as_str(), Some(longer));

    // Opened on it again with "Change the selected one" unticked, it inserts a second proof and
    // leaves the first as it is.
    h.state_mut().run("app.dialog", json!({"id": "equation"})).unwrap();
    h.run_steps(2);
    h.get(egui_kittest::kittest::by().role(Role::CheckBox).label("Change the selected one")).click();
    h.run_steps(2);
    h.get_all(button("Insert")).last().unwrap().click();
    h.run_steps(3);
    assert_eq!(groups(h.state()), 2);
    let sources: Vec<String> = h.state().session.doc().unwrap().doc.slides[0]
        .shapes
        .iter()
        .flat_map(|s| s.tags.iter().filter(|(n, _)| n == "DECKCRAFT_LATEX").map(|(_, v)| v.clone()))
        .collect();
    assert_eq!(sources, vec![longer.to_string(), longer.to_string()]);
}

/// In the equation dialog Enter starts a new line in the LaTeX and Ctrl+Enter (⌘Enter) inserts it;
/// double-clicking what it made (as the canvas does) opens the dialog again on that LaTeX.
#[test]
fn equation_dialog_takes_new_lines_and_double_click_reopens_it() {
    use egui::accesskit::Role;
    use egui::{Key, Modifiers, vec2};
    use egui_kittest::kittest::Queryable as _;
    use serde_json::json;

    use crate::SlideApp;
    let session = deckcraft_engine::Session::with_new();
    let mut h = egui_kittest::Harness::builder().with_size(vec2(1400.0, 900.0)).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        SlideApp::new(session, crate::Services::default()),
    );
    h.run_steps(2);
    let groups = |app: &SlideApp| app.session.doc().unwrap().doc.slides[0].shapes.iter().filter(|s| s.is_group()).cloned().collect::<Vec<_>>();
    let field = || egui_kittest::kittest::by().role(Role::MultilineTextInput).label_contains("Type LaTeX");
    let button = |label: &'static str| egui_kittest::kittest::by().role(Role::Button).label(label);
    let two_lines = "\\hypo{A}\n\\infer1[R]{B}";

    h.state_mut().run("app.dialog", json!({"id": "equation"})).unwrap();
    h.run_steps(2);
    h.get(field()).focus();
    h.run_steps(1);
    h.get(field()).type_text(r"\hypo{A}");
    h.run_steps(1);
    h.key_press(Key::Enter);
    h.run_steps(1);
    h.get(field()).type_text(r"\infer1[R]{B}");
    h.run_steps(2);
    assert_eq!(h.get(field()).value().as_deref(), Some(two_lines));
    assert!(groups(h.state()).is_empty());
    assert_eq!(h.get_all(button("Insert")).count(), 2, "Enter doesn't close the dialog");

    h.key_press_modifiers(Modifiers::COMMAND, Key::Enter);
    h.run_steps(3);
    let made = groups(h.state());
    assert_eq!(made.len(), 1);
    assert_eq!(h.get_all(button("Insert")).count(), 1, "Ctrl+Enter inserted and closed the dialog");
    assert_eq!(h.state_mut().run("latex.get", json!({})).unwrap()["latex"].as_str(), Some(two_lines));

    // Double-click a statement of the proof: the Proof Tree builder opens on it, to update it.
    h.state_mut().run("edit.selectNone", json!({})).ok();
    let g = &made[0];
    let statement = g.children().iter().find(|c| c.text.is_some()).and_then(|c| c.xfrm).unwrap();
    let (x, y) = (g.xfrm.unwrap().x + statement.x + statement.w / 2.0, g.xfrm.unwrap().y + statement.y + statement.h / 2.0);
    let ev =
        deckcraft_engine::PointerEvent { kind: deckcraft_engine::PointerKind::DoubleClick, x, y, mods: Default::default(), tol: 3.0, pressure: None };
    h.state_mut().session.pointer(ev).unwrap();
    h.run_steps(3);
    assert_eq!(h.state().dialog.as_ref().map(|d| d.id.as_str()), Some("proofTree"));
    let b = h.state().proof_builder.as_ref().unwrap();
    assert_eq!(
        deckcraft_engine::typeset::proof_edit::to_ebproof(&b.forest),
        "\\begin{prooftree}\n    \\hypo{A}\n  \\infer1[R]{B}\n\\end{prooftree}\n"
    );
    assert!(h.query(button("Update")).is_some());
}
