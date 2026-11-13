//! Overview grid: layout math for the slide-thumbnail grid.

use egui::{pos2, Rect, vec2};

/// Columns in the overview grid on a normal (wide) viewport.
const GRID_COLUMNS: usize = 4;

/// Columns in the overview grid on a narrow viewport.
const GRID_COLUMNS_NARROW: usize = 3;

/// Viewport aspect (width / height) below which the narrow layout is used.
const NARROW_ASPECT: f32 = 1.0;

/// Gap between grid cells, as a fraction of the cell width.
const GAP_FRACTION: f32 = 0.05;

/// Computes the thumbnail rectangle for each slide in the overview grid.
///
/// Layout: 4 columns (3 on a narrow viewport), row-major, with a small
/// uniform gap between and around cells, centered in `viewport`. Cells keep
/// the given slide `aspect` (width / height); the cell size is the largest
/// that fits both the column count and the resulting row count inside
/// `viewport`. Fewer slides than columns simply use fewer cells in the first
/// row (still left-aligned within the centered grid block).
pub fn grid_geometry(viewport: egui::Rect, slide_count: usize, aspect: f32) -> Vec<egui::Rect> {
    if slide_count == 0 {
        return Vec::new();
    }
    let aspect = aspect.max(f32::EPSILON);

    let size = viewport.size();
    let columns = if size.x / size.y < NARROW_ASPECT {
        GRID_COLUMNS_NARROW
    } else {
        GRID_COLUMNS
    }
    .min(slide_count);

    let rows = slide_count.div_ceil(columns);

    // Solve for the cell width: with gap = GAP_FRACTION * cell,
    //   columns * cell + (columns + 1) * gap <= size.x
    //   rows * cell / aspect + (rows + 1) * gap <= size.y
    let w_by_width = size.x / (columns as f32 * (1.0 + GAP_FRACTION) + GAP_FRACTION);
    let w_by_height = size.y * aspect / (rows as f32 * (1.0 + GAP_FRACTION) + GAP_FRACTION);
    let cell_w = w_by_width.min(w_by_height).max(0.0);
    let cell_h = cell_w / aspect;
    let gap = GAP_FRACTION * cell_w;

    // Center the grid block in the viewport.
    let grid_w = columns as f32 * cell_w + (columns as f32 - 1.0) * gap;
    let grid_h = rows as f32 * cell_h + (rows as f32 - 1.0) * gap;
    let x0 = viewport.left() + (size.x - grid_w) / 2.0;
    let y0 = viewport.top() + (size.y - grid_h) / 2.0;

    (0..slide_count)
        .map(|slide| {
            let col = slide % columns;
            let row = slide / columns;
            Rect::from_min_size(
                pos2(x0 + col as f32 * (cell_w + gap), y0 + row as f32 * (cell_h + gap)),
                vec2(cell_w, cell_h),
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::grid_geometry;

    #[test]
    fn grid_geometry_lays_out_row_major() {
        let rects = grid_geometry(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0)), 8, 16.0/9.0);
        assert_eq!(rects.len(), 8);
        assert!(rects[1].left() > rects[0].left());   // second in row
        assert!(rects[4].top() > rects[0].top());     // second row
        // all rects inside viewport
        assert!(rects.iter().all(|r| egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0)).contains_rect(*r)));
    }
}
