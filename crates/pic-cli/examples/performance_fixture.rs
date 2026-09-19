//! Deterministic benchmark inputs and decoded-pixel comparisons; no external image tools.
use std::{fs, path::Path};

use image::{ExtendedColorType, ImageEncoder, Rgb, RgbImage, Rgba, RgbaImage};

fn color(x: u32, y: u32) -> [u8; 3] {
    // Gradients, sharp tile edges and fixed coordinate noise, independent of RNG versions.
    let noise = x.wrapping_mul(1_664_525) ^ y.wrapping_mul(1_013_904_223);
    [
        ((x / 8 + y / 32 + (noise >> 28)) % 256) as u8,
        ((y / 4 + x / 64 + ((x / 96) % 2) * 64) % 256) as u8,
        ((x / 16 + y / 16 + ((noise >> 24) & 15)) % 256) as u8,
    ]
}

fn generate(dir: &Path) {
    fs::create_dir_all(dir).unwrap();
    for (name, width, height) in [("1080", 1920, 1080), ("4k", 3840, 2160)] {
        let rgb = RgbImage::from_fn(width, height, |x, y| Rgb(color(x, y)));
        image::codecs::jpeg::JpegEncoder::new_with_quality(
            fs::File::create(dir.join(format!("{name}.jpg"))).unwrap(),
            95,
        )
        .write_image(rgb.as_raw(), width, height, ExtendedColorType::Rgb8)
        .unwrap();
        let rgba = RgbaImage::from_fn(width, height, |x, y| {
            let [r, g, b] = color(width - 1 - x, y);
            Rgba([r, g, b, ((x / 8 + y / 8) % 256) as u8])
        });
        image::codecs::png::PngEncoder::new(
            fs::File::create(dir.join(format!("{name}.png"))).unwrap(),
        )
        .write_image(rgba.as_raw(), width, height, ExtendedColorType::Rgba8)
        .unwrap();
    }
}

fn main() {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    match args.as_slice() {
        [mode, dir] if mode == "generate" => generate(Path::new(dir)),
        [mode, left, right] if mode == "compare" => {
            let a = image::open(left).unwrap().into_rgba8();
            let b = image::open(right).unwrap().into_rgba8();
            assert_eq!(a.dimensions(), b.dimensions());
            let mismatches = a
                .as_raw()
                .iter()
                .zip(b.as_raw())
                .filter(|(a, b)| a != b)
                .count();
            println!(
                "{}",
                serde_json::json!({
                    "width": a.width(), "height": a.height(),
                    "decoded_rgba8_channels": a.as_raw().len(), "mismatched_channels": mismatches
                })
            );
            assert_eq!(mismatches, 0, "decoded pixels differ");
        }
        _ => panic!("usage: performance_fixture generate DIR | compare LEFT RIGHT"),
    }
}
