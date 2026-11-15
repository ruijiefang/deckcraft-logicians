//! Insert > Icons popup (#971): clicking inside it, e.g. in the "Search icons" box, must not close it.
#![allow(clippy::expect_used)]

use deckcraft_engine::Session;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;

use crate::{Services, SlideApp};

#[test]
fn clicking_inside_the_icons_popup_keeps_it_open() {
    let mut app = SlideApp::new(Session::with_new(), Services::default());
    app.ui.tab = "Insert".into();
    let mut h = Harness::builder().with_size(egui::vec2(1800.0, 900.0)).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(3);
    h.get_by_label("Icons").click();
    h.run_steps(3);
    // A category heading is plain popup content, like the search box: a click on it is a click inside.
    let first = deckcraft_engine::icons::icons().first().expect("the icon gallery is empty");
    h.get_by_label(first.category).click();
    h.run_steps(3);
    assert!(h.query_by_label(first.category).is_some(), "the icons popup closed after a click inside it");
}

#[test]
fn icon_gallery_pages_and_accessible_cells_insert_in_one_step() {
    let mut app = SlideApp::new(Session::with_new(), Services::default());
    app.ui.tab = "Insert".into();
    let mut h = Harness::builder().with_size(egui::vec2(1800.0, 900.0)).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(3);
    h.get_by_label("Icons").click();
    h.run_steps(3);
    h.get_by_label("Next icons").click();
    h.run_steps(3);
    // The second page is entirely Font Awesome; first-page Lucide cells are no longer laid out.
    assert!(h.query_by_label("Arrow up (Lucide, stroke)").is_none());
    h.get_by_label("Previous icons").click();
    h.run_steps(3);
    h.get_by_label("Arrow up (Lucide, stroke)").click();
    h.run_steps(3);
    assert_eq!(h.state().session.doc().expect("document").shapes().last().expect("shape").name, "Arrow up");
}

#[test]
fn searching_aliases_and_inserting_a_filled_icon_keeps_credits() {
    let mut app = SlideApp::new(Session::with_new(), Services::default());
    app.ui.tab = "Insert".into();
    let mut h = Harness::builder().with_size(egui::vec2(1800.0, 900.0)).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(3);
    h.get_by_label("Icons").click();
    h.run_steps(3);
    h.get_by_role(egui::accesskit::Role::TextInput).click();
    h.run_steps(3);
    h.get_by_role(egui::accesskit::Role::TextInput).type_text("home solid");
    h.run_steps(3);
    h.get_by_label("House (Font Awesome, solid)").click();
    h.run_steps(3);
    let doc = &h.state().session.doc().expect("document").doc;
    assert!(deckcraft_model::icon_credits::text(doc).contains("Fonticons, Inc."));
}
