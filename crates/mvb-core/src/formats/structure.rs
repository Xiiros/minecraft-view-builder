//! Vanilla structure files (`.nbt`), saved by structure blocks and used by data packs and the
//! Create mod's schematics. Gzip-compressed NBT:
//!
//! ```text
//! DataVersion: Int
//! size: List of 3 Int
//! palette: List of { Name: String, Properties: { <key>: String } }
//!   or palettes: List of palettes (variants of one structure, e.g. shipwreck woods); the first is used
//! blocks: List of { state: Int (palette index), pos: List of 3 Int, nbt: Compound (block entity, optional) }
//! entities: List of { pos: List of 3 Double, blockPos: List of 3 Int, nbt: Compound (entity, with id) }
//! author: String (optional, old files)
//! ```
//!
//! Positions missing from `blocks` are structure voids: placing the structure leaves the world
//! untouched there. They are imported as `minecraft:structure_void` so that nothing is lost.
//! An entity's `blockPos` is not kept: it can be recomputed from its position when exporting.

use std::collections::HashMap;

use crate::block::{BlockState, Palette};
use crate::build::{BlockEntity, Build, Entity, Pos, Region, Size, checked_volume};
use crate::formats::export::{ExportError, gzip, int, merged_region};
use crate::formats::{ImportError, decompress, double_triple, int_triple, take};
use crate::nbt::{self, Compound, Tag};

/// Reads a structure file, gzip-compressed or not.
pub fn read(bytes: &[u8]) -> Result<Build, ImportError> {
    let document = decompress(bytes)?;
    let (_, mut root) = nbt::read(&document)?;

    let mut build = Build::new();
    build.data_version = take(&mut root, "DataVersion", |t| t.as_int())?;
    if let Some(author) = root.remove("author").and_then(Tag::into_string) {
        build.metadata.author = author;
    }

    let size = take(&mut root, "size", |t| {
        let [x, y, z] = int_triple(&t)?;
        Some(Size::new(
            u32::try_from(x).ok()?,
            u32::try_from(y).ok()?,
            u32::try_from(z).ok()?,
        ))
    })?;
    let volume = checked_volume(size)?;

    let file_palette = match root.remove("palette") {
        Some(palette) => palette.into_list(),
        None => root
            .remove("palettes")
            .and_then(Tag::into_list)
            .and_then(|palettes| palettes.into_iter().next())
            .and_then(Tag::into_list),
    }
    .ok_or(ImportError::Field("palette"))?;
    // File palette index -> build palette index.
    let state_map = file_palette
        .into_iter()
        .map(|entry| {
            let state = entry
                .into_compound()
                .and_then(BlockState::from_nbt)
                .ok_or(ImportError::Field("palette"))?;
            Ok(build.palette.get_or_insert(state)?)
        })
        .collect::<Result<Vec<u16>, ImportError>>()?;

    let mut blocks = vec![Palette::AIR; volume];
    // One bit per position, set when the file defines a block there.
    let mut defined = vec![0u64; volume.div_ceil(64)];
    let mut block_entities = Vec::new();

    for entry in take(&mut root, "blocks", Tag::into_list)? {
        let mut entry = entry.into_compound().ok_or(ImportError::Field("blocks"))?;
        let state = take(&mut entry, "state", |t| {
            let index = usize::try_from(t.as_int()?).ok()?;
            state_map.get(index).copied()
        })?;
        let pos = take(&mut entry, "pos", |t| int_triple(&t))?;
        let index = local_index(pos, size).ok_or(ImportError::Field("pos"))?;
        blocks[index] = state;
        defined[index / 64] |= 1 << (index % 64);

        if let Some(mut data) = entry.remove("nbt").and_then(Tag::into_compound) {
            let id = data
                .remove("id")
                .and_then(Tag::into_string)
                .unwrap_or_default();
            for key in ["x", "y", "z"] {
                data.remove(key);
            }
            block_entities.push(BlockEntity {
                pos: Pos::new(pos[0], pos[1], pos[2]),
                id,
                data,
            });
        }
    }

    let undefined = |i: usize| defined[i / 64] & (1 << (i % 64)) == 0;
    if (0..volume).any(undefined) {
        let void = build
            .palette
            .get_or_insert(BlockState::new("minecraft:structure_void"))?;
        for (i, block) in blocks.iter_mut().enumerate() {
            if undefined(i) {
                *block = void;
            }
        }
    }

    let mut entities = Vec::new();
    // Absent in some third-party files: no entities then.
    if let Some(list) = root.remove("entities") {
        for entry in list.into_list().ok_or(ImportError::Field("entities"))? {
            let mut entry = entry
                .into_compound()
                .ok_or(ImportError::Field("entities"))?;
            let pos = take(&mut entry, "pos", |t| double_triple(&t))?;
            let mut data = take(&mut entry, "nbt", Tag::into_compound)?;
            let id = take(&mut data, "id", Tag::into_string)?;
            // World position at save time; the structure-relative `pos` above replaces it.
            data.remove("Pos");
            entities.push(Entity { pos, id, data });
        }
    }

    let mut region = Region::from_blocks("", Pos::default(), size, blocks)?;
    region.block_entities = block_entities;
    region.entities = entities;
    build.regions.push(region);
    Ok(build)
}

/// Writes a gzip-compressed structure file. Regions are merged; structure voids are left out of
/// `blocks`, like the game does; an entity's `blockPos` is the block containing it.
pub fn write(build: &Build) -> Result<Vec<u8>, ExportError> {
    let region = merged_region(build)?;
    let size = region.size();
    let void = build
        .palette
        .index_of(&BlockState::new("minecraft:structure_void"));
    let block_entities: HashMap<Pos, &BlockEntity> = region
        .block_entities
        .iter()
        .map(|be| (be.pos, be))
        .collect();

    // Only the states used, in order of first use.
    let mut file_palette = Vec::new();
    let mut file_index: Vec<Option<i32>> = vec![None; build.palette.len()];
    let mut blocks = Vec::new();
    for y in 0..size.y {
        for z in 0..size.z {
            for x in 0..size.x {
                let block = region.get(x, y, z).unwrap_or(Palette::AIR);
                if Some(block) == void {
                    continue;
                }
                let state = *file_index[usize::from(block)].get_or_insert_with(|| {
                    let state = build
                        .palette
                        .get(block)
                        .cloned()
                        .unwrap_or_else(BlockState::air);
                    file_palette.push(Tag::Compound(state.to_nbt()));
                    file_palette.len() as i32 - 1
                });
                let pos = [int(x), int(y), int(z)];
                let mut entry = Compound::new();
                entry.insert("state", state);
                entry.insert("pos", ints(pos));
                if let Some(be) = block_entities.get(&Pos::new(pos[0], pos[1], pos[2])) {
                    let mut data = be.data.clone();
                    if !be.id.is_empty() {
                        data.insert("id", be.id.as_str());
                    }
                    entry.insert("nbt", data);
                }
                blocks.push(Tag::Compound(entry));
            }
        }
    }

    let entities: Vec<Tag> = region
        .entities
        .iter()
        .map(|e| {
            let mut data = e.data.clone();
            data.insert("id", e.id.as_str());
            data.insert("Pos", doubles(e.pos));
            let mut entry = Compound::new();
            entry.insert("pos", doubles(e.pos));
            entry.insert("blockPos", ints(e.pos.map(|p| p.floor() as i32)));
            entry.insert("nbt", data);
            Tag::Compound(entry)
        })
        .collect();

    let mut root = Compound::new();
    root.insert("DataVersion", build.data_version);
    root.insert("size", ints([int(size.x), int(size.y), int(size.z)]));
    root.insert("palette", file_palette);
    root.insert("blocks", blocks);
    root.insert("entities", entities);
    if !build.metadata.author.is_empty() {
        root.insert("author", build.metadata.author.as_str());
    }
    gzip(&nbt::write("", &root)?)
}

fn ints(values: [i32; 3]) -> Tag {
    Tag::List(values.map(Tag::Int).to_vec())
}

fn doubles(values: [f64; 3]) -> Tag {
    Tag::List(values.map(Tag::Double).to_vec())
}

/// Index in a region's block array, if the position is inside `size`.
fn local_index([x, y, z]: [i32; 3], size: Size) -> Option<usize> {
    let (x, y, z) = (
        u32::try_from(x).ok()?,
        u32::try_from(y).ok()?,
        u32::try_from(z).ok()?,
    );
    (x < size.x && y < size.y && z < size.z)
        .then(|| (y as usize * size.z as usize + z as usize) * size.x as usize + x as usize)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::formats::test_util::gzip;
    use crate::nbt::Compound;

    fn ints(values: [i32; 3]) -> Tag {
        Tag::List(values.map(Tag::Int).to_vec())
    }

    fn state(name: &str, properties: &[(&str, &str)]) -> Tag {
        Tag::Compound(BlockState::with_properties(name, properties.iter().copied()).to_nbt())
    }

    fn block(state: i32, pos: [i32; 3]) -> Compound {
        [("state", Tag::Int(state)), ("pos", ints(pos))]
            .into_iter()
            .collect()
    }

    /// A 2x2x1 structure: stairs, a chest, explicit air, and one undefined (void) position.
    fn sample() -> Compound {
        let mut chest = block(2, [1, 0, 0]);
        chest.insert(
            "nbt",
            [
                ("id", Tag::from("minecraft:chest")),
                ("Items", Tag::List(Vec::new())),
            ]
            .into_iter()
            .collect::<Compound>(),
        );

        let frame: Compound = [
            ("id", Tag::from("minecraft:item_frame")),
            ("Pos", Tag::List(vec![Tag::Double(105.5); 3])),
            ("Facing", Tag::Byte(2)),
        ]
        .into_iter()
        .collect();
        let entity: Compound = [
            (
                "pos",
                Tag::List(vec![
                    Tag::Double(0.5),
                    Tag::Double(1.5),
                    Tag::Double(0.03125),
                ]),
            ),
            ("blockPos", ints([0, 1, 0])),
            ("nbt", Tag::Compound(frame)),
        ]
        .into_iter()
        .collect();

        [
            ("DataVersion", Tag::Int(3953)),
            ("size", ints([2, 2, 1])),
            (
                "palette",
                Tag::List(vec![
                    state("minecraft:air", &[]),
                    state(
                        "minecraft:oak_stairs",
                        &[("half", "bottom"), ("facing", "east")],
                    ),
                    state("minecraft:chest", &[("facing", "north")]),
                ]),
            ),
            (
                "blocks",
                Tag::List(vec![
                    Tag::Compound(block(1, [0, 0, 0])),
                    Tag::Compound(chest),
                    Tag::Compound(block(0, [0, 1, 0])),
                ]),
            ),
            ("entities", Tag::List(vec![Tag::Compound(entity)])),
            ("author", Tag::from("Alexis")),
        ]
        .into_iter()
        .collect()
    }

    fn encode(root: &Compound) -> Vec<u8> {
        gzip(&nbt::write("", root).unwrap())
    }

    fn name_at(build: &Build, x: u32, y: u32, z: u32) -> String {
        let index = build.regions[0].get(x, y, z).unwrap();
        build.palette.get(index).unwrap().to_string()
    }

    #[test]
    fn reads_blocks_with_their_properties() {
        let build = read(&encode(&sample())).unwrap();
        assert_eq!(build.data_version, 3953);
        assert_eq!(build.metadata.author, "Alexis");
        assert_eq!(build.regions.len(), 1);
        assert_eq!(build.regions[0].size(), Size::new(2, 2, 1));
        assert_eq!(
            name_at(&build, 0, 0, 0),
            "minecraft:oak_stairs[facing=east,half=bottom]"
        );
        assert_eq!(name_at(&build, 1, 0, 0), "minecraft:chest[facing=north]");
        assert_eq!(name_at(&build, 0, 1, 0), "minecraft:air");
        assert_eq!(build.validate(), Ok(()));
    }

    #[test]
    fn undefined_positions_become_structure_void() {
        let build = read(&encode(&sample())).unwrap();
        assert_eq!(name_at(&build, 1, 1, 0), "minecraft:structure_void");
    }

    #[test]
    fn fully_defined_structure_has_no_void() {
        let mut root = sample();
        root.insert("size", ints([1, 1, 1]));
        root.insert(
            "blocks",
            Tag::List(vec![Tag::Compound(block(1, [0, 0, 0]))]),
        );
        let build = read(&encode(&root)).unwrap();
        let void = BlockState::new("structure_void");
        assert_eq!(build.palette.index_of(&void), None);
    }

    #[test]
    fn keeps_block_entities_and_entities() {
        let build = read(&encode(&sample())).unwrap();
        let region = &build.regions[0];

        let [chest] = region.block_entities.as_slice() else {
            panic!("expected one block entity");
        };
        assert_eq!(chest.pos, Pos::new(1, 0, 0));
        assert_eq!(chest.id, "minecraft:chest");
        assert_eq!(chest.data.get("Items"), Some(&Tag::List(Vec::new())));
        assert_eq!(chest.data.get("id"), None);

        let [frame] = region.entities.as_slice() else {
            panic!("expected one entity");
        };
        assert_eq!(frame.id, "minecraft:item_frame");
        assert_eq!(frame.pos, [0.5, 1.5, 0.03125]);
        assert_eq!(frame.data.get("Facing"), Some(&Tag::Byte(2)));
        assert_eq!(frame.data.get("Pos"), None);
    }

    #[test]
    fn reads_uncompressed_files() {
        let bytes = nbt::write("", &sample()).unwrap();
        assert_eq!(read(&bytes).unwrap(), read(&encode(&sample())).unwrap());
    }

    #[test]
    fn uses_the_first_palette_variant() {
        let mut root = sample();
        let palette = root.remove("palette").unwrap();
        let mut variant = palette.clone().into_list().unwrap();
        variant[1] = state(
            "minecraft:spruce_stairs",
            &[("facing", "east"), ("half", "bottom")],
        );
        root.insert("palettes", vec![palette, Tag::List(variant)]);
        let build = read(&encode(&root)).unwrap();
        assert_eq!(
            name_at(&build, 0, 0, 0),
            "minecraft:oak_stairs[facing=east,half=bottom]"
        );
    }

    #[test]
    fn rejects_invalid_content() {
        let mut outside = sample();
        outside.insert(
            "blocks",
            Tag::List(vec![Tag::Compound(block(1, [2, 0, 0]))]),
        );
        assert!(matches!(
            read(&encode(&outside)),
            Err(ImportError::Field("pos"))
        ));

        let mut bad_state = sample();
        bad_state.insert(
            "blocks",
            Tag::List(vec![Tag::Compound(block(9, [0, 0, 0]))]),
        );
        assert!(matches!(
            read(&encode(&bad_state)),
            Err(ImportError::Field("state"))
        ));

        let mut negative = sample();
        negative.insert("size", ints([-1, 1, 1]));
        assert!(matches!(
            read(&encode(&negative)),
            Err(ImportError::Field("size"))
        ));

        let mut no_palette = sample();
        no_palette.remove("palette");
        assert!(matches!(
            read(&encode(&no_palette)),
            Err(ImportError::Field("palette"))
        ));
    }

    #[test]
    fn huge_size_is_refused_before_allocating() {
        let mut root = sample();
        root.insert("size", ints([i32::MAX, i32::MAX, i32::MAX]));
        assert!(matches!(read(&encode(&root)), Err(ImportError::Model(_))));
    }

    #[test]
    fn written_files_read_back_the_same() {
        let original = read(&encode(&sample())).unwrap();
        let written = write(&original).unwrap();
        let reread = read(&written).unwrap();
        assert!(reread.same_content(&original));
        assert_eq!(reread.data_version, original.data_version);
        assert_eq!(reread.metadata.author, "Alexis");
        // The structure void stays a void (left out of the file, not written as a block).
        let void = BlockState::new("minecraft:structure_void");
        let (_, root) = nbt::read(&decompress(&written).unwrap()).unwrap();
        let palette = root.get("palette").and_then(Tag::as_list).unwrap();
        assert!(!palette.contains(&Tag::Compound(void.to_nbt())));
        // The entity's block position is the block containing it.
        let entity = &root.get("entities").and_then(Tag::as_list).unwrap()[0];
        let block_pos = entity
            .as_compound()
            .and_then(|e| e.get("blockPos"))
            .unwrap();
        assert_eq!(block_pos, &ints([0, 1, 0]));
    }

    #[test]
    fn every_truncation_is_an_error() {
        let bytes = encode(&sample());
        for len in 0..bytes.len() {
            assert!(read(&bytes[..len]).is_err(), "truncated at {len}");
        }
    }
}
