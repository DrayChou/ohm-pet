use image::{DynamicImage, ImageFormat, RgbaImage};
use std::{io::Cursor, path::Path};
use thiserror::Error;

pub const COLUMNS: u32 = 8;
pub const ROWS: u32 = 11;
pub const CELL_WIDTH: u32 = 192;
pub const CELL_HEIGHT: u32 = 208;

#[derive(Debug, Error)]
pub enum AtlasError {
    #[error("failed to decode atlas: {0}")]
    Decode(#[from] image::ImageError),
    #[error("atlas must be 1536×2288, got {0}×{1}")]
    InvalidDimensions(u32, u32),
}

pub struct Atlas {
    image: RgbaImage,
}

impl Atlas {
    pub fn load(path: impl AsRef<Path>) -> Result<Self, AtlasError> {
        let image = image::open(path)?.into_rgba8();
        if image.width() != COLUMNS * CELL_WIDTH || image.height() != ROWS * CELL_HEIGHT {
            return Err(AtlasError::InvalidDimensions(image.width(), image.height()));
        }
        Ok(Self { image })
    }

    pub fn frame_rgba(&self, row: u32, column: u32) -> Vec<u8> {
        self.frame_image(row, column).into_raw()
    }

    pub fn frame_png(&self, row: u32, column: u32) -> Result<Vec<u8>, image::ImageError> {
        let mut output = Cursor::new(Vec::new());
        DynamicImage::ImageRgba8(self.frame_image(row, column))
            .write_to(&mut output, ImageFormat::Png)?;
        Ok(output.into_inner())
    }

    fn frame_image(&self, row: u32, column: u32) -> RgbaImage {
        image::imageops::crop_imm(
            &self.image,
            column * CELL_WIDTH,
            row * CELL_HEIGHT,
            CELL_WIDTH,
            CELL_HEIGHT,
        )
        .to_image()
    }
}
