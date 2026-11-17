//! A window to choose which board to simulate, for a program started without one, e.g. a launcher, see choose: each
//! board in its frame, side by side, with what it shows on its screen, which a click chooses.

use std::num::NonZeroU32;
use std::rc::Rc;

use embedded_graphics::mono_font::MonoTextStyle;
use embedded_graphics::mono_font::ascii::{FONT_8X13, FONT_10X20};
use embedded_graphics::pixelcolor::Rgb888;
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::{PrimitiveStyle, Rectangle};
use embedded_graphics::text::{Alignment, Baseline, Text, TextStyleBuilder};
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::{Window, WindowId};

use crate::board::Board;
use crate::frame::{self, BACKGROUND, Canvas};

/// The space around and between the boards, and the height of the heading above them.
const GAP: u32 = 24;
const HEADING: u32 = 48;

/// Shows the boards, each with its description, e.g. what the firmware shows on it, and returns the one clicked, or
/// None if the window is closed first. It runs on the main thread, as the simulator's window does, and only once in a
/// program, as a window system allows one event loop, so a launcher starts the chosen board's program after it.
pub fn choose(boards: &[(&'static Board, &str)]) -> anyhow::Result<Option<&'static Board>> {
    let event_loop = EventLoop::new()?;
    let tiles = layout(boards);
    let size = Size::new(
        tiles.last().map_or(0, |tile| tile.right() as u32) + GAP,
        HEADING + tiles.iter().map(|tile| tile.size.height).max().unwrap_or(0) + GAP,
    );
    let mut picker = Picker {
        boards: boards
            .iter()
            .map(|(board, about)| (*board, about.to_string()))
            .collect(),
        tiles,
        size,
        window: None,
        cursor: None,
        drawn_hover: None,
        chosen: None,
        failed: None,
    };
    event_loop.run_app(&mut picker)?;
    match picker.failed {
        Some(error) => Err(error),
        None => Ok(picker.chosen),
    }
}

/// Where each board's frame goes, left to right.
fn layout(boards: &[(&'static Board, &str)]) -> Vec<Rectangle> {
    let mut x = GAP as i32;
    boards
        .iter()
        .map(|(board, _)| {
            let tile = Rectangle::new(Point::new(x, HEADING as i32), board.frame_size);
            x += (board.frame_size.width + GAP) as i32;
            tile
        })
        .collect()
}

trait Right {
    fn right(&self) -> i32;
}

impl Right for Rectangle {
    fn right(&self) -> i32 {
        self.top_left.x + self.size.width as i32
    }
}

struct Picker {
    boards: Vec<(&'static Board, String)>,
    tiles: Vec<Rectangle>,
    /// The whole picture, in its pixels, which the window is scaled to.
    size: Size,
    window: Option<crate::window::Shown>,
    /// Where the mouse is, in the picture's pixels.
    cursor: Option<Point>,
    /// The board under the mouse when last drawn, so that it is drawn again when that changes.
    drawn_hover: Option<usize>,
    chosen: Option<&'static Board>,
    /// Why the window could not be shown, if it could not, for choose to return.
    failed: Option<anyhow::Error>,
}

impl Picker {
    /// The board under the mouse.
    fn hovered(&self) -> Option<usize> {
        let at = self.cursor?;
        self.tiles.iter().position(|tile| tile.contains(at))
    }

    /// The picture: the heading, and each board in its frame, its id and description on its screen, outlined if the
    /// mouse is over it.
    fn picture(&self) -> Vec<u32> {
        let mut canvas = Canvas::new(self.size, BACKGROUND);
        let centred = TextStyleBuilder::new()
            .alignment(Alignment::Center)
            .baseline(Baseline::Middle)
            .build();
        let _ = Text::with_text_style(
            "Choose the board to simulate",
            Point::new(self.size.width as i32 / 2, HEADING as i32 / 2),
            MonoTextStyle::new(&FONT_10X20, Rgb888::new(0xe8, 0xea, 0xed)),
            centred,
        )
        .draw(&mut canvas);
        let hovered = self.hovered();
        for (index, ((board, about), tile)) in self.boards.iter().zip(&self.tiles).enumerate() {
            let frame = frame::drawn(board);
            let blank = match board.display {
                crate::DisplayKind::Mono => board.paper,
                crate::DisplayKind::Colour => 0,
            };
            let screen = vec![blank; (board.screen_size.width * board.screen_size.height) as usize];
            let pixels = frame::compose(board, &frame, &screen, None);
            for (row, line) in pixels.chunks(board.frame_size.width as usize).enumerate() {
                let start = (tile.top_left.y as usize + row) * self.size.width as usize + tile.top_left.x as usize;
                canvas.pixels[start..start + line.len()].copy_from_slice(line);
            }
            let ink = match board.display {
                crate::DisplayKind::Mono => {
                    Rgb888::new((board.ink >> 16) as u8, (board.ink >> 8) as u8, board.ink as u8)
                }
                crate::DisplayKind::Colour => Rgb888::new(0x06, 0xb6, 0xd4),
            };
            let centre = tile.top_left + board.screen_at + Point::new(board.screen_size.width as i32 / 2, 0);
            let middle = board.screen_size.height as i32 / 2;
            let _ = Text::with_text_style(
                board.id,
                centre + Point::new(0, middle - 30),
                MonoTextStyle::new(&FONT_10X20, ink),
                centred,
            )
            .draw(&mut canvas);
            for (line, text) in bruce_embedded_std::graphics::wrap(about, 40).iter().enumerate() {
                let _ = Text::with_text_style(
                    text,
                    centre + Point::new(0, middle + line as i32 * 16),
                    MonoTextStyle::new(&FONT_8X13, ink),
                    centred,
                )
                .draw(&mut canvas);
            }
            if hovered == Some(index) {
                let outline = Rectangle::new(tile.top_left - Point::new(4, 4), tile.size + Size::new(8, 8));
                let _ = outline
                    .into_styled(PrimitiveStyle::with_stroke(Rgb888::new(0x06, 0xb6, 0xd4), 3))
                    .draw(&mut canvas);
            }
        }
        canvas.pixels
    }

    /// Draws the picture, scaled to the window, each of the window's pixels the nearest of the picture's.
    fn draw(&mut self) -> anyhow::Result<()> {
        let pixels = self.picture();
        let Some((window, surface)) = self.window.as_mut() else {
            return Ok(());
        };
        let inner = window.inner_size();
        let (Some(width), Some(height)) = (NonZeroU32::new(inner.width), NonZeroU32::new(inner.height)) else {
            return Ok(());
        };
        surface
            .resize(width, height)
            .map_err(|error| anyhow::anyhow!("{error}"))?;
        let mut buffer = surface.buffer_mut().map_err(|error| anyhow::anyhow!("{error}"))?;
        let (picture_width, picture_height) = (self.size.width as usize, self.size.height as usize);
        let (width, height) = (inner.width as usize, inner.height as usize);
        for y in 0..height {
            let row = (y * picture_height / height) * picture_width;
            for x in 0..width {
                buffer[y * width + x] = pixels[row + x * picture_width / width];
            }
        }
        buffer.present().map_err(|error| anyhow::anyhow!("{error}"))?;
        Ok(())
    }
}

impl ApplicationHandler for Picker {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let attributes = Window::default_attributes()
            .with_title("Choose the board to simulate")
            .with_inner_size(LogicalSize::new(self.size.width, self.size.height));
        let opened = event_loop
            .create_window(attributes)
            .map_err(anyhow::Error::from)
            .and_then(|window| {
                let window = Rc::new(window);
                let context = softbuffer::Context::new(window.clone()).map_err(|error| anyhow::anyhow!("{error}"))?;
                let surface =
                    softbuffer::Surface::new(&context, window.clone()).map_err(|error| anyhow::anyhow!("{error}"))?;
                Ok((window, surface))
            });
        match opened {
            Ok(window) => self.window = Some(window),
            Err(error) => {
                self.failed = Some(error.context("Unable to open the window to choose a board"));
                event_loop.exit();
            }
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::RedrawRequested => {
                if let Err(error) = self.draw() {
                    self.failed = Some(error.context("Unable to draw the window to choose a board"));
                    event_loop.exit();
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                if let Some((window, _)) = &self.window {
                    let inner = window.inner_size();
                    let x = position.x * self.size.width as f64 / inner.width.max(1) as f64;
                    let y = position.y * self.size.height as f64 / inner.height.max(1) as f64;
                    self.cursor = Some(Point::new(x as i32, y as i32));
                    let hovered = self.hovered();
                    if hovered != self.drawn_hover {
                        self.drawn_hover = hovered;
                        window.request_redraw();
                    }
                }
            }
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            } => {
                if let Some(index) = self.hovered() {
                    self.chosen = Some(self.boards[index].0);
                    // Closed while the event loop runs, which is when the window system takes it off the screen,
                    // rather than when the picker is dropped, after it, as the chosen board's window opens.
                    if let Some((window, _)) = self.window.take() {
                        window.set_visible(false);
                    }
                    event_loop.exit();
                }
            }
            _ => {}
        }
    }
}
