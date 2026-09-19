//! Small deterministic inputs and decoded-pixel checks for the packaged agent guide.
use std::{fs, path::Path};

use image::{GrayImage, ImageEncoder, Luma, Rgb, RgbImage, Rgba, RgbaImage};

fn generate(dir: &Path) {
    fs::create_dir_all(dir).unwrap();
    for (name, shift) in [("photo.png", 0), ("new-photo.png", 37)] {
        RgbaImage::from_fn(192, 128, |x, y| {
            Rgba([(x + shift) as u8, (y * 2) as u8, (x ^ y) as u8, 255])
        })
        .save(dir.join(name))
        .unwrap();
    }
    let rgb = RgbImage::from_fn(192, 128, |x, y| Rgb([x as u8, y as u8, 96]));
    image::codecs::jpeg::JpegEncoder::new_with_quality(
        fs::File::create(dir.join("photo.jpg")).unwrap(),
        95,
    )
    .write_image(rgb.as_raw(), 192, 128, image::ExtendedColorType::Rgb8)
    .unwrap();
    RgbaImage::from_fn(192, 128, |_, y| {
        if y < 32 {
            Rgba([32, 64, 96, 255])
        } else {
            Rgba([0, 0, 0, 0])
        }
    })
    .save(dir.join("background.png"))
    .unwrap();
    RgbaImage::from_pixel(32, 24, Rgba([255, 0, 0, 255]))
        .save(dir.join("subject.png"))
        .unwrap();
    GrayImage::from_fn(32, 24, |x, _| Luma([if x < 16 { 128 } else { 255 }]))
        .save(dir.join("mask.png"))
        .unwrap();
}

fn main() {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    match args.as_slice() {
        [mode, dir] if mode == "generate" => generate(Path::new(dir)),
        [mode, left, right] if mode == "compare" => {
            let a = image::open(left).unwrap().into_rgba8();
            let b = image::open(right).unwrap().into_rgba8();
            assert_eq!(a.dimensions(), b.dimensions());
            assert_eq!(a, b, "decoded RGBA pixels differ");
            println!(
                "{}",
                serde_json::json!({"width":a.width(),"height":a.height(),
                    "decoded_rgba8_channels":a.as_raw().len(),"mismatched_channels":0})
            );
        }
        _ => panic!("usage: agent_fixtures generate DIR | compare LEFT RIGHT"),
    }
}
