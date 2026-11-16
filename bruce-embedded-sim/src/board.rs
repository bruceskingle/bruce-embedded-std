//! The boards which can be simulated: each one's display, buttons, touch and frame, see Board and BOARDS.

use embedded_graphics::prelude::{Point, Size};

/// What the board's display can show, which decides the display type its firmware draws on, see display.rs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DisplayKind {
    /// Black and white, e.g. the RLCD's reflective display: firmware draws in BinaryColor, On as ink and Off as paper.
    Mono,
    /// Colour, in Rgb565, as the boards' colour displays take.
    Colour,
}

/// A button on the board, which a click on it in the window presses, see Input::Button.
#[derive(Clone, Copy, Debug)]
pub struct ButtonSpec {
    /// Its name, as printed on the board, e.g. "KEY" or "BOOT", which firmware uses to tell them apart.
    pub name: &'static str,
    /// Where it is on the frame, in the frame's pixels, see Board::frame_size.
    pub top_left: Point,
    pub size: Size,
}

/// A board which can be simulated: what its firmware sees, its display and inputs, and how it looks in the window.
#[derive(Clone, Copy, Debug)]
pub struct Board {
    /// The short name given to --board, e.g. "rlcd42".
    pub id: &'static str,
    /// The platform's name for the board, as its `board-*` feature and firmware record have it, e.g.
    /// "wave-esp32s3-rlcd-42".
    pub platform_name: &'static str,
    /// The maker's name for it, shown on the frame and in the window's title.
    pub title: &'static str,
    pub display: DisplayKind,
    /// The display's visible area, in its own pixels, the size firmware draws on.
    pub screen_size: Size,
    /// Whether the screen takes touches, which mouse drags on it give, see Input::Touch.
    pub touch: bool,
    pub buttons: &'static [ButtonSpec],
    /// The whole frame, in its pixels, one of which is one of the screen's, with the screen at screen_at.
    pub frame_size: Size,
    pub screen_at: Point,
    /// An image of the board, a PNG frame_size wide and strip_top high, with a transparent hole where the screen is,
    /// at screen_at, see frames/README.md, or None for a frame drawn in board_colour, see frame.rs.
    pub photo: Option<&'static [u8]>,
    /// Where the strip under the board starts, which holds its name, a note from the window, and its buttons.
    pub strip_top: i32,
    /// The colour of the drawn frame's board, and of the strip under a photo, as 0x00RRGGBB, see frame.rs.
    pub board_colour: u32,
    /// The paper and ink of a black and white display, as 0x00RRGGBB, so that it looks like the real one.
    pub paper: u32,
    pub ink: u32,
}

impl Board {
    /// The board given to --board, by its id or its platform name.
    pub fn named(name: &str) -> Option<&'static Board> {
        BOARDS
            .iter()
            .find(|board| board.id.eq_ignore_ascii_case(name) || board.platform_name.eq_ignore_ascii_case(name))
    }

    /// The board's button whose name is name, e.g. "KEY".
    pub fn button(&self, name: &str) -> Option<&ButtonSpec> {
        self.buttons
            .iter()
            .find(|button| button.name.eq_ignore_ascii_case(name))
    }

    /// Whether the point of the frame is on the screen, and where on it, in the screen's pixels.
    pub fn on_screen(&self, point: Point) -> Option<Point> {
        let on = point - self.screen_at;
        (on.x >= 0 && on.y >= 0 && (on.x as u32) < self.screen_size.width && (on.y as u32) < self.screen_size.height)
            .then_some(on)
    }

    /// The button at the point of the frame, if any.
    pub fn button_at(&self, point: Point) -> Option<&ButtonSpec> {
        self.buttons.iter().find(|button| {
            let on = point - button.top_left;
            on.x >= 0 && on.y >= 0 && (on.x as u32) < button.size.width && (on.y as u32) < button.size.height
        })
    }
}

/// The space around the screen of a drawn frame, and the height of the strip under it which holds the buttons.
const MARGIN: i32 = 28;
const BUTTON_STRIP: u32 = 64;
/// Where a strip's buttons are, down from its top, and their size.
const BUTTON_TOP: i32 = 12;
const BUTTON: Size = Size::new(76, 30);

/// The RLCD's photo, frames/rlcd42.png, its size, and where its screen's hole is.
const RLCD_PHOTO: Size = Size::new(438, 324);
/// The hole is the whole panel, 400 x 300 at (9, 12), of which the screen is all but the pixel round its edge which
/// the bezel hides, see screen_size.
const RLCD_SCREEN_AT: Point = Point::new(10, 13);
/// The bottom of the Guitron's drawn frame's bezel, where its strip starts.
const GUITRON_STRIP: i32 = MARGIN + 320 + 6;

/// The boards which can be simulated.
pub static BOARDS: &[Board] = &[
    Board {
        id: "rlcd42",
        platform_name: "wave-esp32s3-rlcd-42",
        title: "Waveshare ESP32-S3-RLCD-4.2",
        display: DisplayKind::Mono,
        // The panel is 400 x 300, of which a pixel all round is hidden by the bezel, see the platform's st7305.rs.
        screen_size: Size::new(398, 298),
        touch: false,
        // Under the photo, as the photo, of its front, does not show them.
        buttons: &[
            ButtonSpec {
                name: "BOOT",
                top_left: Point::new(438 - 2 * 76 - 2 * 12, 324 + BUTTON_TOP),
                size: BUTTON,
            },
            ButtonSpec {
                name: "KEY",
                top_left: Point::new(438 - 76 - 12, 324 + BUTTON_TOP),
                size: BUTTON,
            },
        ],
        frame_size: Size::new(RLCD_PHOTO.width, RLCD_PHOTO.height + BUTTON_STRIP),
        screen_at: RLCD_SCREEN_AT,
        photo: Some(include_bytes!("../frames/rlcd42.png")),
        strip_top: RLCD_PHOTO.height as i32,
        board_colour: 0x1d1f22,
        // A reflective display is never white or black: grey-green paper, dark grey ink.
        paper: 0xc9cdbf,
        ink: 0x2b2d2c,
    },
    Board {
        id: "guitron",
        platform_name: "guition-jc3248w535c",
        title: "Guition JC3248W535C",
        display: DisplayKind::Colour,
        // Landscape, with its USB port on the right, as the platform's axs15231b.rs has it.
        screen_size: Size::new(480, 320),
        touch: true,
        buttons: &[ButtonSpec {
            name: "BOOT",
            top_left: Point::new(MARGIN + 480 - 76, GUITRON_STRIP + BUTTON_TOP),
            size: BUTTON,
        }],
        frame_size: Size::new(480 + 2 * MARGIN as u32, GUITRON_STRIP as u32 + BUTTON_STRIP),
        screen_at: Point::new(MARGIN, MARGIN),
        photo: None,
        strip_top: GUITRON_STRIP,
        board_colour: 0x0f1012,
        paper: 0,
        ink: 0,
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn boards_are_found_by_id_or_platform_name() {
        assert_eq!(Board::named("RLCD42").unwrap().platform_name, "wave-esp32s3-rlcd-42");
        assert_eq!(Board::named("guition-jc3248w535c").unwrap().id, "guitron");
        assert!(Board::named("cyd").is_none());
    }

    #[test]
    fn buttons_and_screen_are_inside_the_frame_and_apart() {
        for board in BOARDS {
            assert_eq!(board.on_screen(board.screen_at), Some(Point::zero()));
            for button in board.buttons {
                let corner = button.top_left + button.size;
                assert!(corner.x as u32 <= board.frame_size.width && corner.y as u32 <= board.frame_size.height);
                assert!(board.on_screen(button.top_left).is_none());
                assert_eq!(board.button_at(button.top_left).unwrap().name, button.name);
            }
        }
    }
}
