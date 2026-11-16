//! The analog clock on a simulated board, as the board examples of bruce-esp-idf show it, with a message when a button
//! is pressed, and, on a touch screen, a dot where it is touched, to try the simulator:
//!
//! `cargo run -p bruce-embedded-sim --example clock -- --board rlcd42`
//!
//! or, for a screenshot, `... --board guitron --screenshot clock.png --after 2 --headless`.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use bruce_embedded_sim::{Device, DisplayKind, Input};
use bruce_embedded_std::graphics::{ClockRenderer, DisplayManager, DrawTargetResultExt};
use embedded_graphics::mono_font::MonoTextStyle;
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::{Circle, PrimitiveStyle, Rectangle};
use embedded_graphics::text::{Baseline, Text};
use rgb::RGB8;

fn main() -> anyhow::Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let board = bruce_embedded_sim::board_from_args(&mut args)?;
    let options = bruce_embedded_sim::Options::from_args(&mut args)?;
    bruce_embedded_sim::run(board, options, |device| match device.board.display {
        DisplayKind::Mono => firmware(&device, device.mono_display_manager()?),
        DisplayKind::Colour => firmware(&device, device.colour_display_manager()?),
    })
}

/// The clock, filling the screen, redrawn every second, with a message under it while a button is held, and a dot
/// where the screen is touched.
fn firmware<DM: DisplayManager + Send + 'static>(device: &Device, manager: DM) -> anyhow::Result<()> {
    let manager = Arc::new(Mutex::new(manager));
    manager.lock().unwrap().clear()?;
    let mut clock = ClockRenderer::new(
        &manager,
        |area: &Rectangle| {
            let side = area.size.width.min(area.size.height) - 8;
            Rectangle::with_center(area.center(), Size::new(side, side))
        },
        RGB8::new(0, 255, 0),
        RGB8::new(0, 0, 0),
    )?;
    {
        let manager = manager.clone();
        device.on_input(move |input| {
            let mut manager = manager.lock().unwrap();
            let font = manager.console().font;
            let ink = manager.map_rgb8(&RGB8::new(255, 255, 0));
            let result = match input {
                Input::Button { name, pressed } => {
                    let text = if *pressed {
                        format!("{name} pressed ")
                    } else {
                        " ".repeat(name.len() + 8)
                    };
                    Text::with_baseline(&text, Point::new(4, 4), MonoTextStyle::new(font, ink), Baseline::Top)
                        .draw(manager.display())
                        .map(|_| ())
                        .anyhow()
                }
                Input::Touch(point) => Circle::with_center(*point, 9)
                    .into_styled(PrimitiveStyle::with_fill(ink))
                    .draw(manager.display())
                    .anyhow(),
                Input::TouchEnd => Ok(()),
            };
            if let Err(error) = result {
                log::error!("Unable to draw, {error:#}");
            }
        });
    }
    loop {
        clock.update()?;
        std::thread::sleep(Duration::from_millis(1000));
    }
}
