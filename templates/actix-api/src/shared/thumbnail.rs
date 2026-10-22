// Scaffolded once by rok-sqlc; this file is yours. It is never regenerated. See docs/guide/architecture.md.

//! Square JPEG thumbnails of uploaded images, with no framework in sight.
//!
//! Decoding and re-encoding is the safety boundary: what is stored is
//! always an image this module produced, so the original's metadata
//! (EXIF, GPS), trailing bytes and polyglot tricks never reach anyone who
//! fetches it. Decoding runs under [`image::Limits`], so a small file that
//! claims huge dimensions (a decompression bomb) is refused before any
//! pixel is allocated.
//!
//! The work is CPU-bound: call [`square_jpeg`] on a blocking thread
//! (`tokio::task::spawn_blocking`).

use std::io::Cursor;

use image::codecs::jpeg::JpegEncoder;
use image::imageops::FilterType;
use image::{ImageError, ImageFormat, ImageReader, Limits, Rgb, RgbImage};

use crate::error::AppError;

/// The widest or tallest image decoded, in pixels.
pub const MAX_DIMENSION: u32 = 8192;

/// The most memory one decode may allocate: 128 MiB.
pub const MAX_DECODE_BYTES: u64 = 128 * 1024 * 1024;

/// The JPEG quality thumbnails are encoded at.
const JPEG_QUALITY: u8 = 85;

/// `original` (a JPEG, PNG or WebP image) as a `side`×`side` JPEG: scaled
/// to cover the square and centre-cropped, transparency flattened onto
/// white, and re-encoded, which drops every byte of the original but its
/// pixels. Anything else, an image over [`MAX_DIMENSION`] either way, or
/// one needing more than [`MAX_DECODE_BYTES`] to decode is a `422`.
pub fn square_jpeg(original: &[u8], side: u32) -> Result<Vec<u8>, AppError> {
    let mut reader = ImageReader::new(Cursor::new(original))
        .with_guessed_format()
        .map_err(|_| not_an_image())?;
    if !matches!(
        reader.format(),
        Some(ImageFormat::Jpeg | ImageFormat::Png | ImageFormat::WebP)
    ) {
        return Err(not_an_image());
    }
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_DIMENSION);
    limits.max_image_height = Some(MAX_DIMENSION);
    limits.max_alloc = Some(MAX_DECODE_BYTES);
    reader.limits(limits);
    let decoded = reader.decode().map_err(|error| match error {
        ImageError::Limits(_) => unprocessable(format!(
            "the image is too large to process (at most {MAX_DIMENSION}×{MAX_DIMENSION} pixels)"
        )),
        _ => not_an_image(),
    })?;

    let square = decoded
        .resize_to_fill(side, side, FilterType::Lanczos3)
        .to_rgba8();
    let mut flat = RgbImage::new(side, side);
    for (x, y, pixel) in square.enumerate_pixels() {
        let [red, green, blue, alpha] = pixel.0;
        flat.put_pixel(
            x,
            y,
            Rgb([
                over_white(red, alpha),
                over_white(green, alpha),
                over_white(blue, alpha),
            ]),
        );
    }

    let mut jpeg = Vec::new();
    JpegEncoder::new_with_quality(&mut jpeg, JPEG_QUALITY)
        .encode_image(&flat)
        .map_err(|error| AppError::Internal(error.into()))?;
    Ok(jpeg)
}

/// `channel` at opacity `alpha`, composed over white.
fn over_white(channel: u8, alpha: u8) -> u8 {
    let (channel, alpha) = (u16::from(channel), u16::from(alpha));
    let composed = (channel * alpha + 255 * (255 - alpha)) / 255;
    u8::try_from(composed).unwrap_or(u8::MAX)
}

/// The `422` of bytes that aren't a JPEG, PNG or WebP image.
fn not_an_image() -> AppError {
    unprocessable("the file is not a JPEG, PNG or WebP image".to_owned())
}

/// A `422` saying `message`.
fn unprocessable(message: String) -> AppError {
    AppError::Validation {
        message,
        details: None,
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use image::{DynamicImage, ImageFormat, Rgba, RgbaImage};

    use super::*;

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    /// `image` encoded as `format` (without alpha for a JPEG).
    fn encode(image: RgbaImage, format: ImageFormat) -> Result<Vec<u8>, ImageError> {
        let image = DynamicImage::ImageRgba8(image);
        let image = if format == ImageFormat::Jpeg {
            DynamicImage::ImageRgb8(image.to_rgb8())
        } else {
            image
        };
        let mut bytes = Vec::new();
        image.write_to(&mut Cursor::new(&mut bytes), format)?;
        Ok(bytes)
    }

    /// A `width`×`height` image of `color`, encoded as `format`.
    fn plain(
        width: u32,
        height: u32,
        color: Rgba<u8>,
        format: ImageFormat,
    ) -> Result<Vec<u8>, ImageError> {
        encode(RgbaImage::from_pixel(width, height, color), format)
    }

    /// The CRC-32 a PNG chunk ends with.
    fn crc32(bytes: &[u8]) -> u32 {
        let mut crc = u32::MAX;
        for byte in bytes {
            crc ^= u32::from(*byte);
            for _ in 0..8 {
                crc = if crc & 1 == 1 {
                    (crc >> 1) ^ 0xEDB8_8320
                } else {
                    crc >> 1
                };
            }
        }
        !crc
    }

    /// Whether `result` is the `422` of an image refused.
    fn is_unprocessable(result: &Result<Vec<u8>, AppError>) -> bool {
        matches!(result, Err(AppError::Validation { .. }))
    }

    #[test]
    fn each_format_becomes_a_square_jpeg() -> TestResult {
        let red = Rgba([200, 30, 30, 255]);
        for format in [ImageFormat::Png, ImageFormat::Jpeg, ImageFormat::WebP] {
            let jpeg = square_jpeg(&plain(600, 200, red, format)?, 256)?;
            let decoded = image::load_from_memory_with_format(&jpeg, ImageFormat::Jpeg)?;
            assert_eq!(
                (decoded.width(), decoded.height()),
                (256, 256),
                "{format:?}"
            );
            let centre = decoded.to_rgb8().get_pixel(128, 128).0;
            assert!(centre[0] > 170 && centre[1] < 70, "{format:?}: {centre:?}");
        }
        Ok(())
    }

    #[test]
    fn the_centre_is_kept() -> TestResult {
        // Wide: blue left and right thirds, a green centre third.
        let mut image = RgbaImage::from_pixel(300, 100, Rgba([0, 0, 255, 255]));
        for x in 100..200 {
            for y in 0..100 {
                image.put_pixel(x, y, Rgba([0, 255, 0, 255]));
            }
        }
        let jpeg = square_jpeg(&encode(image, ImageFormat::Png)?, 64)?;
        let decoded = image::load_from_memory(&jpeg)?.to_rgb8();
        for (x, y) in [(4, 32), (32, 32), (60, 32)] {
            let pixel = decoded.get_pixel(x, y).0;
            assert!(pixel[1] > 200 && pixel[2] < 60, "({x}, {y}): {pixel:?}");
        }
        Ok(())
    }

    #[test]
    fn transparency_becomes_white() -> TestResult {
        let png = plain(64, 64, Rgba([0, 0, 0, 0]), ImageFormat::Png)?;
        let jpeg = square_jpeg(&png, 256)?;
        let decoded = image::load_from_memory(&jpeg)?.to_rgb8();
        let pixel = decoded.get_pixel(128, 128).0;
        assert!(pixel.iter().all(|channel| *channel > 245), "{pixel:?}");
        Ok(())
    }

    #[test]
    fn exif_is_dropped() -> TestResult {
        let jpeg = plain(32, 32, Rgba([10, 120, 10, 255]), ImageFormat::Jpeg)?;
        // An APP1 Exif segment right after the start-of-image marker.
        let payload = b"Exif\0\0II*\0\x08\0\0\0GPS 51.5N 0.1W secret";
        let length = u16::try_from(payload.len() + 2)?.to_be_bytes();
        let mut with_exif = jpeg[..2].to_vec();
        with_exif.extend_from_slice(&[0xFF, 0xE1, length[0], length[1]]);
        with_exif.extend_from_slice(payload);
        with_exif.extend_from_slice(&jpeg[2..]);
        assert!(with_exif.windows(4).any(|window| window == b"Exif"));

        let out = square_jpeg(&with_exif, 256)?;
        let has = |needle: &[u8]| out.windows(needle.len()).any(|window| window == needle);
        assert!(!has(b"Exif"));
        assert!(!has(b"secret"));
        assert!(!has(&[0xFF, 0xE1]));
        Ok(())
    }

    #[test]
    fn what_is_not_an_image_is_unprocessable() {
        for bytes in [
            &b""[..],
            b"<script>alert(1)</script>",
            b"GIF89a\x01\0\x01\0\x80\0\0\0\0\0\xff\xff\xff,\0\0\0\0\x01\0\x01\0\0\x02\x02D\x01\0;",
            b"\x89PNG\r\n\x1a\n then garbage",
        ] {
            assert!(is_unprocessable(&square_jpeg(bytes, 256)), "{bytes:?}");
        }
    }

    #[test]
    fn a_decompression_bomb_is_refused_before_decoding() -> TestResult {
        // A real 1×1 PNG whose header claims 100000×100000 pixels.
        let mut png = plain(1, 1, Rgba([0, 0, 0, 255]), ImageFormat::Png)?;
        png[16..20].copy_from_slice(&100_000_u32.to_be_bytes());
        png[20..24].copy_from_slice(&100_000_u32.to_be_bytes());
        let crc = crc32(&png[12..29]);
        png[29..33].copy_from_slice(&crc.to_be_bytes());

        let started = Instant::now();
        assert!(is_unprocessable(&square_jpeg(&png, 256)));
        assert!(started.elapsed() < Duration::from_secs(1));
        Ok(())
    }
}
