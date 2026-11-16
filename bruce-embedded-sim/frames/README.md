# Frames

The boards' frames, which the simulator draws around their screens, see `src/board.rs` and `src/frame.rs`: images of
the real boards by Bruce Skingle, licensed as this crate is, MIT OR Apache-2.0, not the makers' images, which are
theirs. Each is a PNG with a transparent rectangle where the board's screen is, at one of the screen's pixels to each
of the image's, e.g. `rlcd42.png`, 438 x 324, whose hole is the RLCD's whole panel, 400 x 300 at (9, 12), in which the screen,
398 x 298 as the bezel hides a pixel all round, is put at (10, 13).
