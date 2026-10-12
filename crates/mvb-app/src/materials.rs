//! Materials panel: the items needed for the current layer (or the layers shown in 3D) or for
//! the whole build, with the number of stacks.

use eframe::egui::{self, Color32, Sense, vec2};
use mvb_core::BlockState;
use mvb_core::materials::{ItemCount, stack_size};

use crate::document::Document;
use crate::i18n::Texts;

/// What the list counts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    /// The layer shown in 2D, or the layers checked in 3D.
    Shown,
    Total,
}

#[derive(PartialEq)]
struct CacheKey {
    scope: Scope,
    in_3d: bool,
    layer: u32,
    region_visible: Vec<bool>,
    layer_visible: Vec<bool>,
}

pub struct MaterialsPanel {
    pub open: bool,
    scope: Scope,
    cache: Option<(CacheKey, Vec<ItemCount>)>,
}

impl Default for MaterialsPanel {
    fn default() -> Self {
        Self {
            open: true,
            scope: Scope::Shown,
            cache: None,
        }
    }
}

impl MaterialsPanel {
    /// Counts again only when what is counted changed.
    fn counts(&mut self, doc: &Document, in_3d: bool, layer: u32) -> &[ItemCount] {
        let key = CacheKey {
            scope: self.scope,
            in_3d,
            layer,
            region_visible: doc.region_visible.clone(),
            layer_visible: if in_3d {
                doc.layer_visible.clone()
            } else {
                Vec::new()
            },
        };
        if self.cache.as_ref().is_none_or(|(cached, _)| *cached != key) {
            let layers: Vec<u32> = match (self.scope, in_3d) {
                (Scope::Total, _) => (0..doc.size[1]).collect(),
                (Scope::Shown, false) => vec![layer],
                (Scope::Shown, true) => (0..doc.size[1])
                    .filter(|&y| doc.layer_visible.get(y as usize).copied().unwrap_or(false))
                    .collect(),
            };
            let counts = doc
                .materials
                .count(layers.into_iter().flat_map(|y| doc.layer_cells(y)));
            self.cache = Some((key, counts));
        }
        &self.cache.as_ref().expect("filled above").1
    }

    pub fn ui(
        &mut self,
        ui: &mut egui::Ui,
        doc: &Document,
        texts: &Texts,
        in_3d: bool,
        layer: u32,
    ) {
        ui.add_space(4.0);
        ui.heading(texts.materials);
        let shown_label = if in_3d {
            texts.visible_layers
        } else {
            texts.scope_layer
        };
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.scope, Scope::Shown, shown_label);
            ui.selectable_value(&mut self.scope, Scope::Total, texts.scope_total);
        });
        let scope_label = match self.scope {
            Scope::Shown if !in_3d => format!("{} {} · y = {layer}", texts.layer, layer + 1),
            Scope::Shown => shown_label.to_owned(),
            Scope::Total => texts.scope_total.to_owned(),
        };
        let counts = self.counts(doc, in_3d, layer);

        let total: u64 = counts.iter().map(|c| c.count).sum();
        ui.label(format!(
            "{total} {} · {} {}",
            texts.items,
            counts.len(),
            texts.kinds
        ));
        if counts.is_empty() {
            ui.weak(texts.no_materials);
            return;
        }
        if ui.button(texts.copy_list).clicked() {
            ui.ctx().copy_text(as_text(&scope_label, counts, texts));
        }
        ui.separator();

        egui::ScrollArea::vertical()
            .id_salt("materials")
            .auto_shrink([false, true])
            .show(ui, |ui| {
                egui::Grid::new("materials_grid")
                    .num_columns(3)
                    .striped(true)
                    .show(ui, |ui| {
                        ui.strong(texts.column_block);
                        ui.strong(texts.column_count);
                        ui.strong(texts.column_stacks);
                        ui.end_row();
                        for item in counts {
                            ui.horizontal(|ui| {
                                let (rect, _) =
                                    ui.allocate_exact_size(vec2(12.0, 12.0), Sense::hover());
                                let color = doc.color(item.block).unwrap_or(Color32::GRAY);
                                ui.painter().rect_filled(rect, 2.0, color);
                                ui.label(display_name(&item.item));
                            });
                            ui.label(item.count.to_string());
                            ui.weak(stacks(item.count, stack_size(&item.item)));
                            ui.end_row();
                        }
                    });
            });
    }
}

fn display_name(item: &str) -> String {
    BlockState::new(item).display_name()
}

/// "2 × 64 + 5" for 133 items stacking by 64; empty when less than one stack.
fn stacks(count: u64, stack: u32) -> String {
    let stack = u64::from(stack);
    if stack <= 1 || count < stack {
        return String::new();
    }
    let (full, rest) = (count / stack, count % stack);
    if rest == 0 {
        format!("{full} × {stack}")
    } else {
        format!("{full} × {stack} + {rest}")
    }
}

/// Tab-separated list for the clipboard (pastes into a spreadsheet).
fn as_text(scope: &str, counts: &[ItemCount], texts: &Texts) -> String {
    let mut text = format!(
        "{} — {scope}\n{}\t{}\t{}\n",
        texts.materials, texts.column_block, texts.column_count, texts.column_stacks
    );
    for item in counts {
        let line = format!(
            "{}\t{}\t{}\n",
            display_name(&item.item),
            item.count,
            stacks(item.count, stack_size(&item.item))
        );
        text.push_str(&line);
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::Language;
    use mvb_core::{Build, Pos, Region, Size};

    #[test]
    fn stacks_are_written_for_the_game_inventory() {
        assert_eq!(stacks(133, 64), "2 × 64 + 5");
        assert_eq!(stacks(128, 64), "2 × 64");
        assert_eq!(stacks(40, 64), "");
        assert_eq!(stacks(20, 16), "1 × 16 + 4");
        assert_eq!(stacks(3, 1), "");
    }

    /// 2 layers: 3 stone below, 1 glass above, plus a hidden-able second region of 2 dirt.
    fn doc() -> Document {
        let mut build = Build::new();
        let mut add = |s: &str| build.palette.get_or_insert(s.parse().unwrap()).unwrap();
        let (stone, glass, dirt) = (add("stone"), add("glass"), add("dirt"));
        let blocks = vec![stone, stone, stone, glass, 0, 0];
        build
            .regions
            .push(Region::from_blocks("a", Pos::default(), Size::new(3, 2, 1), blocks).unwrap());
        build.regions.push(
            Region::from_blocks("b", Pos::new(0, 0, 1), Size::new(2, 1, 1), vec![dirt, dirt])
                .unwrap(),
        );
        Document::new(build, "test".into())
    }

    fn summary(
        panel: &mut MaterialsPanel,
        doc: &Document,
        in_3d: bool,
        layer: u32,
    ) -> Vec<(String, u64)> {
        panel
            .counts(doc, in_3d, layer)
            .iter()
            .map(|c| (display_name(&c.item), c.count))
            .collect()
    }

    #[test]
    fn counts_follow_the_scope() {
        let mut doc = doc();
        let mut panel = MaterialsPanel::default();
        let pair = |name: &str, n| (name.to_owned(), n);
        assert_eq!(
            summary(&mut panel, &doc, false, 0),
            [pair("Stone", 3), pair("Dirt", 2)]
        );
        assert_eq!(summary(&mut panel, &doc, false, 1), [pair("Glass", 1)]);
        // In 3D, the checked layers.
        doc.layer_visible[0] = false;
        assert_eq!(summary(&mut panel, &doc, true, 0), [pair("Glass", 1)]);
        panel.scope = Scope::Total;
        assert_eq!(
            summary(&mut panel, &doc, true, 0),
            [pair("Stone", 3), pair("Dirt", 2), pair("Glass", 1)]
        );
        // Hidden regions are not counted.
        doc.region_visible[1] = false;
        assert_eq!(
            summary(&mut panel, &doc, false, 0),
            [pair("Stone", 3), pair("Glass", 1)]
        );
    }

    #[test]
    fn copied_text_is_tab_separated() {
        let doc = doc();
        let mut panel = MaterialsPanel {
            scope: Scope::Total,
            ..MaterialsPanel::default()
        };
        let texts = Language::English.texts();
        let text = as_text("Whole build", panel.counts(&doc, false, 0), texts);
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines[0], "Materials — Whole build");
        assert_eq!(lines[1], "Block\tCount\tStacks");
        assert_eq!(lines[2], "Stone\t3\t");
        assert_eq!(lines.len(), 5);
    }
}
