//! Reads and prints incoming MIDI messages from the Launchpad Mini MK3.
//!
//! Run with:
//! ```sh
//! cargo run --example read_midi
//! ```

use std::time::Duration;
use launchpad_mini_mk3::{Button, Launchpad, RGB};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Connecting to Launchpad Mini MK3...");
    let mut launchpad = Launchpad::new()?;
    println!("Output port: {}", launchpad.name());
    println!("Input port:  {}", launchpad.input_name());

    println!("Switching to Programmer Mode...");
    launchpad.programmer_mode()?;

    // Read the mode-switch acknowledgement message sent by the device
    if let Some(ack) = launchpad.read_message_timeout(Duration::from_millis(200)) {
        println!("Received SysEx ACK: {:02X?}", &ack[..]);
    }

    // Flush any old buffered events before starting
    launchpad.flush();
    println!("Buffer flushed. Press buttons/pads on the Launchpad (Ctrl+C to exit)...");

    // Light up a few pads to show it is active
    launchpad.set_led_rgb(Button::Pad { x: 0, y: 7 }, RGB::new(0, 127, 0))?;
    launchpad.set_led_rgb(Button::Pad { x: 7, y: 7 }, RGB::new(127, 0, 0))?;

    loop {
        // Blocks until a message is received
        if let Some(msg) = launchpad.read_message_timeout(Duration::from_millis(500)) {
            let hex = msg
                .iter()
                .map(|b| format!("{b:02X}"))
                .collect::<Vec<_>>()
                .join(" ");
            println!("[+{} µs] ({:3} bytes): {}", msg.timestamp, msg.len(), hex);

            // If it's a note on/off or CC, we can see the button index
            if msg.len() >= 3 {
                match msg[0] & 0xF0 {
                    0x90 => {
                        let note = msg[1];
                        let velocity = msg[2];
                        if let Some(btn) = Button::from_index(note) {
                            println!("  -> Pad/Side: {btn:?}, velocity: {velocity}");
                        }
                    }
                    0xB0 => {
                        let cc = msg[1];
                        let value = msg[2];
                        if let Some(btn) = Button::from_index(cc) {
                            println!("  -> Top/Control: {btn:?}, value: {value}");
                        }
                    }
                    _ => {}
                }
            }
        }
    }
}
