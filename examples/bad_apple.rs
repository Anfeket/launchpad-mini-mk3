//! Plays Bad Apple (or any raw 9x9 30fps grayscale video) on the Novation Launchpad Mini MK3.
//!
//! Frame format: 81 bytes per frame (`-pix_fmt gray`, 1 byte per pad, 0..=255).
//!
//! Run with:
//! ```sh
//! cargo run --example bad_apple
//! ```
//! or specify a custom raw file:
//! ```sh
//! cargo run --example bad_apple -- path/to/video9x9.raw
//! ```
//!
//! To generate a compatible video with FFmpeg:
//! ```sh
//! ffmpeg -i badapple.mp4 -vf "scale=9:9:flags=area,fps=30" -c:v rawvideo -pix_fmt gray badapple9x9.raw
//! ```

use std::{
    env,
    fs,
    io,
    path::Path,
    thread,
    time::{Duration, Instant},
};

use launchpad::{Launchpad, RGB};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();
    let video_path = if args.len() > 1 {
        args[1].clone()
    } else if Path::new("examples/badapple9x9.raw").exists() {
        "examples/badapple9x9.raw".to_string()
    } else if Path::new("badapple9x9.raw").exists() {
        "badapple9x9.raw".to_string()
    } else {
        eprintln!("Usage: cargo run --example bad_apple -- <path_to_9x9_gray.raw>");
        return Err(io::Error::new(io::ErrorKind::NotFound, "No video file specified").into());
    };

    println!("Loading raw grayscale video from: {video_path}");

    let mut launchpad = Launchpad::new().expect("Failed to initialize Launchpad");
    println!("Connected to Launchpad: {}", launchpad.name());

    launchpad.programmer_mode()?;
    launchpad.clear()?;

    let data = fs::read(&video_path)?;
    const FRAME_BYTES: usize = 81;
    let total_frames = data.len() / FRAME_BYTES;
    println!(
        "Loaded {total_frames} frames ({:.2} s at 30 fps)",
        total_frames as f64 / 30.0
    );

    let dt = Duration::from_secs_f64(1.0 / 30.0); // 30 FPS
    let start = Instant::now();

    for (i, chunk) in data.as_chunks::<81>().0.iter().enumerate() {
        let target = start + dt * (i as u32);
        let now = Instant::now();
        if now > target + dt {
            eprintln!(
                "Skipping frame {} ({} ms late)",
                i,
                (now - target).as_millis()
            );
            continue;
        }
        thread::sleep(target.saturating_duration_since(now));

        let mut frame = [RGB::BLACK; 81];
        for (f, &pixel) in frame.iter_mut().zip(chunk.iter()) {
            // Convert 8-bit grayscale (0..=255) to 7-bit RGB
            *f = RGB::from_8bit(pixel, pixel, pixel);
        }
        launchpad.set_frame(&frame)?;
    }

    launchpad.clear()?;
    launchpad.standalone_mode()?;
    println!("Playback complete.");
    Ok(())
}
