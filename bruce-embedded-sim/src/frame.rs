//! The frame around the simulated screen: a drawing of the board, its outline, the bezel of its screen, its name and its
//! labelled buttons, until there is a photo of it, and the whole board, frame and screen, as the window and screenshots
//! show it, see compose.

use embedded_graphics::mono_font::{MonoTextStyle, ascii::FONT_6X10, ascii::FONT_7X13_BOLD};
use embedded_graphics::pixelcolor::Rgb888;
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::{CornerRadii, PrimitiveStyle, PrimitiveStyleBuilder, Rectangle, RoundedRectangle};
use embedded_graphics::text::{Alignment, Baseline, Text, TextStyleBuilder};

use crate::board::Board;

/// Pixels as 0x00RRGGBB, row by row, which embedded-graphics draws on, for the frame.
pub struct Canvas {
    pub size: Size,
    pub pixels: Vec<u32>,
}

impl Canvas {
    pub fn new(size: Size, colour: u32) -> Self {
        Canvas {
            size,
            pixels: vec![colour; (size.width * size.height) as usize],
        }
    }
}

impl OriginDimensions for Canvas {
    fn size(&self) -> Size {
        self.size
    }
}

impl DrawTarget for Canvas {
    type Color = Rgb888;
    type Error = core::convert::Infallible;

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<Self::Color>>,
    {
        let (width, height) = (self.size.width as i32, self.size.height as i32);
        for Pixel(point, color) in pixels {
            if point.x >= 0 && point.y >= 0 && point.x < width && point.y < height {
                self.pixels[(point.y * width + point.x) as usize] =
                    ((color.r() as u32) << 16) | ((color.g() as u32) << 8) | color.b() as u32;
            }
        }
        Ok(())
    }
}

fn rgb(colour: u32) -> Rgb888 {
    Rgb888::new((colour >> 16) as u8, (colour >> 8) as u8, colour as u8)
}

/// The colour of the window around the board, which its rounded corners show.
pub const BACKGROUND: u32 = 0x3a3d42;

/// The frame drawn for the board: its body, the bezel round the screen, its name under it, and its buttons.
pub fn drawn(board: &Board) -> Canvas {
    let mut canvas = Canvas::new(board.frame_size, BACKGROUND);
    let body = RoundedRectangle::new(
        Rectangle::new(Point::zero(), board.frame_size),
        CornerRadii::new(Size::new(14, 14)),
    );
    let _ = body
        .into_styled(
            PrimitiveStyleBuilder::new()
                .fill_color(rgb(board.board_colour))
                .stroke_color(rgb(0x55595f))
                .stroke_width(1)
                .build(),
        )
        .draw(&mut canvas);

    // The photo, if there is one, see Board::photo, otherwise a bezel round the screen.
    let photo = board.photo.map(|photo| draw_photo(&mut canvas, photo));
    if let Some(Err(error)) = &photo {
        log::warn!(
            "Unable to draw the photo of the {}, so it is drawn, {error:#}",
            board.title
        );
    }
    if !matches!(photo, Some(Ok(()))) {
        let bezel = Rectangle::new(
            board.screen_at - Point::new(6, 6),
            board.screen_size + Size::new(12, 12),
        );
        let _ = RoundedRectangle::with_equal_corners(bezel, Size::new(4, 4))
            .into_styled(PrimitiveStyle::with_fill(rgb(SCREEN_SURROUND)))
            .draw(&mut canvas);
    }

    let label = MonoTextStyle::new(&FONT_6X10, rgb(0xb8bcc2));
    // The board's name, with a line under it for a note from the window, see draw_note.
    let name_at = Point::new(note_at(board).x, board.strip_top + 18);
    let _ = Text::with_baseline(board.title, name_at, label, Baseline::Middle).draw(&mut canvas);

    let button_label = MonoTextStyle::new(&FONT_7X13_BOLD, rgb(0x202124));
    let centred = TextStyleBuilder::new()
        .alignment(Alignment::Center)
        .baseline(Baseline::Middle)
        .build();
    for button in board.buttons {
        let area = Rectangle::new(button.top_left, button.size);
        let _ = RoundedRectangle::with_equal_corners(area, Size::new(6, 6))
            .into_styled(
                PrimitiveStyleBuilder::new()
                    .fill_color(rgb(0xd9dce0))
                    .stroke_color(rgb(0x8a8f96))
                    .stroke_width(1)
                    .build(),
            )
            .draw(&mut canvas);
        let _ = Text::with_text_style(button.name, area.center(), button_label, centred).draw(&mut canvas);
    }
    canvas
}

/// The whole board as the window shows it: the frame, with the screen's pixels in it, and any button which is pressed
/// darkened, row by row as 0x00RRGGBB, frame_size in size.
pub fn compose(board: &Board, frame: &Canvas, screen: &[u32], pressed: Option<&str>) -> Vec<u32> {
    let mut pixels = frame.pixels.clone();
    let frame_width = board.frame_size.width as usize;
    let screen_width = board.screen_size.width as usize;
    for (row, line) in screen.chunks(screen_width).enumerate() {
        let start = (board.screen_at.y as usize + row) * frame_width + board.screen_at.x as usize;
        pixels[start..start + screen_width].copy_from_slice(line);
    }
    if let Some(button) = pressed.and_then(|name| board.button(name)) {
        for y in 0..button.size.height as usize {
            for x in 0..button.size.width as usize {
                let index = (button.top_left.y as usize + y) * frame_width + button.top_left.x as usize + x;
                // Half as bright, as if pushed in.
                pixels[index] = (pixels[index] >> 1) & 0x7f7f7f;
            }
        }
    }
    pixels
}

/// Where the window's note goes, under the board's name, level with its buttons, in the strip under the board, see
/// draw_note.
fn note_at(board: &Board) -> Point {
    let left = if board.photo.is_some() { 12 } else { board.screen_at.x };
    Point::new(left, board.strip_top + 36)
}

/// The black round a screen, the bezel's, and the photo's hole's which the screen does not fill.
const SCREEN_SURROUND: u32 = 0x050505;

/// Draws the photo, a PNG, at the top left of the canvas: its transparent hole, where the screen goes, in
/// SCREEN_SURROUND, and the partly transparent pixels round the hole's edge over it, as the edge of the screen's glass.
fn draw_photo(canvas: &mut Canvas, photo: &[u8]) -> anyhow::Result<()> {
    let under = SCREEN_SURROUND;
    let mut reader = png::Decoder::new(std::io::Cursor::new(photo)).read_info()?;
    let mut buffer = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buffer)?;
    if info.color_type != png::ColorType::Rgba || info.bit_depth != png::BitDepth::Eight {
        anyhow::bail!(
            "the photo is {:?} {:?}, not 8 bit RGBA",
            info.color_type,
            info.bit_depth
        );
    }
    let width = (info.width as usize).min(canvas.size.width as usize);
    let height = (info.height as usize).min(canvas.size.height as usize);
    let blend = |colour: u8, under: u32, shift: u32, alpha: u32| -> u32 {
        (colour as u32 * alpha + ((under >> shift) & 0xff) * (255 - alpha)) / 255
    };
    for y in 0..height {
        for x in 0..width {
            let at = (y * info.width as usize + x) * 4;
            let [r, g, b, alpha] = [buffer[at], buffer[at + 1], buffer[at + 2], buffer[at + 3]];
            let alpha = alpha as u32;
            canvas.pixels[y * canvas.size.width as usize + x] = if alpha == 0 {
                SCREEN_SURROUND
            } else {
                (blend(r, under, 16, alpha) << 16) | (blend(g, under, 8, alpha) << 8) | blend(b, under, 0, alpha)
            };
        }
    }
    Ok(())
}

/// Writes a note from the window on the board's pixels, as compose made them, e.g. what the user can copy, or that it
/// has been, in the accent colour if highlight. Screenshots have no note.
pub fn draw_note(board: &Board, pixels: &mut Vec<u32>, note: &str, highlight: bool) {
    let mut canvas = Canvas {
        size: board.frame_size,
        pixels: std::mem::take(pixels),
    };
    let colour = if highlight { 0x06b6d4 } else { 0x8a8f96 };
    let style = MonoTextStyle::new(&FONT_6X10, rgb(colour));
    let _ = Text::with_baseline(note, note_at(board), style, Baseline::Middle).draw(&mut canvas);
    *pixels = canvas.pixels;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::BOARDS;

    #[test]
    fn a_photo_is_as_large_as_its_board_and_its_hole_holds_the_screen() {
        for board in BOARDS.iter().filter(|board| board.photo.is_some()) {
            let mut canvas = Canvas::new(board.frame_size, 0x123456);
            draw_photo(&mut canvas, board.photo.unwrap()).unwrap();
            let at = |point: Point| canvas.pixels[(point.y * board.frame_size.width as i32 + point.x) as usize];
            // The screen's corners are in the hole, and the strip under the photo is not drawn on.
            assert_eq!(at(board.screen_at), SCREEN_SURROUND, "{}", board.id);
            assert_eq!(
                at(board.screen_at + board.screen_size - Point::new(1, 1)),
                SCREEN_SURROUND,
                "{}",
                board.id
            );
            // The hole is at most a pixel wider than the screen all round, the panel's edge which its bezel hides.
            assert_ne!(at(board.screen_at - Point::new(2, 2)), SCREEN_SURROUND, "{}", board.id);
            assert_eq!(at(Point::new(0, board.strip_top + 1)), 0x123456, "{}", board.id);
        }
    }

    #[test]
    fn the_screen_is_put_where_the_board_has_it() {
        for board in BOARDS {
            let frame = drawn(board);
            let screen = vec![0x123456; (board.screen_size.width * board.screen_size.height) as usize];
            let pixels = compose(board, &frame, &screen, None);
            let width = board.frame_size.width as i32;
            let at = |point: Point| pixels[(point.y * width + point.x) as usize];
            assert_eq!(at(board.screen_at), 0x123456);
            assert_eq!(at(board.screen_at + board.screen_size - Point::new(1, 1)), 0x123456);
            assert_ne!(at(board.screen_at - Point::new(1, 1)), 0x123456);
        }
    }
}
