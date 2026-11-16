# The Board Simulator

`bruce-embedded-sim` runs firmware written for Bruce's embedded platform on a desktop computer (macOS, Windows or
Linux). It shows the board's display in a window, framed by a photo of the board. Its first use is the *virtual capture
device* of Bruce's Smart Home, for people without an ESP32. It's general, so that any board example can be tried on a
computer, and documentation screenshots can be made from it.

It **simulates**: it doesn't **emulate**. It doesn't imitate the chip, its RAM, or each board's hardware limits. The
display, buttons, touch, speaker and network behave like the real ones, so that the user gets a feel for the real
device.

Agreed with Bruce on 16 November 2026.

## Where things live

- **`bruce-embedded-sim`**, a crate in this workspace, released with `bruce-embedded-std`. It has:
  - the window, drawn with winit and softbuffer, with the board's frame, its display and its buttons;
  - display types which behave as each board's display does, implementing `DisplayManager`;
  - input: mouse clicks on the board's buttons, and touch and swipes as mouse drags on the screen;
  - settings and storage in files, in a folder per simulated device;
  - screenshots of the framed board as PNG.
- **The HTTP code stays in `bruce-esp-idf`, and is compiled for the computer as it is.** That covers the device API,
  adoption, signed requests and the `/config` page. A hardware-agnostic HTTP abstraction has failed twice, see the
  note in `http_server.rs`, so these aren't moved here behind new traits. ESP-IDF's request type is `embedded-svc`'s
  generic `Request<C>`, so `bruce-embedded-sim` provides an HTTP server and client for the computer which implement
  `embedded-svc`'s `Connection` traits. Its server takes handlers in the same shape as `EspHttpServer::fn_handler`, so
  the same source compiles on both, without trait objects or copies. Only what is truly ESP-IDF's gets a twin for the
  computer: NVS, Wi-Fi, the heap's statistics, and making the certificate. (Bruce, 16 November 2026, which replaced
  an earlier plan to move this code into `bruce-embedded-std`.)
- **`bruce-esp-idf`, built for a desktop target** (not `target_os = "espidf"`), provides the same API as on an ESP32,
  `Esp32Platform`, its builder, `Remainder` and the board's capabilities, backed by `bruce-embedded-sim`. So board
  examples and firmware build for the computer with few or no changes. Each still builds with a single `board-*`
  feature, as on the ESP32.

## Boards

These are the first two. More come by adding a `Board` description.

| `--board` | Board | Display | Inputs |
|---|---|---|---|
| `rlcd42` | Waveshare ESP32-S3-RLCD-4.2 (`wave-esp32s3-rlcd-42`) | 398 x 298, reflective, black and white | KEY, BOOT |
| `guitron` | Guition JC3248W535C (`guition-jc3248w535c`) | 480 x 320, colour | BOOT, touch |

- **The frame** is a photo of the real board, taken by Bruce, square on, with the screen's rectangle marked. Photos
  from the makers' web pages aren't used, as they are the makers' copyright. Until a photo exists, a frame is drawn:
  the board's outline, its screen and its labelled buttons.
- **The board's extras:**
  - Sounds play on the computer.
  - Sensors which aren't there read nothing: temperature, humidity and battery show as unavailable, and are never
    made up.

## Running

- **One window per simulated device.** Closing it stops the device. Minimise it to keep it running. A tray-icon
  version, which keeps running without its window, is on the backlog.
- **Storage:** each simulated device keeps what a real one keeps in NVS and on its SD card in a folder under the
  user's data directory, e.g. `~/Library/Application Support` on macOS or `%APPDATA%` on Windows. `--data` puts it
  elsewhere.
- **Network:**
  - Each simulated device has its own port, on the computer's address, chosen on first run and kept, for HTTP and
    HTTPS alike, as the simulator's servers share a port and tell a TLS connection by its first byte. So its address,
    host and port, e.g. `192.168.1.190:60496`, reaches it as a real device's address does on ports 80 and 443, and an
    app needs nothing new to use it. Its screen shows that address.
  - It advertises itself by mDNS, as a real device does, sharing port 5353 with the computer's own responder.
- **No Wi-Fi setup portal,** as the computer is already on the network. The device starts in normal mode, and its
  `/config` page opens from a link in the window.
- **Screenshots:**
  - `--screenshot FILE` saves the framed board as a PNG once the device has run for `--after` seconds, e.g. for the
    docs.
  - For Bruce's Smart Home, `--replay` feeds the screens from a profile's store, e.g. a redacted offline one, rather
    than polling, so that the screenshots are repeatable and safe to publish.

## The capture firmware

- **The screens:** two layouts, chosen by what the display can show. A black and white display gets the clock and
  lines of numbers, as on the RLCD. A colour display gets the graph, as on the Guitron. Layouts the user chooses are
  for later.
- **Builds:** the firmware is built for the computer once per board, as for the ESP32. A small launcher,
  `bruce-smart-home-capture-sim`, takes `--board` and starts the right build, so the user sees one command. The app's
  installers include them all, and its Devices page gets **Add a virtual device**.

## Steps

1. **`bruce-embedded-sim`:** the window, the frames, the displays, input and screenshots, with an example which runs
   the analog clock on both boards.
2. **`embedded-svc` for the computer:** the simulator's HTTP server, over HTTP and HTTPS, and its HTTP client.
3. **`bruce-esp-idf` on the desktop:** the platform's API over the simulator, so that the board examples run, with
   twins of the ESP-IDF-only pieces.
4. **The capture firmware on the desktop:**
   - its HTTP client and server through `embedded-svc`'s traits;
   - its store in the device's folder;
   - the launcher.
5. **Bruce's Smart Home:**
   - devices with a host and port;
   - **Add a virtual device**;
   - `--replay`;
   - the installers.

## Libraries

Their licences are checked with `cargo metadata` before they're added, per Bruce's rules:
- `winit`: Apache-2.0;
- `softbuffer`: MIT OR Apache-2.0;
- `png`: MIT OR Apache-2.0;
- `rustls` with `ring`: already used by Bruce's Smart Home.

Any licence other than MIT or Apache-2.0 among the libraries they pull in is listed for Bruce.
