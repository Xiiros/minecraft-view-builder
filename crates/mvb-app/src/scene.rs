//! CPU side of the 3D view: the shown blocks as a voxel grid, the mesh of their visible faces,
//! and picking the block under the pointer.

use std::sync::Arc;

use eframe::egui::Color32;
use glam::Vec3;
use mvb_core::Palette;

use crate::document::Document;
use crate::shapes::{Box16, Shape};

/// One corner of a face, as uploaded to the GPU: position then sRGB colour, 16 bytes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Vertex {
    pub position: [f32; 3],
    pub color: [u8; 4],
}

#[derive(Debug, Default)]
pub struct Mesh {
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u32>,
}

impl Mesh {
    pub fn vertex_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(self.vertices.len() * 16);
        for vertex in &self.vertices {
            for coordinate in vertex.position {
                bytes.extend_from_slice(&coordinate.to_le_bytes());
            }
            bytes.extend_from_slice(&vertex.color);
        }
        bytes
    }

    pub fn index_bytes(&self) -> Vec<u8> {
        self.indices.iter().flat_map(|i| i.to_le_bytes()).collect()
    }
}

/// A face of the unit cube: outward normal, corners counter-clockwise seen from outside
/// (the GPU culls the other side), brightness, and whether it uses the top colour.
struct Face {
    normal: [i64; 3],
    corners: [[f32; 3]; 4],
    shade: f32,
    top: bool,
}

const FACES: [Face; 6] = [
    Face {
        normal: [0, 1, 0],
        corners: [[0., 1., 0.], [0., 1., 1.], [1., 1., 1.], [1., 1., 0.]],
        shade: 1.0,
        top: true,
    },
    Face {
        normal: [0, -1, 0],
        corners: [[0., 0., 0.], [1., 0., 0.], [1., 0., 1.], [0., 0., 1.]],
        shade: 0.5,
        top: false,
    },
    Face {
        normal: [1, 0, 0],
        corners: [[1., 0., 0.], [1., 1., 0.], [1., 1., 1.], [1., 0., 1.]],
        shade: 0.75,
        top: false,
    },
    Face {
        normal: [-1, 0, 0],
        corners: [[0., 0., 0.], [0., 0., 1.], [0., 1., 1.], [0., 1., 0.]],
        shade: 0.75,
        top: false,
    },
    Face {
        normal: [0, 0, 1],
        corners: [[0., 0., 1.], [1., 0., 1.], [1., 1., 1.], [0., 1., 1.]],
        shade: 0.88,
        top: false,
    },
    Face {
        normal: [0, 0, -1],
        corners: [[0., 0., 0.], [0., 1., 0.], [1., 1., 0.], [1., 0., 0.]],
        shade: 0.88,
        top: false,
    },
];

/// True when the face of `bounds` with this normal lies on the block's boundary.
fn on_boundary(bounds: Box16, normal: [i64; 3]) -> bool {
    match normal {
        [1, _, _] => bounds[3] == 16,
        [-1, _, _] => bounds[0] == 0,
        [_, 1, _] => bounds[4] == 16,
        [_, -1, _] => bounds[1] == 0,
        [_, _, 1] => bounds[5] == 16,
        _ => bounds[2] == 0,
    }
}

fn shaded(color: Color32, shade: f32) -> [u8; 4] {
    let scale = |c: u8| (f32::from(c) * shade).round() as u8;
    [scale(color.r()), scale(color.g()), scale(color.b()), 255]
}

/// The blocks shown in the 3D view, in a dense box the size of the build's bounding box.
pub struct Scene {
    pub size: [u32; 3],
    /// Palette index of each cell, `Palette::AIR` where nothing is shown;
    /// index = (y * size_z + z) * size_x + x.
    cells: Vec<u16>,
    pub mesh: Arc<Mesh>,
}

impl Scene {
    /// Visible regions and layers of `doc`, with the mesh of the faces that can be seen.
    pub fn new(doc: &Document) -> Self {
        let size = doc.size;
        let [width, height, depth] = size.map(|s| s as usize);
        let area = width * depth;
        let mut cells = vec![Palette::AIR; area * height];
        for y in 0..height {
            if doc.layer_visible.get(y).copied().unwrap_or(false) {
                cells[y * area..(y + 1) * area].copy_from_slice(&doc.layer_cells(y as u32));
            }
        }
        let mut scene = Self {
            size,
            cells,
            mesh: Arc::default(),
        };
        scene.mesh = Arc::new(scene.build_mesh(doc));
        scene
    }

    /// Block at a position, `Palette::AIR` outside the box.
    fn get(&self, x: i64, y: i64, z: i64) -> u16 {
        let [w, h, d] = self.size.map(i64::from);
        if x < 0 || y < 0 || z < 0 || x >= w || y >= h || z >= d {
            return Palette::AIR;
        }
        self.cells[((y * d + z) * w + x) as usize]
    }

    /// One quad per face of each box of each block's shape. A face lying on the block's boundary
    /// is skipped when the neighbour on that side is a full cube, which hides it.
    fn build_mesh(&self, doc: &Document) -> Mesh {
        let mut mesh = Mesh::default();
        let shape = |block: u16| doc.shapes.get(usize::from(block));
        let is_full =
            |block: u16| doc.colors(block).is_some() && shape(block).is_some_and(Shape::is_full);
        let [w, h, d] = self.size.map(i64::from);
        for y in 0..h {
            for z in 0..d {
                for x in 0..w {
                    let block = self.get(x, y, z);
                    let (Some(colors), Some(shape)) = (doc.colors(block), shape(block)) else {
                        continue;
                    };
                    for &bounds in &shape.boxes {
                        let [x0, y0, z0, x1, y1, z1] = bounds.map(|v| f32::from(v) / 16.0);
                        for face in &FACES {
                            let [nx, ny, nz] = face.normal;
                            if on_boundary(bounds, face.normal)
                                && is_full(self.get(x + nx, y + ny, z + nz))
                            {
                                continue;
                            }
                            let color =
                                shaded(if face.top { colors.top } else { colors.side }, face.shade);
                            let first = mesh.vertices.len() as u32;
                            for [cx, cy, cz] in face.corners {
                                mesh.vertices.push(Vertex {
                                    position: [
                                        x as f32 + x0 + cx * (x1 - x0),
                                        y as f32 + y0 + cy * (y1 - y0),
                                        z as f32 + z0 + cz * (z1 - z0),
                                    ],
                                    color,
                                });
                            }
                            mesh.indices.extend([
                                first,
                                first + 1,
                                first + 2,
                                first,
                                first + 2,
                                first + 3,
                            ]);
                        }
                    }
                }
            }
        }
        mesh
    }

    /// First shown block hit by a ray, with its cell coordinates. Grid traversal (Amanatides & Woo).
    pub fn pick(&self, origin: Vec3, direction: Vec3) -> Option<([u32; 3], u16)> {
        let size = Vec3::new(
            self.size[0] as f32,
            self.size[1] as f32,
            self.size[2] as f32,
        );
        // Enter the box (slab method).
        let inverse = direction.recip();
        let t0 = (Vec3::ZERO - origin) * inverse;
        let t1 = (size - origin) * inverse;
        let t_enter = t0.min(t1).max_element().max(0.0);
        let t_exit = t0.max(t1).min_element();
        if t_enter.is_nan() || t_exit.is_nan() || t_enter > t_exit {
            return None;
        }
        let start = origin + direction * (t_enter + 1e-4);
        let mut cell = start.floor().clamp(Vec3::ZERO, size - Vec3::ONE);
        let step = direction.signum();
        let next_boundary = cell + step.max(Vec3::ZERO);
        let mut t_max = (next_boundary - start) * inverse;
        let t_delta = inverse.abs();
        for _ in 0..(self.size.iter().sum::<u32>() + 3) {
            let [x, y, z] = cell.to_array().map(|c| c as i64);
            if x < 0 || y < 0 || z < 0 || cell.cmpge(size).any() {
                return None;
            }
            let block = self.get(x, y, z);
            if block != Palette::AIR {
                return Some(([x as u32, y as u32, z as u32], block));
            }
            let axis = if t_max.x < t_max.y {
                if t_max.x < t_max.z { 0 } else { 2 }
            } else if t_max.y < t_max.z {
                1
            } else {
                2
            };
            cell[axis] += step[axis];
            t_max[axis] += t_delta[axis];
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mvb_core::{BlockState, Build, Pos, Region, Size};

    /// 3x2x3 build: a 3x1x3 stone floor and one glass block on top of its centre.
    fn doc() -> Document {
        let mut build = Build::new();
        let stone = build
            .palette
            .get_or_insert(BlockState::new("stone"))
            .unwrap();
        let glass = build
            .palette
            .get_or_insert(BlockState::new("glass"))
            .unwrap();
        let mut region = Region::new("r", Pos::default(), Size::new(3, 2, 3)).unwrap();
        for x in 0..3 {
            for z in 0..3 {
                region.set(x, 0, z, stone);
            }
        }
        region.set(1, 1, 1, glass);
        build.regions.push(region);
        Document::new(build, "test".into())
    }

    #[test]
    fn faces_are_counter_clockwise_seen_from_outside() {
        for face in &FACES {
            let [a, b, c, _] = face.corners.map(Vec3::from);
            let normal = (b - a).cross(c - a);
            let expected = Vec3::new(
                face.normal[0] as f32,
                face.normal[1] as f32,
                face.normal[2] as f32,
            );
            assert_eq!(normal, expected);
        }
    }

    #[test]
    fn hidden_faces_are_skipped() {
        let scene = Scene::new(&doc());
        // Floor: 9 tops (one hidden under the glass) + 9 bottoms + 12 outer sides = 29.
        // Glass: 5 faces (its bottom touches the floor).
        assert_eq!(scene.mesh.vertices.len(), (29 + 5) * 4);
        assert_eq!(scene.mesh.indices.len(), (29 + 5) * 6);
        assert_eq!(
            scene.mesh.vertex_bytes().len(),
            scene.mesh.vertices.len() * 16
        );
    }

    #[test]
    fn hidden_layers_are_not_meshed_and_uncover_faces() {
        let mut doc = doc();
        doc.layer_visible[1] = false;
        let scene = Scene::new(&doc);
        // Only the floor: 9 tops + 9 bottoms + 12 sides.
        assert_eq!(scene.mesh.vertices.len(), 30 * 4);
        doc.layer_visible = vec![false; 2];
        assert!(Scene::new(&doc).mesh.indices.is_empty());
    }

    #[test]
    fn partial_blocks_do_not_hide_their_neighbours() {
        let mut build = Build::new();
        let stone = build
            .palette
            .get_or_insert(BlockState::new("stone"))
            .unwrap();
        let slab: BlockState = "stone_slab[type=bottom]".parse().unwrap();
        let slab = build.palette.get_or_insert(slab).unwrap();
        let region =
            Region::from_blocks("r", Pos::default(), Size::new(2, 1, 1), vec![stone, slab])
                .unwrap();
        build.regions.push(region);
        let scene = Scene::new(&Document::new(build, "test".into()));
        // Stone: 6 faces (its east face is not hidden by the half-height slab).
        // Slab: 5 faces (its west face lies against the full stone).
        assert_eq!(scene.mesh.vertices.len(), (6 + 5) * 4);
        let highest = scene
            .mesh
            .vertices
            .iter()
            .filter(|v| v.position[0] == 2.0)
            .map(|v| v.position[1]);
        assert_eq!(highest.fold(0.0, f32::max), 0.5);
    }

    #[test]
    fn picking_finds_the_first_block_on_the_ray() {
        let scene = Scene::new(&doc());
        let down = Vec3::new(0.0, -1.0, 0.0);
        // Straight down on the centre: the glass block.
        let (cell, _) = scene.pick(Vec3::new(1.5, 10.0, 1.5), down).unwrap();
        assert_eq!(cell, [1, 1, 1]);
        // Straight down on a corner: the floor.
        let (cell, _) = scene.pick(Vec3::new(0.5, 10.0, 2.5), down).unwrap();
        assert_eq!(cell, [0, 0, 2]);
        // Diagonal ray from outside the box.
        let origin = Vec3::new(-2.0, 3.5, 1.5);
        let target = Vec3::new(1.5, 1.5, 1.5);
        let (cell, _) = scene.pick(origin, (target - origin).normalize()).unwrap();
        assert_eq!(cell, [1, 1, 1]);
        // Rays missing the box, or starting inside and leaving through empty space.
        assert!(scene.pick(Vec3::new(10.0, 10.0, 10.0), down).is_none());
        assert!(scene.pick(Vec3::new(0.5, 1.5, 0.5), Vec3::Y).is_none());
    }
}
