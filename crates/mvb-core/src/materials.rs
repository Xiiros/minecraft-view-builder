//! Materials needed to rebuild a build: blocks counted as the items a player places.
//!
//! A block state is not always one item: a double slab takes two slabs, a door is placed once
//! for its two halves, a wall torch is a torch, four candles are four candles...

use std::collections::HashMap;

use crate::block::{BlockState, Palette};

/// Blocks that exist in the world but are never placed from an item.
const NOT_PLACEABLE: [&str; 9] = [
    "piston_head",
    "moving_piston",
    "nether_portal",
    "end_portal",
    "end_gateway",
    "fire",
    "soul_fire",
    "bubble_column",
    "frosted_ice",
];

/// The item placed to get `state` and how many of them it takes, or `None` for states that are
/// not placed directly (air, upper half of a door, head of a bed, piston head...).
pub fn item_for(state: &BlockState) -> Option<(String, u32)> {
    if !state.is_visible() {
        return None;
    }
    let (namespace, name) = state.name().split_once(':')?;
    if namespace == "minecraft" && NOT_PLACEABLE.contains(&name) {
        return None;
    }
    // The other half of a two-block block is placed with it.
    if state.property("half") == Some("upper") || state.property("part") == Some("head") {
        return None;
    }

    let item = if namespace != "minecraft" {
        name.to_owned()
    } else if name == "wall_torch" {
        "torch".to_owned()
    } else if name == "redstone_wire" {
        "redstone".to_owned()
    } else if name == "tripwire" {
        "string".to_owned()
    } else if let Some(base) = name
        .strip_suffix("_wall_torch")
        .map(|b| format!("{b}_torch"))
        .or_else(|| {
            name.strip_suffix("_wall_hanging_sign")
                .map(|b| format!("{b}_hanging_sign"))
        })
        .or_else(|| name.strip_suffix("_wall_sign").map(|b| format!("{b}_sign")))
        .or_else(|| {
            name.strip_suffix("_wall_banner")
                .map(|b| format!("{b}_banner"))
        })
        .or_else(|| name.strip_suffix("_wall_head").map(|b| format!("{b}_head")))
        .or_else(|| {
            name.strip_suffix("_wall_skull")
                .map(|b| format!("{b}_skull"))
        })
        .or_else(|| name.strip_suffix("_wall_fan").map(|b| format!("{b}_fan")))
    {
        base
    } else {
        name.to_owned()
    };

    let count_property = |key: &str| state.property(key).and_then(|v| v.parse::<u32>().ok());
    let quantity = if state.property("type") == Some("double") && name.ends_with("_slab") {
        2
    } else if name.ends_with("candle") {
        count_property("candles").unwrap_or(1)
    } else if name == "sea_pickle" {
        count_property("pickles").unwrap_or(1)
    } else if name == "snow" {
        count_property("layers").unwrap_or(1)
    } else {
        1
    };
    Some((format!("{namespace}:{item}"), quantity.max(1)))
}

/// How many of an item fit in one inventory slot.
pub fn stack_size(item: &str) -> u32 {
    let name = item.strip_prefix("minecraft:").unwrap_or(item);
    if name.ends_with("_bed")
        || name.ends_with("shulker_box")
        || matches!(name, "water" | "lava" | "cake" | "powder_snow")
    {
        1
    } else if name.ends_with("_sign") || name.ends_with("_banner") || name == "snow" {
        16
    } else {
        64
    }
}

/// Count of one item, with a palette entry that produces it (for its colour).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ItemCount {
    pub item: String,
    pub count: u64,
    pub block: u16,
}

/// Item of every palette entry, computed once so that counting is a table lookup per block.
pub struct MaterialIndex {
    items: Vec<(String, u16)>,
    entries: Vec<Option<(usize, u32)>>,
}

impl MaterialIndex {
    pub fn new(palette: &Palette) -> Self {
        let mut items: Vec<(String, u16)> = Vec::new();
        let mut positions: HashMap<String, usize> = HashMap::new();
        let entries = (0..=u16::MAX)
            .zip(palette.states())
            .map(|(block, state)| {
                let (item, quantity) = item_for(state)?;
                let index = *positions.entry(item.clone()).or_insert_with(|| {
                    items.push((item, block));
                    items.len() - 1
                });
                Some((index, quantity))
            })
            .collect();
        Self { items, entries }
    }

    /// Items needed for `blocks` (palette indices), most numerous first.
    pub fn count(&self, blocks: impl IntoIterator<Item = u16>) -> Vec<ItemCount> {
        let mut per_block = vec![0u64; self.entries.len()];
        for block in blocks {
            if let Some(count) = per_block.get_mut(usize::from(block)) {
                *count += 1;
            }
        }
        let mut per_item = vec![0u64; self.items.len()];
        for (count, entry) in per_block.into_iter().zip(&self.entries) {
            if let Some((item, quantity)) = entry {
                per_item[*item] += count * u64::from(*quantity);
            }
        }
        let mut counts: Vec<ItemCount> = per_item
            .into_iter()
            .zip(&self.items)
            .filter(|(count, _)| *count > 0)
            .map(|(count, (item, block))| ItemCount {
                item: item.clone(),
                count,
                block: *block,
            })
            .collect();
        counts.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.item.cmp(&b.item)));
        counts
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(state: &str) -> Option<(String, u32)> {
        item_for(&state.parse().unwrap())
    }

    fn one(name: &str) -> Option<(String, u32)> {
        Some((name.to_owned(), 1))
    }

    #[test]
    fn plain_blocks_are_one_item_whatever_their_properties() {
        assert_eq!(item("stone"), one("minecraft:stone"));
        assert_eq!(
            item("oak_stairs[facing=east,half=top]"),
            one("minecraft:oak_stairs")
        );
        assert_eq!(item("create:shaft[axis=x]"), one("create:shaft"));
    }

    #[test]
    fn invisible_and_unplaceable_blocks_are_not_items() {
        assert_eq!(item("air"), None);
        assert_eq!(item("structure_void"), None);
        assert_eq!(item("piston_head[facing=up]"), None);
    }

    #[test]
    fn two_block_blocks_are_counted_once() {
        assert_eq!(item("oak_door[half=lower]"), one("minecraft:oak_door"));
        assert_eq!(item("oak_door[half=upper]"), None);
        assert_eq!(item("sunflower[half=upper]"), None);
        assert_eq!(item("red_bed[part=foot]"), one("minecraft:red_bed"));
        assert_eq!(item("red_bed[part=head]"), None);
    }

    #[test]
    fn wall_variants_are_the_standing_item() {
        assert_eq!(item("wall_torch[facing=north]"), one("minecraft:torch"));
        assert_eq!(item("soul_wall_torch"), one("minecraft:soul_torch"));
        assert_eq!(item("oak_wall_sign"), one("minecraft:oak_sign"));
        assert_eq!(
            item("oak_wall_hanging_sign"),
            one("minecraft:oak_hanging_sign")
        );
        assert_eq!(item("white_wall_banner"), one("minecraft:white_banner"));
        assert_eq!(item("zombie_wall_head"), one("minecraft:zombie_head"));
        assert_eq!(item("redstone_wire[power=0]"), one("minecraft:redstone"));
    }

    #[test]
    fn some_blocks_take_several_items() {
        assert_eq!(
            item("oak_slab[type=double]"),
            Some(("minecraft:oak_slab".into(), 2))
        );
        assert_eq!(item("oak_slab[type=top]"), one("minecraft:oak_slab"));
        assert_eq!(
            item("white_candle[candles=3]"),
            Some(("minecraft:white_candle".into(), 3))
        );
        assert_eq!(
            item("sea_pickle[pickles=4]"),
            Some(("minecraft:sea_pickle".into(), 4))
        );
        assert_eq!(item("snow[layers=5]"), Some(("minecraft:snow".into(), 5)));
    }

    #[test]
    fn stack_sizes() {
        assert_eq!(stack_size("minecraft:stone"), 64);
        assert_eq!(stack_size("minecraft:oak_sign"), 16);
        assert_eq!(stack_size("minecraft:red_bed"), 1);
        assert_eq!(stack_size("minecraft:shulker_box"), 1);
    }

    #[test]
    fn counts_merge_states_and_sort_by_quantity() {
        let mut palette = Palette::new();
        let mut add = |s: &str| palette.get_or_insert(s.parse().unwrap()).unwrap();
        let stone = add("stone");
        let slab = add("oak_slab[type=bottom]");
        let double = add("oak_slab[type=double]");
        let door_lower = add("oak_door[half=lower]");
        let door_upper = add("oak_door[half=upper]");
        let index = MaterialIndex::new(&palette);

        let blocks = [
            stone,
            slab,
            double,
            double,
            door_lower,
            door_upper,
            Palette::AIR,
            stone,
        ];
        let counts = index.count(blocks);
        let summary: Vec<(&str, u64)> = counts.iter().map(|c| (c.item.as_str(), c.count)).collect();
        assert_eq!(
            summary,
            [
                ("minecraft:oak_slab", 5),
                ("minecraft:stone", 2),
                ("minecraft:oak_door", 1)
            ]
        );
        assert_eq!(counts[0].block, slab);
        assert!(index.count([Palette::AIR, 999]).is_empty());
    }
}
