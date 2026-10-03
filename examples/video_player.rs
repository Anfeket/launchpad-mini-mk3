//! Raw RGB24 9x9 30fps video player for Novation Launchpad Mini MK3.
//!
//! Run with:
//! ```sh
//! cargo run --example video_player -- path/to/video.raw
//! ```
//!
//! You can convert any video using ffmpeg:
//! ```sh
//! ffmpeg -i input.mp4 -vf "scale=9:9:flags=area,fps=30" -c:v rawvideo -pix_fmt rgb24 output.raw
//! ```

use std::{
    env,
    fs,
    io,
    thread,
    time::{Duration, Instant},
};

use launchpad::{Launchpad, RGB};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();
    let video_path = if args.len() > 1 {
        args[1].clone()
    } else {
        eprintln!("Usage: cargo run --example video_player -- <path_to_9x9_rgb24.raw>");
        return Err(io::Error::new(io::ErrorKind::NotFound, "No video file specified").into());
    };

    println!("Loading raw video from: {video_path}");

    let mut launchpad = Launchpad::new().expect("Failed to initialize Launchpad");
    println!("Connected to Launchpad: {}", launchpad.name());

    launchpad.programmer_mode()?;
    launchpad.clear()?;

    let data = fs::read(&video_path)?;
    let frame_bytes = 81 * 3;
    let total_frames = data.len() / frame_bytes;
    println!("Loaded {total_frames} frames ({:.2} s at 30 fps)", total_frames as f64 / 30.0);

    let dt = Duration::from_secs_f64(1.0 / 30.0); // 30 FPS
    let start = Instant::now();

    for (i, chunk) in data.chunks_exact(frame_bytes).enumerate() {
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
        for (f, pixel) in frame.iter_mut().zip(chunk.as_chunks::<3>().0) {
            *f = RGB::from_8bit(pixel[0], pixel[1], pixel[2]);
        }
        launchpad.set_frame(&frame)?;
    }

    launchpad.clear()?;
    launchpad.standalone_mode()?;
    println!("Playback complete.");
    Ok(())
}
