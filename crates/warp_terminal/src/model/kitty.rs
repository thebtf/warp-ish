use std::cmp::min;
use std::io::Read;
#[cfg(all(unix, feature = "local_fs"))]
use std::{env, fs, str};

use anyhow::Result;
use base64::Engine;
use flate2::read::ZlibDecoder;
use image::ImageDecoder;
use pathfinder_geometry::{rect::RectF, vector::Vector2F};
use rand::Rng;
use warpui_core::image_cache::{
    CustomHeaderCreationError, CustomImageFormat, CustomImageHeader, FitType, resize_dimensions,
};
use warpui_core::util::{parse_i32, parse_u32};

use super::escape_sequences::C1;
use super::image_limits::{MAX_TERMINAL_IMAGE_BYTES, valid_image_dimensions};

/// Kitty specifies at most 4 KiB of encoded data per APC. Allow another 4 KiB for controls.
pub const MAX_KITTY_APC_BYTES: usize = 8192;

/// Actions specified by the [Kitty Image Protocol](https://sw.kovidgoyal.net/kitty/graphics-protocol/)
#[derive(Debug, Clone)]
pub enum KittyAction {
    StoreOnly(StoreOnly),
    StoreAndDisplay(StoreAndDisplay),
    DisplayStoredImage(DisplayStoredImage),
    QuerySupport(QuerySupport),
    Delete {
        delete_placements_only: bool,
        deletion_type: DeletionType,
    },
}

#[derive(Debug, Default, Clone)]
pub struct StoreOnly {
    pub image: KittyImage,
    pub image_id: u32,
}

#[derive(Debug, Default, Clone)]
pub struct StoreAndDisplay {
    pub image: KittyImage,
    pub placement_data: KittyPlacementData,
    pub image_id: u32,
    pub placement_id: u32,
}

#[derive(Debug, Default, Clone)]
pub struct DisplayStoredImage {
    pub placement_data: KittyPlacementData,
    pub image_id: u32,
    pub placement_id: u32,
}

#[derive(Debug, Default, Clone)]
pub struct QuerySupport {
    pub image: KittyImage,
    pub image_id: u32,
}

#[derive(Debug, Clone)]
pub enum DeletionType {
    DeleteAll,
    DeleteById(DeleteById),
}

#[derive(Debug, Clone)]
pub struct DeleteById {
    pub image_id: u32,
    pub placement_id: Option<u32>,
}

#[derive(Debug, Default, Clone)]
pub struct KittyImage {
    pub metadata: KittyImageMetadata,
    pub data: Vec<u8>,
}

impl TryFrom<PendingKittyMessage> for KittyMessage {
    type Error = InvalidKittyPayload;

    fn try_from(pending: PendingKittyMessage) -> Result<KittyMessage, InvalidKittyPayload> {
        Ok(KittyMessage {
            control_data: pending.control_data,
            payload: pending.payload?,
        })
    }
}

impl TryFrom<KittyMessage> for KittyImage {
    type Error = InvalidKittyPayload;

    fn try_from(message: KittyMessage) -> Result<Self, InvalidKittyPayload> {
        if message.control_data.unsupported_encoding {
            return Err(InvalidKittyPayload::UnsupportedEncoding);
        }
        let decoded_data =
            decode_kitty_image_data(message.payload, message.control_data.compressed)?;

        let decoded_data = match message.control_data.transmission_medium {
            KittyTransmissionMedium::Direct => decoded_data,
            KittyTransmissionMedium::SimpleFile => {
                cfg_if::cfg_if! {
                    if #[cfg(all(unix, feature = "local_fs"))] {
                        read_file(decoded_data, false)?
                    } else {
                        return Err(InvalidKittyPayload::FileError(FileError::UnsupportedPlatform));
                    }
                }
            }
            KittyTransmissionMedium::TemporaryFile => {
                cfg_if::cfg_if! {
                    if #[cfg(all(unix, feature = "local_fs"))] {
                        read_file(decoded_data, true)?
                    } else {
                        return Err(InvalidKittyPayload::FileError(FileError::UnsupportedPlatform));
                    }
                }
            }
            KittyTransmissionMedium::SharedMemoryObject => {
                cfg_if::cfg_if! {
                    if #[cfg(all(feature = "local_fs", unix))] {
                        read_shared_memory(message.control_data.clone(), decoded_data)?
                    } else {
                        return Err(InvalidKittyPayload::ShmError(ShmError::UnsupportedPlatform));
                    }
                }
            }
        };

        Ok(Self {
            metadata: KittyImageMetadata::from(message.control_data),
            data: decoded_data,
        })
    }
}

pub type KittyResponse = Result<(), KittyError>;

#[derive(Debug, Clone)]
pub enum KittyError {
    InvalidKittyAction(InvalidKittyAction),
    StorageError(StorageError),
    KittyFeatureDisabled,
}

#[derive(Debug, Clone)]
pub enum StorageError {
    UnknownId { id: u32 },
}

impl From<StorageError> for KittyError {
    fn from(value: StorageError) -> Self {
        KittyError::StorageError(value)
    }
}

#[derive(Debug, Clone)]
pub enum InvalidKittyAction {
    UnsupportedAction,
    InvalidKittyPayload(InvalidKittyPayload),
    InvalidControlData(InvalidControlData),
}

impl From<InvalidKittyAction> for KittyError {
    fn from(value: InvalidKittyAction) -> Self {
        KittyError::InvalidKittyAction(value)
    }
}

#[derive(Debug, Clone)]
pub enum InvalidControlData {
    IdMissing,
}

impl From<InvalidControlData> for KittyError {
    fn from(value: InvalidControlData) -> Self {
        KittyError::InvalidKittyAction(InvalidKittyAction::InvalidControlData(value))
    }
}

#[derive(Debug, Clone)]
pub enum InvalidKittyPayload {
    InvalidTransmissionMedium(KittyTransmissionMedium),
    UnsupportedEncoding,
    KittyDecodeError(KittyDecodeError),
    InvalidKittyImage(InvalidKittyImage),
    FileError(FileError),
    ShmError(ShmError),
}

#[derive(Debug, Clone)]
pub enum FileError {
    FileReadError(String),
    UnsupportedPlatform,
}

impl From<InvalidKittyPayload> for KittyError {
    fn from(value: InvalidKittyPayload) -> Self {
        KittyError::InvalidKittyAction(InvalidKittyAction::InvalidKittyPayload(value))
    }
}

#[derive(Debug, Clone)]
pub enum ShmError {
    ObjectOpenError(String),
    MmapError,
    EmptyObject,
    ObjectTooSmall {
        expected_bytes: usize,
        actual_bytes: usize,
    },
    InvalidObjectSize,
    FileStatError(String),
    UnsupportedPlatform,
}

#[derive(Debug, Clone)]
pub enum KittyDecodeError {
    TooLarge,
    InvalidBase64(String),
    InvalidCompression(String),
    InvalidUtf8(String),
}

impl From<KittyDecodeError> for InvalidKittyPayload {
    fn from(value: KittyDecodeError) -> Self {
        InvalidKittyPayload::KittyDecodeError(value)
    }
}

impl From<KittyDecodeError> for KittyError {
    fn from(value: KittyDecodeError) -> Self {
        KittyError::InvalidKittyAction(InvalidKittyAction::InvalidKittyPayload(
            InvalidKittyPayload::KittyDecodeError(value),
        ))
    }
}

#[derive(Debug, Clone)]
pub enum InvalidKittyImage {
    KittyPngError(KittyPngError),
    KittyRgbError(KittyRgbError),
    MalformedImage,
}

impl From<InvalidKittyImage> for KittyError {
    fn from(value: InvalidKittyImage) -> Self {
        KittyError::InvalidKittyAction(InvalidKittyAction::InvalidKittyPayload(
            InvalidKittyPayload::InvalidKittyImage(value),
        ))
    }
}

#[derive(Debug, Clone)]
pub enum KittyPngError {
    InvalidBytes(String),
}

impl From<KittyPngError> for KittyError {
    fn from(value: KittyPngError) -> Self {
        KittyError::InvalidKittyAction(InvalidKittyAction::InvalidKittyPayload(
            InvalidKittyPayload::InvalidKittyImage(InvalidKittyImage::KittyPngError(value)),
        ))
    }
}

#[derive(Debug, Clone)]
pub enum KittyRgbError {
    InvalidDimensions,
    ExpectedDataSizeMismatch {
        expected_bytes: usize,
        actual_bytes: usize,
    },
}

impl From<KittyRgbError> for KittyError {
    fn from(value: KittyRgbError) -> Self {
        KittyError::InvalidKittyAction(InvalidKittyAction::InvalidKittyPayload(
            InvalidKittyPayload::InvalidKittyImage(InvalidKittyImage::KittyRgbError(value)),
        ))
    }
}

impl From<CustomHeaderCreationError> for KittyRgbError {
    fn from(value: CustomHeaderCreationError) -> Self {
        match value {
            CustomHeaderCreationError::ExpectedDataSizeMismatch {
                expected_bytes,
                actual_bytes,
            } => KittyRgbError::ExpectedDataSizeMismatch {
                expected_bytes,
                actual_bytes,
            },
        }
    }
}

impl TryFrom<KittyMessage> for KittyAction {
    type Error = KittyError;

    fn try_from(message: KittyMessage) -> Result<KittyAction, KittyError> {
        match message.control_data.placement_action {
            KittyPlacementAction::StoreOnly => {
                let mut action = StoreOnly {
                    image_id: message
                        .control_data
                        .image_id
                        .unwrap_or(rand::thread_rng().r#gen()),
                    image: KittyImage::try_from(message)?,
                };

                if action.image.metadata.pixel_data_format == KittyPixelDataFormat::Png {
                    action.image = set_kitty_png_size(action.image)?;
                } else {
                    action.image = set_kitty_rgb_headers(action.image)?;
                }

                Ok(KittyAction::StoreOnly(action))
            }
            KittyPlacementAction::StoreAndDisplay => {
                let mut action = StoreAndDisplay {
                    image_id: message
                        .control_data
                        .image_id
                        .unwrap_or(rand::thread_rng().r#gen()),
                    placement_id: message
                        .control_data
                        .placement_id
                        .unwrap_or(rand::thread_rng().r#gen()),
                    placement_data: KittyPlacementData {
                        source: message.control_data.source,
                        z_index: message.control_data.z_index,
                        cols: message.control_data.cols,
                        rows: message.control_data.rows,
                        cursor_movement_policy: message.control_data.cursor_movement_policy,
                        unicode_placeholder: message.control_data.unicode_placeholder,
                    },
                    image: KittyImage::try_from(message)?,
                };

                if action.image.metadata.pixel_data_format == KittyPixelDataFormat::Png {
                    action.image = set_kitty_png_size(action.image)?;
                } else {
                    action.image = set_kitty_rgb_headers(action.image)?;
                }

                Ok(KittyAction::StoreAndDisplay(action))
            }
            KittyPlacementAction::DisplayStoredImage => {
                let id = match message.control_data.image_id {
                    Some(id) => id,
                    None => return Err(InvalidControlData::IdMissing.into()),
                };

                Ok(KittyAction::DisplayStoredImage(DisplayStoredImage {
                    image_id: id,
                    placement_id: message
                        .control_data
                        .placement_id
                        .unwrap_or(rand::thread_rng().r#gen()),
                    placement_data: KittyPlacementData {
                        source: message.control_data.source,
                        z_index: message.control_data.z_index,
                        cols: message.control_data.cols,
                        rows: message.control_data.rows,
                        cursor_movement_policy: message.control_data.cursor_movement_policy,
                        unicode_placeholder: message.control_data.unicode_placeholder,
                    },
                }))
            }
            KittyPlacementAction::QuerySupport => {
                let mut action = QuerySupport {
                    image_id: message
                        .control_data
                        .image_id
                        .unwrap_or(rand::thread_rng().r#gen()),
                    image: KittyImage::try_from(message)?,
                };

                if action.image.metadata.pixel_data_format == KittyPixelDataFormat::Png {
                    action.image = set_kitty_png_size(action.image)?;
                } else {
                    action.image = set_kitty_rgb_headers(action.image)?;
                }

                Ok(KittyAction::QuerySupport(action))
            }
            KittyPlacementAction::Delete => {
                let deletion_type = match message.control_data.delete_action {
                    DeleteAction::Unsupported => {
                        return Err(InvalidKittyAction::UnsupportedAction.into());
                    }
                    DeleteAction::DeleteAll => DeletionType::DeleteAll,
                    DeleteAction::DeleteById => {
                        let image_id = match message.control_data.image_id {
                            Some(image_id) => image_id,
                            None => return Err(InvalidControlData::IdMissing.into()),
                        };

                        DeletionType::DeleteById(DeleteById {
                            image_id,
                            placement_id: message.control_data.placement_id,
                        })
                    }
                };

                Ok(KittyAction::Delete {
                    deletion_type,
                    delete_placements_only: message.control_data.delete_placements_only,
                })
            }
            KittyPlacementAction::Unknown => Err(InvalidKittyAction::UnsupportedAction.into()),
        }
    }
}

#[derive(Debug, Default, Clone)]
pub struct KittyImageMetadata {
    pub pixel_data_format: KittyPixelDataFormat,
    pub transmission_medium: KittyTransmissionMedium,
    pub image_size: Vector2F,
}

#[derive(Debug, Default, Clone)]
pub struct KittyPlacementData {
    pub source: KittySourceRect,
    pub z_index: i32,
    pub cols: Option<u32>,
    pub rows: Option<u32>,
    pub cursor_movement_policy: CursorMovementPolicy,
    /// A virtual placement (`U=1`): [`super::kitty_unicode_placeholder`] cells show the image.
    pub unicode_placeholder: bool,
}

/// Kitty source rectangle in original image pixels. Zero extents run to the far edge.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct KittySourceRect {
    x: u32,
    y: u32,
    width: u32,
    height: u32,
}

impl KittySourceRect {
    pub fn resolve(self, image_size: Vector2F) -> Option<RectF> {
        let remaining_width = (image_size.x() as u32).saturating_sub(self.x);
        let remaining_height = (image_size.y() as u32).saturating_sub(self.y);
        let extent = |requested, available| {
            if requested == 0 {
                available
            } else {
                min(requested, available)
            }
        };
        let width = extent(self.width, remaining_width);
        let height = extent(self.height, remaining_height);
        (width > 0 && height > 0).then(|| {
            RectF::new(
                Vector2F::new(self.x as f32, self.y as f32),
                Vector2F::new(width as f32, height as f32),
            )
        })
    }
}

impl KittyPlacementData {
    pub fn get_desired_dimensions(
        &self,
        image_size: Vector2F,
        cell_height: usize,
        cell_width: usize,
        max_width_px: usize,
        max_height_px: usize,
    ) -> Vector2F {
        let aspect_ratio = image_size.x() / image_size.y();

        let (width_px, height_px) = resize_dimensions(
            image_size.x() as u32,
            image_size.y() as u32,
            min(image_size.x() as u32, max_width_px as u32),
            min(image_size.y() as u32, max_height_px as u32),
            FitType::Contain,
        );

        let (desired_width_px, desired_height_px) = match (self.cols, self.rows) {
            (Some(width), None) => {
                let desired_width_px = cell_width as f32 * width as f32;
                (desired_width_px, desired_width_px / aspect_ratio)
            }
            (None, Some(height)) => {
                let desired_height_px = cell_height as f32 * height as f32;
                (desired_height_px * aspect_ratio, desired_height_px)
            }
            (Some(width), Some(height)) => (
                cell_width as f32 * width as f32,
                cell_height as f32 * height as f32,
            ),
            (None, None) => (width_px as f32, height_px as f32),
        };

        Vector2F::new(
            desired_width_px.min(max_width_px as f32),
            desired_height_px.min(max_height_px as f32),
        )
    }
}

impl From<KittyControlData> for KittyImageMetadata {
    fn from(control_data: KittyControlData) -> Self {
        Self {
            pixel_data_format: control_data.pixel_data_format,
            image_size: Vector2F::new(control_data.width as f32, control_data.height as f32),
            transmission_medium: control_data.transmission_medium,
        }
    }
}

#[derive(Debug, Clone)]
pub struct KittyChunk {
    pub control_data: KittyControlData,
    pub payload: Result<Vec<u8>, KittyDecodeError>,
}

#[derive(Debug, Clone)]
pub struct PendingKittyMessage {
    pub control_data: KittyControlData,
    payload: Result<Vec<u8>, KittyDecodeError>,
}

impl PendingKittyMessage {
    pub fn new(chunk: KittyChunk) -> Self {
        Self {
            control_data: chunk.control_data,
            payload: chunk.payload,
        }
    }

    pub fn append(&mut self, chunk: KittyChunk) {
        // A delete cancels an incomplete transmission and is processed as its own command.
        if matches!(
            chunk.control_data.placement_action,
            KittyPlacementAction::Delete
        ) {
            *self = Self::new(chunk);
            return;
        }
        let Ok(data) = &mut self.payload else { return };
        match chunk.payload {
            Ok(bytes) if bytes.len() <= MAX_TERMINAL_IMAGE_BYTES - data.len() => {
                data.extend(bytes);
            }
            Ok(_) => self.payload = Err(KittyDecodeError::TooLarge),
            Err(err) => self.payload = Err(err),
        }
    }
}

#[derive(Debug, Clone)]
pub struct KittyMessage {
    pub control_data: KittyControlData,
    pub payload: Vec<u8>,
}

#[derive(Default, Debug, Clone)]
pub enum KittyPlacementAction {
    #[default]
    StoreOnly,
    StoreAndDisplay,
    DisplayStoredImage,
    QuerySupport,
    Delete,
    Unknown,
}

#[derive(Debug, Clone)]
pub struct KittyControlData {
    source: KittySourceRect,
    unsupported_encoding: bool,
    pixel_data_format: KittyPixelDataFormat,
    width: u32,
    height: u32,
    pub compressed: bool,
    transmission_medium: KittyTransmissionMedium,
    pub further_chunks: bool,
    pub image_id: Option<u32>,
    pub placement_id: Option<u32>,
    pub placement_action: KittyPlacementAction,
    pub verbosity: KittyResponseVerbosity,
    pub z_index: i32,
    pub delete_action: DeleteAction,
    delete_placements_only: bool,
    pub rows: Option<u32>,
    pub cols: Option<u32>,
    pub cursor_movement_policy: CursorMovementPolicy,
    pub unicode_placeholder: bool,
}

impl Default for KittyControlData {
    fn default() -> Self {
        Self {
            source: KittySourceRect::default(),
            unsupported_encoding: false,
            pixel_data_format: KittyPixelDataFormat::default(),
            width: 0,
            height: 0,
            compressed: false,
            transmission_medium: KittyTransmissionMedium::default(),
            further_chunks: false,
            image_id: None,
            placement_id: None,
            placement_action: KittyPlacementAction::default(),
            verbosity: KittyResponseVerbosity::default(),
            z_index: 0,
            delete_action: DeleteAction::default(),
            delete_placements_only: true,
            rows: None,
            cols: None,
            cursor_movement_policy: CursorMovementPolicy::default(),
            unicode_placeholder: false,
        }
    }
}

#[derive(Default, Debug, PartialEq, Clone, Copy)]
pub enum DeleteAction {
    #[default]
    DeleteAll,
    DeleteById,
    Unsupported,
}

#[derive(Default, Debug, PartialEq, Clone, Copy)]
pub enum KittyPixelDataFormat {
    Rgb24Bit,
    #[default]
    Rgba32Bit,
    Png,
}

#[derive(Default, Debug, PartialEq, Clone, Copy)]
pub enum KittyTransmissionMedium {
    #[default]
    Direct,
    SimpleFile,
    TemporaryFile,
    SharedMemoryObject,
}

#[derive(Default, Debug, Clone, Copy)]
pub enum KittyResponseVerbosity {
    #[default]
    All,
    ErrorsOnly,
    None,
}

impl KittyResponseVerbosity {
    pub fn send_ok(&self) -> bool {
        matches!(self, KittyResponseVerbosity::All)
    }

    pub fn send_error(&self) -> bool {
        matches!(self, KittyResponseVerbosity::All)
            || matches!(self, KittyResponseVerbosity::ErrorsOnly)
    }
}

#[derive(Default, Debug, Clone, Copy)]
pub enum CursorMovementPolicy {
    #[default]
    MoveCursor,
    DoNotMoveCursor,
}

fn parse_kitty_control_data(control_data: &[u8]) -> KittyControlData {
    let params = control_data.split(|&byte| byte == b',');

    let mut parsed_control_data = KittyControlData::default();
    for param in params {
        let (key, value) = match param.iter().position(|&byte| byte == b'=') {
            Some(position) => (&param[..position], &param[position + 1..]),
            None => continue,
        };

        match key {
            b"f" => {
                parsed_control_data.pixel_data_format = match value {
                    b"24" => KittyPixelDataFormat::Rgb24Bit,
                    b"32" => KittyPixelDataFormat::Rgba32Bit,
                    b"100" => KittyPixelDataFormat::Png,
                    _ => {
                        parsed_control_data.unsupported_encoding = true;
                        KittyPixelDataFormat::default()
                    }
                };
            }
            b"s" => {
                if let Some(value) = parse_u32(value) {
                    parsed_control_data.width = value;
                }
            }
            b"v" => {
                if let Some(value) = parse_u32(value) {
                    parsed_control_data.height = value;
                }
            }
            b"o" => {
                parsed_control_data.compressed = value == b"z";
                parsed_control_data.unsupported_encoding |= !parsed_control_data.compressed;
            }
            b"m" => {
                parsed_control_data.further_chunks = value == b"1";
            }
            b"t" => {
                parsed_control_data.transmission_medium = match value {
                    b"d" => KittyTransmissionMedium::Direct,
                    b"f" => KittyTransmissionMedium::SimpleFile,
                    b"t" => KittyTransmissionMedium::TemporaryFile,
                    b"s" => KittyTransmissionMedium::SharedMemoryObject,
                    _ => {
                        parsed_control_data.unsupported_encoding = true;
                        KittyTransmissionMedium::default()
                    }
                }
            }
            b"i" => {
                if let Some(value) = parse_u32(value) {
                    parsed_control_data.image_id = Some(value);
                }
            }
            b"p" => {
                if let Some(value) = parse_u32(value) {
                    parsed_control_data.placement_id = Some(value);
                }
            }
            b"a" => {
                parsed_control_data.placement_action = match value {
                    b"t" => KittyPlacementAction::StoreOnly,
                    b"T" => KittyPlacementAction::StoreAndDisplay,
                    b"p" => KittyPlacementAction::DisplayStoredImage,
                    b"q" => KittyPlacementAction::QuerySupport,
                    b"d" => KittyPlacementAction::Delete,
                    _ => KittyPlacementAction::Unknown,
                }
            }
            b"q" => {
                parsed_control_data.verbosity = match value {
                    b"0" => KittyResponseVerbosity::All,
                    b"1" => KittyResponseVerbosity::ErrorsOnly,
                    b"2" => KittyResponseVerbosity::None,
                    _ => KittyResponseVerbosity::default(),
                }
            }
            b"z" => {
                if let Some(value) = parse_i32(value) {
                    parsed_control_data.z_index = value;
                }
            }
            b"d" => {
                if value.iter().all(|x| x.is_ascii_uppercase()) {
                    parsed_control_data.delete_placements_only = false;
                }

                parsed_control_data.delete_action = match &value.to_ascii_lowercase()[..] {
                    // Note: Kitty Protocol specifies "a" as "Delete all placements on screen",
                    // we're just starting with DeleteAll placements to expedite the launch.
                    b"a" => DeleteAction::DeleteAll,
                    b"i" => DeleteAction::DeleteById,
                    _ => DeleteAction::Unsupported,
                }
            }
            b"x" => parsed_control_data.source.x = parse_u32(value).unwrap_or_default(),
            b"y" => parsed_control_data.source.y = parse_u32(value).unwrap_or_default(),
            b"w" => parsed_control_data.source.width = parse_u32(value).unwrap_or_default(),
            b"h" => parsed_control_data.source.height = parse_u32(value).unwrap_or_default(),
            b"c" => {
                if let Some(value) = parse_u32(value) {
                    parsed_control_data.cols = Some(value);
                }
            }
            b"r" => {
                if let Some(value) = parse_u32(value) {
                    parsed_control_data.rows = Some(value);
                }
            }
            b"C" => {
                parsed_control_data.cursor_movement_policy = match value {
                    b"0" => CursorMovementPolicy::MoveCursor,
                    b"1" => CursorMovementPolicy::DoNotMoveCursor,
                    _ => CursorMovementPolicy::default(),
                }
            }
            b"U" => {
                parsed_control_data.unicode_placeholder = value == b"1";
            }
            _ => {}
        }
    }

    parsed_control_data
}

pub fn parse_kitty_chunk(chunk: &[u8]) -> KittyChunk {
    let (control_data, payload) = match chunk.iter().position(|&byte| byte == b';') {
        Some(position) => (&chunk[..position], &chunk[position + 1..]),
        None => (chunk, &[][..]),
    };

    let control_data = parse_kitty_control_data(control_data);

    KittyChunk {
        control_data,
        payload: if chunk.len() + 1 > MAX_KITTY_APC_BYTES {
            Err(KittyDecodeError::TooLarge)
        } else {
            base64_decode_padding_agnostic(payload)
        },
    }
}

#[cfg(all(unix, feature = "local_fs"))]
fn read_file(decoded_payload: Vec<u8>, is_temp: bool) -> Result<Vec<u8>, InvalidKittyPayload> {
    let path = match str::from_utf8(&decoded_payload[..]) {
        Ok(path) => path,
        Err(err) => return Err(KittyDecodeError::InvalidUtf8(err.to_string()).into()),
    };

    let data = match fs::read(path) {
        Ok(data) => data,
        Err(err) => {
            return Err(InvalidKittyPayload::FileError(FileError::FileReadError(
                err.to_string(),
            )));
        }
    };

    if is_temp {
        safe_delete_temp_file(path);
    }

    Ok(data)
}

#[cfg(all(unix, feature = "local_fs"))]
fn safe_delete_temp_file(path: &str) {
    if is_path_in_temp_dir(path)
        && path.contains("tty-graphics-protocol")
        && let Err(err) = fs::remove_file(path)
    {
        log::warn!("Failed to delete kitty temporary file (path = {path}): {err:#}");
    }
}

#[cfg(all(unix, feature = "local_fs"))]
fn is_path_in_temp_dir(path: &str) -> bool {
    let temp_dirs = vec!["/tmp", "/var/tmp", "/dev/shm"];

    for temp_dir in temp_dirs {
        if path.starts_with(temp_dir) {
            return true;
        }
    }

    if let Ok(temp_dir) = env::var("TMPDIR")
        && path.starts_with(&temp_dir)
    {
        return true;
    }

    false
}

#[cfg(all(feature = "local_fs", unix))]
fn read_shared_memory(
    control_data: KittyControlData,
    decoded_payload: Vec<u8>,
) -> Result<Vec<u8>, InvalidKittyPayload> {
    use nix::sys::mman::{shm_open, shm_unlink};

    let path = match str::from_utf8(&decoded_payload[..]) {
        Ok(path) => path,
        Err(err) => return Err(KittyDecodeError::InvalidUtf8(err.to_string()).into()),
    };

    let fd = match shm_open(
        path,
        nix::fcntl::OFlag::O_RDONLY,
        nix::sys::stat::Mode::empty(),
    ) {
        Ok(fd) => fd,
        Err(err) => {
            return Err(InvalidKittyPayload::ShmError(ShmError::ObjectOpenError(
                err.to_string(),
            )));
        }
    };

    let bytes_per_pixel = match control_data.pixel_data_format {
        KittyPixelDataFormat::Rgb24Bit => Some(3),
        KittyPixelDataFormat::Rgba32Bit => Some(4),
        KittyPixelDataFormat::Png => None,
    };

    let size = bytes_per_pixel.map(|bytes_per_pixel| {
        (bytes_per_pixel * control_data.width * control_data.height) as usize
    });

    let data = read_from_shared_memory_fd(fd, size);

    if let Err(err) = shm_unlink(path) {
        log::warn!("Failed to unlink kitty shm file (path = {path}): {err:?}");
    };

    data
}

#[cfg(all(feature = "local_fs", unix))]
fn read_from_shared_memory_fd(
    fd: i32,
    size: Option<usize>,
) -> Result<Vec<u8>, InvalidKittyPayload> {
    use std::num::NonZero;

    use nix::sys::mman::{MapFlags, ProtFlags, mmap};
    use nix::sys::stat::fstat;

    let file_size = match fstat(fd) {
        Ok(stat) => stat.st_size,
        Err(err) => {
            return Err(InvalidKittyPayload::ShmError(ShmError::FileStatError(
                err.to_string(),
            )));
        }
    };

    let Ok(file_size) = usize::try_from(file_size) else {
        return Err(InvalidKittyPayload::ShmError(ShmError::InvalidObjectSize));
    };

    let size = size.unwrap_or(file_size);

    if file_size < size {
        return Err(InvalidKittyPayload::ShmError(ShmError::ObjectTooSmall {
            expected_bytes: size,
            actual_bytes: file_size,
        }));
    }

    let Some(size) = NonZero::new(size) else {
        return Err(InvalidKittyPayload::ShmError(ShmError::EmptyObject));
    };

    let ptr = unsafe {
        mmap(
            None,
            size,
            ProtFlags::PROT_READ,
            MapFlags::MAP_SHARED,
            fd,
            0,
        )
    };

    let Ok(ptr) = ptr else {
        return Err(InvalidKittyPayload::ShmError(ShmError::MmapError));
    };

    let slice = unsafe { std::slice::from_raw_parts(ptr as *const u8, size.into()) };
    let data = slice.to_vec();

    Ok(data)
}

fn base64_decode_padding_agnostic(data: &[u8]) -> Result<Vec<u8>, KittyDecodeError> {
    if let Ok(decoded_bytes) = base64::engine::general_purpose::STANDARD.decode(data) {
        return Ok(decoded_bytes);
    }

    match base64::engine::general_purpose::STANDARD_NO_PAD.decode(data) {
        Ok(decoded_bytes) => Ok(decoded_bytes),
        Err(err) => Err(KittyDecodeError::InvalidBase64(err.to_string())),
    }
}

pub fn decode_kitty_image_data(
    payload: Vec<u8>,
    compressed: bool,
) -> Result<Vec<u8>, KittyDecodeError> {
    if payload.len() > MAX_TERMINAL_IMAGE_BYTES {
        return Err(KittyDecodeError::TooLarge);
    }
    if !compressed {
        return Ok(payload);
    }
    let mut decoded = Vec::new();
    ZlibDecoder::new(payload.as_slice())
        .take(MAX_TERMINAL_IMAGE_BYTES as u64 + 1)
        .read_to_end(&mut decoded)
        .map_err(|err| KittyDecodeError::InvalidCompression(err.to_string()))?;
    if decoded.len() > MAX_TERMINAL_IMAGE_BYTES {
        return Err(KittyDecodeError::TooLarge);
    }
    Ok(decoded)
}

pub fn set_kitty_png_size(mut image: KittyImage) -> Result<KittyImage, KittyPngError> {
    let mut reader =
        image::ImageReader::with_format(std::io::Cursor::new(&image.data), image::ImageFormat::Png);
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(MAX_TERMINAL_IMAGE_BYTES as u64);
    reader.limits(limits);
    let decoder = reader
        .into_decoder()
        .map_err(|err| KittyPngError::InvalidBytes(err.to_string()))?;
    let (width, height) = decoder.dimensions();
    if !valid_image_dimensions(width, height)
        || decoder.total_bytes() > MAX_TERMINAL_IMAGE_BYTES as u64
    {
        return Err(KittyPngError::InvalidBytes(
            "PNG exceeds terminal image budget".into(),
        ));
    }
    let pixels = image::DynamicImage::from_decoder(decoder)
        .map_err(|err| KittyPngError::InvalidBytes(err.to_string()))?
        .into_rgba8()
        .into_raw();
    // Validate the complete payload (including for a=q) once. Storage and drawing consume
    // the decoded pixels directly; multiple placements do not inflate the PNG again.
    image.data =
        CustomImageHeader::prepend_custom_header(pixels, width, height, CustomImageFormat::Rgba)
            .map_err(|err| KittyPngError::InvalidBytes(format!("{err:?}")))?;
    image.metadata.image_size = Vector2F::new(width as f32, height as f32);
    Ok(image)
}

pub fn set_kitty_rgb_headers(mut image: KittyImage) -> Result<KittyImage, KittyRgbError> {
    let image_format = match image.metadata.pixel_data_format {
        KittyPixelDataFormat::Rgb24Bit => CustomImageFormat::Rgb,
        KittyPixelDataFormat::Rgba32Bit => CustomImageFormat::Rgba,
        KittyPixelDataFormat::Png => return Ok(image),
    };

    if !valid_image_dimensions(
        image.metadata.image_size.x() as u32,
        image.metadata.image_size.y() as u32,
    ) {
        return Err(KittyRgbError::InvalidDimensions);
    }
    image.data = CustomImageHeader::prepend_custom_header(
        image.data,
        image.metadata.image_size.x() as u32,
        image.metadata.image_size.y() as u32,
        image_format,
    )?;

    Ok(image)
}

fn create_kitty_reply(image_id: u32, message: String) -> Vec<u8> {
    [
        C1::APC,
        b"G",
        format!("i={image_id};{message}").as_bytes(),
        C1::ST,
    ]
    .concat()
}

pub fn create_kitty_ok_reply(image_id: u32) -> Vec<u8> {
    create_kitty_reply(image_id, "OK".to_string())
}

pub fn create_kitty_error_reply(image_id: u32, err: KittyError) -> Vec<u8> {
    create_kitty_reply(image_id, format!("{err:?}"))
}

#[cfg(test)]
#[path = "kitty_tests.rs"]
mod tests;
