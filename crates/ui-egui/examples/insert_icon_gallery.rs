//! Inspect the actual Insert Icons popup in both themes. Outputs stay outside the repository.
//! cargo run -p deckcraft-ui-egui --example insert_icon_gallery -- /tmp/insert-icons
use deckcraft_engine::Session;
use deckcraft_ui_egui::{Services, SlideApp};
use egui_kittest::{Harness, kittest::Queryable};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output = std::env::args().nth(1).unwrap_or_else(|| "/tmp/insert-icons".into());
    for dark in [false, true] {
        let mut app = SlideApp::new(Session::with_new(), Services::default());
        app.ui.tab = "Insert".into();
        let mut h = Harness::builder().with_size(egui::vec2(1500.0, 900.0)).wgpu().build_ui_state(
            move |ui, app: &mut SlideApp| {
                ui.ctx().set_visuals(if dark { egui::Visuals::dark() } else { egui::Visuals::light() });
                app.logic(ui.ctx());
                app.ui(ui);
            },
            app,
        );
        h.run_steps(3);
        let start = std::time::Instant::now();
        h.get_by_label("Icons").click();
        h.run_steps(3);
        println!("{} gallery opening (three frames): {:?}", if dark { "dark" } else { "light" }, start.elapsed());
        h.get_by_role(egui::accesskit::Role::TextInput).click();
        h.run_steps(3);
        h.get_by_role(egui::accesskit::Role::TextInput).type_text("heart");
        h.run_steps(3);
        h.render()?.save(format!("{output}-{}.png", if dark { "dark" } else { "light" }))?;
    }
    Ok(())
}
