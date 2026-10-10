//! Decoding with the third-party decoders: zune-jpeg for JPEG, `image` for the rest, kamadak-exif
//! for orientation and fast_image_resize for the oversize downscale. Their output is checked
//! against `load::domain`'s rules before it is trusted.

#![cfg_attr(
    not(test),
    expect(dead_code, reason = "scaffold: workers arrive with the preload cache")
)]

use std::io::Cursor;
use std::path::Path;

use fast_image_resize::{PixelType, Resizer, images::Image};
use image::{DynamicImage, ImageDecoder, ImageError, ImageFormat, ImageReader, Limits};
use zune_jpeg::JpegDecoder;
use zune_jpeg::zune_core::bytestream::ZCursor;
use zune_jpeg::zune_core::colorspace::ColorSpace;
use zune_jpeg::zune_core::options::DecoderOptions;

use crate::image_file::domain::{Format, Orientation};
use crate::load::domain::{DecodeError, Decoded, MAX_PIXELS, check_size, confirm, fit_within};

/// Decodes the image at `path`, downscaling it if a side exceeds `max_dim` (the GPU's texture
/// limit). The format comes from the extension, as for the scan.
pub fn decode(path: &Path, max_dim: u32) -> Result<Decoded, DecodeError> {
    let format = Format::from_path(path)
        .ok_or_else(|| DecodeError::Unsupported("Not an image this viewer opens".to_owned()))?;
    let data = std::fs::read(path).map_err(DecodeError::Io)?;
    decode_bytes(&data, format, max_dim)
}

fn decode_bytes(data: &[u8], format: Format, max_dim: u32) -> Result<Decoded, DecodeError> {
    confirm(format, data)?;
    let (original_size, pixels) = match format {
        Format::Jpeg => decode_jpeg(data)?,
        Format::Png => decode_with_image(data, ImageFormat::Png)?,
        Format::Tga => decode_with_image(data, ImageFormat::Tga)?,
        Format::Bmp => decode_with_image(data, ImageFormat::Bmp)?,
    };
    let orientation = match format {
        Format::Jpeg => jpeg_orientation(data),
        _ => Orientation::Normal,
    };
    let size = fit_within(original_size, max_dim);
    let pixels = if size == original_size {
        pixels
    } else {
        downscale(original_size, pixels, size)
    };
    Ok(Decoded {
        size,
        original_size,
        pixels,
        orientation,
    })
}

fn decode_jpeg(data: &[u8]) -> Result<([u32; 2], Vec<u8>), DecodeError> {
    let options = DecoderOptions::default()
        .jpeg_set_out_colorspace(ColorSpace::RGBA)
        // The default caps each side at 16384; `MAX_PIXELS` is the real limit.
        .set_max_width(usize::from(u16::MAX))
        .set_max_height(usize::from(u16::MAX));
    let mut decoder = JpegDecoder::new_with_options(ZCursor::new(data), options);
    decoder.decode_headers().map_err(DecodeError::damaged)?;
    let info = decoder.info().expect("headers are decoded");
    let size = [u32::from(info.width), u32::from(info.height)];
    check_size(size[0], size[1])?;
    if !scan_is_complete(data) {
        return Err(DecodeError::cut_short());
    }
    let pixels = decoder.decode().map_err(DecodeError::damaged)?;
    if decoder.output_colorspace() != Some(ColorSpace::RGBA) {
        return Err(DecodeError::Unsupported(
            "Cannot convert this JPEG's colours".to_owned(),
        ));
    }
    Ok((size, pixels))
}

/// zune-jpeg's lenient mode fills a cut-short scan with grey and reports success; its strict mode
/// would catch that but also rejects the stray bytes between segments that some cameras write. So
/// decoding stays lenient and this checks that an EOI marker follows the first top-level SOS.
/// Segments are skipped by their lengths, because an EXIF thumbnail holds its own SOS and EOI.
fn scan_is_complete(data: &[u8]) -> bool {
    // Past SOI, which `confirm` checked.
    let mut i = 2;
    loop {
        // Stray bytes before a marker, and fill bytes within one, are skipped as decoders do.
        while data.get(i).is_some_and(|&byte| byte != 0xFF) {
            i += 1;
        }
        while data.get(i) == Some(&0xFF) {
            i += 1;
        }
        let Some(&marker) = data.get(i) else {
            return false;
        };
        i += 1;
        match marker {
            0xDA => break,
            // Markers without a length.
            0x01 | 0xD0..=0xD8 => {}
            0xD9 => return false,
            _ => {
                let Some(length) = data.get(i..i + 2) else {
                    return false;
                };
                i += usize::from(u16::from_be_bytes([length[0], length[1]]));
            }
        }
    }
    // Entropy-coded data stuffs every 0xFF with 0x00, so FF D9 there can only be EOI. Searching
    // from the end finds it in the last few bytes of a normal file.
    data.get(i..)
        .is_some_and(|scan| scan.windows(2).rev().any(|pair| pair == [0xFF, 0xD9]))
}

/// Missing or unreadable EXIF means the pixels are shown as stored.
fn jpeg_orientation(data: &[u8]) -> Orientation {
    let Ok(exif) = exif::Reader::new().read_from_container(&mut Cursor::new(data)) else {
        return Orientation::Normal;
    };
    exif.get_field(exif::Tag::Orientation, exif::In::PRIMARY)
        .and_then(|field| field.value.get_uint(0))
        .map_or(Orientation::Normal, Orientation::from_exif)
}

fn decode_with_image(data: &[u8], format: ImageFormat) -> Result<([u32; 2], Vec<u8>), DecodeError> {
    let mut reader = ImageReader::with_format(Cursor::new(data), format);
    // The default 512 MiB allocation cap would refuse images `MAX_PIXELS` allows; 8 bytes a
    // pixel covers 16-bit RGBA, the widest these formats decode to.
    let mut limits = Limits::default();
    limits.max_alloc = Some(MAX_PIXELS * 8);
    reader.limits(limits);
    let decoder = reader.into_decoder().map_err(image_error)?;
    let (width, height) = decoder.dimensions();
    check_size(width, height)?;
    let image = DynamicImage::from_decoder(decoder).map_err(image_error)?;
    Ok(([width, height], image.into_rgba8().into_raw()))
}

fn image_error(error: ImageError) -> DecodeError {
    match error {
        // The data is in memory, so an I/O error means it ran out.
        ImageError::IoError(_) => DecodeError::cut_short(),
        ImageError::Limits(_) => DecodeError::Unsupported("Too large to open".to_owned()),
        ImageError::Unsupported(e) => DecodeError::Unsupported(format!("Cannot open: {e}")),
        e => DecodeError::damaged(e),
    }
}

fn downscale(from: [u32; 2], pixels: Vec<u8>, to: [u32; 2]) -> Vec<u8> {
    let source = Image::from_vec_u8(from[0], from[1], pixels, PixelType::U8x4)
        .expect("decoders return width × height RGBA pixels");
    let mut target = Image::new(to[0], to[1], PixelType::U8x4);
    // The default filter, Lanczos3, with alpha premultiplied while resampling.
    Resizer::new()
        .resize(&source, &mut target, None)
        .expect("source and target are both RGBA8");
    target.into_vec()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::Instant;

    use image::{GrayImage, Luma, Rgb, RgbImage, Rgba, RgbaImage};

    use super::*;

    const RED: [u8; 4] = [255, 0, 0, 255];
    const BLUE: [u8; 4] = [0, 0, 255, 255];

    fn encode(image: impl Into<DynamicImage>, format: ImageFormat) -> Vec<u8> {
        let mut data = Vec::new();
        image
            .into()
            .write_to(&mut Cursor::new(&mut data), format)
            .unwrap();
        data
    }

    /// Left half red, right half blue.
    fn halves(width: u32, height: u32) -> RgbImage {
        RgbImage::from_fn(width, height, |x, _| {
            if x < width / 2 {
                Rgb([255, 0, 0])
            } else {
                Rgb([0, 0, 255])
            }
        })
    }

    fn pixel(decoded: &Decoded, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * decoded.size[0] + x) * 4) as usize;
        decoded.pixels[i..i + 4].try_into().unwrap()
    }

    fn near(actual: [u8; 4], expected: [u8; 4], tolerance: u8) -> bool {
        actual
            .iter()
            .zip(expected)
            .all(|(&a, e)| a.abs_diff(e) <= tolerance)
    }

    fn fixture(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name)
    }

    #[test]
    fn each_format_decodes_to_rgba() {
        let lossless = [
            (Format::Png, ImageFormat::Png),
            (Format::Tga, ImageFormat::Tga),
            (Format::Bmp, ImageFormat::Bmp),
        ];
        for (format, encoding) in lossless {
            let decoded = decode_bytes(&encode(halves(6, 3), encoding), format, 16384).unwrap();
            assert_eq!(decoded.size, [6, 3], "{format:?}");
            assert_eq!(decoded.original_size, [6, 3], "{format:?}");
            assert_eq!(decoded.orientation, Orientation::Normal, "{format:?}");
            let expected: Vec<u8> = (0..3)
                .flat_map(|_| [RED, RED, RED, BLUE, BLUE, BLUE])
                .flatten()
                .collect();
            assert_eq!(decoded.pixels, expected, "{format:?}");
        }

        let jpeg = encode(halves(32, 16), ImageFormat::Jpeg);
        let jpeg = decode_bytes(&jpeg, Format::Jpeg, 16384).unwrap();
        assert_eq!(jpeg.size, [32, 16]);
        assert_eq!(jpeg.pixels.len(), 32 * 16 * 4);
        assert!(near(pixel(&jpeg, 4, 8), RED, 8), "{:?}", pixel(&jpeg, 4, 8));
        assert!(
            near(pixel(&jpeg, 28, 8), BLUE, 8),
            "{:?}",
            pixel(&jpeg, 28, 8)
        );

        // One-channel JPEGs are common from scanners; they must come out as RGBA too.
        let grey = encode(GrayImage::from_pixel(16, 16, Luma([90])), ImageFormat::Jpeg);
        let grey = decode_bytes(&grey, Format::Jpeg, 16384).unwrap();
        assert!(
            near(pixel(&grey, 8, 8), [90, 90, 90, 255], 2),
            "{:?}",
            pixel(&grey, 8, 8)
        );

        let translucent = RgbaImage::from_pixel(2, 2, Rgba([10, 20, 30, 128]));
        let png = encode(translucent, ImageFormat::Png);
        let png = decode_bytes(&png, Format::Png, 16384).unwrap();
        assert_eq!(pixel(&png, 1, 1), [10, 20, 30, 128]);
    }

    #[test]
    fn decode_reads_the_file_its_extension_names() {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("a.PNG");
        std::fs::write(&path, encode(halves(4, 2), ImageFormat::Png)).unwrap();
        assert_eq!(decode(&path, 16384).unwrap().size, [4, 2]);
        let gone = folder.path().join("gone.png");
        assert!(matches!(decode(&gone, 16384), Err(DecodeError::Io(_))));
        let other = folder.path().join("a.gif");
        assert!(matches!(
            decode(&other, 16384),
            Err(DecodeError::Unsupported(_))
        ));
    }

    #[test]
    fn jpeg_orientation_comes_from_exif() {
        for (name, orientation) in [
            ("orientation-3.jpg", Orientation::Rotate180),
            ("orientation-6.jpg", Orientation::Rotate90),
            ("orientation-8.jpg", Orientation::Rotate270),
        ] {
            let decoded = decode(&fixture(name), 16384).unwrap();
            assert_eq!(decoded.orientation, orientation, "{name}");
            // The pixels stay as stored: turning them upright is the view's job.
            assert_eq!(decoded.size, [8, 4], "{name}");
            assert!(near(pixel(&decoded, 0, 0), RED, 40), "{name}");
        }
        let plain = encode(halves(8, 8), ImageFormat::Jpeg);
        let plain = decode_bytes(&plain, Format::Jpeg, 16384).unwrap();
        assert_eq!(plain.orientation, Orientation::Normal);
    }

    #[test]
    fn oversized_images_shrink_to_the_limit_keeping_aspect() {
        let wide = encode(
            RgbImage::from_pixel(300, 100, Rgb([200, 100, 50])),
            ImageFormat::Png,
        );
        let decoded = decode_bytes(&wide, Format::Png, 64).unwrap();
        assert_eq!(decoded.size, [64, 21]);
        assert_eq!(decoded.original_size, [300, 100]);
        assert_eq!(decoded.pixels.len(), 64 * 21 * 4);
        assert!(decoded.pixels.chunks(4).all(|p| near(
            p.try_into().unwrap(),
            [200, 100, 50, 255],
            1
        )));

        let tall = encode(RgbImage::new(100, 300), ImageFormat::Bmp);
        let decoded = decode_bytes(&tall, Format::Bmp, 64).unwrap();
        assert_eq!(decoded.size, [21, 64]);

        // zune-jpeg's own default caps a side at 16384; a wider JPEG must still open, shrunk.
        let jpeg = encode(halves(17_000, 8), ImageFormat::Jpeg);
        let decoded = decode_bytes(&jpeg, Format::Jpeg, 16384).unwrap();
        assert_eq!(decoded.original_size, [17_000, 8]);
        assert_eq!(decoded.size, [16384, 8]);
    }

    #[test]
    fn broken_files_are_errors_not_panics() {
        let formats = [
            (Format::Png, ImageFormat::Png),
            (Format::Tga, ImageFormat::Tga),
            (Format::Bmp, ImageFormat::Bmp),
            (Format::Jpeg, ImageFormat::Jpeg),
        ];
        for (format, encoding) in formats {
            assert!(
                matches!(
                    decode_bytes(&[], format, 16384),
                    Err(DecodeError::Corrupt(_))
                ),
                "{format:?} empty"
            );
            let data = encode(halves(64, 64), encoding);
            for cut in [10, data.len() / 2] {
                let result = decode_bytes(&data[..cut], format, 16384);
                assert!(
                    matches!(result, Err(DecodeError::Corrupt(_))),
                    "{format:?} cut at {cut}: {:?}",
                    result.err()
                );
            }
        }
        // Only EOI missing: zune-jpeg alone would decode this without complaint.
        let jpeg = encode(halves(64, 64), ImageFormat::Jpeg);
        let result = decode_bytes(&jpeg[..jpeg.len() - 2], Format::Jpeg, 16384);
        assert!(matches!(result, Err(DecodeError::Corrupt(_))));
    }

    #[test]
    fn a_cut_short_jpeg_is_caught_despite_a_complete_thumbnail() {
        // An APP1 segment holding a whole JPEG, as EXIF thumbnails do, with its own SOS and EOI.
        let thumbnail = encode(halves(8, 8), ImageFormat::Jpeg);
        let main = encode(halves(64, 64), ImageFormat::Jpeg);
        let mut data = vec![0xFF, 0xD8, 0xFF, 0xE1];
        data.extend(u16::try_from(thumbnail.len() + 2).unwrap().to_be_bytes());
        data.extend(&thumbnail);
        data.extend(&main[2..]);
        assert_eq!(
            decode_bytes(&data, Format::Jpeg, 16384).unwrap().size,
            [64, 64]
        );
        // Inside the main scan: a cut in its headers fails in zune-jpeg whatever this checks.
        let cut = data.len() - 20;
        let result = decode_bytes(&data[..cut], Format::Jpeg, 16384);
        assert!(matches!(result, Err(DecodeError::Corrupt(_))));
    }

    #[test]
    fn misnamed_files_say_what_they_are() {
        let png = encode(halves(4, 4), ImageFormat::Png);
        let error = decode_bytes(&png, Format::Jpeg, 16384).err().unwrap();
        assert!(matches!(error, DecodeError::Unsupported(_)));
        assert_eq!(error.to_string(), "Misnamed: a PNG file, not JPEG");
        // TGA has no signature, but a TGA name on another format's file is still caught.
        let jpeg = encode(halves(8, 8), ImageFormat::Jpeg);
        assert!(matches!(
            decode_bytes(&jpeg, Format::Tga, 16384),
            Err(DecodeError::Unsupported(_))
        ));
        let error = decode_bytes(b"GIF89a....", Format::Jpeg, 16384)
            .err()
            .unwrap();
        assert_eq!(error.to_string(), "Not a JPEG file");
    }

    #[test]
    fn images_claiming_huge_sizes_are_refused_before_allocating() {
        // A JPEG header patched to 65535 × 65535: 17 GB as RGBA.
        let mut jpeg = encode(halves(8, 8), ImageFormat::Jpeg);
        let sof = jpeg
            .windows(2)
            .position(|pair| pair == [0xFF, 0xC0])
            .unwrap();
        jpeg[sof + 5..sof + 9].copy_from_slice(&[0xFF; 4]);
        let error = decode_bytes(&jpeg, Format::Jpeg, 16384).err().unwrap();
        assert!(matches!(error, DecodeError::Unsupported(_)), "{error}");

        // A BMP header claiming 20000 × 20000 over 8 × 8 pixels of data.
        let mut bmp = encode(halves(8, 8), ImageFormat::Bmp);
        bmp[18..22].copy_from_slice(&20_000i32.to_le_bytes());
        bmp[22..26].copy_from_slice(&20_000i32.to_le_bytes());
        let error = decode_bytes(&bmp, Format::Bmp, 16384).err().unwrap();
        assert!(matches!(error, DecodeError::Unsupported(_)), "{error}");
    }

    /// Times a real file:
    /// `SUPERSLIDE_DECODE_SAMPLE=<jpeg> cargo test --release decode_time -- --ignored --nocapture`.
    #[test]
    #[ignore = "needs a sample file"]
    fn decode_time() {
        let path = std::env::var_os("SUPERSLIDE_DECODE_SAMPLE").expect("SUPERSLIDE_DECODE_SAMPLE");
        for _ in 0..3 {
            let start = Instant::now();
            let decoded = decode(Path::new(&path), 16384).unwrap();
            let [width, height] = decoded.original_size;
            println!("{width} × {height} in {:?}", start.elapsed());
        }
    }
}
