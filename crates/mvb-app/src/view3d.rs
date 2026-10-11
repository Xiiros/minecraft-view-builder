//! 3D view: free orbit around the build, zoom, and only the layers chosen by the user.

use eframe::egui::{self, Align2, FontId, PointerButton, Sense};
use eframe::egui_wgpu;
use glam::Vec2;

use crate::camera::OrbitCamera;
use crate::document::Document;
use crate::i18n::Texts;
use crate::render3d::{Paint3d, fits_in_gpu_buffer};
use crate::scene::Scene;
use crate::view2d::{Hovered, block_tooltip};

/// Buffer size every wgpu device accepts; bigger meshes are refused before reaching the GPU.
const WGPU_DEFAULT_MAX_BUFFER_SIZE: u64 = 256 << 20;

#[derive(Default)]
pub struct View3d {
    camera: Option<OrbitCamera>,
    scene: Option<SceneCache>,
    generation: u64,
}

struct SceneCache {
    layer_visible: Vec<bool>,
    region_visible: Vec<bool>,
    scene: Scene,
}

impl View3d {
    /// Back to the initial camera, with the whole build in view.
    pub fn reset(&mut self) {
        self.camera = None;
    }

    /// Rebuilds the scene when the shown layers or regions changed.
    fn update_scene(&mut self, doc: &Document) {
        let stale = self.scene.as_ref().is_none_or(|cache| {
            cache.layer_visible != doc.layer_visible || cache.region_visible != doc.region_visible
        });
        if stale {
            self.generation += 1;
            self.scene = Some(SceneCache {
                layer_visible: doc.layer_visible.clone(),
                region_visible: doc.region_visible.clone(),
                scene: Scene::new(doc),
            });
        }
    }

    /// Draws the view and handles its input. Returns the block under the pointer, if any.
    /// `gpu` is false when the 3D renderer could not be created.
    pub fn ui(
        &mut self,
        ui: &mut egui::Ui,
        doc: &Document,
        texts: &Texts,
        gpu: bool,
    ) -> Option<Hovered> {
        let (response, painter) = ui.allocate_painter(ui.available_size(), Sense::click_and_drag());
        let rect = response.rect;
        painter.rect_filled(rect, 0.0, ui.visuals().extreme_bg_color);
        if !gpu {
            let color = ui.visuals().error_fg_color;
            painter.text(
                rect.center(),
                Align2::CENTER_CENTER,
                texts.error_no_gpu,
                FontId::proportional(16.0),
                color,
            );
            return None;
        }
        if doc.size.contains(&0) {
            return None;
        }

        let camera = self
            .camera
            .get_or_insert_with(|| OrbitCamera::fit(doc.size));
        let delta = response.drag_delta();
        if response.dragged_by(PointerButton::Primary) {
            camera.orbit(Vec2::new(delta.x, delta.y));
        } else if response.dragged_by(PointerButton::Secondary)
            || response.dragged_by(PointerButton::Middle)
        {
            camera.pan(Vec2::new(delta.x, delta.y), rect.height());
        }
        if response.hovered() {
            let (scroll, pinch) = ui.input(|i| (i.smooth_scroll_delta.y, i.zoom_delta()));
            let factor = (-scroll / 300.0).exp() / pinch;
            if factor != 1.0 {
                camera.zoom(factor);
            }
        }
        let camera = *camera;

        let aspect = rect.width() / rect.height().max(1.0);
        self.update_scene(doc);
        let scene = &self.scene.as_ref().expect("updated above").scene;
        if !fits_in_gpu_buffer(&scene.mesh, WGPU_DEFAULT_MAX_BUFFER_SIZE) {
            let color = ui.visuals().error_fg_color;
            painter.text(
                rect.center(),
                Align2::CENTER_CENTER,
                texts.error_too_large,
                FontId::proportional(16.0),
                color,
            );
            return None;
        }
        painter.add(egui_wgpu::Callback::new_paint_callback(
            rect,
            Paint3d {
                view_proj: camera.view_proj(aspect).to_cols_array(),
                mesh: scene.mesh.clone(),
                generation: self.generation,
            },
        ));

        if response.dragged() {
            return None;
        }
        let pointer = response.hover_pos()?;
        let ndc = Vec2::new(
            (pointer.x - rect.min.x) / rect.width() * 2.0 - 1.0,
            1.0 - (pointer.y - rect.min.y) / rect.height() * 2.0,
        );
        let (origin, direction) = camera.ray(ndc, aspect);
        let ([x, y, z], block) = scene.pick(origin, direction)?;
        let hovered = Hovered { x, y, z, block };
        block_tooltip(ui.ctx(), pointer, doc, hovered);
        Some(hovered)
    }
}
