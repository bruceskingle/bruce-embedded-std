# ChangeLog

# Release 0.3.0

## New

- The mDNS responder advertises the device as services, `MdnsService`, each with its TXT record, `MdnsTxt`, whose
  facts the firmware can change while it runs: `_bruce-device._tcp`, `SERVICE_TYPE`, by which an app or a flasher
  finds the boards on the network and what each is, and `_http._tcp`, `HTTP_SERVICE_TYPE`, for its web page.
- The IMU calibrates itself in its own thread when started, rather than holding the board up for a few seconds;
  `is_calibrated` says when its readings mean something.

## Fixed

- The mDNS responder answers the asker directly as well as by multicast, so that a computer on another network, e.g.
  another VLAN, which a router passes the query to, gets the answer.
- The mDNS responder's thread has a stack large enough for several services, rather than overflowing and restarting
  the board.
- A `bool` setting never set, e.g. on a new device, is read as false, or None if optional, rather than failing with
  "Invalid type".

# Release 0.2.0

## New

- **bruce-embedded-sim**, a board simulator: a platform's firmware, built for a computer rather than an ESP32, runs in
  a window showing the board's display, framed by a picture of the board, with its buttons clicked and its touch screen
  dragged with the mouse. It simulates rather than emulates: the firmware's own code runs, with the ESP-IDF services it
  uses, HTTP and HTTPS servers, an HTTP client and NVS, given by the `svc` feature under the same names, so the same
  files build for both. It simulates the Waveshare ESP32-S3-RLCD-4.2 and the Guition JC3248W535C, offers a window to
  choose one if none is given, remembers each window's place, copies what the screen shows, and takes screenshots, in a
  window or without one. See docs/Simulator.md.

## Improved

- `thread_stack`, the stack size a thread needs: what it is given on an ESP32, and at least 256 KB elsewhere, e.g. in
  the simulator, where a debug build takes more stack.
- The mDNS responder shares its port with other programs on a computer, e.g. several simulated devices, as it is only
  ever alone on an ESP32.

# Release 0.1.0

## New

- *Features*, each with its settings, `#[derive(FeatureConfig)]`, and a config page to change them, including sliders
  which preview their effect, and secrets which the page never shows.
- The traits a platform implements for its hardware, e.g. `Platform`, `ConfigStore`, `HttpServerManager` and
  `DisplayManager`, so that a platform such as bruce-esp-idf supplies only what is specific to it.
- A task scheduler for Cron-like schedules, time zones by their POSIX TZ strings, an mDNS responder, problems to show
  on the config page, graphics such as clocks and colours, and attitude and tilt from an IMU's readings, the `ahrs`
  and `tilt` features.
- It needs no hardware, so it builds, and its tests run, on a computer.
