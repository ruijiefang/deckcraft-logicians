//! Export attribution lives in image metadata, independent of pixel content.
use deckcraft_model::Presentation;

pub fn image_credits(bytes: Vec<u8>, format: &str, doc: &Presentation) -> Vec<u8> {
    let text = deckcraft_model::icon_credits::text(doc);
    if text.is_empty() {
        return bytes;
    }
    match format {
        "png" => super::images::stamp_png_credits(&bytes, &text),
        "jpg" | "jpeg" if bytes.starts_with(&[0xff, 0xd8]) => {
            let mut out = vec![0xff, 0xd8];
            // JPEG COM segments carry at most 65,533 bytes. Split on UTF-8 boundaries so each
            // comment is readable independently, including a large deck's attribution list.
            let mut remaining = text.as_str();
            while !remaining.is_empty() {
                let mut end = remaining.len().min(65_533);
                while !remaining.is_char_boundary(end) {
                    end -= 1;
                }
                let (part, rest) = remaining.split_at(end);
                out.extend_from_slice(&[0xff, 0xfe]);
                out.extend_from_slice(&((part.len() + 2) as u16).to_be_bytes());
                out.extend_from_slice(part.as_bytes());
                remaining = rest;
            }
            out.extend_from_slice(bytes.get(2..).unwrap_or_default());
            out
        }
        "gif" if bytes.starts_with(b"GIF") && bytes.last() == Some(&0x3b) => {
            let mut out = bytes.get(..bytes.len().saturating_sub(1)).unwrap_or_default().to_vec();
            out.extend_from_slice(&[0x21, 0xfe]);
            for part in text.as_bytes().chunks(255) {
                out.push(part.len() as u8);
                out.extend_from_slice(part);
            }
            out.extend_from_slice(&[0, 0x3b]);
            out
        }
        _ => bytes,
    }
}
