//! Attribution travels with inserted artwork in shape tags (native files and PPTX).
use crate::{Presentation, Shape};
use std::collections::BTreeSet;

pub const CREDIT_TAG: &str = "deckcraft.icon.credit";

/// Readable credits for used artwork, deduplicated across slides, layouts, masters and groups.
/// Bounds also apply to untrusted tags imported from external documents.
pub fn text(doc: &Presentation) -> String {
    fn visit(shapes: &[Shape], credits: &mut BTreeSet<String>, depth: usize) {
        if depth > 64 || credits.len() >= 4096 {
            return;
        }
        for shape in shapes {
            for (_, value) in shape.tags.iter().filter(|(key, _)| key.eq_ignore_ascii_case(CREDIT_TAG)) {
                if value.len() <= 4096 && credits.len() < 4096 {
                    credits.insert(value.clone());
                }
            }
            if !shape.children().is_empty() {
                visit(shape.children(), credits, depth + 1);
            }
        }
    }
    let mut credits = BTreeSet::new();
    for slide in &doc.slides {
        visit(&slide.shapes, &mut credits, 0);
    }
    for master in &doc.masters {
        visit(&master.shapes, &mut credits, 0);
        for layout in &master.layouts {
            visit(&layout.shapes, &mut credits, 0);
        }
    }
    credits.into_iter().collect::<Vec<_>>().join("\n")
}

/// Export description keeps human comments intact and avoids duplicate credits after round trips.
pub fn description(doc: &Presentation) -> String {
    let credits = text(doc);
    if credits.is_empty() || doc.props.comments.contains(&credits) {
        return doc.props.comments.clone();
    }
    format!("{}\n\nIcon credits:\n{}", doc.props.comments, credits).trim().to_string()
}
