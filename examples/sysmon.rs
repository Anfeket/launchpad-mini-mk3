//! System resource monitor for Linux.
//!
//! Renders CPU load, system RAM usage and network transmit rate on Launchpad Mini MK3.
//!
//! Run with:
//! ```sh
//! cargo run --example sysmon
//! ```

use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use launchpad_mini_mk3::{Launchpad, RGB};

const COLOR_LAPS: [RGB; 10] = [
    RGB::new(0, 0, 127),
    RGB::new(0, 110, 127),
    RGB::new(0, 127, 0),
    RGB::new(80, 127, 0),
    RGB::new(127, 110, 0),
    RGB::new(127, 55, 0),
    RGB::new(127, 0, 0),
    RGB::new(127, 0, 90),
    RGB::new(70, 0, 127),
    RGB::new(127, 127, 127),
];

const NET_MAX: u64 = 1e9 as u64 / 8 as u64; // 1 GB
const NET_GAMMA: f32 = 0.6;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut launchpad = Launchpad::new().expect("Failed to initialize Launchpad");

    println!("Connected to Launchpad: {}", launchpad.name());
    println!("Running system monitor. Press Enter to exit...");

    launchpad.programmer_mode()?;
    launchpad.clear()?;

    let mut cpus = Vec::new();
    for line in std::fs::read_to_string("/proc/stat")?.lines() {
        if line.starts_with("cpu") && line.chars().nth(3).unwrap_or(' ').is_ascii_digit() {
            cpus.push((0, 0));
        }
    }

    println!("Detected {} CPU cores", cpus.len());

    let mut stat = File::open("/proc/stat")?;
    let mut meminfo = File::open("/proc/meminfo")?;

    let mut stat_buf = String::with_capacity(4096);
    let mut meminfo_buf = String::with_capacity(2048);

    let mut prev_cpus = cpus.clone();
    let mut prev_mem_usage = read_mem_usage(&mut meminfo, &mut meminfo_buf)?;

    let net_interface = "eno1";
    let mut net_rx_bytes = File::open(format!(
        "/sys/class/net/{}/statistics/rx_bytes",
        net_interface
    ))?;
    let mut net_tx_bytes = File::open(format!(
        "/sys/class/net/{}/statistics/tx_bytes",
        net_interface
    ))?;
    let mut net_buf = String::with_capacity(64);
    let mut prev_rx_bytes = read_net_bytes(&mut net_rx_bytes, &mut net_buf)?;
    let mut prev_tx_bytes = read_net_bytes(&mut net_tx_bytes, &mut net_buf)?;

    let mut last_rx_level = 0.0;
    let mut last_tx_level = 0.0;

    let mut last_instant = Instant::now();

    let quit = Arc::new(AtomicBool::new(false));
    let quit_clone = quit.clone();
    thread::spawn(move || {
        let mut input = String::new();
        let _ = std::io::stdin().read_line(&mut input);
        quit_clone.store(true, Ordering::Relaxed);
    });

    loop {
        read_cpu_usage(&mut stat, &mut stat_buf, &mut cpus)?;

        let mut frame = [RGB::BLACK; 81];

        for (i, ((total, idle), (prev_total, prev_idle))) in
            cpus.iter().zip(prev_cpus.iter()).enumerate()
        {
            let total_diff = total.saturating_sub(*prev_total);
            let idle_diff = idle.saturating_sub(*prev_idle);
            let usage = if total_diff > 0 {
                1.0 - (idle_diff as f64 / total_diff as f64)
            } else {
                0.0
            };

            let color = if usage < 0.5 {
                RGB::new((usage * 127.0) as u8, 0, 127)
            } else {
                RGB::new(127, 0, ((1.0 - usage) * 127.0) as u8)
            };

            let pos = 9 + (i % 8) + (i / 8) * 9;
            if pos < frame.len() {
                frame[pos] = color;
            }
        }

        prev_cpus.copy_from_slice(&cpus);

        let (total_mem, free_mem) = read_mem_usage(&mut meminfo, &mut meminfo_buf)?;

        let mem_usage = if total_mem > 0 {
            1.0 - (free_mem as f64 / total_mem as f64)
        } else {
            0.0
        };

        let n = COLOR_LAPS.len();
        let lap = ((mem_usage * n as f64).floor() as usize).min(n - 1);
        let filled = (mem_usage % (1.0 / n as f64) * 80.0).floor() as usize;
        for i in 0..8 {
            let pos = 27 + i;
            if i <= filled {
                frame[pos] = COLOR_LAPS[lap + 1];
            } else {
                frame[pos] = COLOR_LAPS[lap];
            }
        }

        let mem_change = (prev_mem_usage.1 as i64) - (free_mem as i64);
        if mem_change > 0 {
            frame[35] = RGB::RED;
        } else if mem_change < 0 {
            frame[35] = RGB::GREEN;
        } else {
            frame[35] = RGB::BLUE;
        }

        prev_mem_usage = (total_mem, free_mem);

        let rx_bytes = read_net_bytes(&mut net_rx_bytes, &mut net_buf)?;
        let tx_bytes = read_net_bytes(&mut net_tx_bytes, &mut net_buf)?;

        let dt = last_instant.elapsed().as_secs_f32();
        last_instant = Instant::now();

        let rx_rate = rx_bytes.saturating_sub(prev_rx_bytes) as f32 / dt;
        let tx_rate = tx_bytes.saturating_sub(prev_tx_bytes) as f32 / dt;

        let mut rx_level = (rx_rate / NET_MAX as f32).powf(NET_GAMMA) * 8.0;
        let mut tx_level = (tx_rate / NET_MAX as f32).powf(NET_GAMMA) * 8.0;

        if last_rx_level < rx_level {
            rx_level *= 0.95;
        };
        if last_tx_level < tx_level {
            tx_level *= 0.95;
        };

        last_rx_level = rx_level;
        last_tx_level = tx_level;

        for i in 0..8 {
            let pos = 36 + i;
            if i == rx_level.floor() as usize {
                let frac = rx_level - rx_level.floor();
                let color = (182.0 * frac, 146.0 * frac, 123.0 * frac);
                frame[pos] = RGB::from_8bit(color.0 as u8, color.1 as u8, color.2 as u8);
            } else if (i as f32) < rx_level {
                frame[pos] = RGB::from_8bit(182, 146, 123);
            } else {
                frame[pos] = RGB::BLACK;
            }
        }

        for i in 0..8 {
            let pos = 45 + i;
            if i == tx_level.floor() as usize {
                let frac = tx_level - tx_level.floor();
                let color = (123.0 * frac, 182.0 * frac, 146.0 * frac);
                frame[pos] = RGB::from_8bit(color.0 as u8, color.1 as u8, color.2 as u8);
            } else if (i as f32) < tx_level {
                frame[pos] = RGB::from_8bit(123, 182, 146);
            } else {
                frame[pos] = RGB::BLACK;
            }
        }

        prev_rx_bytes = rx_bytes;
        prev_tx_bytes = tx_bytes;

        launchpad.set_frame(&frame)?;
        launchpad.flush();

        thread::sleep(Duration::from_millis(50).saturating_sub(last_instant.elapsed()));

        if quit.load(Ordering::Relaxed) {
            break;
        }
    }

    launchpad.clear()?;
    launchpad.standalone_mode()?;
    launchpad.flush();
    Ok(())
}

fn read_cpu_usage(
    stat: &mut File,
    buf: &mut String,
    cpus: &mut [(u64, u64)],
) -> Result<(), Box<dyn std::error::Error>> {
    buf.clear();
    stat.seek(SeekFrom::Start(0))?;
    stat.read_to_string(buf)?;

    for (i, line) in buf.lines().enumerate() {
        if i >= cpus.len() {
            break;
        }
        let parts = line.split_whitespace().collect::<Vec<_>>();
        let total = parts[1..]
            .iter()
            .map(|v| v.parse::<u64>().unwrap_or(0))
            .sum::<u64>();
        let idle = parts[4..=5]
            .iter()
            .map(|v| v.parse::<u64>().unwrap_or(0))
            .sum::<u64>();
        cpus[i] = (total, idle);
    }

    Ok(())
}

fn read_mem_usage(
    stat: &mut File,
    buf: &mut String,
) -> Result<(u64, u64), Box<dyn std::error::Error>> {
    buf.clear();
    stat.seek(SeekFrom::Start(0))?;
    stat.read_to_string(buf)?;

    let mut total = 0;
    let mut free = 0;

    for line in buf.lines() {
        if line.starts_with("MemTotal:") {
            total = line
                .split_whitespace()
                .nth(1)
                .unwrap_or("0")
                .parse::<u64>()
                .unwrap_or(0);
        } else if line.starts_with("MemAvailable:") {
            free = line
                .split_whitespace()
                .nth(1)
                .unwrap_or("0")
                .parse::<u64>()
                .unwrap_or(0);
        }
    }

    Ok((total, free))
}

fn read_net_bytes(stat: &mut File, buf: &mut String) -> Result<u64, Box<dyn std::error::Error>> {
    buf.clear();
    stat.seek(SeekFrom::Start(0))?;
    stat.read_to_string(buf)?;
    Ok(buf.trim().parse::<u64>().unwrap_or(0))
}
