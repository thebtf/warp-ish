//! SIXEL DCS frames use the same inline raster placement as OSC 1337.
use icy_sixel::{DcsSettings, SixelDecoder};
use pathfinder_geometry::vector::Vector2F;
use warpui_core::image_cache::{CustomImageFormat, CustomImageHeader};

use crate::model::image_limits::{MAX_TERMINAL_IMAGE_PIXELS, valid_image_dimensions};
use crate::model::iterm_image::{
    ITermImage, ITermImageDimensionUnit, ITermImageMetadata, InlineImagePlacement,
};

pub(crate) const MAX_WIDTH: usize = MAX_TERMINAL_IMAGE_PIXELS.isqrt();
// The decoder allocates entire six-pixel bands, including the final one.
pub(crate) const MAX_HEIGHT: usize = MAX_WIDTH / 6 * 6;

#[derive(Debug, Default)]
pub(super) struct SixelState {
    // Color registers belong to the terminal session, unless private palettes are requested.
    decoder: Option<Box<SixelDecoder>>,
    private_palette: bool,
    viewport_origin: bool,
    cursor_right: bool,
}

impl SixelState {
    pub fn set_mode(&mut self, mode: u16, enabled: bool) -> bool {
        match mode {
            80 => self.viewport_origin = enabled,
            1070 => self.private_palette = enabled,
            8452 => self.cursor_right = enabled,
            _ => return false,
        }
        true
    }

    pub fn decode(&mut self, frame: SixelFrame) -> Option<ITermImage> {
        let placement = if self.viewport_origin {
            InlineImagePlacement::SixelViewport
        } else if self.cursor_right {
            InlineImagePlacement::SixelRight
        } else {
            InlineImagePlacement::SixelLeft
        };
        if self.private_palette {
            frame.decode(&mut SixelDecoder::new(), placement)
        } else {
            frame.decode(self.decoder.get_or_insert_with(Default::default), placement)
        }
    }
}

/// Clip the raw raster before upload. The visible part retains its original
/// pixel scale instead of squeezing offscreen columns into the terminal.
pub(crate) fn clip_raster(
    image: &mut ITermImage,
    full_size: (u32, u32),
    visible_size: (u32, u32),
) -> Option<()> {
    if full_size == visible_size {
        return Some(());
    }
    let width = image.metadata.image_size.x() as u32;
    let height = image.metadata.image_size.y() as u32;
    let cropped_axis = |pixels, visible, full| {
        (u64::from(pixels) * u64::from(visible)).div_ceil(u64::from(full)) as u32
    };
    let cropped_width = cropped_axis(width, visible_size.0, full_size.0);
    let cropped_height = cropped_axis(height, visible_size.1, full_size.1);
    let header = CustomImageHeader {
        width,
        height,
        image_format: CustomImageFormat::Rgba,
    };
    let old_header = header.create_header();
    let clipped_header = CustomImageHeader {
        width: cropped_width,
        height: cropped_height,
        ..header
    }
    .create_header();
    if image.data.strip_prefix(old_header.as_bytes())?.len() != width as usize * height as usize * 4
    {
        return None;
    }
    let stride = width as usize * 4;
    let clipped_stride = cropped_width as usize * 4;
    image.data[..clipped_header.len()].copy_from_slice(clipped_header.as_bytes());
    for row in 0..cropped_height as usize {
        let src = old_header.len() + row * stride;
        let dst = clipped_header.len() + row * clipped_stride;
        image.data.copy_within(src..src + clipped_stride, dst);
    }
    image
        .data
        .truncate(clipped_header.len() + cropped_height as usize * clipped_stride);
    image.data.shrink_to_fit();
    Some(())
}

#[derive(Debug)]
pub(super) struct SixelFrame {
    pub payload: Vec<u8>,
    pub params: [Option<u16>; 3],
}

impl SixelFrame {
    fn decode(
        self,
        decoder: &mut SixelDecoder,
        placement: InlineImagePlacement,
    ) -> Option<ITermImage> {
        if self.payload.is_empty() || !canvas_fits(&self.payload) {
            return None;
        }
        let [aspect, background, grid] = self.params;
        let image = decoder
            .decode_from_dcs(&self.payload, DcsSettings::new(aspect, background, grid))
            .ok()?;
        let (display_width, display_height) = image.corrected_dimensions();
        if !valid_image_dimensions(display_width as u32, display_height as u32) {
            return None;
        }
        let data = CustomImageHeader::prepend_custom_header(
            image.pixels,
            image.width.try_into().ok()?,
            image.height.try_into().ok()?,
            CustomImageFormat::Rgba,
        )
        .ok()?;
        Some(ITermImage {
            data,
            metadata: ITermImageMetadata {
                name: "SIXEL".into(),
                inline: true,
                image_size: Vector2F::new(image.width as f32, image.height as f32),
                desired_width: Some((display_width as u32, ITermImageDimensionUnit::Pixel)),
                desired_height: Some((display_height as u32, ITermImageDimensionUnit::Pixel)),
                preserve_aspect_ratio: false,
                ..Default::default()
            },
            placement,
        })
    }
}

/// The decoder has its own, larger allocation limit. Measure the canvas before
/// handing it the frame. A SIXEL advances one column in a band six pixels high,
/// even when every pixel in the column is transparent.
fn canvas_fits(mut input: &[u8]) -> bool {
    let (mut x, mut y) = (0usize, 0usize);
    let (mut width, mut height) = (1usize, 1usize);
    let mut repeat = 1;
    // Normal sixels visit their pixels once per image. Keep overpainting
    // possible without letting compressed repeats monopolize the PTY reader.
    let mut repeated_paints_left = MAX_TERMINAL_IMAGE_PIXELS * 8;
    while let Some((&byte, rest)) = input.split_first() {
        input = rest;
        match byte {
            b'!' => repeat = number(&mut input).max(1),
            b'$' => x = 0,
            b'-' => {
                x = 0;
                y = y.saturating_add(6);
            }
            b'"' => {
                let mut fields = [0; 4];
                for field in &mut fields {
                    *field = number(&mut input);
                    let Some(rest) = input.strip_prefix(b";") else {
                        break;
                    };
                    input = rest;
                }
                width = width.max(fields[2]);
                height = height.max(fields[3]);
            }
            b'?'..=b'~' => {
                if repeat > 1 {
                    let paints = repeat.saturating_mul((byte - b'?').count_ones() as usize);
                    let Some(left) = repeated_paints_left.checked_sub(paints) else {
                        return false;
                    };
                    repeated_paints_left = left;
                }
                let plain = input
                    .iter()
                    .take_while(|&&b| (b'?'..=b'~').contains(&b))
                    .count();
                x = x.saturating_add(repeat).saturating_add(plain);
                input = &input[plain..];
                repeat = 1;
                width = width.max(x);
                height = height.max(y.saturating_add(6));
            }
            _ => continue,
        }
    }
    // icy_sixel grows each backing-canvas axis geometrically. Bound its
    // allocation before decode, including that power-of-two slack.
    width
        .checked_next_power_of_two()
        .zip(height.checked_next_power_of_two())
        .is_some_and(|(w, h)| w.saturating_mul(h) <= MAX_TERMINAL_IMAGE_PIXELS)
}

fn number(input: &mut &[u8]) -> usize {
    let mut value = 0usize;
    while let Some((&digit @ b'0'..=b'9', rest)) = input.split_first() {
        value = value
            .saturating_mul(10)
            .saturating_add(usize::from(digit - b'0'));
        *input = rest;
    }
    value
}
