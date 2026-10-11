//! Native project format (`.mvb`), which keeps every detail of a build.
//!
//! Layout: the 8-byte [`MAGIC`], the format version (`u16`, big-endian), then one zstd frame
//! holding an NBT document:
//!
//! ```text
//! DataVersion: Int
//! Metadata: { Name: String, Author: String, Description: String }
//! Palette: List of { Name: String, Properties: { <key>: String } (omitted when empty) }
//!          entry 0 is minecraft:air
//! Regions: List of {
//!     Name: String
//!     Origin: IntArray [x, y, z]
//!     Size: IntArray [x, y, z]
//!     Blocks: ByteArray    palette indices as u16 little-endian, index = (y * size_z + z) * size_x + x
//!     BlockEntities: List of { Pos: IntArray [x, y, z], Id: String, Data: Compound }
//!     Entities: List of { Pos: List of 3 Double, Id: String, Data: Compound }
//!     Extra: Compound
//! }
//! Extra: Compound
//! ```
//!
//! Positions inside a region are relative to its origin. `Extra` compounds hold format-specific
//! data so that a build re-exported to its original format loses nothing.

use std::io::Read;

use crate::block::{BlockState, Palette};
use crate::build::{BlockEntity, Build, Entity, Metadata, ModelError, Pos, Region, Size};
use crate::nbt::{self, Compound, NbtError, Tag};

/// File signature. The non-ASCII first byte and the line endings detect files damaged by text-mode transfers.
pub const MAGIC: [u8; 8] = *b"\x89MVB\r\n\x1a\n";
/// Latest format version written and read by this application.
pub const FORMAT_VERSION: u16 = 1;

const COMPRESSION_LEVEL: i32 = 9;
/// Refuse to inflate more than this, so a tiny corrupt or malicious file cannot exhaust memory.
const MAX_DECOMPRESSED_SIZE: u64 = 1 << 30;
const ROOT_NAME: &str = "mvb";

#[derive(Debug, thiserror::Error)]
pub enum MvbError {
    #[error("not a project file")]
    NotMvb,
    #[error(
        "project format version {0} is not supported (this application reads up to {FORMAT_VERSION})"
    )]
    UnsupportedVersion(u16),
    #[error("compression error: {0}")]
    Compression(#[from] std::io::Error),
    #[error("project data is larger than {MAX_DECOMPRESSED_SIZE} bytes once decompressed")]
    TooLarge,
    #[error(transparent)]
    Nbt(#[from] NbtError),
    #[error("missing or invalid field `{0}`")]
    Field(&'static str),
    #[error(transparent)]
    Model(#[from] ModelError),
}

/// Serializes a build to the bytes of a `.mvb` file.
pub fn write(build: &Build) -> Result<Vec<u8>, MvbError> {
    let document = nbt::write(ROOT_NAME, &build_to_nbt(build))?;
    let mut out = Vec::with_capacity(document.len() / 4 + MAGIC.len() + 2);
    out.extend_from_slice(&MAGIC);
    out.extend_from_slice(&FORMAT_VERSION.to_be_bytes());
    zstd::stream::copy_encode(document.as_slice(), &mut out, COMPRESSION_LEVEL)?;
    Ok(out)
}

/// Parses the bytes of a `.mvb` file.
pub fn read(bytes: &[u8]) -> Result<Build, MvbError> {
    let rest = bytes
        .strip_prefix(MAGIC.as_slice())
        .ok_or(MvbError::NotMvb)?;
    let (version, payload) = rest.split_first_chunk::<2>().ok_or(MvbError::NotMvb)?;
    let version = u16::from_be_bytes(*version);
    if version == 0 || version > FORMAT_VERSION {
        return Err(MvbError::UnsupportedVersion(version));
    }

    let mut document = Vec::new();
    zstd::stream::read::Decoder::with_buffer(payload)?
        .take(MAX_DECOMPRESSED_SIZE + 1)
        .read_to_end(&mut document)?;
    if document.len() as u64 > MAX_DECOMPRESSED_SIZE {
        return Err(MvbError::TooLarge);
    }

    let (_, root) = nbt::read(&document)?;
    let build = build_from_nbt(root)?;
    build.validate()?;
    Ok(build)
}

fn build_to_nbt(build: &Build) -> Compound {
    let metadata: Compound = [
        ("Name", build.metadata.name.as_str()),
        ("Author", build.metadata.author.as_str()),
        ("Description", build.metadata.description.as_str()),
    ]
    .into_iter()
    .collect();

    let mut root = Compound::new();
    root.insert("DataVersion", build.data_version);
    root.insert("Metadata", metadata);
    root.insert(
        "Palette",
        build
            .palette
            .states()
            .iter()
            .map(|state| Tag::Compound(state.to_nbt()))
            .collect::<Vec<_>>(),
    );
    root.insert(
        "Regions",
        build.regions.iter().map(region_to_nbt).collect::<Vec<_>>(),
    );
    root.insert("Extra", build.extra.clone());
    root
}

fn pos_to_nbt(pos: Pos) -> Tag {
    Tag::IntArray(vec![pos.x, pos.y, pos.z])
}

fn region_to_nbt(region: &Region) -> Tag {
    let size = region.size();
    let blocks: Vec<u8> = region
        .blocks()
        .iter()
        .flat_map(|b| b.to_le_bytes())
        .collect();
    let block_entities: Vec<Tag> = region
        .block_entities
        .iter()
        .map(|be| {
            let mut entry = Compound::new();
            entry.insert("Pos", pos_to_nbt(be.pos));
            entry.insert("Id", be.id.as_str());
            entry.insert("Data", be.data.clone());
            Tag::Compound(entry)
        })
        .collect();
    let entities: Vec<Tag> = region
        .entities
        .iter()
        .map(|e| {
            let mut entry = Compound::new();
            entry.insert("Pos", e.pos.map(Tag::Double).to_vec());
            entry.insert("Id", e.id.as_str());
            entry.insert("Data", e.data.clone());
            Tag::Compound(entry)
        })
        .collect();

    let mut entry = Compound::new();
    entry.insert("Name", region.name.as_str());
    entry.insert("Origin", pos_to_nbt(region.origin));
    // Region volumes are capped well below i32::MAX, so each dimension fits.
    entry.insert("Size", vec![size.x as i32, size.y as i32, size.z as i32]);
    entry.insert("Blocks", blocks);
    entry.insert("BlockEntities", block_entities);
    entry.insert("Entities", entities);
    entry.insert("Extra", region.extra.clone());
    Tag::Compound(entry)
}

/// Removes `key` from `compound` and converts it, or reports the field as invalid.
fn take<T>(
    compound: &mut Compound,
    key: &'static str,
    convert: impl FnOnce(Tag) -> Option<T>,
) -> Result<T, MvbError> {
    compound
        .remove(key)
        .and_then(convert)
        .ok_or(MvbError::Field(key))
}

fn pos_from_nbt(tag: Tag) -> Option<Pos> {
    let &[x, y, z] = tag.as_int_array()? else {
        return None;
    };
    Some(Pos::new(x, y, z))
}

fn size_from_nbt(tag: Tag) -> Option<Size> {
    let &[x, y, z] = tag.as_int_array()? else {
        return None;
    };
    Some(Size::new(
        u32::try_from(x).ok()?,
        u32::try_from(y).ok()?,
        u32::try_from(z).ok()?,
    ))
}

fn build_from_nbt(mut root: Compound) -> Result<Build, MvbError> {
    let mut metadata = take(&mut root, "Metadata", Tag::into_compound)?;
    let metadata = Metadata {
        name: take(&mut metadata, "Name", Tag::into_string)?,
        author: take(&mut metadata, "Author", Tag::into_string)?,
        description: take(&mut metadata, "Description", Tag::into_string)?,
    };

    let states = take(&mut root, "Palette", Tag::into_list)?
        .into_iter()
        .map(|tag| {
            tag.into_compound()
                .and_then(BlockState::from_nbt)
                .ok_or(MvbError::Field("Palette"))
        })
        .collect::<Result<Vec<_>, _>>()?;

    let regions = take(&mut root, "Regions", Tag::into_list)?
        .into_iter()
        .map(region_from_nbt)
        .collect::<Result<Vec<_>, _>>()?;

    Ok(Build {
        metadata,
        data_version: take(&mut root, "DataVersion", |t| t.as_int())?,
        palette: Palette::from_states(states)?,
        regions,
        extra: take(&mut root, "Extra", Tag::into_compound)?,
    })
}

fn region_from_nbt(tag: Tag) -> Result<Region, MvbError> {
    let mut entry = tag.into_compound().ok_or(MvbError::Field("Regions"))?;
    let name = take(&mut entry, "Name", Tag::into_string)?;
    let origin = take(&mut entry, "Origin", pos_from_nbt)?;
    let size = take(&mut entry, "Size", size_from_nbt)?;
    let bytes = take(&mut entry, "Blocks", Tag::into_byte_array)?;
    if bytes.len() % 2 != 0 {
        return Err(MvbError::Field("Blocks"));
    }
    let blocks = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|&b| u16::from_le_bytes(b))
        .collect();

    let mut region = Region::from_blocks(name, origin, size, blocks)?;
    region.block_entities = take(&mut entry, "BlockEntities", Tag::into_list)?
        .into_iter()
        .map(|tag| block_entity_from_nbt(tag).ok_or(MvbError::Field("BlockEntities")))
        .collect::<Result<_, _>>()?;
    region.entities = take(&mut entry, "Entities", Tag::into_list)?
        .into_iter()
        .map(|tag| entity_from_nbt(tag).ok_or(MvbError::Field("Entities")))
        .collect::<Result<_, _>>()?;
    region.extra = take(&mut entry, "Extra", Tag::into_compound)?;
    Ok(region)
}

fn block_entity_from_nbt(tag: Tag) -> Option<BlockEntity> {
    let mut entry = tag.into_compound()?;
    Some(BlockEntity {
        pos: pos_from_nbt(entry.remove("Pos")?)?,
        id: entry.remove("Id")?.into_string()?,
        data: entry.remove("Data")?.into_compound()?,
    })
}

fn entity_from_nbt(tag: Tag) -> Option<Entity> {
    let mut entry = tag.into_compound()?;
    let pos = entry.remove("Pos")?.into_list()?;
    let [x, y, z] = pos.as_slice() else {
        return None;
    };
    Some(Entity {
        pos: [x.as_double()?, y.as_double()?, z.as_double()?],
        id: entry.remove("Id")?.into_string()?,
        data: entry.remove("Data")?.into_compound()?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_build() -> Build {
        let mut build = Build::new();
        build.metadata = Metadata {
            name: "Tour".into(),
            author: "Alexis".into(),
            description: "Une tour \u{1F3F0}".into(),
        };
        build.data_version = 3953;
        let stairs = build
            .palette
            .get_or_insert("oak_stairs[facing=east,half=bottom]".parse().unwrap())
            .unwrap();
        let banner = build
            .palette
            .get_or_insert(BlockState::new("white_banner"))
            .unwrap();

        let mut main = Region::new("main", Pos::new(0, 0, 0), Size::new(3, 2, 2)).unwrap();
        main.set(0, 0, 0, stairs);
        main.set(2, 1, 1, banner);
        let mut patterns = Compound::new();
        patterns.insert("color", "red");
        main.block_entities.push(BlockEntity {
            pos: Pos::new(2, 1, 1),
            id: "minecraft:banner".into(),
            data: [("patterns", Tag::List(vec![Tag::Compound(patterns)]))]
                .into_iter()
                .collect(),
        });
        main.entities.push(Entity {
            pos: [1.5, 1.0, 0.03125],
            id: "minecraft:item_frame".into(),
            data: [("Facing", Tag::Byte(3))].into_iter().collect(),
        });
        main.extra.insert("PendingBlockTicks", Vec::<Tag>::new());
        build.regions.push(main);
        build
            .regions
            .push(Region::new("annexe", Pos::new(-4, 2, 7), Size::new(1, 1, 1)).unwrap());
        build.extra.insert("PreviewImageData", vec![1i32, 2, 3]);
        build
    }

    #[test]
    fn round_trip_keeps_everything() {
        let build = sample_build();
        let bytes = write(&build).unwrap();
        assert!(bytes.starts_with(&MAGIC));
        assert_eq!(read(&bytes).unwrap(), build);
    }

    #[test]
    fn empty_build_round_trips() {
        let build = Build::new();
        assert_eq!(read(&write(&build).unwrap()).unwrap(), build);
    }

    #[test]
    fn rejects_foreign_files() {
        assert!(matches!(read(b""), Err(MvbError::NotMvb)));
        assert!(matches!(
            read(b"\x1f\x8b\x08\x00 gzip"),
            Err(MvbError::NotMvb)
        ));
        assert!(matches!(read(&MAGIC), Err(MvbError::NotMvb)));
    }

    #[test]
    fn rejects_newer_format_versions() {
        let mut bytes = write(&Build::new()).unwrap();
        bytes[MAGIC.len()..MAGIC.len() + 2].copy_from_slice(&(FORMAT_VERSION + 1).to_be_bytes());
        assert!(matches!(
            read(&bytes),
            Err(MvbError::UnsupportedVersion(v)) if v == FORMAT_VERSION + 1
        ));
    }

    #[test]
    fn corrupted_payload_is_an_error_not_a_panic() {
        let bytes = write(&sample_build()).unwrap();
        for len in (MAGIC.len() + 2)..bytes.len() {
            assert!(read(&bytes[..len]).is_err(), "truncated at {len}");
        }
        let mut flipped = bytes.clone();
        let middle = flipped.len() / 2;
        flipped[middle] ^= 0xFF;
        let _ = read(&flipped);
    }

    #[test]
    fn rejects_blocks_outside_the_palette() {
        let mut build = sample_build();
        build.regions[1].set(0, 0, 0, 42);
        let bytes = write(&build).unwrap();
        assert!(matches!(
            read(&bytes),
            Err(MvbError::Model(ModelError::UnknownPaletteIndex {
                index: 42,
                ..
            }))
        ));
    }
}
