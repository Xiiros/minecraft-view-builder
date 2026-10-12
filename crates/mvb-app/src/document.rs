//! An opened build and the data derived from it for display.

use std::path::Path;

use eframe::egui::Color32;
use mvb_core::formats::{FileFormat, ReadError};
use mvb_core::{Build, Palette, Pos};

use crate::colors::{FaceColors, block_colors};
use crate::shapes::{Facing, Shape};

#[derive(Debug)]
pub enum OpenError {
    Unsupported,
    Io(std::io::Error),
    Read(ReadError),
}

pub struct Document {
    pub build: Build,
    /// Build name, or the file name when the build has none.
    pub title: String,
    /// Minimum corner of the bounding box: coordinates shown to the user are relative to it.
    pub min: Pos,
    /// Bounding box size along x, y and z.
    pub size: [u32; 3],
    /// Colours of each palette entry, `None` for blocks that are not drawn.
    pub colors: Vec<Option<FaceColors>>,
    /// Shape of each palette entry.
    pub shapes: Vec<Shape>,
    /// Direction of each palette entry that has one (`facing` property).
    pub facings: Vec<Option<Facing>>,
    pub region_visible: Vec<bool>,
    /// Layers shown in the 3D view, from the bottom.
    pub layer_visible: Vec<bool>,
    /// Number of drawn blocks.
    pub block_count: usize,
}

impl Document {
    pub fn open(path: &Path) -> Result<Self, OpenError> {
        let format = path
            .extension()
            .and_then(|e| e.to_str())
            .and_then(FileFormat::from_extension)
            .ok_or(OpenError::Unsupported)?;
        let bytes = std::fs::read(path).map_err(OpenError::Io)?;
        let build = format.read(&bytes).map_err(OpenError::Read)?;
        let file_name = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        Ok(Self::new(build, file_name))
    }

    pub fn new(build: Build, file_name: String) -> Self {
        let (min, size) = match build.bounds() {
            Some((min, max)) => {
                let span = |lo: i32, hi: i32| {
                    (i64::from(hi) - i64::from(lo) + 1)
                        .try_into()
                        .unwrap_or(u32::MAX)
                };
                (
                    min,
                    [span(min.x, max.x), span(min.y, max.y), span(min.z, max.z)],
                )
            }
            None => (Pos::default(), [0; 3]),
        };
        let colors: Vec<_> = build.palette.states().iter().map(block_colors).collect();
        let shapes = build.palette.states().iter().map(Shape::of).collect();
        let facings = build.palette.states().iter().map(Facing::of).collect();
        let block_count = build
            .regions
            .iter()
            .flat_map(|r| r.blocks())
            .filter(|&&b| colors.get(usize::from(b)).copied().flatten().is_some())
            .count();
        let title = if build.metadata.name.is_empty() {
            file_name
        } else {
            build.metadata.name.clone()
        };
        Self {
            region_visible: vec![true; build.regions.len()],
            layer_visible: vec![true; size[1] as usize],
            build,
            title,
            min,
            size,
            colors,
            shapes,
            facings,
            block_count,
        }
    }

    /// Colours of a palette entry, `None` for blocks that are not drawn.
    pub fn colors(&self, block: u16) -> Option<FaceColors> {
        self.colors.get(usize::from(block)).copied().flatten()
    }

    /// Colour of a palette entry seen from above.
    pub fn color(&self, block: u16) -> Option<Color32> {
        self.colors(block).map(|c| c.top)
    }

    /// Palette indices of one layer (`y` counted from the bottom of the build), x varying
    /// fastest then z. Merged from the visible regions; a drawn block wins over an empty one.
    pub fn layer_cells(&self, y: u32) -> Vec<u16> {
        let [width, _, depth] = self.size.map(|s| s as usize);
        let mut cells = vec![Palette::AIR; width * depth];
        let world_y = i64::from(self.min.y) + i64::from(y);
        let visible = self.build.regions.iter().zip(&self.region_visible);
        for (region, _) in visible.filter(|(_, visible)| **visible) {
            let Ok(local_y) = u32::try_from(world_y - i64::from(region.origin.y)) else {
                continue;
            };
            let Some(layer) = region.layer(local_y) else {
                continue;
            };
            // The bounding box contains every region, so these offsets are non-negative.
            let dx = (i64::from(region.origin.x) - i64::from(self.min.x)) as usize;
            let dz = (i64::from(region.origin.z) - i64::from(self.min.z)) as usize;
            let row_len = region.size().x as usize;
            if row_len == 0 {
                continue;
            }
            for (z, row) in layer.chunks_exact(row_len).enumerate() {
                let start = (dz + z) * width + dx;
                for (cell, &block) in cells[start..start + row_len].iter_mut().zip(row) {
                    if self.color(block).is_some() {
                        *cell = block;
                    }
                }
            }
        }
        cells
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mvb_core::{BlockState, Region, Size};

    /// Two overlapping regions: "a" 2x2x2 of stone at the origin, "b" 1x1x1 of glass at (1, 1, 1),
    /// plus a 1x1x1 region of air at (-1, 0, 0) that widens the bounding box.
    fn sample() -> Document {
        let mut build = Build::new();
        let stone = build
            .palette
            .get_or_insert(BlockState::new("stone"))
            .unwrap();
        let glass = build
            .palette
            .get_or_insert(BlockState::new("glass"))
            .unwrap();
        build.regions.push(
            Region::from_blocks("a", Pos::new(0, 0, 0), Size::new(2, 2, 2), vec![stone; 8])
                .unwrap(),
        );
        build.regions.push(
            Region::from_blocks("b", Pos::new(1, 1, 1), Size::new(1, 1, 1), vec![glass]).unwrap(),
        );
        build
            .regions
            .push(Region::new("air", Pos::new(-1, 0, 0), Size::new(1, 1, 1)).unwrap());
        Document::new(build, "file".into())
    }

    #[test]
    fn bounds_and_counts() {
        let doc = sample();
        assert_eq!(doc.min, Pos::new(-1, 0, 0));
        assert_eq!(doc.size, [3, 2, 2]);
        assert_eq!(doc.block_count, 9);
        assert_eq!(doc.title, "file");
    }

    #[test]
    fn layers_merge_visible_regions() {
        let mut doc = sample();
        let (air, stone, glass) = (0, 1, 2);
        assert_eq!(
            doc.layer_cells(0),
            vec![air, stone, stone, air, stone, stone]
        );
        assert_eq!(
            doc.layer_cells(1),
            vec![air, stone, stone, air, stone, glass]
        );
        doc.region_visible[0] = false;
        assert_eq!(doc.layer_cells(1), vec![air, air, air, air, air, glass]);
        assert_eq!(doc.layer_cells(0), vec![air; 6]);
    }

    #[test]
    fn empty_build_has_no_size() {
        let doc = Document::new(Build::new(), "empty".into());
        assert_eq!(doc.size, [0, 0, 0]);
        assert_eq!(doc.block_count, 0);
    }
}
