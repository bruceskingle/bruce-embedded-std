//! The few functions of ESP-IDF's C API, esp_idf_sys's, which firmware calls directly, for the simulator, with the same
//! names, so that the calls compile as they are. They are unsafe, as the real ones are, so that callers' unsafe blocks
//! are needed on both. The computer's memory is not the device's, so the heap's figures are none, 0, rather than made
//! up, as the simulator never reports what it cannot measure.

use std::sync::OnceLock;
use std::time::Instant;

pub use super::client::esp_crt_bundle_attach;

/// The capabilities of memory heap_caps_get_largest_free_block is asked about, which the simulator ignores.
pub const MALLOC_CAP_DEFAULT: u32 = 1 << 12;
pub const MALLOC_CAP_INTERNAL: u32 = 1 << 11;
pub const MALLOC_CAP_SPIRAM: u32 = 1 << 10;
pub const MALLOC_CAP_DMA: u32 = 1 << 3;

/// When the simulated device started, for esp_timer_get_time.
fn started() -> Instant {
    static STARTED: OnceLock<Instant> = OnceLock::new();
    *STARTED.get_or_init(Instant::now)
}

/// Starts the device's clock, when it starts, see esp_timer_get_time.
pub fn start_timer() {
    started();
}

/// # Safety
///
/// Safe: it only reads the simulator's clock.
pub unsafe fn esp_timer_get_time() -> i64 {
    started().elapsed().as_micros() as i64
}

/// # Safety
///
/// Safe: there is no heap to measure.
pub unsafe fn esp_get_free_heap_size() -> u32 {
    0
}

/// # Safety
///
/// Safe: there is no heap to measure.
pub unsafe fn esp_get_minimum_free_heap_size() -> u32 {
    0
}

/// # Safety
///
/// Safe: there is no heap to measure.
pub unsafe fn heap_caps_get_largest_free_block(_caps: u32) -> usize {
    0
}

/// Fills len bytes at buf with random bytes, as ESP-IDF's, from the system's random source.
///
/// # Safety
///
/// buf must be valid for len bytes of writes.
pub unsafe fn esp_fill_random(buf: *mut core::ffi::c_void, len: usize) {
    let bytes = unsafe { std::slice::from_raw_parts_mut(buf as *mut u8, len) };
    if getrandom::fill(bytes).is_err() {
        // Never expected; the device's would not fail either, so the bytes are filled from the clock rather than left.
        let seed = started().elapsed().as_nanos() as u64 ^ std::process::id() as u64;
        for (index, byte) in bytes.iter_mut().enumerate() {
            *byte = (seed.rotate_left(index as u32 % 64) >> 7) as u8;
        }
    }
}

/// Restarts the simulated device, as ESP-IDF's does the chip: the program starts itself again, with the same
/// arguments, and ends.
///
/// # Safety
///
/// Safe, but it does not return.
pub unsafe fn esp_restart() -> ! {
    log::info!("Restarting");
    if let Ok(program) = std::env::current_exe() {
        let _ = std::process::Command::new(program)
            .args(std::env::args_os().skip(1))
            .spawn();
    }
    std::process::exit(0)
}

/// Writes the program's identity, as ESP-IDF's writes the hex of the firmware image's SHA-256, into buffer, with a
/// nul, returning its length: the simulator's is the hash of the program's file, or nothing if it cannot be read.
///
/// # Safety
///
/// buffer must be valid for len bytes of writes.
pub unsafe fn esp_app_get_elf_sha256(buffer: *mut core::ffi::c_char, len: usize) -> usize {
    static HASH: OnceLock<String> = OnceLock::new();
    let hash = HASH.get_or_init(|| {
        std::env::current_exe()
            .and_then(std::fs::read)
            .map(|bytes| {
                // FNV-1a, enough to tell builds apart, which is all the device's is used for.
                let mut hash: u64 = 0xcbf29ce484222325;
                for byte in bytes {
                    hash = (hash ^ byte as u64).wrapping_mul(0x100000001b3);
                }
                format!("{hash:016x}")
            })
            .unwrap_or_default()
    });
    if len == 0 {
        return 0;
    }
    let bytes = hash.as_bytes();
    let count = bytes.len().min(len - 1);
    let out = unsafe { std::slice::from_raw_parts_mut(buffer as *mut u8, len) };
    out[..count].copy_from_slice(&bytes[..count]);
    out[count] = 0;
    count
}

/// ESP-IDF's error, as the simulator's.
pub type EspError = super::SimError;

/// ESP-IDF's result of success.
pub const ESP_OK: i32 = 0;

/// Sets an environment variable of the simulated device's program, e.g. TZ, its time zone, as ESP-IDF's sets one of
/// the device's, the name and value being nul terminated.
///
/// # Safety
///
/// name and value must be nul terminated strings, and no other thread may be reading the environment.
pub unsafe fn setenv(name: *const u8, value: *const core::ffi::c_char, _overwrite: i32) -> i32 {
    let name = unsafe { std::ffi::CStr::from_ptr(name as *const core::ffi::c_char) }.to_string_lossy();
    let value = unsafe { std::ffi::CStr::from_ptr(value) }.to_string_lossy();
    unsafe { std::env::set_var(name.as_ref(), value.as_ref()) };
    0
}

/// Makes a change to TZ take effect, which the computer's time functions see for themselves.
///
/// # Safety
///
/// Safe: it does nothing.
pub unsafe fn tzset() {}
