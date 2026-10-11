//! Litematica schematics (`.litematic`), from the Litematica mod (Fabric / NeoForge).
//! Gzip-compressed NBT:
//!
//! ```text
//! Version: Int (format version, 1 to 7), SubVersion: Int (optional)
//! MinecraftDataVersion: Int
//! Metadata: { Name, Author, Description, TimeCreated: Long, TimeModified: Long, PreviewImageData,
//!             RegionCount, TotalBlocks, TotalVolume, EnclosingSize (derived values) }
//! Regions: { "<region name>": {
//!     Position: { x, y, z }   corner of the region, relative to the schematic origin
//!     Size: { x, y, z }       may be negative: the region then extends from Position
//!                             towards negative coordinates on that axis
//!     BlockStatePalette: List of { Name, Properties }
//!     BlockStates: LongArray  palette indices packed on max(2, ceil(log2(palette size))) bits,
//!                             an entry can straddle two longs; index = (y * |Size.z| + z) * |Size.x| + x
//!     TileEntities: List of { x, y, z: Int, id: String, <data> }
//!     Entities: List of { Pos: List of 3 Double, id: String, <data> }
//!     PendingBlockTicks, PendingFluidTicks: List
//! } }
//! ```
//!
//! Block and entity positions inside a region are relative to its minimum corner. Data that is not
//! part of the block model (timestamps, preview image, the region's signed geometry, scheduled
//! ticks...) is kept in `Build::extra` and `Region::extra` under [`EXTRA_KEY`].

use crate::block::{BlockState, Palette};
use crate::build::{BlockEntity, Build, Entity, Pos, Region, Size, checked_volume};
use crate::formats::{ImportError, decompress, double_triple, take};
use crate::nbt::{self, Compound, Tag};

/// Key of the format-specific data in `Build::extra` and `Region::extra`.
pub const EXTRA_KEY: &str = "litematica";

/// Latest format version this reader knows.
const MAX_VERSION: i32 = 7;
/// Data version of 17w47a, the snapshot that introduced today's block names ("the flattening").
const FLATTENING_DATA_VERSION: i32 = 1451;

/// Reads a Litematica schematic, gzip-compressed or not.
pub fn read(bytes: &[u8]) -> Result<Build, ImportError> {
    let document = decompress(bytes)?;
    let (_, mut root) = nbt::read(&document)?;

    let version = take(&mut root, "Version", |t| t.as_int())?;
    if !(1..=MAX_VERSION).contains(&version) {
        return Err(ImportError::UnsupportedVersion(version));
    }
    let mut build = Build::new();
    build.data_version = take(&mut root, "MinecraftDataVersion", |t| t.as_int())?;
    if build.data_version < FLATTENING_DATA_VERSION {
        return Err(ImportError::PreFlattening);
    }

    if let Some(mut metadata) = root.remove("Metadata").and_then(Tag::into_compound) {
        let mut text = |key| {
            metadata
                .remove(key)
                .and_then(Tag::into_string)
                .unwrap_or_default()
        };
        build.metadata.name = text("Name");
        build.metadata.author = text("Author");
        build.metadata.description = text("Description");
        // Derived from the regions, recomputed when exporting.
        for key in ["RegionCount", "TotalBlocks", "TotalVolume", "EnclosingSize"] {
            metadata.remove(key);
        }
        if !metadata.is_empty() {
            root.insert("Metadata", metadata);
        }
    }

    for (name, region) in take(&mut root, "Regions", Tag::into_compound)? {
        build
            .regions
            .push(read_region(name, region, &mut build.palette)?);
    }

    root.insert("Version", version);
    build.extra.insert(EXTRA_KEY, root);
    Ok(build)
}

fn xyz(tag: Tag) -> Option<[i32; 3]> {
    let c = tag.into_compound()?;
    Some([
        c.get("x")?.as_int()?,
        c.get("y")?.as_int()?,
        c.get("z")?.as_int()?,
    ])
}

fn xyz_compound([x, y, z]: [i32; 3]) -> Compound {
    [("x", x), ("y", y), ("z", z)].into_iter().collect()
}

fn read_region(name: String, tag: Tag, palette: &mut Palette) -> Result<Region, ImportError> {
    let mut entry = tag.into_compound().ok_or(ImportError::Field("Regions"))?;
    let position = take(&mut entry, "Position", xyz)?;
    let signed_size = take(&mut entry, "Size", xyz)?;

    // Minimum corner and absolute size on each axis.
    let mut origin = [0; 3];
    let mut size = [0; 3];
    for axis in 0..3 {
        let (p, s) = (position[axis], signed_size[axis]);
        size[axis] = s.unsigned_abs();
        origin[axis] = if s < 0 {
            p.checked_add(s + 1).ok_or(ImportError::Field("Position"))?
        } else {
            p
        };
    }
    let size = Size::new(size[0], size[1], size[2]);
    let volume = checked_volume(size)?;

    let state_map = take(&mut entry, "BlockStatePalette", Tag::into_list)?
        .into_iter()
        .map(|tag| {
            let state = tag
                .into_compound()
                .and_then(BlockState::from_nbt)
                .ok_or(ImportError::Field("BlockStatePalette"))?;
            Ok(palette.get_or_insert(state)?)
        })
        .collect::<Result<Vec<u16>, ImportError>>()?;
    let packed = take(&mut entry, "BlockStates", Tag::into_long_array)?;
    let blocks = unpack(&packed, volume, &state_map)?;

    let mut region = Region::from_blocks(
        name,
        Pos::new(origin[0], origin[1], origin[2]),
        size,
        blocks,
    )?;
    if let Some(list) = entry.remove("TileEntities") {
        region.block_entities = list
            .into_list()
            .ok_or(ImportError::Field("TileEntities"))?
            .into_iter()
            .map(|tag| block_entity(tag).ok_or(ImportError::Field("TileEntities")))
            .collect::<Result<_, _>>()?;
    }
    if let Some(list) = entry.remove("Entities") {
        region.entities = list
            .into_list()
            .ok_or(ImportError::Field("Entities"))?
            .into_iter()
            .map(|tag| entity(tag).ok_or(ImportError::Field("Entities")))
            .collect::<Result<_, _>>()?;
    }

    // The signed geometry tells which corner the region was defined from; keep it with the ticks.
    entry.insert("Position", xyz_compound(position));
    entry.insert("Size", xyz_compound(signed_size));
    region.extra.insert(EXTRA_KEY, entry);
    Ok(region)
}

/// Bits per entry used by Litematica for a palette of `len` states.
fn bits_per_entry(len: usize) -> usize {
    let bits = usize::BITS - len.saturating_sub(1).leading_zeros();
    (bits as usize).max(2)
}

/// Unpacks `volume` palette indices stored back to back on a fixed number of bits.
fn unpack(longs: &[i64], volume: usize, state_map: &[u16]) -> Result<Vec<u16>, ImportError> {
    const INVALID: ImportError = ImportError::Field("BlockStates");
    if volume == 0 {
        return Ok(Vec::new());
    }
    if state_map.is_empty() {
        return Err(INVALID);
    }
    let bits = bits_per_entry(state_map.len());
    let needed = (volume as u64 * bits as u64).div_ceil(64);
    if (longs.len() as u64) < needed {
        return Err(INVALID);
    }
    let mask = (1u64 << bits) - 1;
    let mut blocks = Vec::with_capacity(volume);
    for i in 0..volume {
        let bit = i * bits;
        let (word, offset) = (bit / 64, bit % 64);
        let mut value = longs[word] as u64 >> offset;
        if offset + bits > 64 {
            value |= (longs[word + 1] as u64) << (64 - offset);
        }
        let index = (value & mask) as usize;
        blocks.push(*state_map.get(index).ok_or(INVALID)?);
    }
    Ok(blocks)
}

fn block_entity(tag: Tag) -> Option<BlockEntity> {
    let mut data = tag.into_compound()?;
    let mut coordinate = |key| data.remove(key)?.as_int();
    let pos = Pos::new(coordinate("x")?, coordinate("y")?, coordinate("z")?);
    let id = data
        .remove("id")
        .and_then(Tag::into_string)
        .unwrap_or_default();
    Some(BlockEntity { pos, id, data })
}

fn entity(tag: Tag) -> Option<Entity> {
    let mut data = tag.into_compound()?;
    let pos = double_triple(&data.remove("Pos")?)?;
    let id = data.remove("id")?.into_string()?;
    Some(Entity { pos, id, data })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::formats::test_util::gzip;

    /// Inverse of `unpack`, as Litematica writes it.
    fn pack(indices: &[usize], palette_len: usize) -> Vec<i64> {
        let bits = bits_per_entry(palette_len);
        let mut longs = vec![0u64; (indices.len() * bits).div_ceil(64)];
        for (i, &index) in indices.iter().enumerate() {
            let bit = i * bits;
            let (word, offset) = (bit / 64, bit % 64);
            longs[word] |= (index as u64) << offset;
            if offset + bits > 64 {
                longs[word + 1] |= (index as u64) >> (64 - offset);
            }
        }
        longs.into_iter().map(|l| l as i64).collect()
    }

    fn compound<const N: usize>(entries: [(&str, Tag); N]) -> Compound {
        entries.into_iter().collect()
    }

    fn state(name: &str, properties: &[(&str, &str)]) -> Tag {
        Tag::Compound(BlockState::with_properties(name, properties.iter().copied()).to_nbt())
    }

    fn region(
        position: [i32; 3],
        size: [i32; 3],
        palette: Vec<Tag>,
        indices: &[usize],
    ) -> Compound {
        let palette_len = palette.len();
        compound([
            ("Position", Tag::Compound(xyz_compound(position))),
            ("Size", Tag::Compound(xyz_compound(size))),
            ("BlockStatePalette", Tag::List(palette)),
            ("BlockStates", Tag::LongArray(pack(indices, palette_len))),
            ("PendingBlockTicks", Tag::List(Vec::new())),
        ])
    }

    /// Region "main", 2x2x1: stairs and a chest at the bottom, air above, with a chest and an entity.
    fn main_region() -> Compound {
        let mut main = region(
            [0, 0, 0],
            [2, 2, 1],
            vec![
                state("minecraft:air", &[]),
                state(
                    "minecraft:oak_stairs",
                    &[("facing", "east"), ("half", "bottom")],
                ),
                state("minecraft:chest", &[("facing", "north")]),
            ],
            &[1, 2, 0, 0],
        );
        main.insert(
            "TileEntities",
            vec![Tag::Compound(compound([
                ("x", Tag::Int(1)),
                ("y", Tag::Int(0)),
                ("z", Tag::Int(0)),
                ("id", Tag::from("minecraft:chest")),
                ("Items", Tag::List(Vec::new())),
            ]))],
        );
        main.insert(
            "Entities",
            vec![Tag::Compound(compound([
                (
                    "Pos",
                    Tag::List(vec![Tag::Double(0.5), Tag::Double(1.0), Tag::Double(0.5)]),
                ),
                ("id", Tag::from("minecraft:armor_stand")),
                ("Invisible", Tag::Byte(1)),
            ]))],
        );
        main
    }

    fn document(regions: Compound) -> Compound {
        compound([
            ("Version", Tag::Int(6)),
            ("SubVersion", Tag::Int(1)),
            ("MinecraftDataVersion", Tag::Int(3700)),
            (
                "Metadata",
                Tag::Compound(compound([
                    ("Name", Tag::from("Tour")),
                    ("Author", Tag::from("Alexis")),
                    ("Description", Tag::from("Une tour")),
                    ("TimeCreated", Tag::Long(1_700_000_000_000)),
                    ("RegionCount", Tag::Int(1)),
                    ("TotalBlocks", Tag::Int(2)),
                ])),
            ),
            ("Regions", Tag::Compound(regions)),
        ])
    }

    fn sample() -> Compound {
        document(compound([("main", Tag::Compound(main_region()))]))
    }

    fn encode(root: &Compound) -> Vec<u8> {
        gzip(&nbt::write("", root).unwrap())
    }

    fn name_at(build: &Build, region: usize, x: u32, y: u32, z: u32) -> String {
        let index = build.regions[region].get(x, y, z).unwrap();
        build.palette.get(index).unwrap().to_string()
    }

    #[test]
    fn reads_blocks_metadata_and_entities() {
        let build = read(&encode(&sample())).unwrap();
        assert_eq!(build.data_version, 3700);
        assert_eq!(
            (
                build.metadata.name.as_str(),
                build.metadata.author.as_str(),
                build.metadata.description.as_str()
            ),
            ("Tour", "Alexis", "Une tour")
        );
        let region = &build.regions[0];
        assert_eq!(region.name, "main");
        assert_eq!(
            (region.origin, region.size()),
            (Pos::new(0, 0, 0), Size::new(2, 2, 1))
        );
        assert_eq!(
            name_at(&build, 0, 0, 0, 0),
            "minecraft:oak_stairs[facing=east,half=bottom]"
        );
        assert_eq!(name_at(&build, 0, 1, 0, 0), "minecraft:chest[facing=north]");
        assert_eq!(name_at(&build, 0, 1, 1, 0), "minecraft:air");
        assert_eq!(build.validate(), Ok(()));

        let [chest] = region.block_entities.as_slice() else {
            panic!("expected one block entity");
        };
        assert_eq!(
            (chest.pos, chest.id.as_str()),
            (Pos::new(1, 0, 0), "minecraft:chest")
        );
        assert_eq!(chest.data, compound([("Items", Tag::List(Vec::new()))]));

        let [stand] = region.entities.as_slice() else {
            panic!("expected one entity");
        };
        assert_eq!(
            (stand.pos, stand.id.as_str()),
            ([0.5, 1.0, 0.5], "minecraft:armor_stand")
        );
        assert_eq!(stand.data, compound([("Invisible", Tag::Byte(1))]));
    }

    #[test]
    fn keeps_extra_data_but_not_derived_values() {
        let build = read(&encode(&sample())).unwrap();
        let extra = build
            .extra
            .get(EXTRA_KEY)
            .and_then(Tag::as_compound)
            .unwrap();
        assert_eq!(extra.get("Version"), Some(&Tag::Int(6)));
        assert_eq!(extra.get("SubVersion"), Some(&Tag::Int(1)));
        let metadata = extra.get("Metadata").and_then(Tag::as_compound).unwrap();
        assert_eq!(
            metadata.get("TimeCreated"),
            Some(&Tag::Long(1_700_000_000_000))
        );
        assert_eq!(metadata.get("TotalBlocks"), None);
        assert_eq!(metadata.get("Name"), None);

        let region_extra = build.regions[0]
            .extra
            .get(EXTRA_KEY)
            .and_then(Tag::as_compound)
            .unwrap();
        assert_eq!(
            region_extra.get("PendingBlockTicks"),
            Some(&Tag::List(Vec::new()))
        );
        assert_eq!(
            region_extra.get("Size"),
            Some(&Tag::Compound(xyz_compound([2, 2, 1])))
        );
    }

    #[test]
    fn negative_sizes_extend_towards_negative_coordinates() {
        let palette = vec![state("minecraft:air", &[]), state("minecraft:stone", &[])];
        let regions = compound([(
            "r",
            Tag::Compound(region([5, 0, 5], [-2, 1, -3], palette, &[1; 6])),
        )]);
        let build = read(&encode(&document(regions))).unwrap();
        let region = &build.regions[0];
        assert_eq!(region.origin, Pos::new(4, 0, 3));
        assert_eq!(region.size(), Size::new(2, 1, 3));
        let extra = region
            .extra
            .get(EXTRA_KEY)
            .and_then(Tag::as_compound)
            .unwrap();
        assert_eq!(
            extra.get("Size"),
            Some(&Tag::Compound(xyz_compound([-2, 1, -3])))
        );
        assert_eq!(
            extra.get("Position"),
            Some(&Tag::Compound(xyz_compound([5, 0, 5])))
        );
    }

    #[test]
    fn regions_share_the_build_palette() {
        let other = region(
            [-3, 2, 0],
            [1, 1, 2],
            vec![
                state("minecraft:air", &[]),
                state("minecraft:glass", &[]),
                state("minecraft:chest", &[("facing", "north")]),
            ],
            &[1, 2],
        );
        let regions = compound([
            ("main", Tag::Compound(main_region())),
            ("annexe", Tag::Compound(other)),
        ]);
        let build = read(&encode(&document(regions))).unwrap();
        assert_eq!(build.regions.len(), 2);
        assert_eq!(build.regions[1].name, "annexe");
        assert_eq!(build.regions[1].origin, Pos::new(-3, 2, 0));
        // air, stairs, chest, glass: the chest is not duplicated.
        assert_eq!(build.palette.len(), 4);
        assert_eq!(build.regions[1].get(0, 0, 1), build.regions[0].get(1, 0, 0));
        assert_eq!(name_at(&build, 1, 0, 0, 0), "minecraft:glass");
    }

    #[test]
    fn bits_per_entry_matches_litematica() {
        let expected = [
            (1, 2),
            (2, 2),
            (4, 2),
            (5, 3),
            (8, 3),
            (9, 4),
            (16, 4),
            (17, 5),
            (256, 8),
            (257, 9),
        ];
        for (len, bits) in expected {
            assert_eq!(bits_per_entry(len), bits, "palette of {len}");
        }
    }

    #[test]
    fn entries_straddling_two_longs_are_decoded() {
        // 5 states -> 3 bits: entries 21 and 42 cross a long boundary.
        let indices: Vec<usize> = (0..100).map(|i| (i * 7 + i / 3) % 5).collect();
        let map: Vec<u16> = (0..5).collect();
        let decoded = unpack(&pack(&indices, 5), indices.len(), &map).unwrap();
        assert_eq!(
            decoded,
            indices.iter().map(|&i| i as u16).collect::<Vec<_>>()
        );
    }

    #[test]
    fn rejects_invalid_block_states() {
        let map = [0, 1, 2];
        let invalid = |longs: &[i64], volume| {
            matches!(
                unpack(longs, volume, &map),
                Err(ImportError::Field("BlockStates"))
            )
        };
        assert!(invalid(&[0], 33), "33 entries of 2 bits need two longs");
        assert!(
            invalid(&pack(&[3], 4), 1),
            "index outside a 3-entry palette"
        );
        assert!(
            matches!(unpack(&[], 1, &[]), Err(ImportError::Field("BlockStates"))),
            "empty palette"
        );
        assert_eq!(unpack(&[], 0, &map).unwrap(), Vec::<u16>::new());
    }

    #[test]
    fn rejects_unsupported_files() {
        let mut future = sample();
        future.insert("Version", 8);
        assert!(matches!(
            read(&encode(&future)),
            Err(ImportError::UnsupportedVersion(8))
        ));

        let mut old = sample();
        old.insert("MinecraftDataVersion", 1343);
        assert!(matches!(
            read(&encode(&old)),
            Err(ImportError::PreFlattening)
        ));

        let palette = vec![state("minecraft:air", &[])];
        let overflow = compound([(
            "r",
            Tag::Compound(region([i32::MIN, 0, 0], [-2, 1, 1], palette, &[0, 0])),
        )]);
        assert!(matches!(
            read(&encode(&document(overflow))),
            Err(ImportError::Field("Position"))
        ));
    }

    #[test]
    fn every_truncation_is_an_error() {
        let bytes = encode(&sample());
        for len in 0..bytes.len() {
            assert!(read(&bytes[..len]).is_err(), "truncated at {len}");
        }
    }
}
