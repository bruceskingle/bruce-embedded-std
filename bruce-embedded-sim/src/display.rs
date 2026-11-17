//! The simulated displays, which firmware draws on as it does on the real boards', see MonoDisplay and ColourDisplay,
//! with display managers which behave as the boards' do, and the Screen they put their pixels on for the window.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use bruce_embedded_std::graphics::{Color, Console, DisplayManager, rgb565_from_rgb8, to_rgb565};
use embedded_graphics::pixelcolor::{BinaryColor, Rgb565};
use embedded_graphics::prelude::*;

use crate::board::Board;

/// The pixels the window shows, as 0x00RRGGBB, which the displays draw on, see MonoDisplay and ColourDisplay, and which
/// tells the window when they have changed, see take_changed.
pub struct Screen {
    pub size: Size,
    pixels: Mutex<Vec<u32>>,
    changed: AtomicBool,
}

impl Screen {
    pub fn new(size: Size, colour: u32) -> Self {
        Screen {
            size,
            pixels: Mutex::new(vec![colour; (size.width * size.height) as usize]),
            changed: AtomicBool::new(true),
        }
    }

    /// A copy of the pixels, row by row.
    pub fn pixels(&self) -> Vec<u32> {
        self.pixels.lock().unwrap().clone()
    }

    /// Whether the pixels have changed since this was last asked, so that the window draws only then.
    pub fn take_changed(&self) -> bool {
        self.changed.swap(false, Ordering::Relaxed)
    }

    /// Sets the pixels which are on the screen, ignoring any off it, as a real display does.
    fn set(&self, pixels: impl IntoIterator<Item = (Point, u32)>) {
        let mut buffer = self.pixels.lock().unwrap();
        let (width, height) = (self.size.width as i32, self.size.height as i32);
        for (point, colour) in pixels {
            if point.x >= 0 && point.y >= 0 && point.x < width && point.y < height {
                buffer[(point.y * width + point.x) as usize] = colour;
            }
        }
        self.changed.store(true, Ordering::Relaxed);
    }

    fn fill(&self, colour: u32) {
        self.pixels.lock().unwrap().fill(colour);
        self.changed.store(true, Ordering::Relaxed);
    }
}

/// A black and white display, e.g. the RLCD's, drawn in BinaryColor as the real one is: On is ink, Off is paper.
pub struct MonoDisplay {
    screen: Arc<Screen>,
    paper: u32,
    ink: u32,
}

impl MonoDisplay {
    pub fn new(screen: Arc<Screen>, board: &Board) -> Self {
        MonoDisplay {
            screen,
            paper: board.paper,
            ink: board.ink,
        }
    }

    fn colour(&self, color: BinaryColor) -> u32 {
        match color {
            BinaryColor::On => self.ink,
            BinaryColor::Off => self.paper,
        }
    }
}

impl OriginDimensions for MonoDisplay {
    fn size(&self) -> Size {
        self.screen.size
    }
}

impl DrawTarget for MonoDisplay {
    type Color = BinaryColor;
    type Error = core::convert::Infallible;

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<Self::Color>>,
    {
        self.screen.set(
            pixels
                .into_iter()
                .map(|Pixel(point, color)| (point, self.colour(color))),
        );
        Ok(())
    }

    fn clear(&mut self, color: Self::Color) -> Result<(), Self::Error> {
        self.screen.fill(self.colour(color));
        Ok(())
    }
}

/// A colour display, e.g. the Guitron's, drawn in Rgb565 as the real ones are.
pub struct ColourDisplay {
    screen: Arc<Screen>,
}

impl ColourDisplay {
    pub fn new(screen: Arc<Screen>) -> Self {
        ColourDisplay { screen }
    }
}

/// The colour as 0x00RRGGBB, each part's low bits filled from its high ones, so that white is white.
fn rgb565_to_u32(color: Rgb565) -> u32 {
    let (r, g, b) = (color.r() as u32, color.g() as u32, color.b() as u32);
    let (r, g, b) = ((r << 3) | (r >> 2), (g << 2) | (g >> 4), (b << 3) | (b >> 2));
    (r << 16) | (g << 8) | b
}

impl OriginDimensions for ColourDisplay {
    fn size(&self) -> Size {
        self.screen.size
    }
}

impl DrawTarget for ColourDisplay {
    type Color = Rgb565;
    type Error = core::convert::Infallible;

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<Self::Color>>,
    {
        self.screen.set(
            pixels
                .into_iter()
                .map(|Pixel(point, color)| (point, rgb565_to_u32(color))),
        );
        Ok(())
    }

    fn clear(&mut self, color: Self::Color) -> Result<(), Self::Error> {
        self.screen.fill(rgb565_to_u32(color));
        Ok(())
    }
}

/// The display manager of a black and white display, which behaves as the RLCD's, the platform's
/// St7305DisplayManager: its console font, and Color::Black as paper, any other colour as ink.
pub struct MonoDisplayManager {
    pub display: MonoDisplay,
    pub overlay: bool,
    pub generation: u32,
}

impl MonoDisplayManager {
    pub fn new(display: MonoDisplay) -> Self {
        MonoDisplayManager {
            display,
            overlay: false,
            generation: 0,
        }
    }
}

impl DisplayManager for MonoDisplayManager {
    type Display = MonoDisplay;

    fn display(&mut self) -> &mut Self::Display {
        &mut self.display
    }

    fn console(&self) -> Console {
        Console {
            font: &embedded_graphics::mono_font::ascii::FONT_9X15_BOLD,
            margin: 8,
        }
    }

    fn overlay_active(&self) -> bool {
        self.overlay
    }

    fn generation(&self) -> u32 {
        self.generation
    }

    fn set_overlay(&mut self, active: bool) -> anyhow::Result<()> {
        if self.overlay && !active {
            self.clear()?;
        }
        self.overlay = active;
        Ok(())
    }

    fn clear(&mut self) -> anyhow::Result<()> {
        self.fill_color(Color::Black)?;
        self.generation = self.generation.wrapping_add(1);
        Ok(())
    }

    fn fill_color(&mut self, color: Color) -> anyhow::Result<()> {
        let color = self.map_color(&color);
        let _ = self.display.clear(color);
        Ok(())
    }

    fn map_color(&self, color: &Color) -> BinaryColor {
        match color {
            Color::Black => BinaryColor::Off,
            _ => BinaryColor::On,
        }
    }

    fn map_rgb8(&self, color: &rgb::RGB8) -> BinaryColor {
        let luminance = (color.r as u32 * 299 + color.g as u32 * 587 + color.b as u32 * 114) / 1000;
        if luminance < 64 {
            BinaryColor::Off
        } else {
            BinaryColor::On
        }
    }
}

/// The display manager of a colour display, which behaves as the Guitron's, the platform's Axs15231bDisplayManager.
pub struct ColourDisplayManager {
    pub display: ColourDisplay,
    pub overlay: bool,
    pub generation: u32,
}

impl ColourDisplayManager {
    pub fn new(display: ColourDisplay) -> Self {
        ColourDisplayManager {
            display,
            overlay: false,
            generation: 0,
        }
    }
}

impl DisplayManager for ColourDisplayManager {
    type Display = ColourDisplay;

    fn display(&mut self) -> &mut Self::Display {
        &mut self.display
    }

    fn console(&self) -> Console {
        Console {
            font: &embedded_graphics::mono_font::ascii::FONT_10X20,
            margin: 8,
        }
    }

    fn overlay_active(&self) -> bool {
        self.overlay
    }

    fn generation(&self) -> u32 {
        self.generation
    }

    fn set_overlay(&mut self, active: bool) -> anyhow::Result<()> {
        if self.overlay && !active {
            self.clear()?;
        }
        self.overlay = active;
        Ok(())
    }

    fn clear(&mut self) -> anyhow::Result<()> {
        self.fill_color(Color::Black)?;
        self.generation = self.generation.wrapping_add(1);
        Ok(())
    }

    fn fill_color(&mut self, color: Color) -> anyhow::Result<()> {
        let color = self.map_color(&color);
        let _ = self.display.clear(color);
        Ok(())
    }

    fn map_color(&self, color: &Color) -> Rgb565 {
        to_rgb565(color)
    }

    fn map_rgb8(&self, color: &rgb::RGB8) -> Rgb565 {
        rgb565_from_rgb8(color)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::Board;
    use embedded_graphics::primitives::{PrimitiveStyle, Rectangle};

    #[test]
    fn white_and_black_are_exact() {
        assert_eq!(rgb565_to_u32(Rgb565::WHITE), 0xffffff);
        assert_eq!(rgb565_to_u32(Rgb565::BLACK), 0);
        assert_eq!(rgb565_to_u32(Rgb565::RED), 0xff0000);
    }

    #[test]
    fn a_mono_display_draws_ink_on_paper_and_ignores_pixels_off_it() {
        let board = Board::named("rlcd42").unwrap();
        let screen = Arc::new(Screen::new(board.screen_size, 0));
        let mut manager = MonoDisplayManager::new(MonoDisplay::new(screen.clone(), board));
        manager.clear().unwrap();
        assert!(screen.take_changed());
        Rectangle::new(Point::new(-5, -5), Size::new(7, 7))
            .into_styled(PrimitiveStyle::with_fill(BinaryColor::On))
            .draw(manager.display())
            .unwrap();
        let pixels = screen.pixels();
        assert_eq!(pixels[0], board.ink);
        assert_eq!(pixels[2], board.paper);
        assert_eq!(manager.generation(), 1);
    }
}
