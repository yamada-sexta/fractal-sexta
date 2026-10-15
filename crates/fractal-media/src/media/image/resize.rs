use fast_image_resize::{PixelType, Resizer, images::Image};
use gtk::{gdk, glib, prelude::*};

use super::{GlycinFrameExt, ImageError};
use crate::{RUNTIME, utils::media::FrameDimensions};

/// A displayed frame keeps bounded pixels and timing, not the full decoded
/// frame.
#[derive(Clone)]
pub struct DisplayFrame {
    pub texture: gdk::Texture,
    pub delay: Option<std::time::Duration>,
    pub bytes: u64,
}

impl DisplayFrame {
    pub async fn prepare(
        frame: glycin::Frame,
        maximum: Option<FrameDimensions>,
    ) -> Result<Self, ImageError> {
        let delay = frame.delay_duration();
        let texture = frame.texture();
        let bytes = u64::from(frame.stride()) * u64::from(frame.height());
        let source = FrameDimensions::with_texture(&texture).ok_or(ImageError::Unknown)?;
        let Some(maximum) = maximum else {
            return Ok(Self {
                texture,
                delay,
                bytes,
            });
        };
        let target = source.scale_to_fit(maximum, gtk::ContentFit::ScaleDown);
        let target = FrameDimensions {
            width: target.width.max(1),
            height: target.height.max(1),
        };
        if source == target {
            return Ok(Self {
                texture,
                delay,
                bytes,
            });
        }

        // The decoder's scale request is advisory; resize before the original reaches
        // GSK.
        let color_state = texture.color_state();
        let (pixels, format, stride) = RUNTIME
            .spawn_blocking(move || {
                use gdk::MemoryFormat as M;
                let (format, pixel_type) = match texture.format() {
                    M::R16g16b16
                    | M::R16g16b16a16
                    | M::R16g16b16a16Premultiplied
                    | M::G16
                    | M::G16a16
                    | M::G16a16Premultiplied
                    | M::A16 => (M::R16g16b16a16, PixelType::U16x4),
                    M::B8g8r8a8Premultiplied
                    | M::A8r8g8b8Premultiplied
                    | M::R8g8b8a8Premultiplied
                    | M::B8g8r8a8
                    | M::A8r8g8b8
                    | M::R8g8b8a8
                    | M::A8b8g8r8
                    | M::R8g8b8
                    | M::B8g8r8
                    | M::G8a8Premultiplied
                    | M::G8a8
                    | M::G8
                    | M::A8
                    | M::A8b8g8r8Premultiplied
                    | M::B8g8r8x8
                    | M::X8r8g8b8
                    | M::R8g8b8x8
                    | M::X8b8g8r8 => (M::R8g8b8a8, PixelType::U8x4),
                    _ => (M::R32g32b32a32Float, PixelType::F32x4),
                };
                let mut downloader = gdk::TextureDownloader::new(&texture);
                downloader.set_color_state(&texture.color_state());
                downloader.set_format(format);
                let (data, stride) = downloader.download_bytes();
                let pixels = resize_pixels(&data, stride, source, target, pixel_type)?;
                Ok::<_, ImageError>((pixels, format, target.width as usize * pixel_type.size()))
            })
            .await
            .map_err(|error| {
                tracing::warn!("Image resize task failed: {error}");
                ImageError::Unknown
            })??;
        let bytes = pixels.len() as u64;
        let texture = gdk::MemoryTextureBuilder::new()
            .set_width(target.width.try_into().map_err(|_| ImageError::Unknown)?)
            .set_height(target.height.try_into().map_err(|_| ImageError::Unknown)?)
            .set_format(format)
            .set_color_state(&color_state)
            .set_stride(stride)
            .set_bytes(Some(&glib::Bytes::from_owned(pixels)))
            .build();
        Ok(Self {
            texture,
            delay,
            bytes,
        })
    }
}

fn resize_pixels(
    data: &[u8],
    stride: usize,
    source: FrameDimensions,
    target: FrameDimensions,
    pixel_type: PixelType,
) -> Result<Vec<u8>, ImageError> {
    let row = (source.width as usize)
        .checked_mul(pixel_type.size())
        .ok_or(ImageError::Unknown)?;
    let needed = stride
        .checked_mul(source.height.saturating_sub(1) as usize)
        .and_then(|size| size.checked_add(row))
        .ok_or(ImageError::Unknown)?;
    if source.width == 0
        || source.height == 0
        || target.width == 0
        || target.height == 0
        || stride < row
        || data.len() < needed
    {
        return Err(ImageError::Unknown);
    }
    let packed = data
        .chunks(stride)
        .take(source.height as usize)
        .flat_map(|row_data| row_data[..row].iter().copied())
        .collect();
    let source = Image::from_vec_u8(source.width, source.height, packed, pixel_type)
        .map_err(|_| ImageError::Unknown)?;
    let mut target = Image::new(target.width, target.height, pixel_type);
    Resizer::new()
        .resize(&source, &mut target, None)
        .map_err(|_| ImageError::Unknown)?;
    Ok(target.into_vec())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn resize_respects_stride_and_precision() {
        let source = FrameDimensions {
            width: 4,
            height: 2,
        };
        let target = FrameDimensions {
            width: 2,
            height: 1,
        };
        let mut padded = Vec::new();
        for _ in 0..2 {
            padded.extend([255, 0, 0, 255].repeat(4));
            padded.extend([42; 8]);
        }
        assert_eq!(
            resize_pixels(&padded, 24, source, target, PixelType::U8x4).unwrap(),
            [255, 0, 0, 255].repeat(2)
        );
        let pixel: Vec<u8> = [50000u16, 10000, 0, u16::MAX]
            .into_iter()
            .flat_map(u16::to_ne_bytes)
            .collect();
        assert_eq!(
            resize_pixels(&pixel.repeat(8), 32, source, target, PixelType::U16x4).unwrap(),
            pixel.repeat(2)
        );
        let pixel: Vec<u8> = [2.5f32, 0.5, 0.0, 1.0]
            .into_iter()
            .flat_map(f32::to_ne_bytes)
            .collect();
        let resized =
            resize_pixels(&pixel.repeat(8), 64, source, target, PixelType::F32x4).unwrap();
        for (actual, expected) in resized.chunks_exact(4).zip(pixel.repeat(2).chunks_exact(4)) {
            let actual = f32::from_ne_bytes(actual.try_into().unwrap());
            let expected = f32::from_ne_bytes(expected.try_into().unwrap());
            assert!((actual - expected).abs() < 0.00001);
        }
        assert!(resize_pixels(&[], 16, source, target, PixelType::U8x4).is_err());
    }
}
