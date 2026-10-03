# launchpad-mini-mk3

A lightweight Rust library for controlling the **Novation Launchpad Mini [MK3]** over MIDI SysEx messages.

---

## Features

- **Hardware Auto-Detection**: Automatically discovers and connects to the Launchpad Mini MK3 MIDI port using `midir`.
- **Mode Switching**: Cleanly switch between **Programmer Mode** and **Standalone (Live) Mode**.
- **Full-Grid RGB Rendering**: Send an entire 81-LED (9x9) RGB frame in a single low-latency SysEx message (`set_frame`).
- **Flexible LED Addressing**:
  - `Button::Pad { x, y }` (0..=7, 0..=7)
  - `Button::Top(i)` (0..=7)
  - `Button::Side(i)` (0..=7)
  - `Button::Logo`
- **RGB & Palette Support**: Control individual or batched LEDs using 7-bit RGB (`0..=127` per channel) or the built-in 128-color palette.
- **Text Scrolling**: Native SysEx ASCII text scrolling across pads.

---

## Installation

Add this to your `Cargo.toml`:

```toml
[dependencies]
launchpad-mini-mk3 = "0.1.0"
```

Or directly from GitHub:

```toml
[dependencies]
launchpad-mini-mk3 = { git = "https://github.com/anfeket/launchpad-mini-mk3" }
```

---

## Quick Start

```rust
use launchpad_mini_mk3::{Launchpad, Button, RGB};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Connect to attached Launchpad
    let mut launchpad = Launchpad::new()?;
    println!("Connected to: {}", launchpad.name());

    // Switch to Programmer Mode
    launchpad.programmer_mode()?;
    launchpad.clear()?;

    // Light up the top-left pad green
    launchpad.set_led_rgb(Button::Pad { x: 0, y: 7 }, RGB::new(0, 127, 0))?;

    // Reset and return to standalone mode
    launchpad.clear()?;
    launchpad.standalone_mode()?;
    Ok(())
}
```

---

## Examples

### 1. System Resource Monitor (`sysmon`)

Monitors your Linux system's per-core CPU usage and memory consumption in real time on the 9x9 LED grid.

```sh
cargo run --example sysmon
```

- **Top rows**: Per-core CPU load heatmap (blue -> purple -> red).
- **Row 3**: RAM consumption gauge with color laps.
- **Side LED**: Memory pressure delta indicator (green = freed, red = allocated).
- Press **Enter** in the terminal to cleanly restore the Launchpad and exit.

### 3. Full RGB24 Video Player (`video_player`)

Plays full-color 30 FPS RGB24 video (243 bytes per frame, 3 bytes per pad) on the Launchpad grid:

```sh
cargo run --example video_player -- path/to/video.raw
```

#### Converting RGB Videos with FFmpeg
```sh
ffmpeg -i input.mp4 -vf "scale=9:9:flags=area,fps=30" -c:v rawvideo -pix_fmt rgb24 output.raw
```

### 2. Bad Apple (`bad_apple`)

Obligatory Bad Apple player, plays 30 FPS grayscale video on the grid:

```sh
cargo run --example bad_apple
```

#### Converting Grayscale Videos with FFmpeg
```sh
ffmpeg -i badapple.mp4 -vf "scale=9:9:flags=area,fps=30" -c:v rawvideo -pix_fmt gray badapple9x9.raw
```

---

## Requirements & Linux Permissions

This crate relies on [`midir`](https://crates.io/crates/midir) (using ALSA on Linux). Ensure your user belongs to the `audio` group or has access to the ALSA / MIDI devices:

```sh
# Verify Launchpad is visible to ALSA sequencer
aconnect -l
```

---

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.
