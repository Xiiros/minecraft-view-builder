//! What the writers share: what an export loses, errors, merging regions, compression.

use std::io::Write;

use flate2::Compression;
use flate2::write::GzEncoder;

use crate::block::{BlockState, Palette};
use crate::build::{Build, ModelError, Pos, Region, Size};
use crate::formats::{FileFormat, litematica, sponge};
use crate::mvb::MvbError;
use crate::nbt::NbtError;

/// Largest side a structure block can save or load in one piece.
pub const STRUCTURE_BLOCK_LIMIT: u32 = 48;

/// Something an export to a given format cannot keep. Shown to the user before exporting.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Loss {
    /// The format has a single box: this many regions are merged into one.
    RegionsMerged(usize),
    /// Data only another tool understands (Litematica preview and scheduled ticks, WorldEdit
    /// offset and biomes...).
    FormatData(Tool),
    /// The format stores neither name nor description.
    Metadata,
    /// Wider or taller than a structure block can load in one piece.
    StructureBlockLimit,
}

/// Tool whose own data can be kept from one of its files.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    Litematica,
    WorldEdit,
}

#[derive(Debug, thiserror::Error)]
pub enum ExportError {
    #[error(transparent)]
    Nbt(#[from] NbtError),
    #[error(transparent)]
    Model(#[from] ModelError),
    #[error(transparent)]
    Project(#[from] MvbError),
    #[error("compression error: {0}")]
    Io(#[from] std::io::Error),
    #[error("the build is too large for this format: {0}")]
    TooLarge(&'static str),
}

/// Everything `format` will not keep from `build`, empty for a lossless export.
pub fn losses(build: &Build, format: FileFormat) -> Vec<Loss> {
    let mut losses = Vec::new();
    if format == FileFormat::Project {
        return losses;
    }
    let single_box = matches!(format, FileFormat::Sponge | FileFormat::Structure);
    if single_box && build.regions.len() > 1 {
        losses.push(Loss::RegionsMerged(build.regions.len()));
    }
    let own_key = match format {
        FileFormat::Litematica => Some(litematica::EXTRA_KEY),
        FileFormat::Sponge => Some(sponge::EXTRA_KEY),
        _ => None,
    };
    for (key, tool) in [
        (litematica::EXTRA_KEY, Tool::Litematica),
        (sponge::EXTRA_KEY, Tool::WorldEdit),
    ] {
        let present = build.extra.get(key).is_some()
            || build.regions.iter().any(|r| r.extra.get(key).is_some());
        if present && own_key != Some(key) {
            losses.push(Loss::FormatData(tool));
        }
    }
    if format == FileFormat::Structure {
        if !build.metadata.name.is_empty() || !build.metadata.description.is_empty() {
            losses.push(Loss::Metadata);
        }
        let too_big = build.bounds().is_some_and(|(min, max)| {
            let span = |lo: i32, hi: i32| i64::from(hi) - i64::from(lo) + 1;
            [span(min.x, max.x), span(min.y, max.y), span(min.z, max.z)]
                .iter()
                .any(|&s| s > i64::from(STRUCTURE_BLOCK_LIMIT))
        });
        if too_big {
            losses.push(Loss::StructureBlockLimit);
        }
    }
    losses
}

/// All regions in one box covering the bounding box, for single-box formats. Where regions
/// overlap, a drawn block wins over an empty one, then the later region wins.
pub(crate) fn merged_region(build: &Build) -> Result<Region, ExportError> {
    let Some((min, max)) = build.bounds() else {
        return Ok(Region::new("", Pos::default(), Size::new(0, 0, 0))?);
    };
    let span = |lo: i32, hi: i32| u32::try_from(i64::from(hi) - i64::from(lo) + 1).ok();
    let (Some(sx), Some(sy), Some(sz)) =
        (span(min.x, max.x), span(min.y, max.y), span(min.z, max.z))
    else {
        return Err(ExportError::TooLarge("regions too far apart"));
    };
    let mut merged = Region::new("", min, Size::new(sx, sy, sz))?;
    let visible = |block: u16| build.palette.get(block).is_some_and(BlockState::is_visible);
    for region in &build.regions {
        // Offsets are non-negative: `min` is the corner of the bounding box.
        let offset = [
            (i64::from(region.origin.x) - i64::from(min.x)) as u32,
            (i64::from(region.origin.y) - i64::from(min.y)) as u32,
            (i64::from(region.origin.z) - i64::from(min.z)) as u32,
        ];
        let size = region.size();
        for y in 0..size.y {
            for z in 0..size.z {
                for x in 0..size.x {
                    let block = region.get(x, y, z).unwrap_or(Palette::AIR);
                    let (mx, my, mz) = (x + offset[0], y + offset[1], z + offset[2]);
                    if block != Palette::AIR
                        && (visible(block) || merged.get(mx, my, mz) == Some(Palette::AIR))
                    {
                        merged.set(mx, my, mz, block);
                    }
                }
            }
        }
        let shift = |pos: Pos| {
            Pos::new(
                pos.x + offset[0] as i32,
                pos.y + offset[1] as i32,
                pos.z + offset[2] as i32,
            )
        };
        merged
            .block_entities
            .extend(region.block_entities.iter().map(|be| {
                let mut be = be.clone();
                be.pos = shift(be.pos);
                be
            }));
        merged.entities.extend(region.entities.iter().map(|e| {
            let mut e = e.clone();
            for (p, o) in e.pos.iter_mut().zip(offset) {
                *p += f64::from(o);
            }
            e
        }));
    }
    Ok(merged)
}

pub(crate) fn gzip(bytes: &[u8]) -> Result<Vec<u8>, ExportError> {
    let mut encoder = GzEncoder::new(Vec::with_capacity(bytes.len() / 4), Compression::default());
    encoder.write_all(bytes)?;
    Ok(encoder.finish()?)
}

/// Converts a size to the `i32` that file formats store (sizes are capped well below `i32::MAX`).
pub(crate) fn int(size: u32) -> i32 {
    i32::try_from(size).unwrap_or(i32::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::build::BlockEntity;
    use crate::nbt::Compound;

    fn two_regions() -> Build {
        let mut build = Build::new();
        let stone = build
            .palette
            .get_or_insert(BlockState::new("stone"))
            .unwrap();
        let glass = build
            .palette
            .get_or_insert(BlockState::new("glass"))
            .unwrap();
        let mut a = Region::from_blocks(
            "a",
            Pos::new(-1, 0, 0),
            Size::new(2, 1, 1),
            vec![stone, stone],
        )
        .unwrap();
        a.block_entities.push(BlockEntity {
            pos: Pos::new(1, 0, 0),
            id: "minecraft:chest".into(),
            data: Compound::new(),
        });
        let b = Region::from_blocks(
            "b",
            Pos::new(0, 0, 0),
            Size::new(2, 1, 1),
            vec![Palette::AIR, glass],
        )
        .unwrap();
        build.regions.extend([a, b]);
        build
    }

    #[test]
    fn merging_keeps_drawn_blocks_and_shifts_positions() {
        let build = two_regions();
        let merged = merged_region(&build).unwrap();
        assert_eq!(merged.origin, Pos::new(-1, 0, 0));
        assert_eq!(merged.size(), Size::new(3, 1, 1));
        let names: Vec<String> = merged
            .blocks()
            .iter()
            .map(|&b| build.palette.get(b).unwrap().to_string())
            .collect();
        // Region b's air at x = 0 does not erase region a's stone.
        assert_eq!(
            names,
            ["minecraft:stone", "minecraft:stone", "minecraft:glass"]
        );
        assert_eq!(merged.block_entities[0].pos, Pos::new(1, 0, 0));
    }

    #[test]
    fn losses_depend_on_the_format() {
        let mut build = two_regions();
        assert!(losses(&build, FileFormat::Project).is_empty());
        assert!(losses(&build, FileFormat::Litematica).is_empty());
        assert_eq!(losses(&build, FileFormat::Sponge), [Loss::RegionsMerged(2)]);

        build.metadata.name = "Tour".into();
        build.extra.insert(litematica::EXTRA_KEY, Compound::new());
        assert_eq!(losses(&build, FileFormat::Litematica), []);
        assert_eq!(
            losses(&build, FileFormat::Structure),
            [
                Loss::RegionsMerged(2),
                Loss::FormatData(Tool::Litematica),
                Loss::Metadata
            ]
        );
    }

    #[test]
    fn structure_block_limit_is_reported() {
        let mut build = Build::new();
        build
            .regions
            .push(Region::new("", Pos::default(), Size::new(49, 1, 1)).unwrap());
        assert_eq!(
            losses(&build, FileFormat::Structure),
            [Loss::StructureBlockLimit]
        );
    }
}
