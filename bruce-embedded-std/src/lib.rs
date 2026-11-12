use std::sync::{Condvar, Mutex};
use std::time::Duration;

use embedded_graphics::primitives::Rectangle;

#[cfg(any(feature = "tilt", feature = "ahrs"))]
pub mod ahrs;
pub mod command;
pub mod config;
mod config_json;
pub mod config_manager;
pub mod feature;
pub mod graphics;
pub mod http_server;
pub mod listener;
/// Answers mDNS queries for the device's .local name, on any platform with std's UDP sockets.
pub mod mdns;
pub mod platform;
pub mod problem;
pub mod task;
pub mod tz;

pub type Layout = fn(&Rectangle) -> Rectangle;

#[derive(Debug, Clone, Copy)]
pub enum DisplayOrientation {
    Rotate0,
    Rotate90,
    Rotate180,
    Rotate270,
}

pub enum InitStatus {
    Starting,
    AwaitingClientIpAddress,
    AwaitingTimeSync,
    StartingFeatures,
}
pub enum Status {
    Initializing(InitStatus),
    Running,
    Setup,
    Error,
}

struct Shared<T> {
    config: T,
    updated: bool,
}

pub struct WaitNotify<T: Clone> {
    shared: std::sync::Mutex<Shared<T>>,
    notify: std::sync::Condvar,
}

impl<T: Clone> WaitNotify<T> {
    pub fn new(config: T) -> Self {
        Self {
            shared: Mutex::new(Shared { config, updated: false }),
            notify: Condvar::new(),
        }
    }

    pub fn update(&self, new_config: T) {
        let mut shared = self.shared.lock().unwrap();
        shared.config = new_config;
        shared.updated = true;
        self.notify.notify_all();
    }

    pub fn update_if(&self, new_config: T, condition: impl Fn(&T) -> bool) {
        let mut shared = self.shared.lock().unwrap();
        if condition(&shared.config) {
            shared.config = new_config;
            shared.updated = true;
            self.notify.notify_all();
        }
    }

    pub fn wait(&self) -> T {
        let mut shared = self.shared.lock().unwrap();
        while !shared.updated {
            shared = self.notify.wait(shared).unwrap();
        }
        shared.updated = false; // Reset the flag
        shared.config.clone()
    }

    pub fn wait_update(&self, timeout: Duration) -> Option<T> {
        let mut shared = self.shared.lock().unwrap();

        (shared, _) = self.notify.wait_timeout(shared, timeout).unwrap();

        if shared.updated {
            shared.updated = false; // Reset the flag
            Some(shared.config.clone())
        } else {
            None
        }
    }
}
