//! Writes the phase 1 acceptance set (SSR-14) into a directory, which must not be in the repo:
//! `50mp/` holds 30 JPEGs of 8660 × 5773 (every fifth one portrait) and `stripes.png` is the
//! 8000 × 6000 image of 1 px stripes that `SUPERSLIDE_STAND_IN=stripes` draws, as a file so it
//! goes through decode, upload and mips like any image.
//!
//! `cargo run --release --example make_fixtures -- <dir>`

use std::{
    fs,
    io::BufWriter,
    path::{Path, PathBuf},
    sync::atomic::{AtomicUsize, Ordering},
    thread,
};

use image::{GrayImage, RgbImage, codecs::jpeg::JpegEncoder};

const COUNT: usize = 30;
/// 50 MP, the size of the owner's camera files.
const LONG: u32 = 8660;
const SHORT: u32 = 5773;

fn main() {
    let dir = PathBuf::from(std::env::args().nth(1).expect("usage: make_fixtures <dir>"));
    let jpegs = dir.join("50mp");
    fs::create_dir_all(&jpegs).expect("create the output directory");

    // Each image holds about 150 MB of RGB while it encodes, so the thread count bounds memory.
    let threads = thread::available_parallelism().map_or(4, |n| n.get().min(8));
    let next = AtomicUsize::new(0);
    thread::scope(|scope| {
        for _ in 0..threads {
            scope.spawn(|| {
                loop {
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    if index >= COUNT {
                        break;
                    }
                    let path = jpegs.join(format!("{index:02}.jpg"));
                    write_jpeg(&path, &photo_like(index));
                    println!("{}", path.display());
                }
            });
        }
    });

    let stripes = dir.join("stripes.png");
    stripes_image()
        .save(&stripes)
        .expect("write the stripe image");
    println!("{}", stripes.display());
}

fn write_jpeg(path: &Path, image: &RgbImage) {
    let file = BufWriter::new(fs::File::create(path).expect("create a JPEG"));
    JpegEncoder::new_with_quality(file, 90)
        .encode_image(image)
        .expect("encode a JPEG");
}

/// A gradient whose hue shifts per image, grain so the JPEG is about camera-sized rather than a
/// few hundred KB of flat colour, and the index in large digits so a screenshot names the image.
fn photo_like(index: usize) -> RgbImage {
    let (width, height) = if index % 5 == 4 {
        (SHORT, LONG)
    } else {
        (LONG, SHORT)
    };
    let shift = index as u32 * 37;
    let mut seed = 0x9E37_79B9_7F4A_7C15_u64 ^ index as u64;
    let mut image = RgbImage::from_fn(width, height, |x, y| {
        // xorshift64: grain only needs to look random, not be good randomness.
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        let grain = (seed % 33) as i32 - 16;
        let channel = |base: u32| (base as i32 + grain).clamp(0, 255) as u8;
        image::Rgb([
            channel((x * 255 / width + shift) % 256),
            channel((y * 255 / height + shift * 2) % 256),
            channel(((x + y) * 255 / (width + height) + shift * 3) % 256),
        ])
    });
    draw_number(&mut image, index, 300, 300, 1400);
    image
}

/// Seven-segment digits, black on a white plate, top-left at `(left, top)`, `tall` px high.
fn draw_number(image: &mut RgbImage, number: usize, left: u32, top: u32, tall: u32) {
    // Segments a to g, as on a data sheet.
    const DIGITS: [u8; 10] = [
        0b011_1111, 0b000_0110, 0b101_1011, 0b100_1111, 0b110_0110, 0b110_1101, 0b111_1101,
        0b000_0111, 0b111_1111, 0b110_1111,
    ];
    let wide = tall / 2;
    let stroke = tall / 8;
    let text = format!("{number:02}");
    let plate_w = text.len() as u32 * (wide + stroke) + stroke;
    fill(
        image,
        left,
        top,
        plate_w,
        tall + 2 * stroke,
        [255, 255, 255],
    );
    for (i, digit) in text.bytes().enumerate() {
        let x = left + stroke + i as u32 * (wide + stroke);
        let y = top + stroke;
        let half = tall / 2;
        let segments = [
            (x, y, wide, stroke),
            (x + wide - stroke, y, stroke, half),
            (x + wide - stroke, y + half, stroke, half),
            (x, y + tall - stroke, wide, stroke),
            (x, y + half, stroke, half),
            (x, y, stroke, half),
            (x, y + half - stroke / 2, wide, stroke),
        ];
        let lit = DIGITS[usize::from(digit - b'0')];
        for (bit, &(sx, sy, sw, sh)) in segments.iter().enumerate() {
            if lit & (1 << bit) != 0 {
                fill(image, sx, sy, sw, sh, [0, 0, 0]);
            }
        }
    }
}

fn fill(image: &mut RgbImage, left: u32, top: u32, width: u32, height: u32, colour: [u8; 3]) {
    for y in top..top + height {
        for x in left..left + width {
            image.put_pixel(x, y, image::Rgb(colour));
        }
    }
}

/// Matches `shell::render::stripes`: vertical stripes on the left half, horizontal on the right.
fn stripes_image() -> GrayImage {
    GrayImage::from_fn(8000, 6000, |x, y| {
        let stripe = if x < 4000 { x } else { y };
        image::Luma([if stripe.is_multiple_of(2) { 255 } else { 0 }])
    })
}
