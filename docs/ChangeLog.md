# ChangeLog

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
