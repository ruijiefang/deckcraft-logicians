use super::galleries::shapes_gallery;
use super::home::layout_menu;
use super::view::zoom_menu;
use super::{
    Color32, CornerRadius, Icon, Rect, Sense, SlideApp, Stroke, Tokens, Ui, big, big_button, big_button_ex, enabled, group, json, menu_item, pos2,
    run, tip, vec2,
};

/// Photo Album is a split button: the main click starts a new album, the arrow edits one.
fn photo_album_button(app: &mut SlideApp, ui: &mut Ui) {
    let en = enabled(app, "insert.photoAlbum");
    let (main, arrow) = big_button_ex(ui, Icon::Picture, "Photo\nAlbum", en, true, false);
    let _ = main.clone().on_hover_text(tip("insert.photoAlbum", "Photo Album…"));
    main.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, en, "Photo Album…"));
    if let Some(a) = &arrow {
        let _ = a.clone().on_hover_text("Photo Album options");
        a.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, en, "Photo Album options"));
    }
    if main.clicked() {
        run(app, "insert.photoAlbum", json!({}));
    }
    if let Some(a) = arrow {
        egui::Popup::menu(&a).show(|ui| {
            menu_item(app, ui, Icon::Picture, "New Photo Album…", "insert.photoAlbum", json!({}));
            menu_item(app, ui, Icon::Picture, "Edit Photo Album…", "insert.photoAlbumEdit", json!({}));
        });
    }
}

// ---------- Insert ----------

pub(super) fn insert(app: &mut SlideApp, ui: &mut Ui) {
    group(ui, |ui| {
        let (main, arrow) = big_button_ex(ui, Icon::NewSlide, "New\nSlide", enabled(app, "slide.new"), true, false);
        if main.clicked() {
            run(app, "slide.new", json!({}));
        }
        if let Some(a) = arrow {
            egui::Popup::menu(&a).width(420.0).show(|ui| layout_menu(app, ui, "slide.new"));
        }
    });
    group(ui, |ui| {
        let r = big_button(ui, Icon::Table, "Table", enabled(app, "insert.table"));
        egui::Popup::menu(&r).show(|ui| {
            if let Some((rr, cc)) = table_grid_picker(ui) {
                run(app, "insert.table", json!({"rows": rr, "cols": cc}));
                ui.close();
            }
            ui.separator();
            if ui.button("Insert Table…").clicked() {
                app.dialog = Some(crate::dialogs::Dialog::new("table"));
                ui.close();
            }
            if ui.button("Draw Table").clicked() {
                app.session.set_tool(deckcraft_engine::ToolKind::Shape { preset: deckcraft_engine::tools::DRAW_TABLE.into() });
                ui.close();
            }
        });
    });
    group(ui, |ui| {
        big(app, ui, Icon::Picture, "Pictures", "app.insertPictureDialog", json!({}));
        photo_album_button(app, ui);
        let r = big_button(ui, Icon::Screenshot, "Screenshot", false);
        let _ = r.on_hover_text("Screenshot (not available yet)");
    });
    group(ui, |ui| {
        let r = big_button(ui, Icon::Shapes, "Shapes", enabled(app, "shape.insert"));
        egui::Popup::menu(&r).show(|ui| {
            if let Some(p) = shapes_gallery(ui) {
                app.session.set_tool(deckcraft_engine::ToolKind::Shape { preset: p.to_string() });
            }
        });
        let r = big_button(ui, Icon::Icons, "Icons", enabled(app, "insert.icon"));
        egui::Popup::menu(&r).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).width(340.0).show(|ui| icon_library(app, ui));
        big(app, ui, Icon::SmartArt, "Diagram", "app.dialog", json!({"id": "smartart"}));
        let r = big_button(ui, Icon::Chart, "Chart", enabled(app, "insert.chart"));
        egui::Popup::menu(&r).show(|ui| {
            for k in chart_menu_types() {
                if k.is_none() {
                    ui.separator();
                }
                if let Some(k) = k
                    && ui.button(k.label()).clicked()
                {
                    run(app, "insert.chart", json!({"type": format!("{k:?}")}));
                }
            }
        });
    });
    group(ui, |ui| {
        let r = big_button(ui, Icon::Zoom, "Zoom", enabled(app, "insert.zoom"));
        egui::Popup::menu(&r).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).width(300.0).show(|ui| zoom_menu(app, ui));
        big(app, ui, Icon::Link, "Link", "app.dialog", json!({"id": "hyperlink"}));
        let r = big_button(ui, Icon::Action, "Action", enabled(app, "insert.actionButton"));
        egui::Popup::menu(&r).show(|ui| {
            for k in ["back", "forward", "beginning", "end", "home", "information", "return", "movie", "document", "sound", "help", "custom"] {
                if ui.button(format!("Action Button: {k}")).clicked() {
                    run(app, "insert.actionButton", json!({"kind": k}));
                }
            }
            ui.separator();
            let has_sel = app.session.active().is_some_and(|s| !s.selection.shapes.is_empty());
            if ui.add_enabled(has_sel, egui::Button::new("Action Settings…")).clicked() {
                app.dialog = Some(crate::dialogs::Dialog::new("action"));
            }
        });
    });
    group(ui, |ui| {
        big(app, ui, Icon::Comment, "Comment", "app.dialog", json!({"id": "comment"}));
    });
    group(ui, |ui| {
        big(app, ui, Icon::TextBox, "Text\nBox", "insert.textBox", json!({}));
        big(app, ui, Icon::HeaderFooter, "Header &\nFooter", "app.dialog", json!({"id": "headerFooter"}));
        let r = big_button(ui, Icon::WordArt, "Decorative Text", enabled(app, "insert.wordArt"));
        egui::Popup::menu(&r).show(|ui| {
            for i in 0..6 {
                if ui.button(format!("Decorative Text style {}", i + 1)).clicked() {
                    run(app, "insert.wordArt", json!({"style": i}));
                }
            }
        });
        big(app, ui, Icon::DateTime, "Date &\nTime", "insert.dateTime", json!({}));
        big(app, ui, Icon::SlideNumber, "Slide\nNumber", "insert.slideNumber", json!({}));
    });
    group(ui, |ui| {
        big(app, ui, Icon::Equation, "Equation", "app.dialog", json!({"id": "equation"}));
        big(app, ui, Icon::ProofTree, "Proof\nTree", "app.dialog", json!({"id": "proofTree"}));
        big(app, ui, Icon::CommutativeDiagram, "Commutative\nDiagram", "app.dialog", json!({"id": "commutativeDiagram"}));
        big(app, ui, Icon::Pseudocode, "Pseudo-\ncode", "app.dialog", json!({"id": "pseudocode"}));
        big(app, ui, Icon::Symbol, "Symbol", "app.dialog", json!({"id": "symbol"}));
    });
    group(ui, |ui| {
        big(app, ui, Icon::Video, "Video", "app.insertVideoDialog", json!({}));
        big(app, ui, Icon::Audio, "Audio", "app.insertAudioDialog", json!({}));
    });
}

/// Hover-to-size table grid. Returns (rows, cols) when clicked.
pub fn table_grid_picker(ui: &mut Ui) -> Option<(usize, usize)> {
    let t = Tokens::get(ui.ctx());
    let id = ui.scope_id().with("tgrid");
    let mut hover: (usize, usize) = ui.data_mut(|d| d.get_temp(id).unwrap_or((0, 0)));
    let cell = 16.0;
    let label = if hover.0 > 0 { format!("{}x{} Table", hover.1, hover.0) } else { "Insert Table".into() };
    ui.label(label);
    let (rect, resp) = ui.allocate_exact_size(vec2(10.0 * (cell + 2.0), 8.0 * (cell + 2.0)), Sense::click());
    if let Some(p) = resp.hover_pos() {
        let c = ((p.x - rect.min.x) / (cell + 2.0)).floor() as usize + 1;
        let r = ((p.y - rect.min.y) / (cell + 2.0)).floor() as usize + 1;
        hover = (r.min(8), c.min(10));
    }
    for r in 0..8 {
        for c in 0..10 {
            let cr = Rect::from_min_size(rect.min + vec2(c as f32 * (cell + 2.0), r as f32 * (cell + 2.0)), vec2(cell, cell));
            let on = r < hover.0 && c < hover.1;
            ui.painter().rect(
                cr,
                CornerRadius::ZERO,
                if on { t.accent.gamma_multiply(0.3) } else { t.card },
                Stroke::new(1.0, if on { t.accent } else { t.border }),
                egui::StrokeKind::Inside,
            );
        }
    }
    ui.data_mut(|d| d.insert_temp(id, hover));
    if resp.clicked() && hover.0 > 0 { Some(hover) } else { None }
}

const ICON_PAGE: usize = 96;

#[derive(Clone, Default)]
struct IconGallery {
    query: String,
    collection: String,
    style: String,
    category: String,
    page: usize,
    credits: bool,
    // At most two pages of monochrome thumbnails. Texture handles share storage when cloned.
    previews: std::collections::HashMap<&'static str, egui::TextureHandle>,
}

fn icon_filter(ui: &mut Ui, id: &str, selected: &mut String, all: &str, choices: &[&str]) {
    craft_ui::ComboBox::from_id_salt(id).selected_text(if selected.is_empty() { all } else { selected.as_str() }).show_ui(ui, |ui| {
        ui.selectable_value(selected, String::new(), all);
        for choice in choices {
            ui.selectable_value(selected, (*choice).into(), *choice);
        }
    });
}

/// Bounded pages keep layout and thumbnail rendering independent of catalogue size.
fn icon_library(app: &mut SlideApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let id = ui.scope_id().with("icon-gallery");
    let mut state = ui.data_mut(|d| d.get_temp::<IconGallery>(id).unwrap_or_default());
    let before = (state.query.clone(), state.collection.clone(), state.style.clone(), state.category.clone());
    ui.add(egui::TextEdit::singleline(&mut state.query).hint_text("Search icons").desired_width(ui.available_width()).char_limit(256));
    ui.horizontal(|ui| {
        icon_filter(ui, "icon-collection", &mut state.collection, "All collections", &["Lucide", "Font Awesome"]);
        icon_filter(ui, "icon-style", &mut state.style, "All styles", &["stroke", "solid", "regular"]);
    });
    let icons = deckcraft_engine::icons::icons();
    let mut categories: Vec<&str> = icons
        .iter()
        .filter(|i| state.collection.is_empty() || i.collection == state.collection)
        .flat_map(|i| std::iter::once(i.category).chain(i.categories.iter().copied()))
        .collect();
    categories.sort_unstable();
    categories.dedup();
    if !state.category.is_empty() && !categories.contains(&state.category.as_str()) {
        state.category.clear();
    }
    icon_filter(ui, "icon-category", &mut state.category, "All categories", &categories);
    if before != (state.query.clone(), state.collection.clone(), state.style.clone(), state.category.clone()) {
        state.page = 0;
    }
    let matches: Vec<_> = icons
        .iter()
        .filter(|i| {
            (state.collection.is_empty() || state.collection == i.collection)
                && (state.style.is_empty() || state.style == i.style)
                && (state.category.is_empty() || i.in_category(&state.category))
                && i.matches(&state.query)
        })
        .collect();
    let pages = matches.len().div_ceil(ICON_PAGE).max(1);
    state.page = state.page.min(pages - 1);
    ui.horizontal(|ui| {
        if ui.add_enabled(state.page > 0, egui::Button::new("Previous icons")).clicked() {
            state.page = state.page.saturating_sub(1);
        }
        ui.label(format!("{} / {} · {} icons", state.page + 1, pages, matches.len()));
        if ui.add_enabled(state.page + 1 < pages, egui::Button::new("Next icons")).clicked() {
            state.page += 1;
        }
    });
    egui::ScrollArea::vertical().id_salt("icon-scroll").max_height(360.0).show(ui, |ui| {
        if matches.is_empty() {
            ui.label("No icons match.");
        }
        let page: Vec<_> = matches.iter().skip(state.page * ICON_PAGE).take(ICON_PAGE).copied().collect();
        if !state.query.trim().is_empty() || !state.category.is_empty() {
            ui.horizontal_wrapped(|ui| {
                for icon in &page {
                    icon_cell(app, ui, icon, &mut state, t.text, t.hover);
                }
            });
        } else {
            // Keep category headings, including the legacy gallery's first Arrows heading.
            let mut start = 0;
            while let Some(first) = page.get(start) {
                let end = start + page.iter().skip(start).take_while(|i| i.category == first.category).count();
                ui.label(egui::RichText::new(first.category).small().strong());
                ui.horizontal_wrapped(|ui| {
                    for icon in page.get(start..end).unwrap_or_default() {
                        icon_cell(app, ui, icon, &mut state, t.text, t.hover);
                    }
                });
                start = end;
            }
        }
    });
    ui.separator();
    ui.small("Font Awesome Free: Fonticons, Inc. · CC-BY-4.0");
    ui.hyperlink_to("Licence and attribution", "https://creativecommons.org/licenses/by/4.0/");
    if ui.button("Used icon credits").clicked() {
        state.credits = !state.credits;
    }
    if state.credits {
        let text = app.session.execute("insert.iconCredits", &json!({})).ok().and_then(|v| v["text"].as_str().map(str::to_owned)).unwrap_or_default();
        if text.is_empty() {
            ui.label("No Font Awesome icons in this presentation.");
        } else {
            if ui.button("Copy icon credits").clicked() {
                ui.ctx().copy_text(text.clone());
            }
            ui.label("Credits are embedded in exported files. Include these credits when sharing images on services that remove metadata.");
        }
    }
    ui.data_mut(|d| d.insert_temp(id, state));
}

fn icon_cell(app: &mut SlideApp, ui: &mut Ui, icon: &deckcraft_engine::icons::Icon, state: &mut IconGallery, color: Color32, hover: Color32) {
    let (r, resp) = ui.allocate_exact_size(vec2(36.0, 36.0), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, format!("{} ({}, {})", icon.label, icon.collection, icon.style)));
    if resp.hovered() || resp.has_focus() {
        ui.painter().rect_filled(r, CornerRadius::same(4), hover);
    }
    if let Some(texture) = icon_thumbnail(ui.ctx(), icon, state) {
        ui.painter().image(texture, r.shrink(6.0), Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), color);
    }
    let activate = resp.clicked() || (resp.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter) || i.key_pressed(egui::Key::Space)));
    resp.on_hover_text(format!("{} · {} {}", icon.label, icon.collection, icon.style));
    if activate {
        let _ = app.run("insert.icon", json!({"name": icon.name}));
        ui.close();
    }
}

fn icon_thumbnail(ctx: &egui::Context, icon: &deckcraft_engine::icons::Icon, state: &mut IconGallery) -> Option<egui::TextureId> {
    if let Some(tex) = state.previews.get(icon.name) {
        return Some(tex.id());
    }
    let mut shape = deckcraft_engine::icons::shape(icon, deckcraft_geom::Xfrm { x: 2.0, y: 2.0, w: 24.0, h: 24.0, ..Default::default() })?;
    let white = deckcraft_model::Fill::solid(deckcraft_model::ColorRef::rgb(deckcraft_color::Rgba::WHITE));
    if icon.filled() {
        shape.fill = Some(white);
    } else if let Some(line) = &mut shape.line {
        line.fill = Some(white);
    }
    let mut doc = deckcraft_model::defaults::new_presentation(None);
    doc.slide_size = deckcraft_geom::Size::new(28.0, 28.0);
    let slide = std::sync::Arc::make_mut(doc.slides.first_mut()?);
    slide.shapes = vec![shape];
    slide.show_master_shapes = false;
    let img =
        deckcraft_render::render_slide(&doc, 0, &deckcraft_render::RenderOpts { scale: 2.0, transparent_background: true, ..Default::default() });
    let mask = egui::ColorImage::from_rgba_unmultiplied([img.width as usize, img.height as usize], &img.to_straight());
    if state.previews.len() >= ICON_PAGE * 2 {
        state.previews.clear();
    }
    let tex = ctx.load_texture(icon.name, mask, egui::TextureOptions::LINEAR);
    let id = tex.id();
    state.previews.insert(icon.name, tex);
    Some(id)
}

/// The chart types in the Insert Chart and Change Chart Type menus: the main ones, a separator
/// (`None`), then the Office 2016 types not already listed.
pub(super) fn chart_menu_types() -> Vec<Option<deckcraft_model::chart::ChartType>> {
    use deckcraft_model::chart::ChartType;
    let mut v: Vec<Option<ChartType>> = ChartType::MAIN.iter().copied().map(Some).collect();
    v.push(None);
    v.extend(ChartType::OFFICE_2016.iter().copied().filter(|k| !ChartType::MAIN.contains(k)).map(Some));
    v
}
