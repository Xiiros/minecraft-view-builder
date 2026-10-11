//! Block states (`minecraft:oak_stairs[facing=north,half=bottom]`) and palettes.

use std::collections::HashMap;
use std::fmt;
use std::str::FromStr;

use crate::build::ModelError;

/// Namespace assumed when a block name has none (`stone` means `minecraft:stone`).
pub const DEFAULT_NAMESPACE: &str = "minecraft";

/// A block with all its properties (orientation, shape, ...).
///
/// Properties are kept sorted by key, so two equal states always compare and hash equal.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct BlockState {
    name: String,
    properties: Vec<(String, String)>,
}

impl BlockState {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: qualify(name.into()),
            properties: Vec::new(),
        }
    }

    /// Builds a state from a name and properties in any order. For a repeated key, the last value wins.
    pub fn with_properties<K, V>(
        name: impl Into<String>,
        properties: impl IntoIterator<Item = (K, V)>,
    ) -> Self
    where
        K: Into<String>,
        V: Into<String>,
    {
        let mut properties: Vec<(String, String)> = properties
            .into_iter()
            .map(|(k, v)| (k.into(), v.into()))
            .collect();
        // Reverse then stable-sort so that, among equal keys, the last given comes first and survives dedup.
        properties.reverse();
        properties.sort_by(|a, b| a.0.cmp(&b.0));
        properties.dedup_by(|a, b| a.0 == b.0);
        Self {
            name: qualify(name.into()),
            properties,
        }
    }

    pub fn air() -> Self {
        Self::new("minecraft:air")
    }

    /// Full name with namespace, e.g. `minecraft:oak_stairs`.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Properties sorted by key.
    pub fn properties(&self) -> &[(String, String)] {
        &self.properties
    }

    pub fn property(&self, key: &str) -> Option<&str> {
        self.properties
            .binary_search_by(|(k, _)| k.as_str().cmp(key))
            .ok()
            .map(|i| self.properties[i].1.as_str())
    }

    /// True for every kind of air, which is never drawn nor counted as a material.
    pub fn is_air(&self) -> bool {
        matches!(
            self.name.as_str(),
            "minecraft:air" | "minecraft:cave_air" | "minecraft:void_air"
        )
    }
}

fn qualify(name: String) -> String {
    if name.contains(':') {
        name
    } else {
        format!("{DEFAULT_NAMESPACE}:{name}")
    }
}

impl fmt::Display for BlockState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.name)?;
        if !self.properties.is_empty() {
            f.write_str("[")?;
            for (i, (key, value)) in self.properties.iter().enumerate() {
                if i > 0 {
                    f.write_str(",")?;
                }
                write!(f, "{key}={value}")?;
            }
            f.write_str("]")?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ParseBlockStateError {
    #[error("invalid block name `{0}`")]
    InvalidName(String),
    #[error("missing closing `]` in block state")]
    UnclosedBracket,
    #[error("invalid block property `{0}`")]
    InvalidProperty(String),
}

impl FromStr for BlockState {
    type Err = ParseBlockStateError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.trim();
        let (name, properties) = match s.split_once('[') {
            None => (s, ""),
            Some((name, rest)) => (
                name,
                rest.strip_suffix(']')
                    .ok_or(ParseBlockStateError::UnclosedBracket)?,
            ),
        };
        if !is_valid_name(name) {
            return Err(ParseBlockStateError::InvalidName(name.to_owned()));
        }
        let properties = properties
            .split(',')
            .filter(|pair| !pair.is_empty())
            .map(|pair| {
                pair.split_once('=')
                    .map(|(k, v)| (k.trim(), v.trim()))
                    .filter(|(k, v)| is_valid_token(k) && is_valid_token(v))
                    .ok_or_else(|| ParseBlockStateError::InvalidProperty(pair.to_owned()))
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self::with_properties(name, properties))
    }
}

/// `namespace:path` or `path`, with the characters allowed in the game's resource locations.
fn is_valid_name(name: &str) -> bool {
    let valid_part = |part: &str| {
        !part.is_empty()
            && part
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | '/'))
    };
    match name.split_once(':') {
        Some((namespace, path)) => {
            valid_part(namespace) && !namespace.contains('/') && valid_part(path)
        }
        None => valid_part(name),
    }
}

fn is_valid_token(token: &str) -> bool {
    !token.is_empty()
        && token
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
}

/// Distinct block states of a build. Blocks store a `u16` index into it instead of the full state.
///
/// Index 0 is always `minecraft:air`, so a freshly allocated (zeroed) region is empty.
#[derive(Clone, Debug)]
pub struct Palette {
    states: Vec<BlockState>,
    lookup: HashMap<BlockState, u16>,
}

impl Palette {
    pub const AIR: u16 = 0;
    pub const MAX_LEN: usize = u16::MAX as usize + 1;

    pub fn new() -> Self {
        let air = BlockState::air();
        Self {
            lookup: HashMap::from([(air.clone(), Self::AIR)]),
            states: vec![air],
        }
    }

    /// Rebuilds a palette from its ordered entries, e.g. when loading a file.
    pub fn from_states(states: Vec<BlockState>) -> Result<Self, ModelError> {
        if states.first() != Some(&BlockState::air()) {
            return Err(ModelError::FirstPaletteEntryNotAir);
        }
        if states.len() > Self::MAX_LEN {
            return Err(ModelError::PaletteFull);
        }
        let mut lookup = HashMap::with_capacity(states.len());
        for (index, state) in (0..=u16::MAX).zip(&states) {
            if lookup.insert(state.clone(), index).is_some() {
                return Err(ModelError::DuplicatePaletteEntry(state.to_string()));
            }
        }
        Ok(Self { states, lookup })
    }

    /// Index of `state`, adding it to the palette if needed.
    pub fn get_or_insert(&mut self, state: BlockState) -> Result<u16, ModelError> {
        if let Some(&index) = self.lookup.get(&state) {
            return Ok(index);
        }
        let index = u16::try_from(self.states.len()).map_err(|_| ModelError::PaletteFull)?;
        self.lookup.insert(state.clone(), index);
        self.states.push(state);
        Ok(index)
    }

    pub fn index_of(&self, state: &BlockState) -> Option<u16> {
        self.lookup.get(state).copied()
    }

    pub fn get(&self, index: u16) -> Option<&BlockState> {
        self.states.get(usize::from(index))
    }

    /// Entries in index order.
    pub fn states(&self) -> &[BlockState] {
        &self.states
    }

    pub fn len(&self) -> usize {
        self.states.len()
    }

    /// Always false: air is always present.
    pub fn is_empty(&self) -> bool {
        self.states.is_empty()
    }
}

impl Default for Palette {
    fn default() -> Self {
        Self::new()
    }
}

impl PartialEq for Palette {
    fn eq(&self, other: &Self) -> bool {
        self.states == other.states
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_formats_canonically() {
        let state: BlockState = "oak_stairs[half=bottom, facing=north]".parse().unwrap();
        assert_eq!(state.name(), "minecraft:oak_stairs");
        assert_eq!(state.property("facing"), Some("north"));
        assert_eq!(state.property("waterlogged"), None);
        assert_eq!(
            state.to_string(),
            "minecraft:oak_stairs[facing=north,half=bottom]"
        );
        assert_eq!(state.to_string().parse::<BlockState>().unwrap(), state);
    }

    #[test]
    fn property_order_does_not_matter() {
        let a =
            BlockState::with_properties("stone_slab", [("type", "top"), ("waterlogged", "false")]);
        let b =
            BlockState::with_properties("stone_slab", [("waterlogged", "false"), ("type", "top")]);
        assert_eq!(a, b);
    }

    #[test]
    fn last_duplicate_property_wins() {
        let state = BlockState::with_properties("lever", [("face", "wall"), ("face", "floor")]);
        assert_eq!(state.properties().len(), 1);
        assert_eq!(state.property("face"), Some("floor"));
    }

    #[test]
    fn keeps_explicit_namespace() {
        assert_eq!(BlockState::new("create:shaft").name(), "create:shaft");
        assert_eq!(BlockState::new("stone").to_string(), "minecraft:stone");
        assert_eq!(
            "stone[]".parse::<BlockState>().unwrap(),
            BlockState::new("stone")
        );
    }

    #[test]
    fn rejects_malformed_states() {
        for bad in [
            "",
            "a b",
            ":stone",
            "minecraft:",
            "my/ns:stone",
            "stone[facing=north",
        ] {
            assert!(
                bad.parse::<BlockState>().is_err(),
                "{bad:?} should be rejected"
            );
        }
        assert_eq!(
            "stone[facing]".parse::<BlockState>(),
            Err(ParseBlockStateError::InvalidProperty("facing".into()))
        );
        assert!("stone[facing=]".parse::<BlockState>().is_err());
    }

    #[test]
    fn recognises_air_variants() {
        assert!(BlockState::new("cave_air").is_air());
        assert!(!BlockState::new("glass").is_air());
    }

    #[test]
    fn palette_deduplicates_and_keeps_air_first() {
        let mut palette = Palette::new();
        let stone = palette.get_or_insert(BlockState::new("stone")).unwrap();
        assert_eq!(stone, 1);
        assert_eq!(
            palette
                .get_or_insert(BlockState::new("minecraft:stone"))
                .unwrap(),
            1
        );
        assert_eq!(
            palette.get_or_insert(BlockState::air()).unwrap(),
            Palette::AIR
        );
        assert_eq!(palette.len(), 2);
        assert_eq!(palette.get(1), Some(&BlockState::new("stone")));
        assert_eq!(palette.index_of(&BlockState::new("dirt")), None);
    }

    #[test]
    fn palette_from_states_validates_entries() {
        let air = BlockState::air();
        let stone = BlockState::new("stone");
        assert!(Palette::from_states(vec![air.clone(), stone.clone()]).is_ok());
        assert_eq!(
            Palette::from_states(vec![stone.clone()]),
            Err(ModelError::FirstPaletteEntryNotAir)
        );
        assert_eq!(
            Palette::from_states(vec![air, stone.clone(), stone]),
            Err(ModelError::DuplicatePaletteEntry("minecraft:stone".into()))
        );
    }

    #[test]
    fn palette_is_limited_to_u16_indices() {
        let mut palette = Palette::new();
        for i in 1..Palette::MAX_LEN {
            palette
                .get_or_insert(BlockState::new(format!("b{i}")))
                .unwrap();
        }
        assert_eq!(
            palette.get_or_insert(BlockState::new("one_too_many")),
            Err(ModelError::PaletteFull)
        );
    }
}
