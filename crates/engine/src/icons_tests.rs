//! Catalogue, rendering and attribution regressions exercised through the public commands.
use super::*;
use crate::Session;
use serde_json::json;

#[test]
fn names_filters_and_legacy_aliases_are_stable() {
    let mut s = Session::with_new();
    assert_eq!(icons().len(), 1646);
    let mut ids = std::collections::HashSet::new();
    for icon in icons() {
        assert!(ids.insert(icon.name), "{}", icon.name);
    }
    assert_eq!(find("Star").unwrap().collection, "Lucide");
    assert_eq!(find("arrow_right").unwrap().name, "arrow-right");
    assert_eq!(find("house").unwrap().collection, "Lucide");
    assert!(find("fontawesome:solid:house").is_some());
    assert!(find("fontawesome:brands:microsoft").is_none());
    let found = s.execute("insert.icons", &json!({"collection":"Font Awesome", "style":"regular", "query":"HOME", "category":"Buildings"})).unwrap();
    assert!(found.as_array().unwrap().iter().any(|v| v["name"] == "fontawesome:regular:house"));
    assert!(found.as_array().unwrap().iter().all(|v| v["style"] == "regular"));
    assert!(s.execute("insert.icons", &json!({"query":"no-such-icon-xyz"})).unwrap().as_array().unwrap().is_empty());
}

#[test]
fn filled_icon_hole_and_credits_survive_native_and_pptx_round_trips() {
    let mut s = Session::with_new();
    s.execute("insert.icon", &json!({"name":"fontawesome:regular:circle", "rect":[100,100,100,100]})).unwrap();
    let doc = s.doc().unwrap();
    let icon = doc.shapes().last().unwrap();
    let Geom::Custom { paths } = &icon.geom else { panic!("custom geometry") };
    assert_eq!(paths[0].fill, deckcraft_geom::preset::FillMode::Norm);
    assert!(!paths[0].stroke);
    assert!(icon.descr.is_empty(), "credits must not occupy accessibility alt text");
    let credits = deckcraft_model::icon_credits::text(&doc.doc);
    assert!(credits.contains("CC-BY-4.0") && credits.contains("Fonticons, Inc."));
    let native = deckcraft_format::save(&doc.doc).unwrap();
    let native = deckcraft_format::load(&native).unwrap();
    assert_eq!(deckcraft_model::icon_credits::text(&native), credits);
    let bytes = deckcraft_pptx::export(&native).unwrap();
    let pptx = deckcraft_pptx::import(&bytes).unwrap();
    assert_eq!(deckcraft_model::icon_credits::text(&pptx), credits);
    assert!(pptx.props.comments.contains("CC-BY-4.0"));
    for doc in [&native, &pptx] {
        let img = deckcraft_render::render_slide(doc, 0, &Default::default());
        assert!(img.pixel(150, 150)[0] > 230, "circle hole must stay transparent");
        assert!(img.pixel(150, 102)[0] < 100, "circle rim must be filled");
    }
    let bytes = deckcraft_pptx::export(&pptx).unwrap();
    let again = deckcraft_pptx::import(&bytes).unwrap();
    assert_eq!(again.props.comments, pptx.props.comments, "credits must not grow on repeated exports");
    s.execute("edit.undo", &json!({})).unwrap();
    assert!(deckcraft_model::icon_credits::text(&s.doc().unwrap().doc).is_empty());
    s.execute("edit.redo", &json!({})).unwrap();
    assert_eq!(s.execute("insert.iconCredits", &json!({})).unwrap()["text"], credits);
}

#[test]
fn filled_icons_preserve_source_aspect_in_a_square_box() {
    use deckcraft_geom::KurboShape;
    let icon = icons().iter().find(|i| i.filled() && i.view[0] > i.view[1]).unwrap();
    let shape = shape(icon, Xfrm::new(0.0, 0.0, 72.0, 72.0)).unwrap();
    let geometry = deckcraft_render::shape_geometry(&shape, 72.0, 72.0);
    let bounds = geometry.paths[0].path.bounding_box();
    let src = outline(icon.name).unwrap().bounding_box();
    assert!((bounds.width() / bounds.height() - src.width() / src.height()).abs() < 0.001);
}

#[test]
fn image_exports_embed_credits_without_changing_pixels() {
    let mut s = Session::with_new();
    s.execute("insert.icon", &json!({"name":"fontawesome:solid:house"})).unwrap();
    let doc = &s.doc().unwrap().doc;
    let img = deckcraft_render::render_slide(doc, 0, &deckcraft_render::RenderOpts { scale: 0.25, ..Default::default() });
    for (format, bytes) in [("png", img.to_png()), ("jpeg", img.to_jpeg(90))] {
        let before = image::load_from_memory(&bytes).unwrap().to_rgba8();
        let stamped = deckcraft_render::image_credits(bytes, format, doc);
        assert!(stamped.windows(11).any(|w| w == b"CC-BY-4.0 h"));
        assert_eq!(image::load_from_memory(&stamped).unwrap().to_rgba8(), before);
    }
    let gif = crate::gif_export::encode(doc, &crate::gif_export::GifOptions { width: 64, ..Default::default() }).unwrap();
    assert!(gif.bytes.windows(9).any(|w| w == b"CC-BY-4.0"));
    assert!(image::load_from_memory(&gif.bytes).is_ok());
    let (png, _, _) = crate::cmd::file::render_png(doc, 0, 0.25, false);
    assert!(png.windows(9).any(|w| w == b"CC-BY-4.0"));
    let pdf = crate::cmd::file::save_bytes(doc, "pdf").unwrap();
    assert!(pdf.windows(9).any(|w| w == b"CC-BY-4.0"));
}

#[test]
fn malformed_icon_rect_does_not_change_the_document_or_history() {
    let mut s = Session::with_new();
    let before = deckcraft_format::save(&s.doc().unwrap().doc).unwrap();
    for rect in [json!([0, 0, 0, 72]), json!([0, 0, -72, 72]), json!([0, 0, 72]), json!([0, 0, "bad", 72]), json!([0, 0, 1e20, 72])] {
        assert!(s.execute("insert.icon", &json!({"name":"fontawesome:solid:house","rect":rect})).is_err());
    }
    assert_eq!(deckcraft_format::save(&s.doc().unwrap().doc).unwrap(), before);
    assert!(s.doc().unwrap().history.undo.is_empty());
}
