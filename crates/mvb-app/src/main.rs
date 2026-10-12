// Hide the console window on Windows in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod block_colors;
mod camera;
mod colors;
mod document;
mod i18n;
mod render3d;
mod scene;
mod shapes;
mod view2d;
mod view3d;

use std::path::{Path, PathBuf};

use eframe::egui;
use mvb_core::formats::FileFormat;

use document::{Document, OpenError};
use i18n::{Language, Texts};
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
    /// False when the 3D renderer could not be created.
    gpu_3d: bool,
    /// Block under the pointer during the previous frame, shown in the status bar.
    hovered: Option<Hovered>,
    error: Option<(PathBuf, OpenError)>,
}

impl App {
    fn new() -> Self {
        Self {
            language: Language::detect(),
            document: None,
            mode: Mode::Layer2d,
            view2d: View2d::default(),
            view3d: View3d::default(),
            gpu_3d: false,
            hovered: None,
            error: None,
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
                if ui.button(texts.menu_quit).clicked() {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                }
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

    fn status_bar(&self, ui: &mut egui::Ui, texts: &Texts) {
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
    }
}
