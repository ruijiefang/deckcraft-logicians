//! Offline Insert › Icons catalogue: Lucide strokes and Font Awesome Free silhouettes.
use deckcraft_color::SchemeSlot;
use deckcraft_geom::{Affine, BezPath, PathEl, Xfrm};
use deckcraft_model::{ColorRef, CustomPath, Fill, Geom, Line, Shape, ShapeStyle};
use std::sync::OnceLock;

pub const VIEW: f64 = 24.0;

/// One bundled icon. Font Awesome names include collection and style to avoid collisions.
#[derive(Clone)]
pub struct Icon {
    pub name: &'static str,
    pub label: &'static str,
    pub category: &'static str,
    pub collection: &'static str,
    pub style: &'static str,
    pub view: [f64; 2],
    pub terms: &'static str,
    pub categories: &'static [&'static str],
    pub stroke: f64,
    pub d: &'static str,
    cache: OnceLock<Option<BezPath>>,
    search: OnceLock<String>,
}
include!("icon_data.rs");
include!("fontawesome_data.rs");

pub fn icons() -> &'static [Icon] {
    static ALL: OnceLock<Vec<Icon>> = OnceLock::new();
    ALL.get_or_init(|| LUCIDE.iter().chain(FONT_AWESOME.iter()).cloned().collect())
}

/// Legacy labels and underscore names keep resolving to Lucide; new collections require IDs.
pub fn find(name: &str) -> Option<&'static Icon> {
    let raw = name.trim();
    if raw.is_empty() || raw.len() > 256 {
        return None;
    }
    let key = raw.to_ascii_lowercase().replace('_', "-");
    icons().iter().find(|i| i.name == key).or_else(|| icons().iter().find(|i| i.collection == "Lucide" && i.label.eq_ignore_ascii_case(raw)))
}

pub fn outline(name: &str) -> Option<&'static BezPath> {
    let icon = find(name)?;
    icon.cache.get_or_init(|| bezpath(icon.d)).as_ref()
}

pub fn path_data(name: &str) -> Option<String> {
    outline(name).and_then(deck_path)
}

pub fn stroke_width(icon: &Icon, w: f64, h: f64) -> f64 {
    let span = w.min(h).abs().min(4_000.0);
    (span * icon.stroke / VIEW).clamp(0.25, 400.0)
}

impl Icon {
    pub fn filled(&self) -> bool {
        self.stroke == 0.0
    }
    pub fn in_category(&self, category: &str) -> bool {
        self.category == category || self.categories.contains(&category)
    }
    /// Search all words, allowing aliases and category keywords in any order.
    pub fn matches(&self, query: &str) -> bool {
        if query.trim().is_empty() {
            return true;
        }
        let haystack = self.search.get_or_init(|| {
            format!("{} {} {} {} {} {} {}", self.name, self.label, self.category, self.collection, self.style, self.terms, self.categories.join(" "))
                .to_lowercase()
        });
        query.split_whitespace().all(|word| haystack.contains(&word.to_lowercase()))
    }
}

/// Shape construction shared by insertion and thumbnail rendering. Padding preserves the source
/// aspect within the requested box; legacy Lucide geometry and stroke sizes stay unchanged.
pub fn shape(icon: &Icon, rect: Xfrm) -> Option<Shape> {
    if ![rect.x, rect.y, rect.w, rect.h].iter().all(|v| v.is_finite()) || rect.w <= 0.0 || rect.h <= 0.0 {
        return None;
    }
    let mut bp = outline(icon.name)?.clone();
    let [vw, vh] = icon.view;
    let (pw, ph) = if icon.filled() {
        let scale = (rect.w / vw).min(rect.h / vh);
        let (pw, ph) = (rect.w / scale, rect.h / scale);
        bp.apply_affine(Affine::translate(((pw - vw) / 2.0, (ph - vh) / 2.0)));
        (pw, ph)
    } else {
        (vw, vh)
    };
    let filled = icon.filled();
    let mut shape = Shape {
        name: icon.label.into(),
        xfrm: Some(rect),
        geom: Geom::Custom {
            paths: vec![CustomPath {
                w: pw,
                h: ph,
                d: deck_path(&bp)?,
                fill: if filled { deckcraft_geom::preset::FillMode::Norm } else { deckcraft_geom::preset::FillMode::None },
                stroke: !filled,
            }],
        },
        fill: Some(if filled { Fill::solid(ColorRef::scheme(SchemeSlot::Tx1)) } else { Fill::None }),
        line: Some(Line {
            fill: Some(if filled { Fill::None } else { Fill::solid(ColorRef::scheme(SchemeSlot::Tx1)) }),
            width: Some(stroke_width(icon, rect.w, rect.h)),
            cap: Some(deckcraft_model::style::LineCap::Round),
            join: Some(deckcraft_model::style::LineJoin::Round),
            ..Default::default()
        }),
        style: if filled { None } else { Some(ShapeStyle::line(SchemeSlot::Tx1)) },
        ..Default::default()
    };
    if filled {
        let source = format!(
            "https://github.com/FortAwesome/Font-Awesome/tree/41cfe644047fc3a4c49c22acc721cacc3e1021fe/svgs/{}/{}.svg",
            icon.style,
            icon.name.rsplit(':').next()?
        );
        shape.tags = vec![
            ("deckcraft.icon.id".into(), icon.name.into()),
            (
                deckcraft_model::icon_credits::CREDIT_TAG.into(),
                format!(
                    "{} ({}): Font Awesome Free 7.1.0 by Fonticons, Inc. — {source} — CC-BY-4.0 https://creativecommons.org/licenses/by/4.0/ — converted to editable paths; winding normalized where necessary.",
                    icon.label, icon.style
                ),
            ),
        ];
    }
    Some(shape)
}

fn bezpath(d: &str) -> Option<BezPath> {
    let mut out = BezPath::new();
    for part in d.split(" | ") {
        if part.trim().is_empty() {
            continue;
        }
        let bp = BezPath::from_svg(part).ok()?;
        for el in bp.elements() {
            out.push(*el);
        }
    }
    if out.elements().is_empty() || !out.is_finite() { None } else { Some(out) }
}

fn deck_path(bp: &BezPath) -> Option<String> {
    let mut out = String::new();
    for el in bp.elements() {
        let command = match *el {
            PathEl::MoveTo(p) => format!("M {} {}", num(p.x), num(p.y)),
            PathEl::LineTo(p) => format!("L {} {}", num(p.x), num(p.y)),
            PathEl::QuadTo(a, b) => format!("Q {} {} {} {}", num(a.x), num(a.y), num(b.x), num(b.y)),
            PathEl::CurveTo(a, b, c) => format!("C {} {} {} {} {} {}", num(a.x), num(a.y), num(b.x), num(b.y), num(c.x), num(c.y)),
            PathEl::ClosePath => "Z".into(),
        };
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(&command);
    }
    (!out.is_empty()).then_some(out)
}
fn num(v: f64) -> String {
    let s = format!("{v:.4}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" { "0".into() } else { s.into() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use deckcraft_geom::preset::FillMode;
    use deckcraft_model::{Geom, ShapeId};
    use serde_json::json;

    use crate::Session;

    #[test]
    fn every_icon_converts_and_a_second_subpath_keeps_its_own_origin() {
        assert!(icons().len() >= 60);
        for icon in icons() {
            let bp = outline(icon.name).unwrap_or_else(|| panic!("{} did not parse", icon.name));
            assert!(bp.elements().len() >= 2, "{}", icon.name);
            let d = path_data(icon.name).unwrap_or_else(|| panic!("{}", icon.name));
            assert!(d.starts_with('M'), "{d}");
            let again = deckcraft_render::parse_path(&d);
            assert!(!again.elements().is_empty(), "{}", icon.name);
        }
        // `m19 12…` is a second SVG element, not a continuation of the shaft.
        let down = path_data("arrow-down").unwrap();
        assert!(down.contains("M 19 12"), "{down}");
        // Relative arcs in the star become cubics our renderer understands.
        assert!(path_data("star").unwrap().contains(" C "), "{}", path_data("star").unwrap());
        assert!(find("arrow_right").is_some_and(|i| i.name == "arrow-right"));
        assert!(find("Star").is_some_and(|i| i.name == "star"));
        assert!(find("nope").is_none());
    }

    #[test]
    fn insert_icon_strokes_the_outline_and_round_trips_through_pptx() {
        let mut s = Session::with_new();
        assert!(s.execute("insert.icon", &json!({})).is_err());
        assert!(s.execute("insert.icon", &json!({"name": "not-an-icon"})).is_err());
        let listed = s.execute("insert.icons", &json!({})).unwrap();
        assert!(listed.as_array().unwrap().iter().any(|i| i["name"] == "star" && i["category"] == "Marks"));

        let before = s.doc().unwrap().shapes().len();
        let id = s.execute("insert.icon", &json!({"name": "star", "rect": [100.0, 80.0, 72.0, 72.0]})).unwrap()["id"].as_u64().unwrap();
        {
            let st = s.doc().unwrap();
            assert_eq!(st.shapes().len(), before + 1);
            let sh = st.shape(ShapeId(id as u32)).unwrap();
            assert_eq!(sh.name, "Star");
            assert!(sh.fill.as_ref().is_some_and(|f| f.is_none()));
            let Geom::Custom { paths } = &sh.geom else { panic!("{:?}", sh.geom) };
            assert_eq!(paths.len(), 1);
            assert_eq!(paths[0].fill, FillMode::None);
            assert!(paths[0].stroke);
            assert!((paths[0].w - VIEW).abs() < 1e-9 && (paths[0].h - VIEW).abs() < 1e-9);
            assert!(paths[0].d.contains('M') && paths[0].d.contains('C'));
            let width = sh.line.as_ref().and_then(|l| l.width).unwrap();
            assert!((width - 6.0).abs() < 1e-6, "{width}");
            let img = deckcraft_render::render_slide(&st.doc, st.selection.slide, &deckcraft_render::RenderOpts { scale: 2.0, ..Default::default() });
            let ink = ink_in(&img, 100.0, 80.0, 72.0, 72.0, 2.0);
            assert!(ink > 40, "star stroke pixels {ink}");
            let mid = img.pixel((100.0 + 36.0) as u32 * 2, (80.0 + 36.0) as u32 * 2);
            let sum = mid[0] as u16 + mid[1] as u16 + mid[2] as u16;
            assert!(sum > 680, "a stroked star is hollow: {mid:?}");
        }
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(s.doc().unwrap().shapes().len(), before);

        s.execute("insert.icon", &json!({"name": "arrow-right", "rect": [200.0, 200.0, 96.0, 96.0]})).unwrap();
        {
            let st = s.doc().unwrap();
            let sh = st.shapes().iter().find(|s| s.name == "Arrow right").unwrap();
            let width = sh.line.as_ref().and_then(|l| l.width).unwrap();
            assert!((width - 8.0).abs() < 1e-6, "{width}");
            let img = deckcraft_render::render_slide(&st.doc, st.selection.slide, &deckcraft_render::RenderOpts { scale: 2.0, ..Default::default() });
            assert!(ink_in(&img, 200.0, 200.0, 96.0, 96.0, 2.0) > 40);
        }

        let bytes = deckcraft_pptx::export(&s.doc().unwrap().doc).unwrap();
        let p = deckcraft_pptx::import(&bytes).unwrap();
        let sh = p.slides[0].shapes.iter().find(|s| s.name == "Arrow right").unwrap();
        let Geom::Custom { paths } = &sh.geom else { panic!("{:?}", sh.geom) };
        assert!(paths.iter().any(|path| path.fill == FillMode::None && path.stroke && path.d.contains('M')));
        let width = sh.line.as_ref().and_then(|l| l.width).unwrap();
        assert!((width - 8.0).abs() < 0.05, "{width}");
    }

    fn ink_in(img: &deckcraft_render::Image, x: f64, y: f64, w: f64, h: f64, scale: f64) -> usize {
        let x0 = (x * scale).floor().max(0.0) as u32;
        let y0 = (y * scale).floor().max(0.0) as u32;
        let x1 = ((x + w) * scale).ceil().min(img.width as f64) as u32;
        let y1 = ((y + h) * scale).ceil().min(img.height as f64) as u32;
        let mut n = 0;
        for yy in y0..y1 {
            for xx in x0..x1 {
                let p = img.pixel(xx, yy);
                let sum = p[0] as u16 + p[1] as u16 + p[2] as u16;
                if p[3] > 200 && sum < 680 {
                    n += 1;
                }
            }
        }
        n
    }
}

#[cfg(test)]
#[path = "icons_tests.rs"]
mod catalogue_tests;
