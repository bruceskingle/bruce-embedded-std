//! Bruce's Standard Library Embedded Platform's board simulator: firmware for the platform run on a desktop computer,
//! with its board's display in a window, framed by a drawing or photo of the board, its buttons pressed with the mouse,
//! and its touch screen, if it has one, touched by dragging on it. It simulates rather than emulates: the display,
//! buttons and touch behave as the real ones do, but nothing of the chip or its limits is imitated. See
//! docs/Simulator.md.
//!
//! The firmware runs on a thread of its own, see run, given a Device, which has the board's display, see
//! Device::mono_display_manager and colour_display_manager, and its inputs, see Device::on_input, while the window runs
//! on the main thread, as macOS needs it to.

pub mod board;
mod clipboard;
pub mod display;
pub mod frame;
pub mod picker;
mod screenshot;
#[cfg(feature = "svc")]
pub mod svc;
mod window;

use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::anyhow;
use embedded_graphics::prelude::Point;

pub use board::{BOARDS, Board, ButtonSpec, DisplayKind};
pub use display::{ColourDisplay, ColourDisplayManager, MonoDisplay, MonoDisplayManager, Screen};

/// What the user did to the board in the window.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Input {
    /// A button was pressed, or let go, by its name, e.g. "KEY".
    Button { name: &'static str, pressed: bool },
    /// The screen was touched, or a touch moved, at the point, in the screen's pixels, by the mouse's left button held
    /// down on it.
    Touch(Point),
    /// The touch ended, the mouse's button let go.
    TouchEnd,
}

/// Something which is told of each Input, see Device::on_input.
type InputListener = Box<dyn Fn(&Input) + Send>;

/// The simulated board, as the firmware sees it: its description, its display and its inputs.
#[derive(Clone)]
pub struct Device {
    pub board: &'static Board,
    pub screen: Arc<Screen>,
    listeners: Arc<Mutex<Vec<InputListener>>>,
    /// Where an app reaches the device, once the firmware has said, see set_address.
    address: Arc<Mutex<Option<String>>>,
    /// What the user copies instead of the address, while the firmware says, see set_copyable.
    copyable: Arc<Mutex<Option<(&'static str, String)>>>,
    /// Where the window's place on the screen is kept, see set_window_file.
    window_file: Arc<Mutex<Option<std::path::PathBuf>>>,
}

impl Device {
    /// The simulated board, with its screen blank, before its firmware runs, see run_device.
    pub fn new(board: &'static Board) -> Self {
        let blank = match board.display {
            DisplayKind::Mono => board.paper,
            DisplayKind::Colour => 0,
        };
        Device {
            board,
            screen: Arc::new(Screen::new(board.screen_size, blank)),
            listeners: Arc::new(Mutex::new(Vec::new())),
            address: Arc::new(Mutex::new(None)),
            copyable: Arc::new(Mutex::new(None)),
            window_file: Arc::new(Mutex::new(None)),
        }
    }

    /// Says where an app reaches the device, e.g. 192.168.1.190:60496, which the window shows in its title, and
    /// copies for the user to paste, e.g. into the app, with Cmd+C or Ctrl+C, or a right click, see copyable.
    pub fn set_address(&self, address: String) {
        *self.address.lock().unwrap() = Some(address);
    }

    pub fn address(&self) -> Option<String> {
        self.address.lock().unwrap().clone()
    }

    /// Keeps the window's place on the screen, and its size, in file, so that it opens where it was when the device
    /// starts again, e.g. as it restarts itself, which starts a new program with a new window, see the window.
    pub fn set_window_file(&self, file: std::path::PathBuf) {
        *self.window_file.lock().unwrap() = Some(file);
    }

    fn window_file(&self) -> Option<std::path::PathBuf> {
        self.window_file.lock().unwrap().clone()
    }

    /// Says what the user copies in place of the address, by what it is and its text, e.g. ("pairing code",
    /// "123456") while the screen shows one, or, with None, that it is the address again.
    pub fn set_copyable(&self, copyable: Option<(&'static str, String)>) {
        *self.copyable.lock().unwrap() = copyable;
    }

    /// What the user copies now, by what it is and its text: what the firmware has said, see set_copyable, or the
    /// address, if there is one yet.
    pub fn copyable(&self) -> Option<(&'static str, String)> {
        self.copyable
            .lock()
            .unwrap()
            .clone()
            .or_else(|| self.address().map(|address| ("address", address)))
    }

    /// The display manager of the board's black and white display, or an error if its display is in colour.
    pub fn mono_display_manager(&self) -> anyhow::Result<MonoDisplayManager> {
        match self.board.display {
            DisplayKind::Mono => Ok(MonoDisplayManager::new(MonoDisplay::new(
                self.screen.clone(),
                self.board,
            ))),
            DisplayKind::Colour => Err(anyhow!("The {} has a colour display", self.board.title)),
        }
    }

    /// The display manager of the board's colour display, or an error if its display is black and white.
    pub fn colour_display_manager(&self) -> anyhow::Result<ColourDisplayManager> {
        match self.board.display {
            DisplayKind::Colour => Ok(ColourDisplayManager::new(ColourDisplay::new(self.screen.clone()))),
            DisplayKind::Mono => Err(anyhow!("The {} has a black and white display", self.board.title)),
        }
    }

    /// Calls listener with each Input, on the simulator's input thread, not the window's, so that it may take a while,
    /// e.g. to draw or play a sound, as a real button's action may.
    pub fn on_input(&self, listener: impl Fn(&Input) + Send + 'static) {
        self.listeners.lock().unwrap().push(Box::new(listener));
    }

    /// Gives each Input from the window to the listeners, see on_input, until the window has gone.
    fn dispatch(&self, inputs: Receiver<Input>) {
        for input in inputs {
            for listener in self.listeners.lock().unwrap().iter() {
                listener(&input);
            }
        }
    }
}

/// How the simulator runs, from its command line, see Options::from_args.
#[derive(Clone, Debug, Default)]
pub struct Options {
    /// Where to save a screenshot of the board, once it has run for screenshot_after, after which it ends.
    pub screenshot: Option<PathBuf>,
    pub screenshot_after: Duration,
    /// How many of the screenshot's pixels each of the board's is across and down, 2 by default, for sharp images on
    /// high resolution screens.
    pub screenshot_scale: u32,
    /// Without a window, e.g. to take screenshots from a script, or on a computer with no screen.
    pub headless: bool,
}

impl Options {
    /// The options the simulator takes, from args, which are left with those it did not take, for the firmware:
    /// --screenshot FILE, --after SECONDS, --scale N and --headless.
    pub fn from_args(args: &mut Vec<String>) -> anyhow::Result<Self> {
        let mut options = Options {
            screenshot_scale: 2,
            ..Default::default()
        };
        let mut rest = Vec::new();
        let mut iter = std::mem::take(args).into_iter();
        while let Some(arg) = iter.next() {
            let mut value = |name: &str| iter.next().ok_or_else(|| anyhow!("{name} needs a value"));
            match arg.as_str() {
                "--screenshot" => options.screenshot = Some(PathBuf::from(value("--screenshot")?)),
                "--after" => {
                    let seconds: f64 = value("--after")?
                        .parse()
                        .map_err(|_| anyhow!("--after takes seconds"))?;
                    options.screenshot_after = Duration::from_secs_f64(seconds);
                }
                "--scale" => {
                    options.screenshot_scale =
                        value("--scale")?.parse().map_err(|_| anyhow!("--scale takes 1 to 8"))?;
                    if !(1..=8).contains(&options.screenshot_scale) {
                        return Err(anyhow!("--scale takes 1 to 8"));
                    }
                }
                "--headless" => options.headless = true,
                _ => rest.push(arg),
            }
        }
        *args = rest;
        Ok(options)
    }
}

/// The board given by --board NAME in args, taken out of them, see Board::named, or an error listing the boards.
pub fn board_from_args(args: &mut Vec<String>) -> anyhow::Result<&'static Board> {
    let names = || BOARDS.iter().map(|board| board.id).collect::<Vec<_>>().join(", ");
    let position = args
        .iter()
        .position(|arg| arg == "--board")
        .ok_or_else(|| anyhow!("Say which board to simulate with --board, one of {}", names()))?;
    let name = args
        .get(position + 1)
        .cloned()
        .ok_or_else(|| anyhow!("--board needs one of {}", names()))?;
    args.drain(position..=position + 1);
    Board::named(&name).ok_or_else(|| anyhow!("There is no board {name}, only {}", names()))
}

/// Runs firmware on the simulated board: firmware on a thread of its own, given the Device, and the window on this
/// one, until the window is closed, or a screenshot has been taken, see Options, when the process ends, as firmware
/// never returns. It returns only if the window cannot be opened.
pub fn run(
    board: &'static Board,
    options: Options,
    firmware: impl FnOnce(Device) -> anyhow::Result<()> + Send + 'static,
) -> anyhow::Result<()> {
    let device = Device::new(board);
    let given = device.clone();
    start(device, options, Box::new(move || firmware(given)))
}

/// Runs firmware on device, a board made before it with Device::new, as run does, for firmware which is not Send, e.g.
/// a platform built on the main thread with what it needs, which must give the main thread to the window, as macOS
/// requires, see bruce-esp-idf's platform for the computer.
///
/// # Safety
///
/// Nothing firmware holds, e.g. an Rc, may be shared with what stays on this thread, as firmware runs on another: it
/// must be moved whole, with every clone of anything in it, and this thread does nothing else but run the window.
pub unsafe fn run_device(
    device: Device,
    options: Options,
    firmware: impl FnOnce() -> anyhow::Result<()> + 'static,
) -> anyhow::Result<()> {
    /// The firmware, moved to its thread, see run_device's safety.
    struct Moved<F>(F);
    // SAFETY: see run_device: the firmware is moved whole to the one thread which uses it.
    unsafe impl<F> Send for Moved<F> {}
    impl<F: FnOnce() -> anyhow::Result<()>> Moved<F> {
        fn call(self) -> anyhow::Result<()> {
            (self.0)()
        }
    }
    let moved = Moved(firmware);
    start(device, options, Box::new(move || moved.call()))
}

/// Starts the firmware's thread, the inputs' and any screenshot's, then runs the window, see run.
fn start(
    device: Device,
    options: Options,
    firmware: Box<dyn FnOnce() -> anyhow::Result<()> + Send>,
) -> anyhow::Result<()> {
    let (inputs, received): (Sender<Input>, Receiver<Input>) = std::sync::mpsc::channel();
    {
        let device = device.clone();
        std::thread::Builder::new()
            .name("inputs".into())
            .spawn(move || device.dispatch(received))?;
    }
    std::thread::Builder::new()
        // As large as a computer gives the main thread, as the firmware, written for the device, may assume it is
        // the main thread.
        .stack_size(8 * 1024 * 1024)
        .name("firmware".into())
        .spawn(move || {
            if let Err(error) = firmware() {
                log::error!("The firmware stopped, {error:#}");
            }
        })?;
    if let Some(path) = &options.screenshot {
        let path = path.clone();
        let device = device.clone();
        std::thread::spawn(move || {
            std::thread::sleep(options.screenshot_after);
            let result = screenshot::save(&device, &path, options.screenshot_scale);
            match result {
                Ok(()) => {
                    log::info!("Saved a screenshot to {}", path.display());
                    std::process::exit(0)
                }
                Err(error) => {
                    log::error!("Unable to save a screenshot to {}, {error:#}", path.display());
                    std::process::exit(1)
                }
            }
        });
    }
    if options.headless {
        loop {
            std::thread::park();
        }
    }
    window::run(device, inputs)?;
    std::process::exit(0)
}
