//! Display colour of each block.

use eframe::egui::Color32;
use eframe::egui::ecolor::Hsva;
use mvb_core::BlockState;

/// Colour of a block, or `None` for blocks that are not drawn (air, structure voids).
///
/// Temporary: derived from the block name until the colour table generated from the game's
/// textures exists. Properties are ignored, so a block keeps its colour whatever its orientation.
pub fn block_color(state: &BlockState) -> Option<Color32> {
    if !state.is_visible() {
        return None;
    }
    // FNV-1a: stable across runs and platforms.
    let hash = state
        .name()
        .bytes()
        .fold(0xcbf2_9ce4_8422_2325_u64, |h, b| {
            (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3)
        });
    let hue = (hash % 360) as f32 / 360.0;
    let saturation = 0.35 + ((hash >> 16) % 30) as f32 / 100.0;
    let value = 0.55 + ((hash >> 32) % 30) as f32 / 100.0;
    Some(Hsva::new(hue, saturation, value, 1.0).into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invisible_blocks_have_no_colour() {
        assert_eq!(block_color(&BlockState::air()), None);
        assert_eq!(block_color(&BlockState::new("structure_void")), None);
    }

    #[test]
    fn colour_ignores_properties() {
        let north: BlockState = "oak_stairs[facing=north]".parse().unwrap();
        let south: BlockState = "oak_stairs[facing=south]".parse().unwrap();
        assert_eq!(block_color(&north), block_color(&south));
        assert_ne!(block_color(&north), block_color(&BlockState::new("stone")));
    }
}
