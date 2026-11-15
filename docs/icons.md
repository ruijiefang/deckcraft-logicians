# Insert Icons

> **Last reviewed:** 2026-11-15 · **Last updated:** 2026-11-15 · **Change:** minor (offline Font Awesome Free catalogue)

Insert → Icons includes 70 Lucide 1.56.0 stroke icons and 1,576 Font Awesome Free 7.1.0
Solid/Regular icons. Brands and Pro artwork are excluded. The gallery searches labels, IDs,
aliases, keywords and categories, filters collection/style/category, and shows 96 icons per page.
Cells have accessible labels and support keyboard focus and Enter/Space activation.

All artwork is bundled. Opening the gallery and inserting icons require no network, fonts,
JavaScript or API credentials. Ribbon and menu drawings are independent of this catalogue.

## Commands and geometry

`insert.icons` returns the catalogue; optional `query`, `collection`, `style`, `category` filters
also work through CLI, control and MCP. `insert.icon` inserts one editable custom shape in one
undo step. Existing names (`star`, `arrow_right`, `Arrow right`) retain their Lucide meaning.
Font Awesome IDs include their style: `fontawesome:solid:house`, `fontawesome:regular:heart`.

Font Awesome silhouettes have a theme Text 1 fill, no outline, and preserve source proportions
inside the requested `rect`. Existing Lucide line width and insertion behavior are retained.
Use the existing Shape Format commands to recolor and resize inserted shapes.

SVG paths become native M/L/C/Q/Z geometry. The importer compares nonzero and even-odd polygon
coverage at 0.05 source-unit flattening tolerance. Curves are retained where coverage agrees;
21 icons need resolved polygon boundaries to preserve nonzero fill in native/PPTX even-odd
geometry. Compound paths are retained so counters and holes remain transparent. This is a
catalogue converter, not a general SVG file importer.

Paths are parsed lazily once per icon, bounded by the compiled catalogue. The gallery holds at
most 192 monochrome preview textures (56 × 56 pixels each, about 2.3 MiB before GPU overhead),
tinted to the current theme. It lays out at most 96 icon cells per page.

## Attribution when sharing

Font Awesome Free icons are by Fonticons, Inc., under CC-BY-4.0. Converted geometry remains
CC-BY-4.0 artwork; the application's MIT OR Apache-2.0 licence does not relicense these paths.
The licence and original notices are in `assets/licenses/` and `NOTICE`.

Inserted shapes carry their canonical ID and readable credit, including source, version,
licence link and conversion disclosure, in native/PPTX shape tags. Alt text remains available
for accessibility. `insert.iconCredits` returns a deduplicated readable credit list; the gallery's
**Used icon credits → Copy icon credits** copies it for sharing.

Exports carry credits in PPTX core description and shape tags, PDF description, PNG UTF-8 iTXt,
JPEG comments, and GIF comment extensions. Credits cover the presentation's tagged artwork,
including groups, masters and layouts. When sharing through services that strip metadata,
include the copied credits in the accompanying caption or document. Raw renderer `Image`
encoders have no document context; callers exporting presentation artwork must use
`deckcraft_render::image_credits` (the application and CLI export paths do this).

## Reproducing the catalogue

Pinned upstream: Font Awesome Free 7.1.0, commit
`41cfe644047fc3a4c49c22acc721cacc3e1021fe` in
[FortAwesome/Font-Awesome](https://github.com/FortAwesome/Font-Awesome/tree/41cfe644047fc3a4c49c22acc721cacc3e1021fe).

Place these unmodified files from that commit in one directory:

- `metadata/icons.json` as `icons.json` (SHA-256 `1de0b31da59a209788e3353e01a1d91da4374619a283718f173d4e901750ff40`)
- `metadata/categories.yml` as `categories.yml` (SHA-256 `817e18cf50580a3f44823d6a080949f679921b73682ae1ab90ba776af8f7ed58`)
- `LICENSE.txt` (SHA-256 `329f24fdba0c57714d7d202d30a74767feac6907685ee24f0abbf1bd47581a07`)

Run `cargo xtask icons SOURCE crates/engine/src/fontawesome_data.rs`, then `cargo fmt --all`.
The Rust generator verifies SHA-256 inputs before conversion, rejects unexpected
metadata and excessive paths, and produces stable IDs and order. Ordinary builds consume the
committed output and do not run the generator. Updates require deliberately changing the pin,
checksums, notices and catalogue together, followed by geometry, round-trip and visual checks.

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-11-15 | minor | Bundled Font Awesome Free Solid/Regular alongside Lucide; paged gallery, editable silhouettes and export attribution |
