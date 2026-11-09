//! PDF export of draft state cards: one section per chosen draft position, cards one per
//! page or packed in a grid, with optional title page, bookmarks, page numbers and a group
//! legend at the end.

use krilla::Document;
use krilla::destination::XyzDestination;
use krilla::geom::{Point, Transform};
use krilla::metadata::Metadata;
use krilla::outline::{Outline, OutlineNode};
use krilla::page::PageSettings;

use super::cards::{self, CARD_H, CARD_W};
use super::draw::legend;
use super::icons::Icons;
use super::model::{self, Snapshot};
use super::pdf_draw::PdfPainter;
use super::scene::{Anchor, Scene, palette, text_width};
use super::{Cancelled, Progress};

/// A4 in points.
const A4: (f32, f32) = (595.276, 841.89);
/// 12 mm.
const MARGIN: f32 = 12.0 * 72.0 / 25.4;
const HEADER_H: f32 = 26.0;
const CELL_GAP: f32 = 10.0;

#[derive(Clone, Debug, PartialEq)]
pub struct PdfOptions {
    /// Draft depths (1-20), one section each, in the order they appear.
    pub sections: Vec<usize>,
    /// Cards per page as (columns, rows); (1, 1) is one card per page.
    pub grid: (usize, usize),
    pub landscape: bool,
    pub page_numbers: bool,
    pub bookmarks: bool,
    pub legend_page: bool,
    /// Title page text; `None` for no title page.
    pub title_page: Option<String>,
}

impl PdfOptions {
    pub fn page_size(&self) -> (f32, f32) {
        if self.landscape { (A4.1, A4.0) } else { A4 }
    }

    fn per_page(&self) -> usize {
        (self.grid.0 * self.grid.1).max(1)
    }
}

/// Default packed grid for an orientation.
pub fn default_grid(landscape: bool) -> (usize, usize) {
    if landscape { (3, 2) } else { (2, 3) }
}

enum PageKind {
    Title,
    /// Section index, the states on this page, and whether it continues the section.
    Cards(usize, Vec<usize>, bool),
    /// Groups (indices into the used-group list) on this legend page.
    Legend(Vec<usize>, bool),
    /// The fearless bans of the series.
    Fearless,
}

/// Number of pages the export will have (for the options panel).
pub fn page_count(snap: &Snapshot, options: &PdfOptions) -> usize {
    plan(snap, options).len()
}

fn legend_width(options: &PdfOptions) -> f32 {
    options.page_size().0 - 2.0 * MARGIN
}

fn content_height(options: &PdfOptions) -> f32 {
    options.page_size().1 - 2.0 * MARGIN - HEADER_H
}

fn plan(snap: &Snapshot, options: &PdfOptions) -> Vec<PageKind> {
    let mut pages = Vec::new();
    if options.title_page.is_some() {
        pages.push(PageKind::Title);
    }
    for (s, &depth) in options.sections.iter().enumerate() {
        let states = snap.at_depth(depth);
        for (k, chunk) in states.chunks(options.per_page()).enumerate() {
            pages.push(PageKind::Cards(s, chunk.to_vec(), k > 0));
        }
    }
    if options.legend_page && snap.fearless.is_active() {
        pages.push(PageKind::Fearless);
    }
    if options.legend_page {
        let groups = legend_groups(snap, options);
        // Fill each page with as many groups as fit.
        let width = legend_width(options);
        let max_h = content_height(options);
        let mut start = 0;
        while start < groups.len() {
            let mut end = start + 1;
            while end < groups.len()
                && legend(snap, &groups[start..=end], width, "").height <= max_h
            {
                end += 1;
            }
            pages.push(PageKind::Legend((start..end).collect(), start > 0));
            start = end;
        }
    }
    pages
}

/// Groups appearing on any exported card.
fn legend_groups<'a>(snap: &'a Snapshot, options: &PdfOptions) -> Vec<&'a crate::groups::Group> {
    let mut nodes = Vec::new();
    for &depth in &options.sections {
        for state in snap.at_depth(depth) {
            nodes.extend(snap.path(state));
        }
    }
    snap.groups_in(nodes)
}

fn section_title(snap: &Snapshot, depth: usize) -> String {
    let count = snap.at_depth(depth).len();
    format!(
        "{} ({} state{})",
        model::depth_title(depth),
        count,
        if count == 1 { "" } else { "s" }
    )
}

/// Builds the PDF. `progress` counts cards drawn.
pub fn render(
    snap: &Snapshot,
    options: &PdfOptions,
    icons: &Icons,
    date: &str,
    progress: &Progress,
) -> Result<Vec<u8>, Cancelled> {
    let pages = plan(snap, options);
    let total_pages = pages.len().max(1);
    let (pw, ph) = options.page_size();
    let groups = legend_groups(snap, options);
    let mut painter = PdfPainter::new(icons);
    let mut doc = Document::new();
    let mut outline = Outline::new();
    let mut legend_bookmarked = false;

    for (index, page_kind) in pages.iter().enumerate() {
        if progress.is_cancelled() {
            return Err(Cancelled);
        }
        let mut page = doc.start_page_with(PageSettings::from_wh(pw, ph).expect("A4 is valid"));
        let mut surface = page.surface();
        let top = MARGIN;
        match page_kind {
            PageKind::Title => {
                let title = options.title_page.as_deref().unwrap_or_default();
                let mut s = Scene::new(pw, ph);
                let mut y = ph * 0.32;
                for line in super::scene::wrap_text(title, 28.0, true, pw - 2.0 * MARGIN) {
                    s.text(pw / 2.0, y, 28.0, palette::TEXT, true, Anchor::Middle, line);
                    y += 36.0;
                }
                y += 10.0;
                s.text(
                    pw / 2.0,
                    y,
                    12.0,
                    palette::MUTED,
                    false,
                    Anchor::Middle,
                    date,
                );
                y += 40.0;
                let scope = if snap.scope_root == 0 {
                    "Whole tree".to_string()
                } else {
                    format!(
                        "Subtree from {}",
                        model::depth_title(snap.nodes[snap.scope_root].depth).to_lowercase()
                    )
                };
                s.text(
                    pw / 2.0,
                    y,
                    12.0,
                    palette::TEXT,
                    false,
                    Anchor::Middle,
                    scope,
                );
                y += 28.0;
                if snap.fearless.is_active() {
                    let f = &snap.fearless;
                    s.text(
                        pw / 2.0,
                        y - 10.0,
                        11.0,
                        palette::MUTED,
                        false,
                        Anchor::Middle,
                        format!(
                            "Fearless: game {}, {} fearless, {} champions locked",
                            f.game,
                            f.mode.label().to_lowercase(),
                            f.count()
                        ),
                    );
                    y += 18.0;
                }
                for &depth in &options.sections {
                    s.text(
                        pw / 2.0,
                        y,
                        11.0,
                        palette::TEXT,
                        false,
                        Anchor::Middle,
                        section_title(snap, depth),
                    );
                    y += 17.0;
                }
                painter.draw(&mut surface, &s, Transform::identity());
                if options.bookmarks {
                    outline.push_child(OutlineNode::new(
                        "Title".into(),
                        XyzDestination::new(index, Point::from_xy(0.0, 0.0)),
                    ));
                }
            }
            PageKind::Cards(section, states, continued) => {
                let depth = options.sections[*section];
                let mut title = section_title(snap, depth);
                if *continued {
                    title.push_str(" (continued)");
                }
                painter.text(
                    &mut surface,
                    MARGIN,
                    top + 14.0,
                    13.0,
                    palette::TEXT,
                    true,
                    &title,
                );
                let (cols, rows) = options.grid;
                let area_w = pw - 2.0 * MARGIN;
                let area_h = content_height(options);
                let cell_w = area_w / cols as f32;
                let cell_h = area_h / rows as f32;
                let scale = ((cell_w - CELL_GAP) / CARD_W).min((cell_h - CELL_GAP) / CARD_H);
                let all = snap.at_depth(depth);
                for (k, &state) in states.iter().enumerate() {
                    let seq = all.iter().position(|&s| s == state).unwrap_or(0) + 1;
                    let card = cards::card(snap, state, seq, all.len());
                    let (c, r) = (k % cols, k / cols);
                    let x = MARGIN + c as f32 * cell_w + (cell_w - CARD_W * scale) / 2.0;
                    let y = top + HEADER_H + r as f32 * cell_h + (cell_h - CARD_H * scale) / 2.0;
                    // A thin frame so packed cards read as separate drafts.
                    let mut frame = Scene::new(CARD_W, CARD_H);
                    frame.outline(0.0, 0.0, CARD_W, CARD_H, 8.0, palette::BORDER, 1.0, false);
                    let ts = Transform::from_row(scale, 0.0, 0.0, scale, x, y);
                    painter.draw(&mut surface, &card, ts);
                    painter.draw(&mut surface, &frame, ts);
                    progress.step();
                }
                if options.bookmarks && !continued {
                    outline.push_child(OutlineNode::new(
                        section_title(snap, depth),
                        XyzDestination::new(index, Point::from_xy(0.0, 0.0)),
                    ));
                }
            }
            PageKind::Fearless => {
                painter.text(
                    &mut surface,
                    MARGIN,
                    top + 14.0,
                    13.0,
                    palette::TEXT,
                    true,
                    "Fearless bans",
                );
                if let Some(scene) = super::draw::fearless_legend(snap, legend_width(options)) {
                    painter.draw(
                        &mut surface,
                        &scene,
                        Transform::from_translate(MARGIN, top + HEADER_H),
                    );
                }
                if options.bookmarks {
                    outline.push_child(OutlineNode::new(
                        "Fearless bans".into(),
                        XyzDestination::new(index, Point::from_xy(0.0, 0.0)),
                    ));
                }
            }
            PageKind::Legend(indices, continued) => {
                let title = if *continued {
                    "Groups (continued)"
                } else {
                    "Groups"
                };
                painter.text(
                    &mut surface,
                    MARGIN,
                    top + 14.0,
                    13.0,
                    palette::TEXT,
                    true,
                    title,
                );
                let chosen: Vec<_> = indices.iter().map(|&i| groups[i]).collect();
                let scene = legend(snap, &chosen, legend_width(options), "");
                painter.draw(
                    &mut surface,
                    &scene,
                    Transform::from_translate(MARGIN, top + HEADER_H),
                );
                if options.bookmarks && !legend_bookmarked {
                    legend_bookmarked = true;
                    outline.push_child(OutlineNode::new(
                        "Groups".into(),
                        XyzDestination::new(index, Point::from_xy(0.0, 0.0)),
                    ));
                }
            }
        }
        if options.page_numbers {
            let text = format!("{} / {}", index + 1, total_pages);
            let w = text_width(&text, 9.0, false);
            painter.text(
                &mut surface,
                (pw - w) / 2.0,
                ph - MARGIN / 2.0 + 3.0,
                9.0,
                palette::MUTED,
                false,
                &text,
            );
        }
        surface.finish();
        page.finish();
    }

    if pages.is_empty() {
        // An empty document is not valid PDF; write one page that says so.
        let mut page = doc.start_page_with(PageSettings::from_wh(pw, ph).expect("A4 is valid"));
        let mut surface = page.surface();
        painter.text(
            &mut surface,
            MARGIN,
            MARGIN + 14.0,
            13.0,
            palette::MUTED,
            false,
            "No draft states at the chosen positions.",
        );
        surface.finish();
        page.finish();
    }

    if options.bookmarks {
        doc.set_outline(outline);
    }
    let title = options
        .title_page
        .clone()
        .unwrap_or_else(|| "Draft states".to_string());
    doc.set_metadata(Metadata::new().title(title).creator("Recvrve".to_string()));
    Ok(doc.finish().expect("PDF serializes"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::export::icons;
    use crate::tree::DraftTree;
    use std::collections::HashMap;

    fn sample() -> Snapshot {
        let mut t = DraftTree::new();
        let root = t.root();
        // Seven first ban phases that share only the root.
        for k in 0..7u8 {
            let mut at = root;
            for v in [16 + k, 1, 30 + k, 50 + k, 3, 1] {
                at = t.add_child(at, v).unwrap();
            }
        }
        Snapshot::build(&t, None, &HashMap::new(), &[], false, true)
    }

    fn options(grid: (usize, usize)) -> PdfOptions {
        PdfOptions {
            sections: vec![1, 6],
            grid,
            landscape: false,
            page_numbers: true,
            bookmarks: true,
            legend_page: true,
            title_page: Some("Prep".into()),
        }
    }

    #[test]
    fn plans_sections_and_pages() {
        let snap = sample();
        assert_eq!(snap.at_depth(1).len(), 7);
        // Title, 7 + 7 single pages, legend.
        assert_eq!(page_count(&snap, &options((1, 1))), 1 + 14 + 1);
        // 2x3 grid: 2 pages per section.
        assert_eq!(page_count(&snap, &options((2, 3))), 1 + 4 + 1);
    }

    #[test]
    fn writes_a_pdf_with_each_icon_once() {
        let snap = sample();
        let progress = Progress::default();
        let bytes = render(
            &snap,
            &options((2, 3)),
            icons::for_tests(),
            "today",
            &progress,
        )
        .unwrap();
        assert!(bytes.starts_with(b"%PDF-"));
        assert_eq!(progress.done(), 14);
        // Rendering the same states one per page adds pages but no image data.
        let single = render(
            &snap,
            &options((1, 1)),
            icons::for_tests(),
            "today",
            &Progress::default(),
        )
        .unwrap();
        assert!(
            (single.len() as f64) < bytes.len() as f64 * 1.5,
            "{} vs {}",
            single.len(),
            bytes.len()
        );
    }
}
