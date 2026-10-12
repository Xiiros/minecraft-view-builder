//! In-memory representation of a build: regions of blocks sharing one palette, plus entities.

use std::collections::BTreeMap;

use crate::block::Palette;
use crate::nbt::Compound;

/// Largest region accepted, in blocks (512 MiB of block indices). Protects against corrupt sizes.
pub const MAX_REGION_VOLUME: u64 = 1 << 28;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Pos {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

impl Pos {
    pub const fn new(x: i32, y: i32, z: i32) -> Self {
        Self { x, y, z }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Size {
    pub x: u32,
    pub y: u32,
    pub z: u32,
}

impl Size {
    pub const fn new(x: u32, y: u32, z: u32) -> Self {
        Self { x, y, z }
    }

    /// Number of blocks, saturating at `u64::MAX` (sizes read from files can be absurd).
    pub const fn volume(self) -> u64 {
        (self.x as u64)
            .saturating_mul(self.y as u64)
            .saturating_mul(self.z as u64)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ModelError {
    #[error("region volume {0} exceeds the limit of {MAX_REGION_VOLUME} blocks")]
    VolumeTooLarge(u64),
    #[error("region needs {expected} blocks but {actual} were given")]
    BlockCountMismatch { expected: u64, actual: usize },
    #[error("block uses palette index {index} but the palette has {len} entries")]
    UnknownPaletteIndex { index: u16, len: usize },
    #[error("too many distinct block states (maximum {})", Palette::MAX_LEN)]
    PaletteFull,
    #[error("the first palette entry must be minecraft:air")]
    FirstPaletteEntryNotAir,
    #[error("duplicate palette entry {0}")]
    DuplicatePaletteEntry(String),
}

/// Extra data attached to one block (chest content, sign text, banner patterns...), kept as is.
#[derive(Clone, Debug, PartialEq)]
pub struct BlockEntity {
    /// Position relative to the region origin.
    pub pos: Pos,
    /// Block entity type, e.g. `minecraft:banner`.
    pub id: String,
    /// Everything else, without position nor id.
    pub data: Compound,
}

/// Non-block object such as an item frame or an armor stand.
#[derive(Clone, Debug, PartialEq)]
pub struct Entity {
    /// Position relative to the region origin, in blocks.
    pub pos: [f64; 3],
    /// Entity type, e.g. `minecraft:item_frame`.
    pub id: String,
    /// Everything else, without position nor id.
    pub data: Compound,
}

/// A box of blocks. Builds from Litematica can have several, other formats have one.
#[derive(Clone, Debug, PartialEq)]
pub struct Region {
    pub name: String,
    /// Position of the region's minimum corner in build coordinates.
    pub origin: Pos,
    size: Size,
    /// Palette indices, one horizontal layer after another: index = (y * size.z + z) * size.x + x.
    blocks: Vec<u16>,
    pub block_entities: Vec<BlockEntity>,
    pub entities: Vec<Entity>,
    /// Format-specific data kept for a lossless re-export.
    pub extra: Compound,
}

impl Region {
    /// A region filled with air.
    pub fn new(name: impl Into<String>, origin: Pos, size: Size) -> Result<Self, ModelError> {
        let volume = checked_volume(size)?;
        Ok(Self::with_blocks(
            name.into(),
            origin,
            size,
            vec![Palette::AIR; volume],
        ))
    }

    /// A region from its palette indices, in the layout described on [`Region::blocks`].
    pub fn from_blocks(
        name: impl Into<String>,
        origin: Pos,
        size: Size,
        blocks: Vec<u16>,
    ) -> Result<Self, ModelError> {
        let volume = checked_volume(size)?;
        if blocks.len() != volume {
            return Err(ModelError::BlockCountMismatch {
                expected: size.volume(),
                actual: blocks.len(),
            });
        }
        Ok(Self::with_blocks(name.into(), origin, size, blocks))
    }

    fn with_blocks(name: String, origin: Pos, size: Size, blocks: Vec<u16>) -> Self {
        Self {
            name,
            origin,
            size,
            blocks,
            block_entities: Vec::new(),
            entities: Vec::new(),
            extra: Compound::new(),
        }
    }

    pub fn size(&self) -> Size {
        self.size
    }

    /// Palette indices, layer by layer from the bottom: index = (y * size.z + z) * size.x + x.
    pub fn blocks(&self) -> &[u16] {
        &self.blocks
    }

    fn index(&self, x: u32, y: u32, z: u32) -> Option<usize> {
        let Size {
            x: sx,
            y: sy,
            z: sz,
        } = self.size;
        (x < sx && y < sy && z < sz)
            .then(|| (y as usize * sz as usize + z as usize) * sx as usize + x as usize)
    }

    /// Block at a position relative to the region origin.
    pub fn get(&self, x: u32, y: u32, z: u32) -> Option<u16> {
        self.index(x, y, z).map(|i| self.blocks[i])
    }

    /// Sets a block; returns false if the position is outside the region.
    pub fn set(&mut self, x: u32, y: u32, z: u32, block: u16) -> bool {
        match self.index(x, y, z) {
            Some(i) => {
                self.blocks[i] = block;
                true
            }
            None => false,
        }
    }

    /// One horizontal layer (`size.x * size.z` blocks, x varying fastest).
    pub fn layer(&self, y: u32) -> Option<&[u16]> {
        let area = self.size.x as usize * self.size.z as usize;
        (y < self.size.y).then(|| &self.blocks[y as usize * area..(y as usize + 1) * area])
    }
}

pub(crate) fn checked_volume(size: Size) -> Result<usize, ModelError> {
    let volume = size.volume();
    if volume > MAX_REGION_VOLUME {
        return Err(ModelError::VolumeTooLarge(volume));
    }
    // Fits: MAX_REGION_VOLUME is far below usize::MAX on every supported platform.
    Ok(volume as usize)
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Metadata {
    pub name: String,
    pub author: String,
    pub description: String,
}

/// A complete build, as loaded from any supported file.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Build {
    pub metadata: Metadata,
    /// Game data version the block names come from (e.g. 3953 for 1.21).
    pub data_version: i32,
    pub palette: Palette,
    pub regions: Vec<Region>,
    /// Format-specific data kept for a lossless re-export.
    pub extra: Compound,
}

impl Build {
    pub fn new() -> Self {
        Self::default()
    }

    /// Inclusive bounding box (min, max) of all non-empty regions, or `None` if there are none.
    pub fn bounds(&self) -> Option<(Pos, Pos)> {
        self.regions
            .iter()
            .filter(|r| r.size.volume() > 0)
            .map(|r| {
                let max = Pos::new(
                    r.origin.x.saturating_add_unsigned(r.size.x - 1),
                    r.origin.y.saturating_add_unsigned(r.size.y - 1),
                    r.origin.z.saturating_add_unsigned(r.size.z - 1),
                );
                (r.origin, max)
            })
            .reduce(|(min_a, max_a), (min_b, max_b)| {
                (
                    Pos::new(
                        min_a.x.min(min_b.x),
                        min_a.y.min(min_b.y),
                        min_a.z.min(min_b.z),
                    ),
                    Pos::new(
                        max_a.x.max(max_b.x),
                        max_a.y.max(max_b.y),
                        max_a.z.max(max_b.z),
                    ),
                )
            })
    }

    /// True when both builds hold the same blocks, block entities and entities at the same
    /// positions relative to their bounding box, whatever the region split, palette order or
    /// format-specific data. Used to check that a file written then read back is the same build.
    pub fn same_content(&self, other: &Build) -> bool {
        self.content() == other.content()
    }

    /// Non-air blocks by position (merged like single-box exports do: a drawn block wins, then the
    /// later region), and sorted descriptions of block entities and entities.
    fn content(&self) -> (BTreeMap<[i64; 3], String>, Vec<String>, Vec<String>) {
        let min = self.bounds().map_or(Pos::default(), |(min, _)| min);
        let relative = |origin: Pos, [x, y, z]: [i64; 3]| {
            [
                i64::from(origin.x) - i64::from(min.x) + x,
                i64::from(origin.y) - i64::from(min.y) + y,
                i64::from(origin.z) - i64::from(min.z) + z,
            ]
        };
        let (mut blocks, mut block_entities, mut entities) =
            (BTreeMap::new(), Vec::new(), Vec::new());
        for region in &self.regions {
            let size = region.size;
            for y in 0..size.y {
                for z in 0..size.z {
                    for x in 0..size.x {
                        let block = region.get(x, y, z).unwrap_or(Palette::AIR);
                        let Some(state) = self.palette.get(block).filter(|_| block != Palette::AIR)
                        else {
                            continue;
                        };
                        let pos = relative(region.origin, [x, y, z].map(i64::from));
                        if state.is_visible() {
                            blocks.insert(pos, state.to_string());
                        } else {
                            blocks.entry(pos).or_insert_with(|| state.to_string());
                        }
                    }
                }
            }
            for be in &region.block_entities {
                let pos = relative(region.origin, [be.pos.x, be.pos.y, be.pos.z].map(i64::from));
                block_entities.push(format!("{pos:?} {} {:?}", be.id, be.data));
            }
            for e in &region.entities {
                let shift = relative(region.origin, [0; 3]);
                let pos: Vec<f64> = e.pos.iter().zip(shift).map(|(p, s)| p + s as f64).collect();
                entities.push(format!("{pos:?} {} {:?}", e.id, e.data));
            }
        }
        block_entities.sort();
        entities.sort();
        (blocks, block_entities, entities)
    }

    /// Checks that every block refers to an existing palette entry.
    pub fn validate(&self) -> Result<(), ModelError> {
        let len = self.palette.len();
        for region in &self.regions {
            if let Some(&index) = region.blocks.iter().find(|&&b| usize::from(b) >= len) {
                return Err(ModelError::UnknownPaletteIndex { index, len });
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block::BlockState;

    #[test]
    fn new_region_is_air() {
        let region = Region::new("main", Pos::default(), Size::new(2, 3, 4)).unwrap();
        assert_eq!(region.blocks().len(), 24);
        assert!(region.blocks().iter().all(|&b| b == Palette::AIR));
    }

    #[test]
    fn layers_are_contiguous() {
        let mut region = Region::new("main", Pos::default(), Size::new(3, 2, 2)).unwrap();
        assert!(region.set(2, 1, 1, 7));
        assert_eq!(region.get(2, 1, 1), Some(7));
        assert_eq!(region.layer(0).unwrap(), &[0; 6]);
        assert_eq!(region.layer(1).unwrap(), &[0, 0, 0, 0, 0, 7]);
        assert_eq!(region.layer(2), None);
    }

    #[test]
    fn out_of_bounds_access_is_refused() {
        let mut region = Region::new("main", Pos::default(), Size::new(1, 1, 1)).unwrap();
        assert_eq!(region.get(1, 0, 0), None);
        assert!(!region.set(0, 0, 1, 1));
    }

    #[test]
    fn oversized_or_mismatched_regions_are_refused() {
        assert_eq!(
            Region::new("big", Pos::default(), Size::new(1 << 20, 1 << 10, 1)),
            Err(ModelError::VolumeTooLarge(1 << 30))
        );
        assert_eq!(
            Region::from_blocks("r", Pos::default(), Size::new(2, 1, 1), vec![0]),
            Err(ModelError::BlockCountMismatch {
                expected: 2,
                actual: 1
            })
        );
    }

    #[test]
    fn volume_never_overflows() {
        assert_eq!(Size::new(u32::MAX, u32::MAX, u32::MAX).volume(), u64::MAX);
        assert!(Region::new("huge", Pos::default(), Size::new(u32::MAX, u32::MAX, 2)).is_err());
    }

    #[test]
    fn bounds_cover_all_regions() {
        let mut build = Build::new();
        assert_eq!(build.bounds(), None);
        build
            .regions
            .push(Region::new("a", Pos::new(0, 0, 0), Size::new(2, 2, 2)).unwrap());
        build
            .regions
            .push(Region::new("b", Pos::new(-3, 5, 1), Size::new(1, 1, 4)).unwrap());
        build
            .regions
            .push(Region::new("empty", Pos::new(100, 100, 100), Size::new(0, 1, 1)).unwrap());
        assert_eq!(
            build.bounds(),
            Some((Pos::new(-3, 0, 0), Pos::new(1, 5, 4)))
        );
    }

    #[test]
    fn validate_detects_unknown_palette_index() {
        let mut build = Build::new();
        let stone = build
            .palette
            .get_or_insert(BlockState::new("stone"))
            .unwrap();
        let mut region = Region::new("r", Pos::default(), Size::new(1, 1, 2)).unwrap();
        region.set(0, 0, 0, stone);
        build.regions.push(region);
        assert_eq!(build.validate(), Ok(()));
        build.regions[0].set(0, 0, 1, 9);
        assert_eq!(
            build.validate(),
            Err(ModelError::UnknownPaletteIndex { index: 9, len: 2 })
        );
    }
}
