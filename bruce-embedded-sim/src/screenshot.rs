//! Screenshots of the simulated board, frame and screen, as PNG, e.g. for documentation, see Options::screenshot.

use std::path::Path;

use crate::Device;

/// Saves the board as it is now, frame and screen, to path as a PNG, each of its pixels scale pixels across and down.
pub fn save(device: &Device, path: &Path, scale: u32) -> anyhow::Result<()> {
    let board = device.board;
    let frame = crate::frame::drawn(board);
    let pixels = crate::frame::compose(board, &frame, &device.screen.pixels(), None);
    let (width, height) = (board.frame_size.width * scale, board.frame_size.height * scale);
    let mut rgb = Vec::with_capacity((width * height * 3) as usize);
    for y in 0..height {
        for x in 0..width {
            let pixel = pixels[((y / scale) * board.frame_size.width + x / scale) as usize];
            rgb.extend([(pixel >> 16) as u8, (pixel >> 8) as u8, pixel as u8]);
        }
    }
    let file = std::io::BufWriter::new(std::fs::File::create(path)?);
    let mut encoder = png::Encoder::new(file, width, height);
    encoder.set_color(png::ColorType::Rgb);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header()?.write_image_data(&rgb)?;
    Ok(())
}
