//! Simplified block shapes, so that the views show orientation (stairs, slabs, doors...).
//!
//! A shape is a few boxes in sixteenths of a block, built from the block's properties. Boxes are
//! written for a block facing north and turned for the other directions. Blocks not listed here
//! are full cubes.

use mvb_core::BlockState;

/// Axis-aligned box in sixteenths of a block: min x, y, z then max x, y, z.
pub type Box16 = [u8; 6];

const FULL: Box16 = [0, 0, 0, 16, 16, 16];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Shape {
    pub boxes: Vec<Box16>,
}

/// Direction a block can point to, from its `facing` property.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Facing {
    North,
    East,
    South,
    West,
    Up,
    Down,
}

impl Facing {
    pub fn of(state: &BlockState) -> Option<Self> {
        Some(match state.property("facing")? {
            "north" => Facing::North,
            "east" => Facing::East,
            "south" => Facing::South,
            "west" => Facing::West,
            "up" => Facing::Up,
            "down" => Facing::Down,
            _ => return None,
        })
    }

    /// Quarter turns clockwise (seen from above) from north; `None` for up and down.
    fn quarter_turns(self) -> Option<u8> {
        match self {
            Facing::North => Some(0),
            Facing::East => Some(1),
            Facing::South => Some(2),
            Facing::West => Some(3),
            Facing::Up | Facing::Down => None,
        }
    }
}

/// Turns a box clockwise (seen from above) around the block centre.
fn turn([x0, y0, z0, x1, y1, z1]: Box16, quarter_turns: u8) -> Box16 {
    match quarter_turns % 4 {
        0 => [x0, y0, z0, x1, y1, z1],
        // North (-z) becomes east (+x): (x, z) -> (16 - z, x).
        1 => [16 - z1, y0, x0, 16 - z0, y1, x1],
        2 => [16 - x1, y0, 16 - z1, 16 - x0, y1, 16 - z0],
        _ => [z0, y0, 16 - x1, z1, y1, 16 - x0],
    }
}

/// Mirrors a box top to bottom (upside-down stairs, top slabs...).
fn flip([x0, y0, z0, x1, y1, z1]: Box16) -> Box16 {
    [x0, 16 - y1, z0, x1, 16 - y0, z1]
}

fn is(state: &BlockState, key: &str, value: &str) -> bool {
    state.property(key) == Some(value)
}

impl Shape {
    pub fn full() -> Self {
        Self { boxes: vec![FULL] }
    }

    /// True for a full cube: neighbours' faces against it are hidden.
    pub fn is_full(&self) -> bool {
        self.boxes == [FULL]
    }

    pub fn of(state: &BlockState) -> Self {
        let name = state
            .name()
            .strip_prefix("minecraft:")
            .unwrap_or(state.name());
        let turns = Facing::of(state)
            .and_then(Facing::quarter_turns)
            .unwrap_or(0);
        let facing_north = |boxes: &[Box16]| Self {
            boxes: boxes.iter().map(|&b| turn(b, turns)).collect(),
        };
        let top_half = is(state, "half", "top");
        let boxes = |list: &[Box16]| Self {
            boxes: list.to_vec(),
        };

        if name.ends_with("_slab") {
            return match state.property("type") {
                Some("top") => boxes(&[[0, 8, 0, 16, 16, 16]]),
                Some("double") => Self::full(),
                _ => boxes(&[[0, 0, 0, 16, 8, 16]]),
            };
        }
        if name.ends_with("_stairs") {
            // Facing north: the high step is on the north side; "left" is west.
            let mut list = vec![[0, 0, 0, 16, 8, 16]];
            list.extend_from_slice(match state.property("shape") {
                Some("outer_left") => &[[0, 8, 0, 8, 16, 8]],
                Some("outer_right") => &[[8, 8, 0, 16, 16, 8]],
                Some("inner_left") => &[[0, 8, 0, 16, 16, 8], [0, 8, 8, 8, 16, 16]],
                Some("inner_right") => &[[0, 8, 0, 16, 16, 8], [8, 8, 8, 16, 16, 16]],
                _ => &[[0, 8, 0, 16, 16, 8]],
            });
            if top_half {
                list = list.into_iter().map(flip).collect();
            }
            return facing_north(&list);
        }
        if name.ends_with("_trapdoor") {
            return if is(state, "open", "true") {
                // Open, facing north: against the south side of the block.
                facing_north(&[[0, 0, 13, 16, 16, 16]])
            } else if top_half {
                boxes(&[[0, 13, 0, 16, 16, 16]])
            } else {
                boxes(&[[0, 0, 0, 16, 3, 16]])
            };
        }
        if name.ends_with("_door") {
            // Closed, facing north: against the south side. Opening swings it to a side.
            let side = if !is(state, "open", "true") {
                [0, 0, 13, 16, 16, 16]
            } else if is(state, "hinge", "right") {
                [0, 0, 0, 3, 16, 16]
            } else {
                [13, 0, 0, 16, 16, 16]
            };
            return facing_north(&[side]);
        }
        if name.ends_with("_fence_gate") {
            let open = is(state, "open", "true");
            let list: &[Box16] = if open {
                &[[0, 5, 7, 2, 16, 9], [14, 5, 7, 16, 16, 9]]
            } else {
                &[[0, 5, 7, 16, 16, 9]]
            };
            return facing_north(list);
        }
        if name.ends_with("_fence") {
            return connected(state, [6, 0, 6, 10, 16, 10], &[[7, 6, 0, 9, 15, 6]]);
        }
        if name.ends_with("_wall") {
            let mut list = vec![];
            if !is(state, "up", "false") {
                list.push([4, 0, 4, 12, 16, 12]);
            }
            for (turns, side) in [(0, "north"), (1, "east"), (2, "south"), (3, "west")] {
                let height = match state.property(side) {
                    Some("low") => 14,
                    Some("tall") => 16,
                    _ => continue,
                };
                list.push(turn([5, 0, 0, 11, height, 8], turns));
            }
            if list.is_empty() {
                list.push([4, 0, 4, 12, 16, 12]);
            }
            return Self { boxes: list };
        }
        if name.ends_with("_pane") || name == "iron_bars" {
            return connected(state, [7, 0, 7, 9, 16, 9], &[[7, 0, 0, 9, 16, 7]]);
        }
        if name.ends_with("carpet") {
            return boxes(&[[0, 0, 0, 16, 1, 16]]);
        }
        if name.ends_with("_pressure_plate") {
            return boxes(&[[1, 0, 1, 15, 1, 15]]);
        }
        if name == "snow" {
            let layers: u8 = state
                .property("layers")
                .and_then(|l| l.parse().ok())
                .unwrap_or(1);
            return boxes(&[[0, 0, 0, 16, (layers.clamp(1, 8)) * 2, 16]]);
        }
        if name.ends_with("_wall_torch") {
            // Points away from the wall: facing north, the wall is to the south.
            return facing_north(&[[7, 3, 11, 9, 13, 16]]);
        }
        if name.ends_with("torch") {
            return boxes(&[[7, 0, 7, 9, 10, 9]]);
        }
        if name.ends_with("_button") {
            return match state.property("face") {
                Some("floor") => facing_north(&[[5, 0, 6, 11, 2, 10]]),
                Some("ceiling") => facing_north(&[[5, 14, 6, 11, 16, 10]]),
                _ => facing_north(&[[5, 6, 14, 11, 10, 16]]),
            };
        }
        if name == "ladder" {
            return facing_north(&[[0, 0, 13, 16, 16, 16]]);
        }
        if name.ends_with("chest") && name != "chest_minecart" {
            return boxes(&[[1, 0, 1, 15, 14, 15]]);
        }
        if name.ends_with("_bed") {
            return boxes(&[[0, 0, 0, 16, 9, 16]]);
        }
        if name.ends_with("rail") {
            return boxes(&[[0, 0, 0, 16, 2, 16]]);
        }
        if name.ends_with("chain") {
            return Self {
                boxes: vec![match state.property("axis") {
                    Some("x") => [0, 6, 6, 16, 10, 10],
                    Some("z") => [6, 6, 0, 10, 10, 16],
                    _ => [6, 0, 6, 10, 16, 10],
                }],
            };
        }
        Self::full()
    }

    /// Height of the top surface (0 = nothing, 16 = block top) for each of `res × res` columns
    /// seen from above, row by row (z) then column (x).
    pub fn top_heights(&self, res: usize) -> Vec<u8> {
        let mut heights = vec![0; res * res];
        for &[x0, _, z0, x1, y1, z1] in &self.boxes {
            for (i, height) in heights.iter_mut().enumerate() {
                // Sample at the centre of the column, in sixteenths.
                let x = ((i % res) as f32 + 0.5) * 16.0 / res as f32;
                let z = ((i / res) as f32 + 0.5) * 16.0 / res as f32;
                if x >= f32::from(x0)
                    && x < f32::from(x1)
                    && z >= f32::from(z0)
                    && z < f32::from(z1)
                {
                    *height = (*height).max(y1);
                }
            }
        }
        heights
    }
}

/// Central post plus one arm per side whose property is `true` (fences, panes); arms are
/// written for the north side.
fn connected(state: &BlockState, post: Box16, arm: &[Box16]) -> Shape {
    let mut boxes = vec![post];
    for (turns, side) in [(0, "north"), (1, "east"), (2, "south"), (3, "west")] {
        if is(state, side, "true") {
            boxes.extend(arm.iter().map(|&b| turn(b, turns)));
        }
    }
    Shape { boxes }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shape(state: &str) -> Shape {
        Shape::of(&state.parse().unwrap())
    }

    #[test]
    fn plain_blocks_are_full_cubes() {
        assert!(shape("stone").is_full());
        assert!(shape("oak_slab[type=double]").is_full());
        assert!(!shape("oak_slab[type=bottom]").is_full());
    }

    #[test]
    fn turning_keeps_boxes_inside_the_block_and_four_turns_are_identity() {
        let b = [1, 2, 3, 9, 10, 4];
        for turns in 0..4 {
            let t = turn(b, turns);
            assert!(t[0] < t[3] && t[2] < t[5] && t[3] <= 16 && t[5] <= 16);
        }
        assert_eq!(turn(turn(b, 1), 3), b);
        // The north edge (z = 0..1) ends up on the east edge (x = 15..16).
        assert_eq!(turn([0, 0, 0, 16, 16, 1], 1), [15, 0, 0, 16, 16, 16]);
    }

    #[test]
    fn stairs_rise_towards_their_facing() {
        let north = shape("oak_stairs[facing=north,half=bottom,shape=straight]").top_heights(2);
        // Rows are z: the north row (z = 0) is the high one.
        assert_eq!(north, vec![16, 16, 8, 8]);
        let east = shape("oak_stairs[facing=east,half=bottom,shape=straight]").top_heights(2);
        assert_eq!(east, vec![8, 16, 8, 16]);
        let outer = shape("oak_stairs[facing=north,shape=outer_left]").top_heights(2);
        assert_eq!(outer, vec![16, 8, 8, 8]);
        let inner = shape("oak_stairs[facing=north,shape=inner_right]").top_heights(2);
        assert_eq!(inner, vec![16, 16, 8, 16]);
    }

    #[test]
    fn upside_down_stairs_look_full_from_above() {
        let heights = shape("oak_stairs[facing=north,half=top]").top_heights(4);
        assert!(heights.iter().all(|&h| h == 16));
    }

    #[test]
    fn slabs_doors_and_trapdoors() {
        assert_eq!(shape("oak_slab[type=bottom]").top_heights(1), vec![8]);
        assert_eq!(shape("oak_slab[type=top]").top_heights(1), vec![16]);
        // Closed door facing north stands against the south side.
        let door = shape("oak_door[facing=north,open=false,hinge=left,half=lower]");
        assert_eq!(door.boxes, vec![[0, 0, 13, 16, 16, 16]]);
        let open = shape("oak_door[facing=north,open=true,hinge=left]");
        assert_eq!(open.boxes, vec![[13, 0, 0, 16, 16, 16]]);
        assert_eq!(
            shape("oak_trapdoor[half=top,open=false]").boxes,
            vec![[0, 13, 0, 16, 16, 16]]
        );
        let open_trapdoor = shape("oak_trapdoor[facing=west,open=true]");
        assert_eq!(open_trapdoor.boxes, vec![turn([0, 0, 13, 16, 16, 16], 3)]);
    }

    #[test]
    fn fences_and_panes_follow_their_connections() {
        assert_eq!(shape("oak_fence").boxes.len(), 1);
        assert_eq!(shape("oak_fence[north=true,east=true]").boxes.len(), 3);
        let pane = shape("glass_pane[east=true,west=true]").top_heights(16);
        // A horizontal line through the middle rows only.
        assert_eq!(pane[8 * 16], 16);
        assert_eq!(pane[0], 0);
    }

    #[test]
    fn misc_shapes() {
        assert_eq!(shape("snow[layers=3]").top_heights(1), vec![6]);
        assert_eq!(shape("white_carpet").top_heights(1), vec![1]);
        assert!(!shape("torch").is_full());
        assert!(!shape("oak_wall_sign[facing=north]").boxes.is_empty());
        assert_eq!(shape("cobblestone_wall[up=true]").boxes.len(), 1);
    }
}
