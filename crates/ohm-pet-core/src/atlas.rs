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

    pub fn from_state_frames(rows: &[Vec<RgbaImage>]) -> Self {
        let mut atlas = RgbaImage::new(COLUMNS * CELL_WIDTH, ROWS * CELL_HEIGHT);
        for row in 0..ROWS as usize {
            let frames = rows.get(row).filter(|frames| !frames.is_empty());
            for column in 0..COLUMNS as usize {
                let Some(frame) = frames.and_then(|frames| frames.get(column % frames.len()))
                else {
                    continue;
                };
                let fitted = fit_frame(frame);
                image::imageops::overlay(
                    &mut atlas,
                    &fitted,
                    (column as u32 * CELL_WIDTH) as i64,
                    (row as u32 * CELL_HEIGHT) as i64,
                );
            }
        }
        Self { image: atlas }
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

fn fit_frame(frame: &RgbaImage) -> RgbaImage {
    let scale = (CELL_WIDTH as f64 / frame.width().max(1) as f64)
        .min(CELL_HEIGHT as f64 / frame.height().max(1) as f64);
    let width = ((frame.width() as f64 * scale).round() as u32).max(1);
    let height = ((frame.height() as f64 * scale).round() as u32).max(1);
    let resized =
        image::imageops::resize(frame, width, height, image::imageops::FilterType::Nearest);
    let mut cell = RgbaImage::new(CELL_WIDTH, CELL_HEIGHT);
    let x = (CELL_WIDTH - width) / 2;
    let y = CELL_HEIGHT - height;
    image::imageops::overlay(&mut cell, &resized, i64::from(x), i64::from(y));
    cell
}
