//! Generates `crates/mvb-app/src/block_colors.rs`: the average colour of the top and side faces of
//! every block, computed from the textures of the game's client jar. Only colours are written,
//! never textures.
//!
//! ```sh
//! cargo run --release -p colorgen -- "%APPDATA%/.minecraft/versions/1.21.11/1.21.11.jar"
//! ```
//!
//! For each block: blockstate → model (the variant without rotation when there is one) → parent
//! models → the texture of the highest "up" face and of a side face → average colour of the
//! texture's opaque pixels, multiplied by the biome tint for faces that are tinted in game.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::io::Read;
use std::process::ExitCode;

use serde_json::Value;

const DEFAULT_OUTPUT: &str = "crates/mvb-app/src/block_colors.rs";
/// Guards against cyclic parents or texture references.
const MAX_CHAIN: usize = 32;

// Default (plains biome) tints.
const GRASS: u32 = 0x91BD59;
const FOLIAGE: u32 = 0x77AB2F;
const BIRCH_FOLIAGE: u32 = 0x80A755;
const SPRUCE_FOLIAGE: u32 = 0x619961;
const MANGROVE_FOLIAGE: u32 = 0x92C648;
const WATER: u32 = 0x3F76E4;
const LILY_PAD: u32 = 0x208030;
const REDSTONE: u32 = 0xC80000;

/// The files of the jar that matter, by path relative to `assets/minecraft/`.
struct Assets {
    files: HashMap<String, Vec<u8>>,
}

impl Assets {
    fn read_jar(path: &str) -> Result<(Self, String), String> {
        let file = std::fs::File::open(path).map_err(|e| format!("{path}: {e}"))?;
        let mut jar = zip::ZipArchive::new(file).map_err(|e| format!("{path}: {e}"))?;
        let mut files = HashMap::new();
        let mut version = String::from("unknown version");
        for i in 0..jar.len() {
            let mut entry = jar.by_index(i).map_err(|e| e.to_string())?;
            let name = entry.name().map_err(|e| e.to_string())?.into_owned();
            let wanted = name
                .strip_prefix("assets/minecraft/")
                .filter(|p| {
                    p.starts_with("blockstates/")
                        || p.starts_with("models/block/")
                        || p.starts_with("textures/block/")
                })
                .map(str::to_owned);
            if wanted.is_none() && name != "version.json" {
                continue;
            }
            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes).map_err(|e| e.to_string())?;
            match wanted {
                Some(path) => {
                    files.insert(path, bytes);
                }
                None => {
                    if let Ok(json) = serde_json::from_slice::<Value>(&bytes)
                        && let Some(name) = json["name"].as_str()
                    {
                        version = name.to_owned();
                    }
                }
            }
        }
        Ok((Self { files }, version))
    }

    fn json(&self, path: &str) -> Option<Value> {
        serde_json::from_slice(self.files.get(path)?).ok()
    }
}

/// `minecraft:block/oak_log` or `block/oak_log` → `oak_log` style path inside a folder.
fn resource_path(id: &str) -> &str {
    id.strip_prefix("minecraft:").unwrap_or(id)
}

/// Textures and elements of a model after following its parents.
#[derive(Debug, Default)]
struct Model {
    textures: HashMap<String, String>,
    elements: Vec<Value>,
}

fn load_model(assets: &Assets, id: &str) -> Option<Model> {
    let mut model = Model::default();
    let mut elements = None;
    let mut next = Some(id.to_owned());
    for _ in 0..MAX_CHAIN {
        let Some(id) = next.take() else { break };
        let json = assets.json(&format!("models/{}.json", resource_path(&id)))?;
        if let Some(textures) = json["textures"].as_object() {
            for (key, value) in textures {
                if let Some(value) = value.as_str() {
                    // The child is read first: it wins over its parents.
                    model
                        .textures
                        .entry(key.clone())
                        .or_insert_with(|| value.to_owned());
                }
            }
        }
        if elements.is_none() {
            elements = json["elements"].as_array().cloned();
        }
        next = json["parent"].as_str().map(str::to_owned);
    }
    model.elements = elements.unwrap_or_default();
    Some(model)
}

impl Model {
    /// Follows `#name` references to a texture id.
    fn texture(&self, reference: &str) -> Option<String> {
        let mut current = reference.to_owned();
        for _ in 0..MAX_CHAIN {
            match current.strip_prefix('#') {
                Some(name) => current = self.textures.get(name)?.clone(),
                None => return Some(current),
            }
        }
        None
    }

    /// Texture of the given face of an element, and whether it is tinted.
    fn face(&self, element: &Value, direction: &str) -> Option<(String, bool)> {
        let face = &element["faces"][direction];
        let reference = face["texture"].as_str()?;
        // Some models name a texture variable without the leading `#`; the game accepts it.
        let reference = if !reference.starts_with('#') && self.textures.contains_key(reference) {
            format!("#{reference}")
        } else {
            reference.to_owned()
        };
        let texture = self.texture(&reference)?;
        Some((texture, face.get("tintindex").is_some()))
    }

    /// Faces seen from above and from the side; the particle texture for models drawn by code.
    fn top_and_side(&self) -> Option<((String, bool), (String, bool))> {
        let height = |e: &Value| e["to"][1].as_f64().unwrap_or(0.0);
        let volume = |e: &Value| {
            (0..3)
                .map(|i| e["to"][i].as_f64().unwrap_or(0.0) - e["from"][i].as_f64().unwrap_or(0.0))
                .product::<f64>()
        };
        let top = first_max(
            self.elements
                .iter()
                .filter(|e| e["faces"]["up"].is_object()),
            height,
        )
        .and_then(|e| self.face(e, "up"));
        let side = first_max(self.elements.iter(), volume).and_then(|e| {
            ["north", "south", "east", "west"]
                .into_iter()
                .find_map(|direction| self.face(e, direction))
        });
        let particle = || Some((self.texture("#particle")?, false));
        match (top, side) {
            (Some(top), Some(side)) => Some((top, side)),
            (Some(face), None) | (None, Some(face)) => Some((face.clone(), face)),
            (None, None) => particle().map(|p| (p.clone(), p)),
        }
    }
}

/// Element with the largest `key`; the first one on ties, since later elements are overlays
/// (grass side fringe, redstone dust overlay...).
fn first_max<'a>(
    elements: impl Iterator<Item = &'a Value>,
    key: impl Fn(&Value) -> f64,
) -> Option<&'a Value> {
    elements.fold(None, |best, e| match best {
        Some(b) if key(b) >= key(e) => Some(b),
        _ => Some(e),
    })
}

/// Model of the variant without rotation (the block's reference orientation) when there is one.
fn block_model(blockstate: &Value) -> Option<String> {
    let candidates: Vec<&Value> = if let Some(variants) = blockstate["variants"].as_object() {
        variants
            .values()
            .flat_map(|v| match v {
                Value::Array(list) => list.iter().collect(),
                other => vec![other],
            })
            .collect()
    } else {
        blockstate["multipart"]
            .as_array()?
            .iter()
            .flat_map(|part| match &part["apply"] {
                Value::Array(list) => list.iter().collect(),
                other => vec![other],
            })
            .collect()
    };
    let unrotated =
        |v: &&&Value| v["x"].as_i64().unwrap_or(0) == 0 && v["y"].as_i64().unwrap_or(0) == 0;
    let chosen = candidates
        .iter()
        .find(unrotated)
        .or_else(|| candidates.first())?;
    chosen["model"].as_str().map(str::to_owned)
}

/// Colour multiplied into tinted faces, which are greyscale in the texture files.
fn tint_for(block: &str) -> u32 {
    match block {
        "birch_leaves" => BIRCH_FOLIAGE,
        "spruce_leaves" => SPRUCE_FOLIAGE,
        "mangrove_leaves" => MANGROVE_FOLIAGE,
        "lily_pad" => LILY_PAD,
        "redstone_wire" => REDSTONE,
        b if b.contains("water") || b == "bubble_column" => WATER,
        b if b.contains("leaves") || b.contains("vine") => FOLIAGE,
        _ => GRASS,
    }
}

fn srgb_to_linear(c: u8) -> f64 {
    let c = f64::from(c) / 255.0;
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn linear_to_srgb(c: f64) -> u8 {
    let c = if c <= 0.003_130_8 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    };
    (c.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// Average colour (0xRRGGBB) of a PNG's opaque pixels, first animation frame only,
/// optionally multiplied by `tint` like the game does.
fn average_color(png_bytes: &[u8], tint: Option<u32>) -> Option<u32> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(png_bytes));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().ok()?;
    let mut buffer = vec![0; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut buffer).ok()?;
    let channels = info.color_type.samples();
    let width = info.width as usize;
    // Animated textures stack their frames vertically: keep the first square.
    let height = (info.height as usize).min(width);

    let [tr, tg, tb] = match tint {
        Some(t) => [(t >> 16) as u8, (t >> 8) as u8, t as u8].map(|c| f64::from(c) / 255.0),
        None => [1.0; 3],
    };
    let (mut sum, mut weight) = ([0.0f64; 3], 0.0f64);
    for pixel in buffer[..width * height * channels].chunks_exact(channels) {
        let (rgb, alpha) = match channels {
            1 => ([pixel[0]; 3], 255),
            2 => ([pixel[0]; 3], pixel[1]),
            3 => ([pixel[0], pixel[1], pixel[2]], 255),
            _ => ([pixel[0], pixel[1], pixel[2]], pixel[3]),
        };
        if alpha == 0 {
            continue;
        }
        let a = f64::from(alpha) / 255.0;
        let tinted = [
            f64::from(rgb[0]) * tr,
            f64::from(rgb[1]) * tg,
            f64::from(rgb[2]) * tb,
        ];
        for (s, c) in sum.iter_mut().zip(tinted) {
            *s += srgb_to_linear(c.round() as u8) * a;
        }
        weight += a;
    }
    if weight == 0.0 {
        return None;
    }
    let [r, g, b] = sum.map(|s| u32::from(linear_to_srgb(s / weight)));
    Some(r << 16 | g << 8 | b)
}

/// Top and side colours of one block, or `None` for blocks without a texture (air...).
fn block_colors(assets: &Assets, block: &str) -> Option<(u32, u32)> {
    let blockstate = assets.json(&format!("blockstates/{block}.json"))?;
    let model = load_model(assets, &block_model(&blockstate)?)?;
    let (top, side) = model.top_and_side()?;
    // Fluids have no elements, hence no tint index, but are tinted in game.
    let always_tinted = block == "water" || block == "bubble_column";
    let color = |(texture, tinted): (String, bool)| {
        let png = assets
            .files
            .get(&format!("textures/{}.png", resource_path(&texture)))?;
        average_color(png, (tinted || always_tinted).then(|| tint_for(block)))
    };
    let top = color(top)?;
    let side = color(side).unwrap_or(top);
    Some((top, side))
}

fn generate(assets: &Assets, version: &str) -> (String, usize, Vec<String>) {
    let mut blocks: Vec<&str> = assets
        .files
        .keys()
        .filter_map(|p| p.strip_prefix("blockstates/")?.strip_suffix(".json"))
        .collect();
    blocks.sort_unstable();

    let mut out = String::new();
    let _ = writeln!(
        out,
        "// @generated by `cargo run -p colorgen` from Minecraft {version}. Do not edit."
    );
    let _ = writeln!(out);
    let _ = writeln!(
        out,
        "/// (block name without the `minecraft:` namespace, top colour, side colour),"
    );
    let _ = writeln!(
        out,
        "/// colours as 0xRRGGBB, sorted by name for binary search."
    );
    let _ = writeln!(out, "#[rustfmt::skip]");
    let _ = writeln!(out, "pub static BLOCK_COLORS: &[(&str, u32, u32)] = &[");
    let (mut count, mut missing) = (0, Vec::new());
    for block in blocks {
        match block_colors(assets, block) {
            Some((top, side)) => {
                let _ = writeln!(out, "    (\"{block}\", 0x{top:06X}, 0x{side:06X}),");
                count += 1;
            }
            None => missing.push(block.to_owned()),
        }
    }
    let _ = writeln!(out, "];");
    (out, count, missing)
}

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let Some(jar) = args.next() else {
        eprintln!("usage: colorgen <client .jar> [output .rs]");
        return ExitCode::FAILURE;
    };
    let output = args.next().unwrap_or_else(|| DEFAULT_OUTPUT.to_owned());
    let (assets, version) = match Assets::read_jar(&jar) {
        Ok(read) => read,
        Err(error) => {
            eprintln!("cannot read the jar: {error}");
            return ExitCode::FAILURE;
        }
    };
    let (source, count, missing) = generate(&assets, &version);
    if let Err(error) = std::fs::write(&output, source) {
        eprintln!("cannot write {output}: {error}");
        return ExitCode::FAILURE;
    }
    println!("{count} blocks written to {output} (Minecraft {version})");
    println!("{} without colour: {}", missing.len(), missing.join(", "));
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 2x4 RGBA image: an animated texture with two 2x2 frames.
    fn png(frames: &[[u8; 4]], width: u32, height: u32) -> Vec<u8> {
        let mut out = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut out, width, height);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().unwrap();
            let data: Vec<u8> = frames.iter().flatten().copied().collect();
            writer.write_image_data(&data).unwrap();
        }
        out
    }

    fn assets(files: &[(&str, &str)], textures: &[(&str, Vec<u8>)]) -> Assets {
        let mut map: HashMap<String, Vec<u8>> = files
            .iter()
            .map(|(path, json)| (path.to_string(), json.as_bytes().to_vec()))
            .collect();
        for (path, bytes) in textures {
            map.insert(path.to_string(), bytes.clone());
        }
        Assets { files: map }
    }

    const CUBE: &str = r##"{"elements": [{"from": [0,0,0], "to": [16,16,16], "faces": {
        "up": {"texture": "#up"}, "north": {"texture": "#north"}}}]}"##;
    const CUBE_COLUMN: &str = r##"{"parent": "block/cube", "textures": {
        "particle": "#side", "up": "#end", "north": "#side"}}"##;

    #[test]
    fn follows_parents_and_texture_references() {
        let red = png(&[[255, 0, 0, 255]; 4], 2, 2);
        let blue = png(&[[0, 0, 255, 255]; 4], 2, 2);
        let assets = assets(
            &[
                (
                    "blockstates/log.json",
                    r#"{"variants": {
                    "axis=x": {"model": "minecraft:block/log_horizontal", "x": 90},
                    "axis=y": {"model": "minecraft:block/log"}}}"#,
                ),
                (
                    "models/block/log.json",
                    r#"{"parent": "minecraft:block/cube_column", "textures": {
                    "end": "minecraft:block/log_top", "side": "block/log_side"}}"#,
                ),
                ("models/block/cube_column.json", CUBE_COLUMN),
                ("models/block/cube.json", CUBE),
            ],
            &[
                ("textures/block/log_top.png", red),
                ("textures/block/log_side.png", blue),
            ],
        );
        assert_eq!(block_colors(&assets, "log"), Some((0xFF0000, 0x0000FF)));
    }

    #[test]
    fn models_drawn_by_code_use_the_particle_texture() {
        let grey = png(&[[128, 128, 128, 255]; 4], 2, 2);
        let assets = assets(
            &[
                (
                    "blockstates/chest.json",
                    r#"{"variants": {"": {"model": "minecraft:block/chest"}}}"#,
                ),
                (
                    "models/block/chest.json",
                    r#"{"textures": {"particle": "minecraft:block/planks"}}"#,
                ),
            ],
            &[("textures/block/planks.png", grey)],
        );
        assert_eq!(block_colors(&assets, "chest"), Some((0x808080, 0x808080)));
    }

    #[test]
    fn tinted_faces_are_multiplied_by_the_biome_colour() {
        let white = png(&[[255, 255, 255, 255]; 4], 2, 2);
        let assets = assets(
            &[
                (
                    "blockstates/oak_leaves.json",
                    r#"{"variants": {"": {"model": "minecraft:block/oak_leaves"}}}"#,
                ),
                (
                    "models/block/oak_leaves.json",
                    r##"{"textures": {"all": "block/oak_leaves"}, "elements": [
                    {"from": [0,0,0], "to": [16,16,16], "faces": {"up": {"texture": "#all", "tintindex": 0}}}]}"##,
                ),
            ],
            &[("textures/block/oak_leaves.png", white)],
        );
        assert_eq!(
            block_colors(&assets, "oak_leaves"),
            Some((FOLIAGE, FOLIAGE))
        );
    }

    #[test]
    fn average_ignores_transparency_and_later_frames() {
        // First frame: two opaque black pixels and two transparent ones; second frame white.
        let frames = [
            [0, 0, 0, 255],
            [9, 9, 9, 0],
            [0, 0, 0, 255],
            [9, 9, 9, 0],
            [255; 4],
            [255; 4],
            [255; 4],
            [255; 4],
        ];
        assert_eq!(average_color(&png(&frames, 2, 4), None), Some(0x000000));
        let fully_transparent = png(&[[1, 2, 3, 0]; 4], 2, 2);
        assert_eq!(average_color(&fully_transparent, None), None);
    }

    #[test]
    fn average_is_computed_in_linear_light() {
        // Black and white average to mid-grey light, which is 188 in sRGB, not 128.
        let checker = png(&[[0, 0, 0, 255], [255; 4], [255; 4], [0, 0, 0, 255]], 2, 2);
        assert_eq!(average_color(&checker, None), Some(0xBCBCBC));
    }

    #[test]
    fn overlays_and_references_without_hash_are_handled() {
        let dirt = png(&[[100, 60, 20, 255]; 4], 2, 2);
        let fringe = png(&[[0, 255, 0, 255]; 4], 2, 2);
        let assets = assets(
            &[
                (
                    "blockstates/grassy.json",
                    r#"{"variants": {"": {"model": "block/grassy"}}}"#,
                ),
                (
                    "models/block/grassy.json",
                    r#"{"textures": {"side": "block/dirt", "overlay": "block/fringe"}, "elements": [
                    {"from": [0,0,0], "to": [16,16,16], "faces": {"up": {"texture": "side"}, "north": {"texture": "side"}}},
                    {"from": [0,0,0], "to": [16,16,16], "faces": {"up": {"texture": "overlay"}, "north": {"texture": "overlay"}}}]}"#,
                ),
            ],
            &[
                ("textures/block/dirt.png", dirt),
                ("textures/block/fringe.png", fringe),
            ],
        );
        assert_eq!(block_colors(&assets, "grassy"), Some((0x643C14, 0x643C14)));
    }

    #[test]
    fn unrotated_variant_is_preferred() {
        let state: Value = serde_json::from_str(
            r#"{"variants": {
            "facing=east": {"model": "a", "y": 90}, "facing=north": {"model": "b"}}}"#,
        )
        .unwrap();
        assert_eq!(block_model(&state).as_deref(), Some("b"));
        let multipart: Value =
            serde_json::from_str(r#"{"multipart": [{"apply": [{"model": "c"}]}]}"#).unwrap();
        assert_eq!(block_model(&multipart).as_deref(), Some("c"));
    }

    #[test]
    fn cyclic_references_do_not_loop() {
        let assets = assets(
            &[
                (
                    "blockstates/loop.json",
                    r#"{"variants": {"": {"model": "block/loop"}}}"#,
                ),
                (
                    "models/block/loop.json",
                    r##"{"parent": "block/loop", "textures": {"a": "#b", "b": "#a", "particle": "#a"}}"##,
                ),
            ],
            &[],
        );
        assert_eq!(block_colors(&assets, "loop"), None);
    }
}
