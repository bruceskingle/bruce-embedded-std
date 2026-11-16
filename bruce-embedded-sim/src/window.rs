//! The simulator's window: the board, frame and screen, drawn whenever the screen changes, scaled to the window, and the
//! mouse's clicks on its buttons and drags on its screen, as Inputs, see lib.rs.

use std::num::NonZeroU32;
use std::rc::Rc;
use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};

use embedded_graphics::prelude::Point;
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalPosition, PhysicalSize};
use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key, ModifiersState};
use winit::window::{Window, WindowId};

use crate::frame::{self, Canvas};
use crate::{Device, Input};

/// How often the window looks for a change to the screen, about 30 times a second.
const FRAME_INTERVAL: Duration = Duration::from_millis(33);

/// Runs the window until it is closed.
pub fn run(device: Device, inputs: Sender<Input>) -> anyhow::Result<()> {
    let event_loop = EventLoop::new()?;
    let frame = frame::drawn(device.board);
    let mut app = App {
        device,
        inputs,
        frame,
        window: None,
        cursor: None,
        pressed: None,
        touching: false,
        redraw: true,
        modifiers: ModifiersState::empty(),
        titled: None,
        copied: None,
        noted: None,
    };
    event_loop.run_app(&mut app)?;
    Ok(())
}

struct App {
    device: Device,
    inputs: Sender<Input>,
    frame: Canvas,
    window: Option<(Rc<Window>, softbuffer::Surface<Rc<Window>, Rc<Window>>)>,
    /// Where the mouse is, in the frame's pixels.
    cursor: Option<Point>,
    /// The button the mouse is holding down, if any.
    pressed: Option<&'static str>,
    /// Whether the mouse is holding down a touch on the screen.
    touching: bool,
    /// Whether to draw again, even if the screen has not changed, e.g. as a button was pressed.
    redraw: bool,
    /// The keyboard's modifiers held down, for Cmd+C and Ctrl+C.
    modifiers: ModifiersState,
    /// The address the title shows, see Device::set_address.
    titled: Option<String>,
    /// What was last copied, and when, which the note says for COPIED_NOTE, see note.
    copied: Option<(String, Instant)>,
    /// The note last drawn, so that it is drawn again when it changes.
    noted: Option<(String, bool)>,
}

/// How long the note says what was copied.
const COPIED_NOTE: Duration = Duration::from_secs(3);

/// The keys which copy, as the computer has them, in ASCII, as the frame's font has nothing else.
fn copy_keys() -> &'static str {
    if cfg!(target_os = "macos") { "Cmd+C" } else { "Ctrl+C" }
}

/// The window's place, x, y, width and height, in the screen's pixels, kept in file, see Device::set_window_file.
fn read_place(file: &std::path::Path) -> Option<[i32; 4]> {
    let text = std::fs::read_to_string(file).ok()?;
    let numbers: Vec<i32> = text
        .split_whitespace()
        .filter_map(|number| number.parse().ok())
        .collect();
    numbers.try_into().ok()
}

impl App {
    /// Keeps where the window is, and its size, for when the device next starts, see read_place.
    fn save_place(&self) {
        let (Some((window, _)), Some(file)) = (&self.window, self.device.window_file()) else {
            return;
        };
        // As with_position takes it when the window is made: its content's top left on macOS, its frame's elsewhere.
        let position = if cfg!(target_os = "macos") {
            window.inner_position()
        } else {
            window.outer_position()
        };
        let Ok(position) = position else {
            return;
        };
        let size = window.inner_size();
        let place = format!("{} {} {} {}\n", position.x, position.y, size.width, size.height);
        if let Err(error) = std::fs::write(&file, place) {
            log::debug!("Unable to keep where the window is, {error}");
        }
    }

    /// Copies what can be copied, the device's address, or e.g. its pairing code while it shows one, for the user to
    /// paste into an app, see Device::copyable, and says so in the note.
    fn copy(&mut self) {
        match self.device.copyable() {
            Some((what, text)) => match crate::clipboard::copy(&text) {
                Ok(()) => {
                    log::info!("Copied the {what}, {text}");
                    self.copied = Some((format!("Copied the {what}, {text}"), Instant::now()));
                }
                Err(error) => log::warn!("Unable to copy the {what}, {text}, {error:#}"),
            },
            None => log::info!("The device has no address yet"),
        }
    }

    /// The note under the board's name: what was just copied, highlighted, or what can be, and how.
    fn note(&self) -> Option<(String, bool)> {
        if let Some((copied, when)) = &self.copied
            && when.elapsed() < COPIED_NOTE
        {
            return Some((copied.clone(), true));
        }
        self.device
            .copyable()
            .map(|(what, _)| (format!("{} or right click: copy the {what}", copy_keys()), false))
    }

    /// Where on the frame the position in the window is, in the frame's pixels.
    fn to_frame(&self, position: PhysicalPosition<f64>) -> Option<Point> {
        let (window, _) = self.window.as_ref()?;
        let size = window.inner_size();
        let board = self.device.board;
        let x = position.x * board.frame_size.width as f64 / size.width.max(1) as f64;
        let y = position.y * board.frame_size.height as f64 / size.height.max(1) as f64;
        Some(Point::new(x as i32, y as i32))
    }

    fn send(&self, input: Input) {
        let _ = self.inputs.send(input);
    }

    /// The mouse's left button went down or up, at the cursor.
    fn click(&mut self, state: ElementState) {
        let board = self.device.board;
        match state {
            ElementState::Pressed => {
                let Some(at) = self.cursor else { return };
                if let Some(button) = board.button_at(at) {
                    self.pressed = Some(button.name);
                    self.redraw = true;
                    self.send(Input::Button {
                        name: button.name,
                        pressed: true,
                    });
                } else if let Some(point) = board.on_screen(at).filter(|_| board.touch) {
                    self.touching = true;
                    self.send(Input::Touch(point));
                }
            }
            ElementState::Released => {
                if let Some(name) = self.pressed.take() {
                    self.redraw = true;
                    self.send(Input::Button { name, pressed: false });
                }
                if std::mem::take(&mut self.touching) {
                    self.send(Input::TouchEnd);
                }
            }
        }
    }

    /// Draws the board, scaled to the window, each of the window's pixels the nearest of the board's.
    fn draw(&mut self) -> anyhow::Result<()> {
        let Some((window, surface)) = self.window.as_mut() else {
            return Ok(());
        };
        let size = window.inner_size();
        let (Some(width), Some(height)) = (NonZeroU32::new(size.width), NonZeroU32::new(size.height)) else {
            return Ok(());
        };
        surface
            .resize(width, height)
            .map_err(|error| anyhow::anyhow!("{error}"))?;
        let board = self.device.board;
        let mut pixels = frame::compose(board, &self.frame, &self.device.screen.pixels(), self.pressed);
        if let Some((note, highlight)) = &self.noted {
            frame::draw_note(board, &mut pixels, note, *highlight);
        }
        let mut buffer = surface.buffer_mut().map_err(|error| anyhow::anyhow!("{error}"))?;
        let (frame_width, frame_height) = (board.frame_size.width as usize, board.frame_size.height as usize);
        let (width, height) = (size.width as usize, size.height as usize);
        let columns: Vec<usize> = (0..width).map(|x| x * frame_width / width).collect();
        for y in 0..height {
            let row = (y * frame_height / height) * frame_width;
            let line = &mut buffer[y * width..(y + 1) * width];
            for (x, pixel) in line.iter_mut().enumerate() {
                *pixel = pixels[row + columns[x]];
            }
        }
        buffer.present().map_err(|error| anyhow::anyhow!("{error}"))?;
        Ok(())
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let board = self.device.board;
        let mut attributes = Window::default_attributes()
            .with_title(format!("{} (simulated)", board.title))
            .with_inner_size(LogicalSize::new(board.frame_size.width, board.frame_size.height))
            .with_min_inner_size(LogicalSize::new(
                board.frame_size.width / 2,
                board.frame_size.height / 2,
            ));
        // Where it was, if it has been open before, see save_place.
        if let Some([x, y, width, height]) = self.device.window_file().and_then(|file| read_place(&file)) {
            attributes = attributes
                .with_position(PhysicalPosition::new(x, y))
                .with_inner_size(PhysicalSize::new(width.max(1) as u32, height.max(1) as u32));
        }
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
                log::error!("Unable to open the simulator's window, {error:#}");
                event_loop.exit();
            }
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::RedrawRequested => {
                if let Err(error) = self.draw() {
                    log::error!("Unable to draw the simulator's window, {error:#}");
                }
            }
            WindowEvent::Resized(_) => {
                self.redraw = true;
                self.save_place();
            }
            WindowEvent::Moved(_) => self.save_place(),
            WindowEvent::CursorMoved { position, .. } => {
                self.cursor = self.to_frame(position);
                if self.touching {
                    let point = self.cursor.and_then(|at| self.device.board.on_screen(at));
                    match point {
                        Some(point) => self.send(Input::Touch(point)),
                        // Dragged off the screen, which ends the touch, as lifting the finger would.
                        None => {
                            self.touching = false;
                            self.send(Input::TouchEnd);
                        }
                    }
                }
            }
            WindowEvent::CursorLeft { .. } => self.cursor = None,
            WindowEvent::ModifiersChanged(modifiers) => self.modifiers = modifiers.state(),
            // Cmd+C on macOS, Ctrl+C elsewhere, copies the device's address, as there is no text to select.
            WindowEvent::KeyboardInput { event, .. }
                if event.state == ElementState::Pressed
                    && event.logical_key == Key::Character("c".into())
                    && (if cfg!(target_os = "macos") {
                        self.modifiers.super_key()
                    } else {
                        self.modifiers.control_key()
                    }) =>
            {
                self.copy()
            }
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Right,
                ..
            } => self.copy(),
            WindowEvent::MouseInput {
                state,
                button: MouseButton::Left,
                ..
            } => self.click(state),
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let note = self.note();
        if note != self.noted {
            self.noted = note;
            self.redraw = true;
        }
        let address = self.device.address();
        if address != self.titled
            && let Some((window, _)) = &self.window
        {
            let board = self.device.board;
            window.set_title(&match &address {
                Some(address) => format!("{} (simulated) at {address}", board.title),
                None => format!("{} (simulated)", board.title),
            });
            self.titled = address;
        }
        if (self.device.screen.take_changed() || std::mem::take(&mut self.redraw))
            && let Some((window, _)) = &self.window
        {
            window.request_redraw();
        }
        event_loop.set_control_flow(ControlFlow::WaitUntil(Instant::now() + FRAME_INTERVAL));
    }
}
