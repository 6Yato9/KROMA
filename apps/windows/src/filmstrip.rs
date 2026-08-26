//! The filmstrip: a column of thumbnails down the left of the window.
//!
//! Down rather than across, because the window is wider than it is tall and a
//! photograph is not. A horizontal strip costs height, which is the dimension
//! the picture is already short of; a vertical one costs width, of which there
//! is more to spare — and it holds more frames at the same cell size.
//!
//! Only the cells actually on screen are laid out, hit-tested or drawn, and
//! only those have their thumbnails asked for. A folder of a thousand
//! photographs is a perfectly ordinary thing to open, and the difference
//! between a strip that handles it and one that does not is entirely in
//! whether it does work per photograph or per *visible* photograph.

use crate::library::{Library, Thumbnails};

/// Size of one cell's picture area.
const CELL: egui::Vec2 = egui::vec2(104.0, 74.0);
const GAP: f32 = 6.0;

/// How many cells past the edge of the view to ask for.
///
/// Enough that a thumbnail is usually already there by the time it scrolls
/// into sight, few enough that opening a large folder does not queue hundreds
/// of decodes for photographs nobody has looked at.
const LOOKAHEAD: usize = 8;

/// Which cells a scroll offset puts on screen.
///
/// Written against a bare offset rather than a direction, so it says the same
/// thing whichever way the strip runs. It is the part that decides how much
/// work the strip does per frame, and it is much easier to be sure of as
/// arithmetic than by scrolling and watching.
fn visible(from: f32, to: f32, stride: f32, count: usize) -> std::ops::Range<usize> {
    if count == 0 || stride <= 0.0 {
        return 0..0;
    }
    let first = (from / stride).floor().max(0.0) as usize;
    let last = ((to / stride).ceil().max(0.0) as usize + 1).min(count);
    first.min(count)..last
}

/// What the user did to the strip.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Action {
    Show(usize),
    /// Take a photograph out of the set. The file is untouched — this is a
    /// list, not a folder, and nothing in this program deletes anything.
    Remove(usize),
}

/// Draw the strip. Returns what the user did, if anything.
///
/// The thumbnails come in beside the library rather than out of it: what the
/// library holds is bytes, and the texture made from them belongs to the
/// shell — see [`crate::library`].
pub fn strip(ui: &mut egui::Ui, library: &mut Library, thumbs: &Thumbnails) -> Option<Action> {
    let mut clicked = None;
    let stride = CELL.y + GAP;
    let count = library.len();
    let current = library.current();

    egui::ScrollArea::vertical()
        .auto_shrink([true, false])
        .show_viewport(ui, |ui, viewport| {
            let (rect, _) = ui.allocate_exact_size(
                egui::vec2(CELL.x, stride * count as f32),
                egui::Sense::hover(),
            );
            let range = visible(viewport.min.y, viewport.max.y, stride, count);
            library.request(range.start..range.end + LOOKAHEAD);

            for i in range {
                let cell = egui::Rect::from_min_size(
                    egui::pos2(rect.min.x, rect.min.y + i as f32 * stride),
                    CELL,
                );
                let response = ui.interact(cell, ui.id().with(i), egui::Sense::click());
                let entry = &library.entries()[i];
                let texture = thumbs.get(&entry.path);
                draw(ui, cell, entry, texture, i == current, response.hovered());
                if response.clicked() {
                    clicked = Some(Action::Show(i));
                }
                let name = entry.name();
                response
                    .context_menu(|ui| {
                        ui.label(egui::RichText::new(&name).small().weak());
                        if ui.button("Remove from set").clicked() {
                            clicked = Some(Action::Remove(i));
                            ui.close();
                        }
                    })
                    .map(|r| r.response.on_hover_text(name));
            }
        });

    clicked
}

fn draw(
    ui: &egui::Ui,
    cell: egui::Rect,
    entry: &crate::library::Entry,
    texture: Option<&egui::TextureHandle>,
    selected: bool,
    hovered: bool,
) {
    let painter = ui.painter_at(cell);
    painter.rect_filled(cell, 3.0, crate::theme::colour::WELL);

    // The texture rather than `entry.thumb`: bytes that have arrived but not
    // yet been uploaded are one frame away from being drawable, and until they
    // are there is nothing to put in the cell.
    match (texture, entry.failed) {
        (Some(texture), _) => {
            // Fit rather than fill: a filmstrip is for recognising a frame,
            // and cropping the frame to a common shape is a strange way to go
            // about that.
            let size = texture.size_vec2();
            let scale = (cell.width() / size.x).min(cell.height() / size.y);
            let target = egui::Rect::from_center_size(cell.center(), size * scale).shrink(2.0);
            painter.add(egui::Shape::image(
                texture.id(),
                target,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            ));
        }
        (None, true) => {
            painter.text(
                cell.center(),
                egui::Align2::CENTER_CENTER,
                "unreadable",
                egui::FontId::proportional(10.0),
                egui::Color32::from_gray(120),
            );
        }
        (None, false) => {
            // Deliberately quiet. A strip of spinners on a folder of a
            // thousand is a light show, not information.
            painter.text(
                cell.center(),
                egui::Align2::CENTER_CENTER,
                "…",
                egui::FontId::proportional(14.0),
                egui::Color32::from_gray(80),
            );
        }
    }

    if selected {
        painter.rect_stroke(
            cell,
            3.0,
            egui::Stroke::new(2.0_f32, egui::Color32::from_gray(235)),
            egui::StrokeKind::Inside,
        );
    } else if hovered {
        painter.rect_stroke(
            cell,
            3.0,
            egui::Stroke::new(1.0_f32, egui::Color32::from_gray(130)),
            egui::StrokeKind::Inside,
        );
    }

    if entry.edited() {
        // A photograph that has been worked on, so a set half way through a
        // pass is readable at a glance.
        let at = egui::pos2(cell.max.x - 7.0, cell.min.y + 7.0);
        painter.circle_filled(at, 4.0, egui::Color32::from_black_alpha(160));
        painter.circle_filled(at, 2.5, egui::Color32::from_rgb(120, 200, 255));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_cells_on_screen_are_visible() {
        // A window 500 wide over cells of 110, scrolled to 1000.
        let r = visible(1000.0, 1500.0, 110.0, 1000);
        assert_eq!(r.start, 9);
        assert!(r.end <= 15, "{r:?} is more than the window can hold");
    }

    /// The property the whole strip rests on: what it costs per frame is set
    /// by the size of the window, not by how many photographs are open.
    #[test]
    fn the_cost_does_not_grow_with_the_number_of_photographs() {
        let small = visible(0.0, 800.0, 110.0, 20);
        let huge = visible(0.0, 800.0, 110.0, 100_000);
        assert_eq!(huge.len(), small.len().min(huge.len()));
        assert!(
            huge.len() < 12,
            "a wide-open folder drew {} cells",
            huge.len()
        );
    }

    #[test]
    fn the_range_stops_at_the_last_photograph() {
        let r = visible(0.0, 5000.0, 110.0, 3);
        assert_eq!(r, 0..3);
    }

    #[test]
    fn an_empty_library_asks_for_nothing() {
        assert_eq!(visible(0.0, 800.0, 110.0, 0), 0..0);
    }

    /// Scrolled past the end — which egui will do momentarily during an
    /// elastic overscroll — must not produce a range that starts after it
    /// ends.
    #[test]
    fn scrolling_past_the_end_is_not_an_inverted_range() {
        let r = visible(9000.0, 9500.0, 110.0, 5);
        assert!(r.start <= r.end, "{r:?}");
        assert!(r.is_empty());
    }
}
