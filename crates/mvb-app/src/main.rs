// Hide the console window on Windows in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod block_colors;
mod camera;
mod colors;
mod document;
mod i18n;
mod materials;
mod render3d;
mod scene;
mod shapes;
mod view2d;
mod view3d;

use std::path::{Path, PathBuf};

use eframe::egui;
use mvb_core::formats::{self, FileFormat, Loss, Tool};

use document::{Document, OpenError};
use i18n::{Language, Texts};
use materials::MaterialsPanel;
use render3d::Renderer3d;
use view2d::{Hovered, View2d, ZOOM_STEP};
use view3d::View3d;

/// Display name of the application. The final name is not chosen yet.
const APP_NAME: &str = "MVB";

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(window_title(None))
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([800.0, 500.0])
            .with_drag_and_drop(true),
        depth_buffer: render3d::DEPTH_BITS,
        multisampling: render3d::MSAA_SAMPLES,
        ..Default::default()
    };
    // A file given on the command line (or by "Open with") is opened at start-up.
    let initial_file = std::env::args_os().nth(1).map(PathBuf::from);
    eframe::run_native(
        APP_NAME,
        options,
        Box::new(|cc| {
            let mut app = App::new();
            if let Some(render_state) = &cc.wgpu_render_state {
                Renderer3d::install(render_state);
                app.gpu_3d = true;
            }
            if let Some(path) = initial_file {
                app.open(&cc.egui_ctx, &path);
            }
            Ok(Box::new(app))
        }),
    )
}

/// How long the "saved" confirmation stays in the status bar.
const NOTICE_SECONDS: f64 = 6.0;

fn format_label(texts: &Texts, format: FileFormat) -> &'static str {
    match format {
        FileFormat::Project => texts.format_project,
        FileFormat::Litematica => texts.format_litematica,
        FileFormat::Sponge => texts.format_sponge,
        FileFormat::Structure => texts.format_structure,
    }
}

fn loss_text(texts: &Texts, loss: &Loss) -> String {
    match loss {
        Loss::RegionsMerged(count) => texts.loss_regions.replace("{n}", &count.to_string()),
        Loss::FormatData(Tool::Litematica) => texts.loss_litematica_data.to_owned(),
        Loss::FormatData(Tool::WorldEdit) => texts.loss_worldedit_data.to_owned(),
        Loss::Metadata => texts.loss_metadata.to_owned(),
        Loss::StructureBlockLimit => texts
            .loss_structure_limit
            .replace("{n}", &formats::STRUCTURE_BLOCK_LIMIT.to_string()),
    }
}

/// A build title usable as a file name on every system.
fn file_name(title: &str) -> String {
    let name: String = title
        .chars()
        .map(|c| {
            if c.is_control() || r#"<>:"/\|?*"#.contains(c) {
                '_'
            } else {
                c
            }
        })
        .collect();
    let name = name.trim().trim_end_matches('.');
    if name.is_empty() {
        "build".to_owned()
    } else {
        name.to_owned()
    }
}

fn window_title(document: Option<&str>) -> String {
    let base = format!("{APP_NAME} {}", mvb_core::VERSION);
    match document {
        Some(title) => format!("{title} — {base}"),
        None => base,
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Layer2d,
    View3d,
}

struct App {
    language: Language,
    document: Option<Document>,
    mode: Mode,
    view2d: View2d,
    view3d: View3d,
    materials: MaterialsPanel,
    /// False when the 3D renderer could not be created.
    gpu_3d: bool,
    /// Block under the pointer during the previous frame, shown in the status bar.
    hovered: Option<Hovered>,
    error: Option<(PathBuf, OpenError)>,
    /// Export waiting for the user to accept what it loses.
    pending_export: Option<(FileFormat, Vec<Loss>)>,
    save_error: Option<(PathBuf, String)>,
    /// Confirmation shown in the status bar, with the time it was set.
    notice: Option<(String, f64)>,
}

impl App {
    fn new() -> Self {
        Self {
            language: Language::detect(),
            document: None,
            mode: Mode::Layer2d,
            view2d: View2d::default(),
            view3d: View3d::default(),
            materials: MaterialsPanel::default(),
            gpu_3d: false,
            hovered: None,
            error: None,
            pending_export: None,
            save_error: None,
            notice: None,
        }
    }

    fn open(&mut self, ctx: &egui::Context, path: &Path) {
        match Document::open(path) {
            Ok(document) => {
                ctx.send_viewport_cmd(egui::ViewportCommand::Title(window_title(Some(
                    &document.title,
                ))));
                self.document = Some(document);
                self.view2d = View2d::default();
                self.view3d = View3d::default();
                self.hovered = None;
            }
            Err(error) => self.error = Some((path.to_owned(), error)),
        }
    }

    fn open_dialog(&mut self, ctx: &egui::Context) {
        let texts = self.language.texts();
        let extensions = FileFormat::ALL.map(FileFormat::extension);
        if let Some(path) = rfd::FileDialog::new()
            .add_filter(texts.file_filter, &extensions)
            .pick_file()
        {
            self.open(ctx, &path);
        }
    }

    fn menu_bar(&mut self, ui: &mut egui::Ui) {
        let texts = self.language.texts();
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button(texts.menu_file, |ui| {
                if ui.button(texts.menu_open).clicked() {
                    self.open_dialog(ui.ctx());
                }
                let has_document = self.document.is_some();
                if ui
                    .add_enabled(has_document, egui::Button::new(texts.menu_save_project))
                    .clicked()
                {
                    self.start_export(ui.ctx(), FileFormat::Project);
                }
                ui.add_enabled_ui(has_document, |ui| {
                    ui.menu_button(texts.menu_export, |ui| {
                        for format in [
                            FileFormat::Litematica,
                            FileFormat::Sponge,
                            FileFormat::Structure,
                        ] {
                            if ui.button(format_label(texts, format)).clicked() {
                                self.start_export(ui.ctx(), format);
                            }
                        }
                    });
                });
                ui.separator();
                if ui.button(texts.menu_quit).clicked() {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });
            ui.menu_button(texts.menu_view, |ui| {
                ui.checkbox(&mut self.materials.open, texts.show_materials);
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                egui::ComboBox::from_id_salt("language")
                    .selected_text(self.language.native_name())
                    .show_ui(ui, |ui| {
                        for language in Language::ALL {
                            ui.selectable_value(
                                &mut self.language,
                                language,
                                language.native_name(),
                            );
                        }
                    });
                ui.label(texts.language);
            });
        });
    }

    fn side_panel(&mut self, ui: &mut egui::Ui, texts: &Texts) {
        let Some(document) = &mut self.document else {
            return;
        };
        ui.add_space(4.0);
        ui.heading(&document.title);
        egui::Grid::new("build_info").num_columns(2).show(ui, |ui| {
            if !document.build.metadata.author.is_empty() {
                ui.label(texts.author);
                ui.label(&document.build.metadata.author);
                ui.end_row();
            }
            let [x, y, z] = document.size;
            ui.label(texts.size);
            ui.label(format!("{x} × {y} × {z}"));
            ui.end_row();
            ui.label(texts.blocks);
            ui.label(document.block_count.to_string());
            ui.end_row();
        });

        ui.separator();
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.mode, Mode::Layer2d, texts.view_2d);
            ui.selectable_value(&mut self.mode, Mode::View3d, texts.view_3d);
        });
        ui.separator();

        match self.mode {
            Mode::Layer2d => Self::view_2d_controls(ui, texts, document, &mut self.view2d),
            Mode::View3d => {
                if ui.button(texts.reset_view).clicked() {
                    self.view3d.reset();
                }
            }
        }

        if document.build.regions.len() > 1 {
            ui.separator();
            ui.strong(texts.regions);
            let regions = document.build.regions.iter();
            for (i, (region, visible)) in regions.zip(&mut document.region_visible).enumerate() {
                let name = if region.name.is_empty() {
                    format!("{} {}", texts.region_fallback, i + 1)
                } else {
                    region.name.clone()
                };
                ui.checkbox(visible, name);
            }
        }

        if self.mode == Mode::View3d {
            ui.separator();
            Self::layer_list(ui, texts, document);
        }
    }

    fn view_2d_controls(ui: &mut egui::Ui, texts: &Texts, document: &Document, view: &mut View2d) {
        let height = document.size[1];
        if height > 0 {
            ui.strong(texts.layer);
            ui.horizontal(|ui| {
                let down = ui.add_enabled(view.layer > 0, egui::Button::new("⏷"));
                if down.on_hover_text(texts.layer_down).clicked() {
                    view.layer -= 1;
                }
                ui.label(format!("{} / {height}", view.layer + 1));
                let up = ui.add_enabled(view.layer + 1 < height, egui::Button::new("⏶"));
                if up.on_hover_text(texts.layer_up).clicked() {
                    view.layer += 1;
                }
                ui.weak(format!("y = {}", view.layer));
            });
            ui.add(egui::Slider::new(&mut view.layer, 0..=height - 1).show_value(false));
            ui.separator();
        }

        ui.strong(texts.view);
        ui.horizontal(|ui| {
            if ui.button("−").on_hover_text(texts.zoom_out).clicked() {
                view.zoom_by(1.0 / ZOOM_STEP);
            }
            if ui.button("+").on_hover_text(texts.zoom_in).clicked() {
                view.zoom_by(ZOOM_STEP);
            }
            if ui.button("⟲").on_hover_text(texts.rotate_left).clicked() {
                view.rotate(-1);
            }
            if ui.button("⟳").on_hover_text(texts.rotate_right).clicked() {
                view.rotate(1);
            }
        });
        if ui.button(texts.reset_view).clicked() {
            view.reset();
        }
        ui.checkbox(&mut view.show_below, texts.show_below);
        ui.checkbox(&mut view.show_grid, texts.show_grid);
    }

    /// Checkbox per layer for the 3D view, top layer first like in the game.
    fn layer_list(ui: &mut egui::Ui, texts: &Texts, document: &mut Document) {
        ui.strong(texts.visible_layers);
        ui.horizontal(|ui| {
            if ui.button(texts.all_layers).clicked() {
                document.layer_visible.fill(true);
            }
            if ui.button(texts.no_layers).clicked() {
                document.layer_visible.fill(false);
            }
            if ui.button(texts.invert_layers).clicked() {
                document.layer_visible.iter_mut().for_each(|v| *v = !*v);
            }
        });
        egui::ScrollArea::vertical()
            .id_salt("layer_list")
            .auto_shrink([false, true])
            .show(ui, |ui| {
                for (y, visible) in document.layer_visible.iter_mut().enumerate().rev() {
                    ui.checkbox(visible, format!("{} {} · y = {y}", texts.layer, y + 1));
                }
            });
    }

    /// Exports right away when nothing is lost, otherwise asks first.
    fn start_export(&mut self, ctx: &egui::Context, format: FileFormat) {
        let Some(document) = &self.document else {
            return;
        };
        let losses = formats::losses(&document.build, format);
        if losses.is_empty() {
            self.save_dialog(ctx, format);
        } else {
            self.pending_export = Some((format, losses));
        }
    }

    fn save_dialog(&mut self, ctx: &egui::Context, format: FileFormat) {
        let Some(document) = &self.document else {
            return;
        };
        let texts = self.language.texts();
        let extension = format.extension();
        let Some(path) = rfd::FileDialog::new()
            .add_filter(format_label(texts, format), &[extension])
            .set_file_name(format!("{}.{extension}", file_name(&document.title)))
            .save_file()
        else {
            return;
        };
        let result = format
            .write(&document.build)
            .map_err(|e| e.to_string())
            .and_then(|bytes| std::fs::write(&path, bytes).map_err(|e| e.to_string()));
        match result {
            Ok(()) => {
                let time = ctx.input(|i| i.time);
                self.notice = Some((format!("{} {}", texts.saved, path.display()), time));
            }
            Err(error) => self.save_error = Some((path, error)),
        }
    }

    /// Lists what the export loses, with Cancel / Export anyway.
    fn export_warning(&mut self, ctx: &egui::Context, texts: &Texts) {
        let Some((format, losses)) = &self.pending_export else {
            return;
        };
        let format = *format;
        let mut choice = None;
        let modal = egui::Modal::new(egui::Id::new("export_warning")).show(ctx, |ui| {
            ui.set_max_width(460.0);
            ui.heading(texts.export_warning_title);
            ui.label(format_label(texts, format));
            ui.add_space(6.0);
            for loss in losses {
                ui.label(format!("• {}", loss_text(texts, loss)));
            }
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if ui.button(texts.cancel).clicked() {
                    choice = Some(false);
                }
                if ui.button(texts.export_anyway).clicked() {
                    choice = Some(true);
                }
            });
        });
        if modal.should_close() {
            choice.get_or_insert(false);
        }
        if let Some(accepted) = choice {
            self.pending_export = None;
            if accepted {
                self.save_dialog(ctx, format);
            }
        }
    }

    fn save_error_modal(&mut self, ctx: &egui::Context, texts: &Texts) {
        let Some((path, error)) = &self.save_error else {
            return;
        };
        let modal = egui::Modal::new(egui::Id::new("save_error")).show(ctx, |ui| {
            ui.set_max_width(420.0);
            ui.heading(texts.error_save_title);
            ui.label(path.display().to_string());
            ui.add_space(6.0);
            ui.label(error);
            ui.add_space(6.0);
            ui.button(texts.ok).clicked()
        });
        if modal.inner || modal.should_close() {
            self.save_error = None;
        }
    }

    fn status_bar(&self, ui: &mut egui::Ui, texts: &Texts) {
        if let Some((notice, since)) = &self.notice {
            let elapsed = ui.input(|i| i.time) - since;
            if elapsed < NOTICE_SECONDS {
                ui.label(notice);
                ui.ctx()
                    .request_repaint_after_secs((NOTICE_SECONDS - elapsed) as f32);
                return;
            }
        }
        let hovered = self.hovered.and_then(|h| {
            let state = self.document.as_ref()?.build.palette.get(h.block)?;
            Some((h, state))
        });
        match hovered {
            Some((h, state)) => {
                let name = state.display_name();
                ui.label(format!("{name} — x {} · y {} · z {}", h.x, h.y, h.z));
            }
            None => {
                ui.weak(match self.mode {
                    Mode::Layer2d => texts.status_hint,
                    Mode::View3d => texts.status_hint_3d,
                });
            }
        }
    }

    fn empty_state(&mut self, ui: &mut egui::Ui, texts: &Texts) {
        ui.vertical_centered(|ui| {
            ui.add_space(ui.available_height() / 3.0);
            ui.heading(texts.no_build_loaded);
            ui.add_space(8.0);
            if ui.button(texts.open_button).clicked() {
                self.open_dialog(ui.ctx());
            }
            ui.weak(texts.drop_hint);
        });
    }

    fn error_modal(&mut self, ctx: &egui::Context, texts: &Texts) {
        let Some((path, error)) = &self.error else {
            return;
        };
        let modal = egui::Modal::new(egui::Id::new("open_error")).show(ctx, |ui| {
            ui.set_max_width(420.0);
            ui.heading(texts.error_title);
            ui.label(path.display().to_string());
            ui.add_space(6.0);
            match error {
                OpenError::Unsupported => {
                    ui.label(texts.error_unsupported);
                }
                OpenError::Io(e) => {
                    ui.label(texts.error_read);
                    ui.label(e.to_string());
                }
                OpenError::Read(e) => {
                    ui.label(texts.error_read);
                    ui.label(e.to_string());
                }
            }
            ui.add_space(6.0);
            ui.button(texts.ok).clicked()
        });
        if modal.inner || modal.should_close() {
            self.error = None;
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let texts = self.language.texts();
        let dropped = ui.input(|i| i.raw.dropped_files.first().map(|f| f.path().to_owned()));
        if let Some(path) = dropped {
            self.open(ui.ctx(), &path);
        }

        egui::Panel::top("menu_bar").show(ui, |ui| self.menu_bar(ui));

        if self.document.is_some() {
            egui::Panel::bottom("status_bar").show(ui, |ui| self.status_bar(ui, texts));
        }

        if self.document.is_some() {
            egui::Panel::left("side_panel")
                .resizable(false)
                .exact_size(240.0)
                .show(ui, |ui| self.side_panel(ui, texts));
        }
        if let Some(document) = &self.document
            && self.materials.open
        {
            let in_3d = self.mode == Mode::View3d;
            let layer = self.view2d.layer.min(document.size[1].saturating_sub(1));
            egui::Panel::right("materials_panel")
                .resizable(true)
                .default_size(280.0)
                .show(ui, |ui| {
                    self.materials.ui(ui, document, texts, in_3d, layer)
                });
        }
        let central = egui::CentralPanel::default();
        match &self.document {
            Some(document) => {
                self.hovered = match self.mode {
                    Mode::Layer2d => central.show(ui, |ui| self.view2d.ui(ui, document, texts)),
                    Mode::View3d => {
                        let gpu = self.gpu_3d;
                        central.show(ui, |ui| self.view3d.ui(ui, document, texts, gpu))
                    }
                }
                .inner;
            }
            None => {
                central.show(ui, |ui| self.empty_state(ui, texts));
            }
        }

        self.error_modal(ui.ctx(), texts);
        self.export_warning(ui.ctx(), texts);
        self.save_error_modal(ui.ctx(), texts);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_names_are_sanitised() {
        assert_eq!(file_name("Complet_AutoBrewer*12"), "Complet_AutoBrewer_12");
        assert_eq!(file_name("a/b:c?"), "a_b_c_");
        assert_eq!(file_name("Tour du château"), "Tour du château");
        assert_eq!(file_name(" .. "), "build");
    }

    #[test]
    fn loss_texts_fill_their_placeholders() {
        for language in Language::ALL {
            let texts = language.texts();
            let losses = [
                Loss::RegionsMerged(3),
                Loss::FormatData(Tool::Litematica),
                Loss::FormatData(Tool::WorldEdit),
                Loss::Metadata,
                Loss::StructureBlockLimit,
            ];
            for loss in &losses {
                let text = loss_text(texts, loss);
                assert!(!text.contains('{'), "{text}");
            }
        }
        let french = Language::French.texts();
        assert_eq!(
            loss_text(french, &Loss::RegionsMerged(3)),
            "Les 3 régions seront fusionnées en une seule."
        );
    }
}
