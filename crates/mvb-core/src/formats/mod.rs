//! Readers and writers for the build file formats of other tools.

pub mod litematica;
pub mod sponge;
pub mod structure;

use std::borrow::Cow;
use std::io::Read;

use flate2::read::GzDecoder;

use crate::block::ParseBlockStateError;
use crate::build::ModelError;
use crate::nbt::{Compound, NbtError, Tag};

/// Refuse to inflate more than this, so a tiny corrupt or malicious file cannot exhaust memory.
pub const MAX_DECOMPRESSED_SIZE: u64 = 1 << 30;

#[derive(Debug, thiserror::Error)]
pub enum ImportError {
    #[error("could not decompress the file: {0}")]
    Decompression(#[from] std::io::Error),
    #[error("file content is larger than {MAX_DECOMPRESSED_SIZE} bytes once decompressed")]
    TooLarge,
    #[error(transparent)]
    Nbt(#[from] NbtError),
    #[error("missing or invalid field `{0}`")]
    Field(&'static str),
    #[error("format version {0} is not supported")]
    UnsupportedVersion(i32),
    #[error("files from before Minecraft 1.13 are not supported")]
    PreFlattening,
    #[error(transparent)]
    BlockState(#[from] ParseBlockStateError),
    #[error(transparent)]
    Model(#[from] ModelError),
}

/// Returns the NBT document inside `bytes`, inflating it first if it is gzip-compressed.
pub(crate) fn decompress(bytes: &[u8]) -> Result<Cow<'_, [u8]>, ImportError> {
    const GZIP_MAGIC: [u8; 2] = [0x1f, 0x8b];
    if !bytes.starts_with(&GZIP_MAGIC) {
        return Ok(Cow::Borrowed(bytes));
    }
    let mut document = Vec::new();
    GzDecoder::new(bytes)
        .take(MAX_DECOMPRESSED_SIZE + 1)
        .read_to_end(&mut document)?;
    if document.len() as u64 > MAX_DECOMPRESSED_SIZE {
        return Err(ImportError::TooLarge);
    }
    Ok(Cow::Owned(document))
}

/// Removes `key` from `compound` and converts it, or reports the field as invalid.
pub(crate) fn take<T>(
    compound: &mut Compound,
    key: &'static str,
    convert: impl FnOnce(Tag) -> Option<T>,
) -> Result<T, ImportError> {
    compound
        .remove(key)
        .and_then(convert)
        .ok_or(ImportError::Field(key))
}

/// Reads a list of exactly three ints, as used for positions and sizes.
pub(crate) fn int_triple(tag: &Tag) -> Option<[i32; 3]> {
    let [x, y, z] = tag.as_list()? else {
        return None;
    };
    Some([x.as_int()?, y.as_int()?, z.as_int()?])
}

/// Reads a list of exactly three doubles, as used for entity positions.
pub(crate) fn double_triple(tag: &Tag) -> Option<[f64; 3]> {
    let [x, y, z] = tag.as_list()? else {
        return None;
    };
    Some([x.as_double()?, y.as_double()?, z.as_double()?])
}

#[cfg(test)]
pub(crate) mod test_util {
    use std::io::Write;

    use flate2::{Compression, write::GzEncoder};

    pub fn gzip(data: &[u8]) -> Vec<u8> {
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(data).unwrap();
        encoder.finish().unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_data_is_returned_as_is() {
        let data = [10, 0, 0, 0];
        assert!(matches!(decompress(&data).unwrap(), Cow::Borrowed(d) if d == data));
    }

    #[test]
    fn gzip_data_is_inflated() {
        let data = b"some nbt document".repeat(100);
        let compressed = test_util::gzip(&data);
        assert_eq!(decompress(&compressed).unwrap().as_ref(), data.as_slice());
    }

    #[test]
    fn truncated_gzip_is_an_error() {
        let compressed = test_util::gzip(&b"some nbt document".repeat(100));
        let truncated = &compressed[..compressed.len() / 2];
        assert!(matches!(
            decompress(truncated),
            Err(ImportError::Decompression(_))
        ));
    }

    #[test]
    fn triples_need_exactly_three_values_of_the_right_type() {
        let ints = Tag::List(vec![Tag::Int(1), Tag::Int(-2), Tag::Int(3)]);
        assert_eq!(int_triple(&ints), Some([1, -2, 3]));
        assert_eq!(int_triple(&Tag::List(vec![Tag::Int(1)])), None);
        assert_eq!(double_triple(&ints), None);
        let doubles = Tag::List(vec![Tag::Double(0.5); 3]);
        assert_eq!(double_triple(&doubles), Some([0.5; 3]));
    }
}
