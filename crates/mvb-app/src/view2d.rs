//! Top-down view of one horizontal layer.
//!
//! The layer is uploaded as a texture with one pixel per block and drawn as a single quad, so the
//! cost of a frame does not depend on the number of blocks. Rotation only changes the texture
//! coordinates of that quad.

use eframe::egui;
use eframe::egui::{
    Align2, Color32, ColorImage, FontId, Key, Mesh, Modifiers, Painter, Pos2, Rect, Sense, Stroke,
    StrokeKind, TextureHandle, TextureOptions, Vec2, pos2, vec2,
};

use crate::document::Document;
use crate::i18n::Texts;

const MIN_ZOOM: f32 = 0.25;
const MAX_ZOOM: f32 = 96.0;
/// Below this many points per block, grid lines would hide the blocks.
const GRID_MIN_ZOOM: f32 = 6.0;
/// Opacity of the layer below, drawn where the current layer is empty.
const BELOW_OPACITY: f32 = 0.25;
/// Minimum distance between two coordinate labels, in points.
const LABEL_SPACING: f32 = 28.0;
/// Width of the left ruler and height of the top ruler.
const RULER: Vec2 = vec2(36.0, 20.0);
/// Zoom factor of the zoom buttons.
pub const ZOOM_STEP: f32 = 1.25;

/// The block under the pointer, in coordinates relative to the build.
#[derive(Clone, Copy, Debug)]
pub struct Hovered {
    pub x: u32,
    pub y: u32,
    pub z: u32,
    pub block: u16,
}

pub struct View2d {
    /// Displayed layer, counted from the bottom of the build.
    pub layer: u32,
    /// Quarter turns clockwise.
    rotation: u8,
    /// Screen points per block.
    zoom: f32,
    /// Offset of the layer's centre from the canvas centre.
    pan: Vec2,
    fit_pending: bool,
    pub show_below: bool,
    pub show_grid: bool,
    cache: Option<LayerCache>,
}

#[derive(PartialEq)]
struct CacheKey {
    layer: u32,
    show_below: bool,
    region_visible: Vec<bool>,
}

struct LayerCache {
    key: CacheKey,
    cells: Vec<u16>,
    texture: TextureHandle,
}

impl Default for View2d {
    fn default() -> Self {
        Self {
            layer: 0,
            rotation: 0,
            zoom: 16.0,
            pan: Vec2::ZERO,
            fit_pending: true,
            show_below: true,
            show_grid: true,
            cache: None,
        }
    }
}

impl View2d {
    /// Back to the initial rotation, with the whole layer fitted in the canvas.
    pub fn reset(&mut self) {
        self.rotation = 0;
        self.fit_pending = true;
    }

    pub fn rotate(&mut self, quarter_turns: i8) {
        self.rotation = (i16::from(self.rotation) + i16::from(quarter_turns)).rem_euclid(4) as u8;
    }

    /// Zooms around the canvas centre.
    pub fn zoom_by(&mut self, factor: f32) {
        self.zoom_at(Vec2::ZERO, factor);
    }

    /// Zooms while keeping the point at `anchor` (relative to the canvas centre) still.
    fn zoom_at(&mut self, anchor: Vec2, factor: f32) {
        let zoom = (self.zoom * factor).clamp(MIN_ZOOM, MAX_ZOOM);
        let applied = zoom / self.zoom;
        self.pan = anchor - (anchor - self.pan) * applied;
        self.zoom = zoom;
    }

    /// Draws the canvas and handles its input. Returns the block under the pointer, if any.
    pub fn ui(&mut self, ui: &mut egui::Ui, doc: &Document, texts: &Texts) -> Option<Hovered> {
        let (response, painter) = ui.allocate_painter(ui.available_size(), Sense::click_and_drag());
        let canvas = response.rect;
        // Coordinate rulers along the top and left edges, the layer in the rest.
        let map = Rect::from_min_max(canvas.min + RULER, canvas.max);
        let map_painter = painter.with_clip_rect(map);
        let [width, height, depth] = doc.size;
        if width == 0 || height == 0 || depth == 0 {
            return None;
        }
        let max_side = ui.input(|i| i.max_texture_side);
        if width as usize > max_side || depth as usize > max_side {
            map_painter.text(
                map.center(),
                Align2::CENTER_CENTER,
                texts.error_too_large,
                FontId::proportional(16.0),
                ui.visuals().error_fg_color,
            );
            return None;
        }

        if !ui.ctx().egui_wants_keyboard_input() {
            ui.input_mut(|i| {
                if i.consume_key(Modifiers::NONE, Key::ArrowUp)
                    || i.consume_key(Modifiers::NONE, Key::PageUp)
                {
                    self.layer += 1;
                }
                if i.consume_key(Modifiers::NONE, Key::ArrowDown)
                    || i.consume_key(Modifiers::NONE, Key::PageDown)
                {
                    self.layer = self.layer.saturating_sub(1);
                }
            });
        }
        self.layer = self.layer.min(height - 1);

        let (rot_w, rot_d) = rotated_dims(width, depth, self.rotation);
        if self.fit_pending {
            let fit = (map.width() / rot_w as f32).min(map.height() / rot_d as f32) * 0.9;
            self.zoom = fit.clamp(MIN_ZOOM, MAX_ZOOM);
            self.pan = Vec2::ZERO;
            self.fit_pending = false;
        }
        if response.dragged() {
            self.pan += response.drag_delta();
        }
        if let Some(pointer) = response.hover_pos() {
            let (scroll, pinch) = ui.input(|i| (i.smooth_scroll_delta.y, i.zoom_delta()));
            let factor = pinch * (scroll / 400.0).exp();
            if factor != 1.0 {
                self.zoom_at(pointer - map.center(), factor);
            }
        }

        let rect = Rect::from_center_size(
            map.center() + self.pan,
            vec2(rot_w as f32, rot_d as f32) * self.zoom,
        );
        map_painter.rect_filled(rect, 0.0, ui.visuals().faint_bg_color);
        let texture = self.update_cache(ui.ctx(), doc).texture.id();
        let mut mesh = Mesh::with_texture(texture);
        for (corner, screen) in [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)]
            .into_iter()
            .zip([
                rect.left_top(),
                rect.right_top(),
                rect.right_bottom(),
                rect.left_bottom(),
            ])
        {
            mesh.vertices.push(egui::epaint::Vertex {
                pos: screen,
                uv: texture_uv(corner, self.rotation),
                color: Color32::WHITE,
            });
        }
        mesh.add_triangle(0, 1, 2);
        mesh.add_triangle(0, 2, 3);
        map_painter.add(mesh);

        let stroke_color = ui.visuals().text_color();
        if self.show_grid && self.zoom >= GRID_MIN_ZOOM {
            self.draw_grid(&map_painter, map, rect, [rot_w, rot_d], stroke_color);
        }
        map_painter.rect_stroke(
            rect,
            0.0,
            Stroke::new(1.0, stroke_color.gamma_multiply(0.6)),
            StrokeKind::Outside,
        );
        self.draw_rulers(&painter, canvas, map, rect, [width, depth], ui.visuals());
        self.draw_compass(&map_painter, map, stroke_color);

        let pointer = response
            .hover_pos()
            .filter(|p| map.contains(*p) && rect.contains(*p))?;
        let u = (((pointer.x - rect.min.x) / self.zoom) as u32).min(rot_w - 1);
        let v = (((pointer.y - rect.min.y) / self.zoom) as u32).min(rot_d - 1);
        let (x, z) = from_rotated(u, v, width, depth, self.rotation);
        let block = self.cache.as_ref()?.cells[(z * width + x) as usize];
        doc.color(block)?;
        let hovered = Hovered {
            x,
            y: self.layer,
            z,
            block,
        };

        let cell = Rect::from_min_size(
            rect.min + vec2(u as f32, v as f32) * self.zoom,
            Vec2::splat(self.zoom),
        );
        map_painter.rect_stroke(
            cell,
            0.0,
            Stroke::new(2.0, ui.visuals().selection.stroke.color),
            StrokeKind::Outside,
        );
        if !response.dragged() {
            block_tooltip(ui.ctx(), pointer, doc, hovered);
        }
        Some(hovered)
    }

    /// Rebuilds the layer texture when the layer, the regions shown or the options changed.
    fn update_cache(&mut self, ctx: &egui::Context, doc: &Document) -> &LayerCache {
        let key = CacheKey {
            layer: self.layer,
            show_below: self.show_below,
            region_visible: doc.region_visible.clone(),
        };
        if self.cache.as_ref().is_some_and(|cache| cache.key == key) {
            return self.cache.as_ref().expect("checked above");
        }

        let cells = doc.layer_cells(self.layer);
        let below = (self.show_below && self.layer > 0).then(|| doc.layer_cells(self.layer - 1));
        let pixels = cells
            .iter()
            .enumerate()
            .map(|(i, &block)| {
                doc.color(block)
                    .or_else(|| {
                        let below = below.as_ref()?[i];
                        Some(doc.color(below)?.gamma_multiply(BELOW_OPACITY))
                    })
                    .unwrap_or(Color32::TRANSPARENT)
            })
            .collect();
        let image = ColorImage::new([doc.size[0] as usize, doc.size[2] as usize], pixels);

        let texture = match self.cache.take() {
            Some(mut cache) => {
                cache.texture.set(image, TextureOptions::NEAREST);
                cache.texture
            }
            None => ctx.load_texture("layer", image, TextureOptions::NEAREST),
        };
        self.cache.insert(LayerCache {
            key,
            cells,
            texture,
        })
    }

    fn draw_grid(
        &self,
        painter: &Painter,
        map: Rect,
        rect: Rect,
        [rot_w, rot_d]: [u32; 2],
        color: Color32,
    ) {
        let thin = Stroke::new(1.0, color.gamma_multiply(0.12));
        let thick = Stroke::new(1.0, color.gamma_multiply(0.35));
        let (u_axis, v_axis) = axes(self.rotation);
        // A thicker line every 10 blocks, counted from the build's 0 coordinate.
        let stroke = |boundary: u32, len: u32, increasing: bool| {
            let coordinate = if increasing { boundary } else { len - boundary };
            if coordinate.is_multiple_of(10) {
                thick
            } else {
                thin
            }
        };
        for u in visible_range(rect.min.x, map.min.x, map.max.x, self.zoom, rot_w) {
            let x = rect.min.x + u as f32 * self.zoom;
            painter.line_segment(
                [pos2(x, rect.min.y), pos2(x, rect.max.y)],
                stroke(u, rot_w, u_axis.1),
            );
        }
        for v in visible_range(rect.min.y, map.min.y, map.max.y, self.zoom, rot_d) {
            let y = rect.min.y + v as f32 * self.zoom;
            painter.line_segment(
                [pos2(rect.min.x, y), pos2(rect.max.x, y)],
                stroke(v, rot_d, v_axis.1),
            );
        }
    }

    /// Coordinates of the columns (top ruler) and rows (left ruler) shown in the map.
    fn draw_rulers(
        &self,
        painter: &Painter,
        canvas: Rect,
        map: Rect,
        rect: Rect,
        [width, depth]: [u32; 2],
        visuals: &egui::Visuals,
    ) {
        let color = visuals.text_color();
        let top = Rect::from_min_max(pos2(map.min.x, canvas.min.y), pos2(map.max.x, map.min.y));
        let left = Rect::from_min_max(pos2(canvas.min.x, map.min.y), pos2(map.min.x, map.max.y));
        let corner = Rect::from_min_max(canvas.min, map.min);
        for band in [top, left, corner] {
            painter.rect_filled(band, 0.0, visuals.faint_bg_color);
        }

        let step = [1, 2, 5, 10, 20, 50, 100, 200, 500, 1000, 2000, 5000]
            .into_iter()
            .find(|&step| step as f32 * self.zoom >= LABEL_SPACING)
            .unwrap_or(10_000);
        let font = FontId::monospace(11.0);
        let (u_axis, v_axis) = axes(self.rotation);
        let (rot_w, rot_d) = rotated_dims(width, depth, self.rotation);
        let coordinate = |index: u32, len: u32, increasing: bool| {
            if increasing { index } else { len - 1 - index }
        };

        let top_painter = painter.with_clip_rect(top);
        // Boundary ranges include the far edge; labels are for cells, so it is skipped.
        let columns = visible_range(rect.min.x, map.min.x, map.max.x, self.zoom, rot_w);
        for u in columns.filter(|&u| u < rot_w) {
            let value = coordinate(u, rot_w, u_axis.1);
            if value.is_multiple_of(step) {
                let x = rect.min.x + (u as f32 + 0.5) * self.zoom;
                let pos = pos2(x, top.center().y);
                top_painter.text(
                    pos,
                    Align2::CENTER_CENTER,
                    value.to_string(),
                    font.clone(),
                    color,
                );
            }
        }
        let left_painter = painter.with_clip_rect(left);
        let rows = visible_range(rect.min.y, map.min.y, map.max.y, self.zoom, rot_d);
        for v in rows.filter(|&v| v < rot_d) {
            let value = coordinate(v, rot_d, v_axis.1);
            if value.is_multiple_of(step) {
                let y = rect.min.y + (v as f32 + 0.5) * self.zoom;
                let pos = pos2(left.max.x - 4.0, y);
                left_painter.text(
                    pos,
                    Align2::RIGHT_CENTER,
                    value.to_string(),
                    font.clone(),
                    color,
                );
            }
        }

        // Split header cell: axis of the columns top right, axis of the rows bottom left.
        let weak = color.gamma_multiply(0.6);
        let diagonal = Stroke::new(1.0, color.gamma_multiply(0.2));
        painter.line_segment([corner.left_top(), corner.right_bottom()], diagonal);
        painter.text(
            corner.right_top() + vec2(-3.0, 1.0),
            Align2::RIGHT_TOP,
            u_axis.0,
            font.clone(),
            weak,
        );
        painter.text(
            corner.left_bottom() + vec2(3.0, -1.0),
            Align2::LEFT_BOTTOM,
            v_axis.0,
            font,
            weak,
        );
    }

    /// Arrow pointing north (towards negative z), so the view can be matched with the game.
    fn draw_compass(&self, painter: &Painter, map: Rect, color: Color32) {
        let centre = map.right_top() + vec2(-44.0, 44.0);
        let north = match self.rotation {
            0 => vec2(0.0, -1.0),
            1 => vec2(1.0, 0.0),
            2 => vec2(0.0, 1.0),
            _ => vec2(-1.0, 0.0),
        };
        painter.circle_stroke(centre, 20.0, Stroke::new(1.0, color.gamma_multiply(0.5)));
        painter.arrow(centre - north * 12.0, north * 24.0, Stroke::new(2.0, color));
        painter.text(
            centre + north * 30.0,
            Align2::CENTER_CENTER,
            "N",
            FontId::proportional(13.0),
            color,
        );
    }
}

/// Floating box describing the block under the pointer, shown without delay.
pub fn block_tooltip(ctx: &egui::Context, pointer: Pos2, doc: &Document, hovered: Hovered) {
    let Some(state) = doc.build.palette.get(hovered.block) else {
        return;
    };
    egui::Area::new(egui::Id::new("block_tooltip"))
        .order(egui::Order::Tooltip)
        .fixed_pos(pointer + vec2(16.0, 16.0))
        .interactable(false)
        .show(ctx, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
                ui.strong(state.display_name());
                ui.label(egui::RichText::new(state.to_string()).small().weak());
                ui.label(format!(
                    "x {} · y {} · z {}",
                    hovered.x, hovered.y, hovered.z
                ));
            });
        });
}

/// Indices of the cell boundaries (0..=len) that are inside the canvas along one axis.
fn visible_range(
    start: f32,
    canvas_min: f32,
    canvas_max: f32,
    zoom: f32,
    len: u32,
) -> std::ops::RangeInclusive<u32> {
    let first = ((canvas_min - start) / zoom).floor().max(0.0) as u32;
    let last = (((canvas_max - start) / zoom).ceil().max(0.0) as u32).min(len);
    first.min(len)..=last
}

/// Size of the layer on screen, in blocks, after rotation.
fn rotated_dims(width: u32, depth: u32, rotation: u8) -> (u32, u32) {
    if rotation.is_multiple_of(2) {
        (width, depth)
    } else {
        (depth, width)
    }
}

/// Screen cell (u, v) of the block (x, z), u to the right and v downwards.
#[cfg(test)]
fn to_rotated(x: u32, z: u32, width: u32, depth: u32, rotation: u8) -> (u32, u32) {
    match rotation {
        0 => (x, z),
        1 => (depth - 1 - z, x),
        2 => (width - 1 - x, depth - 1 - z),
        _ => (z, width - 1 - x),
    }
}

/// Block (x, z) shown in the screen cell (u, v); inverse of [`to_rotated`].
fn from_rotated(u: u32, v: u32, width: u32, depth: u32, rotation: u8) -> (u32, u32) {
    match rotation {
        0 => (u, v),
        1 => (v, depth - 1 - u),
        2 => (width - 1 - u, depth - 1 - v),
        _ => (width - 1 - v, u),
    }
}

/// Texture coordinate shown at a corner of the quad, `corner` being (0|1, 0|1) in screen space.
fn texture_uv((u, v): (f32, f32), rotation: u8) -> Pos2 {
    let (x, z) = match rotation {
        0 => (u, v),
        1 => (v, 1.0 - u),
        2 => (1.0 - u, 1.0 - v),
        _ => (1.0 - v, u),
    };
    pos2(x, z)
}

/// Axis shown along the screen's horizontal and vertical directions, and whether its
/// coordinate increases to the right / downwards.
fn axes(rotation: u8) -> ((&'static str, bool), (&'static str, bool)) {
    match rotation {
        0 => (("x", true), ("z", true)),
        1 => (("z", false), ("x", true)),
        2 => (("x", false), ("z", false)),
        _ => (("z", true), ("x", false)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotation_round_trips_every_cell() {
        let (width, depth) = (5, 3);
        for rotation in 0..4 {
            let (rot_w, rot_d) = rotated_dims(width, depth, rotation);
            for x in 0..width {
                for z in 0..depth {
                    let (u, v) = to_rotated(x, z, width, depth, rotation);
                    assert!(u < rot_w && v < rot_d, "rotation {rotation}");
                    assert_eq!(from_rotated(u, v, width, depth, rotation), (x, z));
                }
            }
        }
    }

    #[test]
    fn quarter_turn_is_clockwise() {
        // Block in the north-west corner (x 0, z 0) goes to the top-right after one turn.
        assert_eq!(to_rotated(0, 0, 5, 3, 1), (2, 0));
    }

    #[test]
    fn texture_corners_follow_the_cells() {
        // The screen corner of each rotation shows the texture corner of the block found there.
        let (width, depth) = (5, 3);
        for rotation in 0..4 {
            let (rot_w, rot_d) = rotated_dims(width, depth, rotation);
            for (corner, cell) in [((0.0, 0.0), (0, 0)), ((1.0, 1.0), (rot_w - 1, rot_d - 1))] {
                let (x, z) = from_rotated(cell.0, cell.1, width, depth, rotation);
                let uv = texture_uv(corner, rotation);
                assert_eq!(uv.x.round() as u32 * (width - 1), x, "rotation {rotation}");
                assert_eq!(uv.y.round() as u32 * (depth - 1), z, "rotation {rotation}");
            }
        }
    }

    #[test]
    fn axes_match_rotation() {
        for rotation in 0..4 {
            let (width, depth) = (5, 3);
            let (u_axis, v_axis) = axes(rotation);
            let (x0, z0) = from_rotated(0, 0, width, depth, rotation);
            let (x1, z1) = from_rotated(1, 0, width, depth, rotation);
            let (along_u, increasing) = if x0 != x1 {
                ("x", x1 > x0)
            } else {
                ("z", z1 > z0)
            };
            assert_eq!(u_axis, (along_u, increasing), "rotation {rotation}");
            let (x1, z1) = from_rotated(0, 1, width, depth, rotation);
            let (along_v, increasing) = if x0 != x1 {
                ("x", x1 > x0)
            } else {
                ("z", z1 > z0)
            };
            assert_eq!(v_axis, (along_v, increasing), "rotation {rotation}");
        }
    }

    #[test]
    fn zoom_keeps_the_anchor_still() {
        let mut view = View2d {
            pan: vec2(10.0, -5.0),
            zoom: 8.0,
            ..View2d::default()
        };
        let anchor = vec2(100.0, 50.0);
        // The build point under the anchor, in blocks from the layer centre.
        let before = (anchor - view.pan) / view.zoom;
        view.zoom_at(anchor, 2.0);
        assert_eq!(view.zoom, 16.0);
        assert_eq!((anchor - view.pan) / view.zoom, before);
        view.zoom_at(anchor, 1000.0);
        assert_eq!(view.zoom, MAX_ZOOM);
    }

    /// Runs the view for real in a headless egui context: every rotation, zoom level and pointer
    /// position must draw without panicking.
    #[test]
    fn draws_every_rotation_without_panicking() {
        use mvb_core::{BlockState, Build, Pos, Region, Size};

        let mut build = Build::new();
        let stone = build
            .palette
            .get_or_insert(BlockState::new("stone"))
            .unwrap();
        let mut region = Region::new("r", Pos::default(), Size::new(7, 3, 4)).unwrap();
        for i in 0..7 {
            region.set(i, 1, i % 4, stone);
        }
        build.regions.push(region);
        let doc = Document::new(build, "test".into());
        let texts = crate::i18n::Language::English.texts();
        let ctx = egui::Context::default();
        let screen = Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0));

        for rotation in 0..4 {
            for zoom in [MIN_ZOOM, 3.0, 20.0, MAX_ZOOM] {
                for pointer in [pos2(400.0, 300.0), pos2(30.0, 10.0), pos2(790.0, 590.0)] {
                    let mut view = View2d {
                        rotation,
                        zoom,
                        layer: 1,
                        fit_pending: false,
                        ..View2d::default()
                    };
                    for _ in 0..2 {
                        let input = egui::RawInput {
                            screen_rect: Some(screen),
                            events: vec![egui::Event::PointerMoved(pointer)],
                            ..Default::default()
                        };
                        let mut output = ctx.run_ui(input, |ui| {
                            view.ui(ui, &doc, texts);
                        });
                        // No renderer here: texture uploads are dropped on purpose.
                        output.textures_delta.clear();
                    }
                }
            }
        }
    }

    #[test]
    fn visible_range_is_clamped() {
        // Layer starting 20 points left of a 0..100 canvas, 10 points per block, 5 blocks wide.
        assert_eq!(visible_range(-20.0, 0.0, 100.0, 10.0, 5), 2..=5);
        // Layer entirely right of the canvas.
        assert_eq!(visible_range(200.0, 0.0, 100.0, 10.0, 5), 0..=0);
    }
}
