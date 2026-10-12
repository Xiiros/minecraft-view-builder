//! Sponge schematics (`.schem`), the format of WorldEdit 7+ and FastAsyncWorldEdit.
//! Gzip-compressed NBT, in three versions:
//!
//! ```text
//! v1 and v2, root compound named "Schematic":
//!   Version: Int, DataVersion: Int (v2), Width / Height / Length: Short (read as unsigned)
//!   Offset: IntArray [x, y, z], Metadata: { Name, Author, Date, RequiredMods, ... }
//!   PaletteMax: Int, Palette: { "<block state>": Int }
//!   BlockData: ByteArray of varint palette indices, index = x + z * Width + y * Width * Length
//!   BlockEntities (v1: TileEntities): List of { Pos: IntArray [x, y, z], Id: String, <data> }
//!   Entities (v2): List of { Pos: List of 3 Double, Id: String, <data> }
//!   BiomePalette, BiomeData (v2, optional)
//! v3, unnamed root holding a "Schematic" compound:
//!   Version, DataVersion, Width / Height / Length, Offset, Metadata
//!   Blocks: { Palette, Data (varints), BlockEntities: List of { Pos, Id, Data: Compound } }
//!   Entities: List of { Pos, Id, Data: Compound }, Biomes: { Palette, Data }
//! ```
//!
//! Positions are relative to the schematic's minimum corner. Everything that is not part of the
//! block model (offset, extra metadata, biomes...) is kept in `Build::extra` under [`EXTRA_KEY`]
//! for a lossless re-export.

use crate::block::{BlockState, Palette};
use crate::build::{BlockEntity, Build, Entity, Pos, Region, Size, checked_volume};
use crate::formats::export::{ExportError, gzip, int, merged_region};
use crate::formats::{ImportError, decompress, double_triple, take};
use crate::nbt::{self, Compound, Tag};

/// Key of the format-specific data in `Build::extra`.
pub const EXTRA_KEY: &str = "sponge";

/// Palette indices above this are refused, which bounds the lookup table built while decoding.
const MAX_PALETTE_INDEX: usize = 1 << 20;

/// Reads a Sponge schematic of version 1, 2 or 3, gzip-compressed or not.
pub fn read(bytes: &[u8]) -> Result<Build, ImportError> {
    let document = decompress(bytes)?;
    let (_, mut root) = nbt::read(&document)?;
    if matches!(root.get("Schematic"), Some(Tag::Compound(_))) {
        root = take(&mut root, "Schematic", Tag::into_compound)?;
    }

    let version = take(&mut root, "Version", |t| t.as_int())?;
    if !(1..=3).contains(&version) {
        return Err(ImportError::UnsupportedVersion(version));
    }

    let mut build = Build::new();
    build.data_version = match version {
        // Version 1 predates DataVersion; 0 means unknown.
        1 => root
            .remove("DataVersion")
            .and_then(|t| t.as_int())
            .unwrap_or(0),
        _ => take(&mut root, "DataVersion", |t| t.as_int())?,
    };

    let size = Size::new(
        dimension(&mut root, "Width")?,
        dimension(&mut root, "Height")?,
        dimension(&mut root, "Length")?,
    );
    let volume = checked_volume(size)?;

    if let Some(mut metadata) = root.remove("Metadata").and_then(Tag::into_compound) {
        if let Some(name) = metadata.remove("Name").and_then(Tag::into_string) {
            build.metadata.name = name;
        }
        if let Some(author) = metadata.remove("Author").and_then(Tag::into_string) {
            build.metadata.author = author;
        }
        if let Some(description) = metadata.remove("Description").and_then(Tag::into_string) {
            build.metadata.description = description;
        }
        if !metadata.is_empty() {
            root.insert("Metadata", metadata);
        }
    }

    let (palette, data, block_entities) = if version == 3 {
        let mut blocks = take(&mut root, "Blocks", Tag::into_compound)?;
        let parts = (
            take(&mut blocks, "Palette", Tag::into_compound)?,
            take(&mut blocks, "Data", Tag::into_byte_array)?,
            blocks.remove("BlockEntities"),
        );
        if !blocks.is_empty() {
            root.insert("Blocks", blocks);
        }
        parts
    } else {
        root.remove("PaletteMax");
        let block_entities_key = if version == 1 {
            "TileEntities"
        } else {
            "BlockEntities"
        };
        (
            take(&mut root, "Palette", Tag::into_compound)?,
            take(&mut root, "BlockData", Tag::into_byte_array)?,
            root.remove(block_entities_key),
        )
    };

    let state_map = palette_map(palette, &mut build.palette)?;
    let blocks = decode_blocks(&data, volume, &state_map)?;
    let mut region = Region::from_blocks("", Pos::default(), size, blocks)?;

    if let Some(list) = block_entities {
        region.block_entities = list
            .into_list()
            .ok_or(ImportError::Field("BlockEntities"))?
            .into_iter()
            .map(|tag| block_entity(tag, version).ok_or(ImportError::Field("BlockEntities")))
            .collect::<Result<_, _>>()?;
    }
    if let Some(list) = root.remove("Entities") {
        region.entities = list
            .into_list()
            .ok_or(ImportError::Field("Entities"))?
            .into_iter()
            .map(|tag| entity(tag, version).ok_or(ImportError::Field("Entities")))
            .collect::<Result<_, _>>()?;
    }

    // What is left (offset, biomes, extra metadata...) is kept for re-export.
    root.insert("Version", version);
    build.extra.insert(EXTRA_KEY, root);
    build.regions.push(region);
    Ok(build)
}

/// Writes a gzip-compressed schematic: version 2 (read by every WorldEdit 7 and
/// FastAsyncWorldEdit), or version 3 when the build was read from a version 3 file. Regions are
/// merged. Offset, metadata and biomes read from a schematic are written back.
pub fn write(build: &Build) -> Result<Vec<u8>, ExportError> {
    let mut extra = build
        .extra
        .get(EXTRA_KEY)
        .and_then(Tag::as_compound)
        .cloned()
        .unwrap_or_default();
    let version = match extra.remove("Version").and_then(|t| t.as_int()) {
        Some(3) => 3,
        _ => 2,
    };
    let region = merged_region(build)?;
    let size = region.size();
    let dimension = |value: u32| {
        u16::try_from(value)
            .map(|v| Tag::Short(v as i16))
            .map_err(|_| ExportError::TooLarge("a schematic is at most 65535 blocks wide"))
    };

    // Only the states used, in order of first use, as varint indices.
    let mut palette = Compound::new();
    let mut file_index: Vec<Option<u32>> = vec![None; build.palette.len()];
    let mut data = Vec::with_capacity(region.blocks().len());
    for &block in region.blocks() {
        let index = *file_index[usize::from(block)].get_or_insert_with(|| {
            let state = build
                .palette
                .get(block)
                .cloned()
                .unwrap_or_else(BlockState::air);
            let index = palette.len() as u32;
            palette.insert(state.to_string(), index as i32);
            index
        });
        write_varint(&mut data, index);
    }

    let nested = version == 3;
    let block_entities: Vec<Tag> = region
        .block_entities
        .iter()
        .map(|be| {
            let pos = Tag::IntArray(vec![be.pos.x, be.pos.y, be.pos.z]);
            Tag::Compound(with_position(pos, &be.id, &be.data, nested))
        })
        .collect();
    let entities: Vec<Tag> = region
        .entities
        .iter()
        .map(|e| {
            let pos = Tag::List(e.pos.map(Tag::Double).to_vec());
            Tag::Compound(with_position(pos, &e.id, &e.data, nested))
        })
        .collect();

    let mut metadata = extra
        .remove("Metadata")
        .and_then(Tag::into_compound)
        .unwrap_or_default();
    for (key, value) in [
        ("Name", &build.metadata.name),
        ("Author", &build.metadata.author),
        ("Description", &build.metadata.description),
    ] {
        if !value.is_empty() {
            metadata.insert(key, value.as_str());
        }
    }
    let offset = extra
        .remove("Offset")
        .unwrap_or_else(|| Tag::IntArray(vec![0; 3]));

    // Format-specific data read from a schematic comes first, the build's own data overrides it.
    let mut schematic = extra;
    schematic.insert("Version", version);
    schematic.insert("DataVersion", build.data_version);
    schematic.insert("Width", dimension(size.x)?);
    schematic.insert("Height", dimension(size.y)?);
    schematic.insert("Length", dimension(size.z)?);
    schematic.insert("Offset", offset);
    schematic.insert("Metadata", metadata);
    if nested {
        let mut blocks = schematic
            .remove("Blocks")
            .and_then(Tag::into_compound)
            .unwrap_or_default();
        blocks.insert("Palette", palette);
        blocks.insert("Data", data);
        blocks.insert("BlockEntities", block_entities);
        schematic.insert("Blocks", blocks);
        schematic.insert("Entities", entities);
        let mut root = Compound::new();
        root.insert("Schematic", schematic);
        gzip(&nbt::write("", &root)?)
    } else {
        schematic.insert("PaletteMax", int(palette.len() as u32));
        schematic.insert("Palette", palette);
        schematic.insert("BlockData", data);
        schematic.insert("BlockEntities", block_entities);
        schematic.insert("Entities", entities);
        gzip(&nbt::write("Schematic", &schematic)?)
    }
}

/// `{ Pos, Id, <data> }` (version 2) or `{ Pos, Id, Data: <data> }` (version 3).
fn with_position(pos: Tag, id: &str, data: &Compound, nested: bool) -> Compound {
    let mut entry = if nested {
        let mut entry = Compound::new();
        entry.insert("Data", data.clone());
        entry
    } else {
        data.clone()
    };
    entry.insert("Pos", pos);
    entry.insert("Id", id);
    entry
}

fn write_varint(out: &mut Vec<u8>, mut value: u32) {
    while value >= 0x80 {
        out.push((value as u8 & 0x7F) | 0x80);
        value >>= 7;
    }
    out.push(value as u8);
}

/// Reads a dimension stored as a short that is meant as unsigned (up to 65535).
fn dimension(root: &mut Compound, key: &'static str) -> Result<u32, ImportError> {
    take(root, key, |tag| match tag {
        Tag::Short(v) => Some(u32::from(v as u16)),
        Tag::Int(v) => u32::try_from(v).ok(),
        _ => None,
    })
}

/// Adds the file's palette to the build palette and returns the lookup table
/// file index -> build index.
fn palette_map(
    palette: Compound,
    build_palette: &mut Palette,
) -> Result<Vec<Option<u16>>, ImportError> {
    let mut map = Vec::new();
    for (key, value) in palette {
        let index = value
            .as_int()
            .and_then(|i| usize::try_from(i).ok())
            .filter(|&i| i < MAX_PALETTE_INDEX)
            .ok_or(ImportError::Field("Palette"))?;
        let state: BlockState = key.parse()?;
        if index >= map.len() {
            map.resize(index + 1, None);
        }
        map[index] = Some(build_palette.get_or_insert(state)?);
    }
    Ok(map)
}

/// Decodes the varint (LEB128) palette indices of exactly `volume` blocks.
fn decode_blocks(
    data: &[u8],
    volume: usize,
    state_map: &[Option<u16>],
) -> Result<Vec<u16>, ImportError> {
    const INVALID: ImportError = ImportError::Field("BlockData");
    // Each block takes at least one byte: reject before allocating.
    if data.len() < volume {
        return Err(INVALID);
    }
    let mut blocks = Vec::with_capacity(volume);
    let mut value: u32 = 0;
    let mut shift = 0;
    for &byte in data {
        value |= u32::from(byte & 0x7F) << shift;
        if byte & 0x80 != 0 {
            shift += 7;
            if shift > 28 {
                return Err(INVALID);
            }
            continue;
        }
        if blocks.len() == volume {
            return Err(INVALID);
        }
        let state = state_map
            .get(value as usize)
            .copied()
            .flatten()
            .ok_or(INVALID)?;
        blocks.push(state);
        value = 0;
        shift = 0;
    }
    if shift != 0 || blocks.len() != volume {
        return Err(INVALID);
    }
    Ok(blocks)
}

/// Removes keys duplicated by the fields of `BlockEntity` / `Entity`.
fn strip_position_and_id(data: &mut Compound) {
    for key in ["id", "Id", "Pos", "x", "y", "z"] {
        data.remove(key);
    }
}

/// v1/v2 store the data next to `Pos` and `Id`; v3 nests it in `Data`.
fn split_data(mut entry: Compound, version: i32) -> Option<Compound> {
    let mut data = if version == 3 {
        match entry.remove("Data") {
            Some(data) => data.into_compound()?,
            None => Compound::new(),
        }
    } else {
        entry.remove("ContentVersion");
        entry
    };
    strip_position_and_id(&mut data);
    Some(data)
}

fn block_entity(tag: Tag, version: i32) -> Option<BlockEntity> {
    let mut entry = tag.into_compound()?;
    let &[x, y, z] = entry.remove("Pos")?.as_int_array()? else {
        return None;
    };
    let id = entry.remove("Id")?.into_string()?;
    Some(BlockEntity {
        pos: Pos::new(x, y, z),
        id,
        data: split_data(entry, version)?,
    })
}

fn entity(tag: Tag, version: i32) -> Option<Entity> {
    let mut entry = tag.into_compound()?;
    let pos = double_triple(&entry.remove("Pos")?)?;
    let id = entry.remove("Id")?.into_string()?;
    Some(Entity {
        pos,
        id,
        data: split_data(entry, version)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::formats::test_util::gzip;

    fn varints(values: &[u32]) -> Vec<u8> {
        let mut out = Vec::new();
        for &value in values {
            let mut value = value;
            while value >= 0x80 {
                out.push((value as u8 & 0x7F) | 0x80);
                value >>= 7;
            }
            out.push(value as u8);
        }
        out
    }

    fn compound<const N: usize>(entries: [(&str, Tag); N]) -> Compound {
        entries.into_iter().collect()
    }

    fn palette() -> Compound {
        compound([
            ("minecraft:air", Tag::Int(0)),
            ("minecraft:oak_stairs[half=bottom,facing=east]", Tag::Int(1)),
            ("minecraft:chest[facing=north]", Tag::Int(2)),
        ])
    }

    /// 2x2x1: stairs and a chest on the bottom layer, air above. x varies fastest.
    fn block_data() -> Vec<u8> {
        varints(&[1, 2, 0, 0])
    }

    fn header(version: i32) -> Compound {
        compound([
            ("Version", Tag::Int(version)),
            ("DataVersion", Tag::Int(3953)),
            ("Width", Tag::Short(2)),
            ("Height", Tag::Short(2)),
            ("Length", Tag::Short(1)),
            ("Offset", Tag::IntArray(vec![10, 64, -5])),
            (
                "Metadata",
                Tag::Compound(compound([
                    ("Name", Tag::from("Tour")),
                    ("Author", Tag::from("Alexis")),
                    ("Date", Tag::Long(1_700_000_000_000)),
                ])),
            ),
        ])
    }

    fn v2() -> Compound {
        let mut root = header(2);
        root.insert("PaletteMax", 3);
        root.insert("Palette", palette());
        root.insert("BlockData", block_data());
        root.insert(
            "BlockEntities",
            vec![Tag::Compound(compound([
                ("Pos", Tag::IntArray(vec![1, 0, 0])),
                ("Id", Tag::from("minecraft:chest")),
                ("Items", Tag::List(Vec::new())),
            ]))],
        );
        root.insert(
            "Entities",
            vec![Tag::Compound(compound([
                (
                    "Pos",
                    Tag::List(vec![Tag::Double(0.5), Tag::Double(1.0), Tag::Double(0.5)]),
                ),
                ("Id", Tag::from("minecraft:armor_stand")),
                ("Invisible", Tag::Byte(1)),
            ]))],
        );
        root
    }

    fn v3() -> Compound {
        let mut schematic = header(3);
        schematic.insert(
            "Blocks",
            compound([
                ("Palette", Tag::Compound(palette())),
                ("Data", Tag::ByteArray(block_data())),
                (
                    "BlockEntities",
                    Tag::List(vec![Tag::Compound(compound([
                        ("Pos", Tag::IntArray(vec![1, 0, 0])),
                        ("Id", Tag::from("minecraft:chest")),
                        (
                            "Data",
                            Tag::Compound(compound([("Items", Tag::List(Vec::new()))])),
                        ),
                    ]))]),
                ),
            ]),
        );
        schematic.insert(
            "Entities",
            vec![Tag::Compound(compound([
                (
                    "Pos",
                    Tag::List(vec![Tag::Double(0.5), Tag::Double(1.0), Tag::Double(0.5)]),
                ),
                ("Id", Tag::from("minecraft:armor_stand")),
                (
                    "Data",
                    Tag::Compound(compound([("Invisible", Tag::Byte(1))])),
                ),
            ]))],
        );
        compound([("Schematic", Tag::Compound(schematic))])
    }

    fn encode(root: &Compound) -> Vec<u8> {
        gzip(&nbt::write("Schematic", root).unwrap())
    }

    fn name_at(build: &Build, x: u32, y: u32, z: u32) -> String {
        let index = build.regions[0].get(x, y, z).unwrap();
        build.palette.get(index).unwrap().to_string()
    }

    fn check_sample(build: &Build) {
        assert_eq!(build.metadata.name, "Tour");
        assert_eq!(build.metadata.author, "Alexis");
        let region = &build.regions[0];
        assert_eq!(region.size(), Size::new(2, 2, 1));
        assert_eq!(
            name_at(build, 0, 0, 0),
            "minecraft:oak_stairs[facing=east,half=bottom]"
        );
        assert_eq!(name_at(build, 1, 0, 0), "minecraft:chest[facing=north]");
        assert_eq!(name_at(build, 0, 1, 0), "minecraft:air");
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
    fn reads_version_2() {
        let build = read(&encode(&v2())).unwrap();
        assert_eq!(build.data_version, 3953);
        check_sample(&build);
    }

    #[test]
    fn reads_version_3() {
        let build = read(&encode(&v3())).unwrap();
        assert_eq!(build.data_version, 3953);
        check_sample(&build);
    }

    #[test]
    fn reads_version_1() {
        let mut root = v2();
        root.insert("Version", 1);
        root.remove("DataVersion");
        root.remove("Entities");
        let mut tile_entities = root.remove("BlockEntities").unwrap().into_list().unwrap();
        if let Tag::Compound(chest) = &mut tile_entities[0] {
            chest.insert("ContentVersion", 1);
        }
        root.insert("TileEntities", tile_entities);
        let build = read(&encode(&root)).unwrap();
        assert_eq!(build.data_version, 0);
        assert_eq!(
            build.regions[0].block_entities[0]
                .data
                .get("ContentVersion"),
            None
        );
        assert_eq!(name_at(&build, 1, 0, 0), "minecraft:chest[facing=north]");
    }

    #[test]
    fn keeps_offset_metadata_and_version_for_export() {
        let build = read(&encode(&v3())).unwrap();
        let extra = build
            .extra
            .get(EXTRA_KEY)
            .and_then(Tag::as_compound)
            .unwrap();
        assert_eq!(extra.get("Version"), Some(&Tag::Int(3)));
        assert_eq!(extra.get("Offset"), Some(&Tag::IntArray(vec![10, 64, -5])));
        let metadata = extra.get("Metadata").and_then(Tag::as_compound).unwrap();
        assert_eq!(metadata.get("Date"), Some(&Tag::Long(1_700_000_000_000)));
        assert_eq!(metadata.get("Name"), None);
    }

    #[test]
    fn decodes_multi_byte_indices() {
        // 300 states: indices >= 128 take two varint bytes.
        let mut root = v2();
        let palette: Compound = (0..300)
            .map(|i| {
                let name = if i == 0 {
                    "minecraft:air".to_owned()
                } else {
                    format!("minecraft:block_{i}")
                };
                (name, Tag::Int(i))
            })
            .collect();
        root.insert("Palette", palette);
        root.insert("Width", Tag::Short(300));
        root.insert("Height", Tag::Short(1));
        root.insert("BlockData", varints(&(0..300).collect::<Vec<_>>()));
        root.remove("BlockEntities");
        let build = read(&encode(&root)).unwrap();
        assert_eq!(name_at(&build, 299, 0, 0), "minecraft:block_299");
        assert_eq!(name_at(&build, 128, 0, 0), "minecraft:block_128");
    }

    #[test]
    fn dimensions_are_unsigned_shorts() {
        let mut root = compound([("Width", Tag::Short(-25536))]);
        assert_eq!(dimension(&mut root, "Width").unwrap(), 40000);
    }

    #[test]
    fn rejects_invalid_block_data() {
        let map = [Some(0), Some(1)];
        let invalid = |data: &[u8], volume| {
            matches!(
                decode_blocks(data, volume, &map),
                Err(ImportError::Field("BlockData"))
            )
        };
        assert!(invalid(&[0, 1], 3), "too few blocks");
        assert!(invalid(&[0, 1, 1], 2), "too many blocks");
        assert!(invalid(&[0, 5], 2), "index outside the palette");
        assert!(invalid(&[0, 0x81], 2), "truncated varint");
        assert!(
            invalid(&[0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x01], 1),
            "varint too long"
        );
        assert_eq!(decode_blocks(&[1, 0], 2, &map).unwrap(), vec![1, 0]);
    }

    #[test]
    fn rejects_bad_headers() {
        let mut future = v2();
        future.insert("Version", 4);
        assert!(matches!(
            read(&encode(&future)),
            Err(ImportError::UnsupportedVersion(4))
        ));

        let mut no_data_version = v2();
        no_data_version.remove("DataVersion");
        assert!(matches!(
            read(&encode(&no_data_version)),
            Err(ImportError::Field("DataVersion"))
        ));

        let mut bad_state = v2();
        bad_state.insert(
            "Palette",
            compound([("minecraft:air", Tag::Int(0)), ("not a block", Tag::Int(1))]),
        );
        assert!(matches!(
            read(&encode(&bad_state)),
            Err(ImportError::BlockState(_))
        ));

        let mut huge_index = v2();
        huge_index.insert("Palette", compound([("minecraft:air", Tag::Int(i32::MAX))]));
        assert!(matches!(
            read(&encode(&huge_index)),
            Err(ImportError::Field("Palette"))
        ));
    }

    #[test]
    fn writer_varints_match_the_reader() {
        let mut data = Vec::new();
        for value in [0, 127, 128, 300, 70_000] {
            write_varint(&mut data, value);
        }
        assert_eq!(data, varints(&[0, 127, 128, 300, 70_000]));
    }

    #[test]
    fn written_files_read_back_the_same() {
        for (root, version) in [(v2(), 2), (v3(), 3)] {
            let original = read(&encode(&root)).unwrap();
            let reread = read(&write(&original).unwrap()).unwrap();
            assert!(reread.same_content(&original), "version {version}");
            assert_eq!(reread.metadata, original.metadata);
            let extra = reread
                .extra
                .get(EXTRA_KEY)
                .and_then(Tag::as_compound)
                .unwrap();
            assert_eq!(extra.get("Version"), Some(&Tag::Int(version)));
            assert_eq!(extra.get("Offset"), Some(&Tag::IntArray(vec![10, 64, -5])));
        }
    }

    #[test]
    fn builds_from_other_formats_are_written_as_version_2() {
        let mut build = read(&encode(&v3())).unwrap();
        build.extra = Compound::new();
        build.metadata.description = "Une tour".into();
        let reread = read(&write(&build).unwrap()).unwrap();
        assert!(reread.same_content(&build));
        assert_eq!(reread.metadata.description, "Une tour");
        let extra = reread
            .extra
            .get(EXTRA_KEY)
            .and_then(Tag::as_compound)
            .unwrap();
        assert_eq!(extra.get("Version"), Some(&Tag::Int(2)));
    }

    #[test]
    fn every_truncation_is_an_error() {
        for root in [v2(), v3()] {
            let bytes = encode(&root);
            for len in 0..bytes.len() {
                assert!(read(&bytes[..len]).is_err(), "truncated at {len}");
            }
        }
    }
}
