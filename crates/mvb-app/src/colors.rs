//! Display colour of each block.

use eframe::egui::Color32;
use eframe::egui::ecolor::Hsva;
use mvb_core::BlockState;

use crate::block_colors::BLOCK_COLORS;

/// Colour of a block seen from above, or `None` for blocks that are not drawn (air, structure voids).
///
/// Vanilla blocks use the table generated from the game's textures. A log, pillar or chain lying
/// along x or z shows its side upwards, so it takes the side colour. Blocks missing from the table
/// (mods, blocks newer than the table) get a stable colour derived from their name.
pub fn block_color(state: &BlockState) -> Option<Color32> {
    if !state.is_visible() {
        return None;
    }
    let lying = matches!(state.property("axis"), Some("x" | "z"));
    let from_table = state.name().strip_prefix("minecraft:").and_then(|path| {
        let index = BLOCK_COLORS
            .binary_search_by(|(name, ..)| name.cmp(&path))
            .ok()?;
        let (_, top, side) = BLOCK_COLORS[index];
        Some(if lying { side } else { top })
    });
    Some(from_table.map_or_else(|| name_color(state.name()), rgb))
}

fn rgb(color: u32) -> Color32 {
    let [_, r, g, b] = color.to_be_bytes();
    Color32::from_rgb(r, g, b)
}

/// Stable colour derived from a block name, for blocks without a known colour.
fn name_color(name: &str) -> Color32 {
    // FNV-1a: stable across runs and platforms.
    let hash = name.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3)
    });
    let hue = (hash % 360) as f32 / 360.0;
    let saturation = 0.35 + ((hash >> 16) % 30) as f32 / 100.0;
    let value = 0.55 + ((hash >> 32) % 30) as f32 / 100.0;
    Hsva::new(hue, saturation, value, 1.0).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_is_sorted_and_unique() {
        assert!(BLOCK_COLORS.len() > 1000);
        assert!(BLOCK_COLORS.windows(2).all(|w| w[0].0 < w[1].0));
    }

    #[test]
    fn vanilla_blocks_use_the_table() {
        let stone = block_color(&BlockState::new("stone")).unwrap();
        // Stone is a neutral grey.
        assert_eq!(stone.r(), stone.g());
        assert_eq!(stone.g(), stone.b());
        assert_ne!(stone, name_color("minecraft:stone"));
    }

    #[test]
    fn lying_logs_show_their_side() {
        let upright: BlockState = "oak_log[axis=y]".parse().unwrap();
        let lying: BlockState = "oak_log[axis=x]".parse().unwrap();
        assert_ne!(block_color(&upright), block_color(&lying));
        let along_z: BlockState = "oak_log[axis=z]".parse().unwrap();
        assert_eq!(block_color(&lying), block_color(&along_z));
    }

    #[test]
    fn unknown_blocks_get_a_stable_colour() {
        let shaft = BlockState::new("create:shaft");
        assert_eq!(block_color(&shaft), Some(name_color("create:shaft")));
    }

    #[test]
    fn invisible_blocks_have_no_colour() {
        assert_eq!(block_color(&BlockState::air()), None);
        assert_eq!(block_color(&BlockState::new("structure_void")), None);
    }

    #[test]
    fn colour_ignores_orientation() {
        let north: BlockState = "oak_stairs[facing=north]".parse().unwrap();
        let south: BlockState = "oak_stairs[facing=south]".parse().unwrap();
        assert_eq!(block_color(&north), block_color(&south));
    }
}
