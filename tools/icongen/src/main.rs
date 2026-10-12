//! Draws the application icon (an isometric grass block whose sides show stacked layers) and
//! writes `packaging/icons/mvb.png` (512 px), `mvb.ico` (Windows) and `mvb.icns` (macOS).
//!
//! ```sh
//! cargo run -p icongen
//! ```

use std::process::ExitCode;

const OUTPUT_DIR: &str = "packaging/icons";
/// Samples per pixel side, for anti-aliased edges.
const SUPERSAMPLING: u32 = 4;
/// Layers drawn on the sides of the cube.
const LAYERS: f32 = 4.0;

type Rgb = [f32; 3];

const GRASS_TOP: Rgb = [0.42, 0.72, 0.29];
const GRASS_SIDE: Rgb = [0.37, 0.62, 0.24];
const DIRT: Rgb = [0.59, 0.42, 0.29];
/// Brightness of the right face, turned away from the light.
const SHADE_RIGHT: f32 = 0.78;
/// Brightness of the lines between layers.
const SHADE_LINE: f32 = 0.78;

type Point = (f32, f32);

/// True when `p` is inside the convex polygon (vertices in either winding order).
fn inside(p: Point, polygon: &[Point]) -> bool {
    let mut sign = 0.0;
    for (i, &a) in polygon.iter().enumerate() {
        let b = polygon[(i + 1) % polygon.len()];
        let cross = (b.0 - a.0) * (p.1 - a.1) - (b.1 - a.1) * (p.0 - a.0);
        if cross != 0.0 {
            if sign != 0.0 && cross.signum() != sign {
                return false;
            }
            sign = cross.signum();
        }
    }
    true
}

/// Colour of a side face at `depth` (0 at the top edge, 1 at the bottom edge).
fn side_color(depth: f32) -> Rgb {
    let base = if depth < 0.2 { GRASS_SIDE } else { DIRT };
    let layer = depth * LAYERS;
    let on_line = depth > 0.05 && (layer - layer.round()).abs() < 0.035 * LAYERS / 4.0;
    if on_line {
        base.map(|c| c * SHADE_LINE)
    } else {
        base
    }
}

/// Colour and coverage of one sample, `None` outside the cube.
fn sample(x: f32, y: f32, size: f32) -> Option<Rgb> {
    let edge = size * 0.44;
    let half_width = edge * 3f32.sqrt() / 2.0;
    let centre_x = size / 2.0;
    let top_y = size * 0.06;
    let top = (centre_x, top_y);
    let centre = (centre_x, top_y + edge);
    let left = (centre_x - half_width, top_y + edge / 2.0);
    let right = (centre_x + half_width, top_y + edge / 2.0);
    let bottom = (centre_x, top_y + 2.0 * edge);
    let left_bottom = (left.0, left.1 + edge);
    let right_bottom = (right.0, right.1 + edge);

    if inside((x, y), &[top, right, centre, left]) {
        return Some(GRASS_TOP);
    }
    // Depth below the top edge of a side face, which slopes between two vertices.
    let depth_below = |a: Point, b: Point| {
        let edge_y = a.1 + (x - a.0) * (b.1 - a.1) / (b.0 - a.0);
        ((y - edge_y) / edge).clamp(0.0, 1.0)
    };
    if inside((x, y), &[left, centre, bottom, left_bottom]) {
        return Some(side_color(depth_below(left, centre)));
    }
    if inside((x, y), &[centre, right, right_bottom, bottom]) {
        return Some(side_color(depth_below(centre, right)).map(|c| c * SHADE_RIGHT));
    }
    None
}

/// RGBA pixels of the icon at `size` × `size`.
fn render(size: u32) -> Vec<u8> {
    let mut pixels = Vec::with_capacity((size * size * 4) as usize);
    let n = SUPERSAMPLING;
    for py in 0..size {
        for px in 0..size {
            let (mut rgb, mut coverage) = ([0.0f32; 3], 0.0f32);
            for sy in 0..n {
                for sx in 0..n {
                    let x = px as f32 + (sx as f32 + 0.5) / n as f32;
                    let y = py as f32 + (sy as f32 + 0.5) / n as f32;
                    if let Some(color) = sample(x, y, size as f32) {
                        for (sum, c) in rgb.iter_mut().zip(color) {
                            *sum += c;
                        }
                        coverage += 1.0;
                    }
                }
            }
            if coverage == 0.0 {
                pixels.extend_from_slice(&[0; 4]);
            } else {
                let to_byte = |c: f32| (c.clamp(0.0, 1.0) * 255.0).round() as u8;
                pixels.extend(rgb.map(|c| to_byte(c / coverage)));
                pixels.push(to_byte(coverage / (n * n) as f32));
            }
        }
    }
    pixels
}

fn png(size: u32) -> Result<Vec<u8>, png::EncodingError> {
    let mut out = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out, size, size);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header()?;
        writer.write_image_data(&render(size))?;
    }
    Ok(out)
}

/// Windows icon: a directory of PNG images (supported since Windows Vista).
fn ico(images: &[(u32, Vec<u8>)]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&[0, 0, 1, 0]);
    out.extend_from_slice(&(images.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * images.len() as u32;
    for (size, data) in images {
        // 256 is written as 0.
        let side = if *size >= 256 { 0 } else { *size as u8 };
        out.extend_from_slice(&[side, side, 0, 0]);
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&32u16.to_le_bytes());
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(&offset.to_le_bytes());
        offset += data.len() as u32;
    }
    for (_, data) in images {
        out.extend_from_slice(data);
    }
    out
}

/// macOS icon: typed PNG entries in a big-endian container.
fn icns(images: &[(&[u8; 4], Vec<u8>)]) -> Vec<u8> {
    let body: Vec<u8> = images
        .iter()
        .flat_map(|(kind, data)| {
            let mut entry = kind.to_vec();
            entry.extend_from_slice(&(data.len() as u32 + 8).to_be_bytes());
            entry.extend_from_slice(data);
            entry
        })
        .collect();
    let mut out = b"icns".to_vec();
    out.extend_from_slice(&(body.len() as u32 + 8).to_be_bytes());
    out.extend(body);
    out
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    std::fs::create_dir_all(OUTPUT_DIR)?;
    std::fs::write(format!("{OUTPUT_DIR}/mvb.png"), png(512)?)?;

    let ico_images = [16, 24, 32, 48, 64, 128, 256]
        .into_iter()
        .map(|size| Ok((size, png(size)?)))
        .collect::<Result<Vec<_>, png::EncodingError>>()?;
    std::fs::write(format!("{OUTPUT_DIR}/mvb.ico"), ico(&ico_images))?;

    let icns_images = [
        (b"icp4", 16),
        (b"icp5", 32),
        (b"icp6", 64),
        (b"ic07", 128),
        (b"ic08", 256),
        (b"ic09", 512),
        (b"ic10", 1024),
    ]
    .into_iter()
    .map(|(kind, size)| Ok((kind, png(size)?)))
    .collect::<Result<Vec<_>, png::EncodingError>>()?;
    std::fs::write(format!("{OUTPUT_DIR}/mvb.icns"), icns(&icns_images))?;
    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => {
            println!("icons written to {OUTPUT_DIR}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icon_is_opaque_in_the_middle_and_transparent_in_the_corners() {
        let size = 64;
        let pixels = render(size);
        let alpha = |x: u32, y: u32| pixels[((y * size + x) * 4 + 3) as usize];
        assert_eq!(alpha(0, 0), 0);
        assert_eq!(alpha(size / 2, size / 2), 255);
        assert_eq!(alpha(size / 2, size - 1), 0);
    }

    #[test]
    fn containers_have_the_right_headers() {
        let ico = ico(&[(16, vec![1, 2, 3]), (256, vec![4])]);
        assert_eq!(&ico[..6], &[0, 0, 1, 0, 2, 0]);
        // Second entry: size 256 written as 0, data right after both entries and the first image.
        assert_eq!(ico[6 + 16], 0);
        assert_eq!(
            u32::from_le_bytes(ico[6 + 16 + 12..6 + 16 + 16].try_into().unwrap()),
            6 + 32 + 3
        );

        let icns = icns(&[(b"ic07", vec![9; 10])]);
        assert_eq!(&icns[..4], b"icns");
        assert_eq!(
            u32::from_be_bytes(icns[4..8].try_into().unwrap()),
            8 + 8 + 10
        );
        assert_eq!(&icns[8..12], b"ic07");
    }

    #[test]
    fn point_in_polygon() {
        let square = [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)];
        assert!(inside((0.5, 0.5), &square));
        assert!(!inside((1.5, 0.5), &square));
    }
}
