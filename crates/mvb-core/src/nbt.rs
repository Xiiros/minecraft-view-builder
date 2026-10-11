//! Reader and writer for NBT (Named Binary Tag), the binary tree format used by the game's files.
//!
//! Java edition flavour: big-endian numbers, strings in Java's "modified UTF-8". The reader works
//! on untrusted input: declared lengths are checked against the remaining bytes before anything is
//! allocated, and nesting depth is limited.

use std::borrow::Cow;

/// Maximum nesting of lists and compounds, same limit as the game.
pub const MAX_DEPTH: usize = 512;

mod id {
    pub const END: u8 = 0;
    pub const BYTE: u8 = 1;
    pub const SHORT: u8 = 2;
    pub const INT: u8 = 3;
    pub const LONG: u8 = 4;
    pub const FLOAT: u8 = 5;
    pub const DOUBLE: u8 = 6;
    pub const BYTE_ARRAY: u8 = 7;
    pub const STRING: u8 = 8;
    pub const LIST: u8 = 9;
    pub const COMPOUND: u8 = 10;
    pub const INT_ARRAY: u8 = 11;
    pub const LONG_ARRAY: u8 = 12;
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum NbtError {
    #[error("unexpected end of NBT data")]
    UnexpectedEof,
    #[error("unknown NBT tag type {0}")]
    UnknownTag(u8),
    #[error("NBT root must be a compound, found tag type {0}")]
    RootNotCompound(u8),
    #[error("negative NBT length {0}")]
    NegativeLength(i32),
    #[error("NBT nesting deeper than {MAX_DEPTH} levels")]
    TooDeep,
    #[error("invalid NBT string encoding")]
    InvalidString,
    #[error("NBT string longer than 65535 bytes")]
    StringTooLong,
    #[error("NBT array or list longer than {} elements", i32::MAX)]
    TooLong,
    #[error("NBT list mixes tag types")]
    MixedList,
}

/// One NBT value. Lists hold tags of a single type, checked when writing.
#[derive(Clone, Debug, PartialEq)]
pub enum Tag {
    Byte(i8),
    Short(i16),
    Int(i32),
    Long(i64),
    Float(f32),
    Double(f64),
    /// Raw bytes. The game treats them as signed, but the bits are the same.
    ByteArray(Vec<u8>),
    String(String),
    List(Vec<Tag>),
    Compound(Compound),
    IntArray(Vec<i32>),
    LongArray(Vec<i64>),
}

macro_rules! value_accessors {
    ($($name:ident => $variant:ident: $ty:ty),* $(,)?) => {
        $(
            pub fn $name(&self) -> Option<$ty> {
                match self {
                    Tag::$variant(v) => Some(*v),
                    _ => None,
                }
            }
        )*
    };
}

macro_rules! ref_accessors {
    ($($name:ident => $variant:ident: $ty:ty),* $(,)?) => {
        $(
            pub fn $name(&self) -> Option<&$ty> {
                match self {
                    Tag::$variant(v) => Some(v),
                    _ => None,
                }
            }
        )*
    };
}

macro_rules! into_accessors {
    ($($name:ident => $variant:ident: $ty:ty),* $(,)?) => {
        $(
            pub fn $name(self) -> Option<$ty> {
                match self {
                    Tag::$variant(v) => Some(v),
                    _ => None,
                }
            }
        )*
    };
}

impl Tag {
    value_accessors! {
        as_byte => Byte: i8,
        as_short => Short: i16,
        as_int => Int: i32,
        as_long => Long: i64,
        as_float => Float: f32,
        as_double => Double: f64,
    }

    ref_accessors! {
        as_byte_array => ByteArray: [u8],
        as_str => String: str,
        as_list => List: [Tag],
        as_compound => Compound: Compound,
        as_int_array => IntArray: [i32],
        as_long_array => LongArray: [i64],
    }

    into_accessors! {
        into_byte_array => ByteArray: Vec<u8>,
        into_string => String: String,
        into_list => List: Vec<Tag>,
        into_compound => Compound: Compound,
        into_int_array => IntArray: Vec<i32>,
        into_long_array => LongArray: Vec<i64>,
    }

    fn id(&self) -> u8 {
        match self {
            Tag::Byte(_) => id::BYTE,
            Tag::Short(_) => id::SHORT,
            Tag::Int(_) => id::INT,
            Tag::Long(_) => id::LONG,
            Tag::Float(_) => id::FLOAT,
            Tag::Double(_) => id::DOUBLE,
            Tag::ByteArray(_) => id::BYTE_ARRAY,
            Tag::String(_) => id::STRING,
            Tag::List(_) => id::LIST,
            Tag::Compound(_) => id::COMPOUND,
            Tag::IntArray(_) => id::INT_ARRAY,
            Tag::LongArray(_) => id::LONG_ARRAY,
        }
    }
}

macro_rules! tag_from {
    ($($ty:ty => $variant:ident),* $(,)?) => {
        $(
            impl From<$ty> for Tag {
                fn from(value: $ty) -> Self {
                    Tag::$variant(value)
                }
            }
        )*
    };
}

tag_from! {
    i8 => Byte,
    i16 => Short,
    i32 => Int,
    i64 => Long,
    f32 => Float,
    f64 => Double,
    Vec<u8> => ByteArray,
    String => String,
    Vec<Tag> => List,
    Compound => Compound,
    Vec<i32> => IntArray,
    Vec<i64> => LongArray,
}

impl From<&str> for Tag {
    fn from(value: &str) -> Self {
        Tag::String(value.to_owned())
    }
}

/// Named tags, in insertion order. Compounds are small in practice, so a vector beats a hash map.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Compound {
    entries: Vec<(String, Tag)>,
}

impl Compound {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn get(&self, key: &str) -> Option<&Tag> {
        self.entries.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }

    /// Sets `key`, returning the previous value if there was one.
    pub fn insert(&mut self, key: impl Into<String>, value: impl Into<Tag>) -> Option<Tag> {
        let key = key.into();
        let value = value.into();
        match self.entries.iter_mut().find(|(k, _)| *k == key) {
            Some((_, slot)) => Some(std::mem::replace(slot, value)),
            None => {
                self.entries.push((key, value));
                None
            }
        }
    }

    pub fn remove(&mut self, key: &str) -> Option<Tag> {
        let index = self.entries.iter().position(|(k, _)| k == key)?;
        Some(self.entries.remove(index).1)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &Tag)> {
        self.entries.iter().map(|(k, v)| (k.as_str(), v))
    }
}

impl IntoIterator for Compound {
    type Item = (String, Tag);
    type IntoIter = std::vec::IntoIter<(String, Tag)>;

    fn into_iter(self) -> Self::IntoIter {
        self.entries.into_iter()
    }
}

impl<K: Into<String>, V: Into<Tag>> FromIterator<(K, V)> for Compound {
    fn from_iter<I: IntoIterator<Item = (K, V)>>(iter: I) -> Self {
        let mut compound = Compound::new();
        for (key, value) in iter {
            compound.insert(key, value);
        }
        compound
    }
}

/// Parses an uncompressed NBT document and returns the root's name and content.
/// Bytes after the root tag are ignored.
pub fn read(data: &[u8]) -> Result<(String, Compound), NbtError> {
    let mut reader = Reader { data, pos: 0 };
    let root_id = reader.u8()?;
    if root_id != id::COMPOUND {
        return Err(NbtError::RootNotCompound(root_id));
    }
    let name = reader.string()?;
    let root = reader.compound(1)?;
    Ok((name, root))
}

/// Serializes `root` as an uncompressed NBT document named `name`.
pub fn write(name: &str, root: &Compound) -> Result<Vec<u8>, NbtError> {
    let mut writer = Writer { out: Vec::new() };
    writer.out.push(id::COMPOUND);
    writer.string(name)?;
    writer.compound(root)?;
    Ok(writer.out)
}

struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], NbtError> {
        let end = self
            .pos
            .checked_add(n)
            .filter(|&end| end <= self.data.len())
            .ok_or(NbtError::UnexpectedEof)?;
        let bytes = &self.data[self.pos..end];
        self.pos = end;
        Ok(bytes)
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N], NbtError> {
        Ok(self
            .take(N)?
            .try_into()
            .expect("take returns exactly N bytes"))
    }

    fn u8(&mut self) -> Result<u8, NbtError> {
        Ok(self.array::<1>()?[0])
    }

    fn i32(&mut self) -> Result<i32, NbtError> {
        Ok(i32::from_be_bytes(self.array()?))
    }

    /// Reads an element count and checks that `count * min_size` bytes are actually available,
    /// so a corrupt length can never trigger a huge allocation.
    fn count(&mut self, min_size: usize) -> Result<usize, NbtError> {
        let count = self.i32()?;
        let count = usize::try_from(count).map_err(|_| NbtError::NegativeLength(count))?;
        if count.saturating_mul(min_size) > self.data.len() - self.pos {
            return Err(NbtError::UnexpectedEof);
        }
        Ok(count)
    }

    fn string(&mut self) -> Result<String, NbtError> {
        let len = u16::from_be_bytes(self.array()?);
        let bytes = self.take(usize::from(len))?;
        cesu8::from_java_cesu8(bytes)
            .map(Cow::into_owned)
            .map_err(|_| NbtError::InvalidString)
    }

    fn compound(&mut self, depth: usize) -> Result<Compound, NbtError> {
        if depth > MAX_DEPTH {
            return Err(NbtError::TooDeep);
        }
        let mut compound = Compound::new();
        loop {
            let tag_id = self.u8()?;
            if tag_id == id::END {
                return Ok(compound);
            }
            let name = self.string()?;
            let tag = self.payload(tag_id, depth)?;
            compound.entries.push((name, tag));
        }
    }

    fn payload(&mut self, tag_id: u8, depth: usize) -> Result<Tag, NbtError> {
        Ok(match tag_id {
            id::BYTE => Tag::Byte(i8::from_be_bytes(self.array()?)),
            id::SHORT => Tag::Short(i16::from_be_bytes(self.array()?)),
            id::INT => Tag::Int(self.i32()?),
            id::LONG => Tag::Long(i64::from_be_bytes(self.array()?)),
            id::FLOAT => Tag::Float(f32::from_be_bytes(self.array()?)),
            id::DOUBLE => Tag::Double(f64::from_be_bytes(self.array()?)),
            id::BYTE_ARRAY => {
                let len = self.count(1)?;
                Tag::ByteArray(self.take(len)?.to_vec())
            }
            id::STRING => Tag::String(self.string()?),
            id::LIST => {
                if depth >= MAX_DEPTH {
                    return Err(NbtError::TooDeep);
                }
                let element_id = self.u8()?;
                let len = self.count(min_payload_size(element_id)?)?;
                if element_id == id::END {
                    // Some writers declare empty lists with a non-zero length: nothing to read.
                    return Ok(Tag::List(Vec::new()));
                }
                let mut items = Vec::with_capacity(len);
                for _ in 0..len {
                    items.push(self.payload(element_id, depth + 1)?);
                }
                Tag::List(items)
            }
            id::COMPOUND => Tag::Compound(self.compound(depth + 1)?),
            id::INT_ARRAY => {
                let len = self.count(4)?;
                let bytes = self.take(len * 4)?;
                Tag::IntArray(
                    bytes
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .map(|&c| i32::from_be_bytes(c))
                        .collect(),
                )
            }
            id::LONG_ARRAY => {
                let len = self.count(8)?;
                let bytes = self.take(len * 8)?;
                Tag::LongArray(
                    bytes
                        .as_chunks::<8>()
                        .0
                        .iter()
                        .map(|&c| i64::from_be_bytes(c))
                        .collect(),
                )
            }
            other => return Err(NbtError::UnknownTag(other)),
        })
    }
}

/// Smallest encoded size of a payload of the given type, used to validate list lengths.
fn min_payload_size(tag_id: u8) -> Result<usize, NbtError> {
    Ok(match tag_id {
        id::END => 0,
        id::BYTE | id::COMPOUND => 1,
        id::SHORT | id::STRING => 2,
        id::INT | id::FLOAT | id::BYTE_ARRAY | id::INT_ARRAY | id::LONG_ARRAY => 4,
        id::LIST => 5,
        id::LONG | id::DOUBLE => 8,
        other => return Err(NbtError::UnknownTag(other)),
    })
}

struct Writer {
    out: Vec<u8>,
}

impl Writer {
    fn count(&mut self, len: usize) -> Result<(), NbtError> {
        let len = i32::try_from(len).map_err(|_| NbtError::TooLong)?;
        self.out.extend_from_slice(&len.to_be_bytes());
        Ok(())
    }

    fn string(&mut self, s: &str) -> Result<(), NbtError> {
        let bytes = cesu8::to_java_cesu8(s);
        let len = u16::try_from(bytes.len()).map_err(|_| NbtError::StringTooLong)?;
        self.out.extend_from_slice(&len.to_be_bytes());
        self.out.extend_from_slice(&bytes);
        Ok(())
    }

    fn compound(&mut self, compound: &Compound) -> Result<(), NbtError> {
        for (name, tag) in &compound.entries {
            self.out.push(tag.id());
            self.string(name)?;
            self.payload(tag)?;
        }
        self.out.push(id::END);
        Ok(())
    }

    fn payload(&mut self, tag: &Tag) -> Result<(), NbtError> {
        match tag {
            Tag::Byte(v) => self.out.extend_from_slice(&v.to_be_bytes()),
            Tag::Short(v) => self.out.extend_from_slice(&v.to_be_bytes()),
            Tag::Int(v) => self.out.extend_from_slice(&v.to_be_bytes()),
            Tag::Long(v) => self.out.extend_from_slice(&v.to_be_bytes()),
            Tag::Float(v) => self.out.extend_from_slice(&v.to_be_bytes()),
            Tag::Double(v) => self.out.extend_from_slice(&v.to_be_bytes()),
            Tag::ByteArray(v) => {
                self.count(v.len())?;
                self.out.extend_from_slice(v);
            }
            Tag::String(v) => self.string(v)?,
            Tag::List(items) => {
                let element_id = items.first().map_or(id::END, Tag::id);
                if items.iter().any(|item| item.id() != element_id) {
                    return Err(NbtError::MixedList);
                }
                self.out.push(element_id);
                self.count(items.len())?;
                for item in items {
                    self.payload(item)?;
                }
            }
            Tag::Compound(v) => self.compound(v)?,
            Tag::IntArray(v) => {
                self.count(v.len())?;
                self.out.extend(v.iter().flat_map(|x| x.to_be_bytes()));
            }
            Tag::LongArray(v) => {
                self.count(v.len())?;
                self.out.extend(v.iter().flat_map(|x| x.to_be_bytes()));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Compound {
        let mut nested = Compound::new();
        nested.insert("name", "minecraft:oak_stairs");
        nested.insert("emoji", "sign \u{1F600} text\0with nul");
        [
            ("byte", Tag::Byte(-5)),
            ("short", Tag::Short(-300)),
            ("int", Tag::Int(123_456)),
            ("long", Tag::Long(-9_000_000_000)),
            ("float", Tag::Float(1.5)),
            ("double", Tag::Double(-2.25)),
            ("bytes", Tag::ByteArray(vec![0, 255, 7])),
            ("string", Tag::from("héllo")),
            ("list", Tag::List(vec![Tag::Int(1), Tag::Int(2)])),
            ("empty_list", Tag::List(Vec::new())),
            ("compound", Tag::Compound(nested)),
            ("ints", Tag::IntArray(vec![i32::MIN, 0, i32::MAX])),
            ("longs", Tag::LongArray(vec![i64::MIN, 0, i64::MAX])),
        ]
        .into_iter()
        .collect()
    }

    #[test]
    fn round_trip_preserves_every_tag_type() {
        let root = sample();
        let bytes = write("root", &root).unwrap();
        let (name, decoded) = read(&bytes).unwrap();
        assert_eq!(name, "root");
        assert_eq!(decoded, root);
    }

    #[test]
    fn strings_use_java_modified_utf8() {
        let mut root = Compound::new();
        root.insert("s", "\0\u{1F600}");
        let bytes = write("", &root).unwrap();
        // NUL is encoded on two bytes and the emoji as a surrogate pair (2 x 3 bytes).
        let encoded = [0xC0, 0x80, 0xED, 0xA0, 0xBD, 0xED, 0xB8, 0x80];
        assert!(bytes.windows(encoded.len()).any(|w| w == encoded));
    }

    #[test]
    fn known_encoding_is_decoded() {
        // Root compound "hi" containing Int "a" = 1.
        let bytes = [10, 0, 2, b'h', b'i', 3, 0, 1, b'a', 0, 0, 0, 1, 0];
        let (name, root) = read(&bytes).unwrap();
        assert_eq!(name, "hi");
        assert_eq!(root.get("a"), Some(&Tag::Int(1)));
    }

    #[test]
    fn every_truncation_is_an_error() {
        let bytes = write("root", &sample()).unwrap();
        for len in 0..bytes.len() {
            assert!(read(&bytes[..len]).is_err(), "truncated at {len}");
        }
    }

    #[test]
    fn huge_declared_length_does_not_allocate() {
        // Int array claiming i32::MAX elements with no data behind it.
        let bytes = [10, 0, 0, 11, 0, 1, b'a', 0x7F, 0xFF, 0xFF, 0xFF];
        assert_eq!(read(&bytes), Err(NbtError::UnexpectedEof));
        // List of compounds claiming i32::MAX elements.
        let bytes = [10, 0, 0, 9, 0, 1, b'a', 10, 0x7F, 0xFF, 0xFF, 0xFF];
        assert_eq!(read(&bytes), Err(NbtError::UnexpectedEof));
    }

    #[test]
    fn negative_length_is_rejected() {
        let bytes = [10, 0, 0, 7, 0, 1, b'a', 0xFF, 0xFF, 0xFF, 0xFF];
        assert_eq!(read(&bytes), Err(NbtError::NegativeLength(-1)));
    }

    #[test]
    fn excessive_nesting_is_rejected() {
        let mut bytes = vec![10, 0, 0];
        for _ in 0..MAX_DEPTH + 10 {
            bytes.extend_from_slice(&[10, 0, 0]);
        }
        assert_eq!(read(&bytes), Err(NbtError::TooDeep));
    }

    #[test]
    fn root_must_be_a_compound() {
        assert_eq!(
            read(&[3, 0, 0, 0, 0, 0, 1]),
            Err(NbtError::RootNotCompound(3))
        );
    }

    #[test]
    fn mixed_list_cannot_be_written() {
        let mut root = Compound::new();
        root.insert("l", vec![Tag::Int(1), Tag::Byte(1)]);
        assert_eq!(write("", &root), Err(NbtError::MixedList));
    }

    #[test]
    fn insert_replaces_existing_key() {
        let mut c = Compound::new();
        assert_eq!(c.insert("k", 1), None);
        assert_eq!(c.insert("k", 2), Some(Tag::Int(1)));
        assert_eq!(c.len(), 1);
        assert_eq!(c.get("k").and_then(Tag::as_int), Some(2));
        assert_eq!(c.remove("k"), Some(Tag::Int(2)));
        assert!(c.is_empty());
    }
}
