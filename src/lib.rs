//! Lightweight Rust library for controlling the Novation Launchpad Mini MK3
//! over MIDI SysEx messages.
//!
//! # Features
//! - Whole-grid (9x9) RGB frame rendering
//! - LED control with Palette colors or 24-bit (7-bit per channel) RGB
//! - Scrolling text display
//! - Real-time MIDI input reading and buffer flushing (prevents device freezes)
//!
//! # Example
//! ```no_run
//! use launchpad_mini_mk3::{Launchpad, Button, RGB};
//!
//! fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let mut launchpad = Launchpad::new()?;
//!     launchpad.programmer_mode()?;
//!
//!     // Light up the top-left pad with green
//!     launchpad.set_led_rgb(Button::Pad { x: 0, y: 7 }, RGB::new(0, 127, 0))?;
//!
//!     launchpad.clear()?;
//!     launchpad.standalone_mode()?;
//!     Ok(())
//! }
//! ```

use std::{
    collections::VecDeque,
    fmt,
    sync::{Arc, Condvar, Mutex},
    time::{Duration, Instant},
};

#[derive(Debug)]
pub enum Error {
    /// Failed to initialize the host MIDI subsystem (e.g. ALSA).
    MidiInit(midir::InitError),
    /// Failed to query port metadata.
    PortInfo(midir::PortInfoError),
    /// No connected Launchpad Mini MK3 was detected.
    DeviceNotFound,
    /// Failed to open a connection to the Launchpad MIDI output port.
    Connection(midir::ConnectError<midir::MidiOutput>),
    /// Failed to open a connection to the Launchpad MIDI input port.
    InputConnection(midir::ConnectError<midir::MidiInput>),
    /// Failed to send a MIDI message to the device (e.g. disconnected).
    Send(midir::SendError),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::MidiInit(e) => write!(f, "failed to initialize MIDI subsystem: {e}"),
            Error::PortInfo(e) => write!(f, "failed to read MIDI port metadata: {e}"),
            Error::DeviceNotFound => write!(
                f,
                "no Launchpad Mini MK3 found (check USB connection and permissions)"
            ),
            Error::Connection(e) => write!(f, "failed to connect to Launchpad output: {e}"),
            Error::InputConnection(e) => write!(f, "failed to connect to Launchpad input: {e}"),
            Error::Send(e) => write!(f, "failed to send MIDI message to Launchpad: {e}"),
        }
    }
}
impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::MidiInit(e) => Some(e),
            Error::PortInfo(e) => Some(e),
            Error::Connection(e) => Some(e),
            Error::InputConnection(e) => Some(e),
            Error::Send(e) => Some(e),
            Error::DeviceNotFound => None,
        }
    }
}

impl From<midir::InitError> for Error {
    fn from(e: midir::InitError) -> Self {
        Error::MidiInit(e)
    }
}

impl From<midir::PortInfoError> for Error {
    fn from(e: midir::PortInfoError) -> Self {
        Error::PortInfo(e)
    }
}

impl From<midir::ConnectError<midir::MidiOutput>> for Error {
    fn from(e: midir::ConnectError<midir::MidiOutput>) -> Self {
        Error::Connection(e)
    }
}

impl From<midir::ConnectError<midir::MidiInput>> for Error {
    fn from(e: midir::ConnectError<midir::MidiInput>) -> Self {
        Error::InputConnection(e)
    }
}

impl From<midir::SendError> for Error {
    fn from(e: midir::SendError) -> Self {
        Error::Send(e)
    }
}

// Convenient type alias:
pub type Result<T> = std::result::Result<T, Error>;

const SYSEX_HEADER: [u8; 6] = [0xF0, 0x00, 0x20, 0x29, 0x02, 0x0D];
const SYSEX_FOOTER: u8 = 0xF7;

mod command {
    pub const MODE_SWITCH: u8 = 0x0E;
    pub const LED_SET: u8 = 0x03;
    pub const TEXT_SCROLL: u8 = 0x07;
}

mod mode {
    pub const PROGRAMMER: u8 = 0x01;
    pub const STANDALONE: u8 = 0x00;
}

#[allow(dead_code)]
mod led_type {
    pub const PALETTE: u8 = 0x00;
    pub const FLASHING: u8 = 0x01;
    pub const PULSING: u8 = 0x02;
    pub const RGB: u8 = 0x03;
}

#[allow(dead_code)]
mod text_type {
    pub const PALETTE: u8 = 0x00;
    pub const RGB: u8 = 0x01;
}

/// Represents a physical button or pad on the Launchpad Mini MK3.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Button {
    /// An 8x8 grid pad at coordinates `(x, y)` where `0 <= x <= 7` and `0 <= y <= 7`.
    /// `(0, 0)` is the bottom-left pad, and `(7, 7)` is the top-right pad.
    Pad { x: u8, y: u8 },
    /// One of the 8 round buttons along the top row (`0 <= i <= 7`, left to right).
    Top(u8),
    /// One of the 8 round scene-launch buttons along the right edge (`0 <= i <= 7`, bottom to top).
    Side(u8),
    /// The Novation logo button in the top-right corner.
    Logo,
}

impl Button {
    /// Returns the MIDI SysEx index code for this button.
    #[must_use]
    pub const fn index(self) -> u8 {
        match self {
            Button::Pad { x, y } => (y + 1) * 10 + (x + 1),
            Button::Top(i) => 91 + i,
            Button::Side(i) => (i + 1) * 10 + 9,
            Button::Logo => 99,
        }
    }

    /// Converts a MIDI SysEx button index into a [`Button`], if valid.
    #[must_use]
    pub const fn from_index(index: u8) -> Option<Self> {
        let (row, col) = (index / 10, index % 10);
        match (row, col) {
            (9, 9) => Some(Button::Logo),
            (9, 1..=8) => Some(Button::Top(col - 1)),
            (1..=8, 9) => Some(Button::Side(row - 1)),
            (1..=8, 1..=8) => Some(Button::Pad {
                x: col - 1,
                y: row - 1,
            }),
            _ => None,
        }
    }
}

/// An index into the Launchpad's built-in 128-color palette (0–127).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Palette(pub u8);

impl Palette {
    pub const OFF: Palette = Palette(0);
    pub const WHITE: Palette = Palette(3);
    pub const RED: Palette = Palette(5);
    pub const GREEN: Palette = Palette(21);
    pub const BLUE: Palette = Palette(45);
}

/// An RGB color specification with 7-bit channels (0–127).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct RGB {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl RGB {
    pub const BLACK: RGB = RGB::new(0, 0, 0);
    pub const WHITE: RGB = RGB::new(127, 127, 127);
    pub const RED: RGB = RGB::new(127, 0, 0);
    pub const GREEN: RGB = RGB::new(0, 127, 0);
    pub const BLUE: RGB = RGB::new(0, 0, 127);

    /// Creates a new RGB color. Channel values are 7-bit (0–127).
    #[must_use]
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        assert!(r <= 127, "Red channel value must be 0–127");
        assert!(g <= 127, "Green channel value must be 0–127");
        assert!(b <= 127, "Blue channel value must be 0–127");
        Self { r, g, b }
    }

    /// Returns `Some(RGB)` if all channels are `<= 127`, or `None` otherwise.
    #[must_use]
    pub const fn try_new(r: u8, g: u8, b: u8) -> Option<Self> {
        if r <= 127 && g <= 127 && b <= 127 {
            Some(Self { r, g, b })
        } else {
            None
        }
    }

    /// Scales 8-bit (0–255) channel values down to 7-bit (0–127) and returns a new RGB color.
    #[must_use]
    pub const fn from_8bit(r: u8, g: u8, b: u8) -> Self {
        Self {
            r: r >> 1,
            g: g >> 1,
            b: b >> 1,
        }
    }
}

/// An unparsed incoming MIDI message received from the Launchpad Mini MK3.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    /// Timestamp in microseconds (relative to an arbitrary point in time when the input was opened).
    pub timestamp: u64,
    /// Raw MIDI bytes received from the device.
    pub bytes: Vec<u8>,
}

impl Message {
    /// Creates a new `Message` from a timestamp and raw MIDI bytes.
    #[must_use]
    pub const fn new(timestamp: u64, bytes: Vec<u8>) -> Self {
        Self { timestamp, bytes }
    }

    /// Returns a slice to the raw MIDI bytes.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }
}

impl std::ops::Deref for Message {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        &self.bytes
    }
}

impl AsRef<[u8]> for Message {
    fn as_ref(&self) -> &[u8] {
        &self.bytes
    }
}

const DEFAULT_INCOMING_CAPACITY: usize = 1024;

#[derive(Debug)]
struct InnerQueue {
    messages: VecDeque<Message>,
    capacity: usize,
}

/// Handle to a connected Launchpad Mini MK3 MIDI device.
pub struct Launchpad {
    connection: midir::MidiOutputConnection,
    _input_connection: midir::MidiInputConnection<()>,
    queue: Arc<(Mutex<InnerQueue>, Condvar)>,
    name: String,
    input_name: String,
}

impl Launchpad {
    /// Attempts to locate and connect to an attached Launchpad Mini MK3 MIDI input and output ports.
    pub fn new() -> Result<Self> {
        let output = midir::MidiOutput::new("Launchpad")?;

        let launchpad_out_port = output
            .ports()
            .into_iter()
            .find(|port| {
                let name = output.port_name(port).unwrap_or_default();
                name.contains("LPMiniMK3") && name.contains("MI")
            })
            .ok_or(Error::DeviceNotFound)?;

        let name = output.port_name(&launchpad_out_port)?;
        let connection = output.connect(&launchpad_out_port, "Launchpad Connection")?;

        let mut input = midir::MidiInput::new("Launchpad Input")?;
        input.ignore(midir::Ignore::None);

        let launchpad_in_port = input
            .ports()
            .into_iter()
            .find(|port| {
                let name = input.port_name(port).unwrap_or_default();
                name.contains("LPMiniMK3") && name.contains("MI")
            })
            .ok_or(Error::DeviceNotFound)?;

        let input_name = input.port_name(&launchpad_in_port)?;

        let queue = Arc::new((
            Mutex::new(InnerQueue {
                messages: VecDeque::with_capacity(128),
                capacity: DEFAULT_INCOMING_CAPACITY,
            }),
            Condvar::new(),
        ));

        let queue_clone = Arc::clone(&queue);
        let input_connection = input.connect(
            &launchpad_in_port,
            "Launchpad Input Connection",
            move |timestamp, bytes, ()| {
                let (lock, cvar) = &*queue_clone;
                if let Ok(mut inner) = lock.lock() {
                    if inner.messages.len() >= inner.capacity {
                        inner.messages.pop_front();
                    }
                    inner.messages.push_back(Message {
                        timestamp,
                        bytes: bytes.to_vec(),
                    });
                    cvar.notify_one();
                }
            },
            (),
        )?;

        Ok(Launchpad {
            connection,
            _input_connection: input_connection,
            queue,
            name,
            input_name,
        })
    }

    /// Returns the output port name of the connected Launchpad.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the input port name of the connected Launchpad.
    #[must_use]
    pub fn input_name(&self) -> &str {
        &self.input_name
    }

    /// Polls for the next received message without blocking.
    ///
    /// Returns `Some(Message)` if a message is available, or `None` if the queue is empty.
    pub fn poll_message(&mut self) -> Option<Message> {
        let (lock, _) = &*self.queue;
        lock.lock().ok()?.messages.pop_front()
    }

    /// Blocks until a message is received from the Launchpad, or returns `None` if the queue was poisoned.
    pub fn read_message(&mut self) -> Option<Message> {
        let (lock, cvar) = &*self.queue;
        let mut inner = lock.lock().ok()?;
        loop {
            if let Some(msg) = inner.messages.pop_front() {
                return Some(msg);
            }
            inner = cvar.wait(inner).ok()?;
        }
    }

    /// Waits up to `timeout` for a message to arrive from the Launchpad.
    ///
    /// Returns `Some(Message)` if a message arrived within the timeout, or `None` if timed out.
    pub fn read_message_timeout(&mut self, timeout: Duration) -> Option<Message> {
        let (lock, cvar) = &*self.queue;
        let mut inner = lock.lock().ok()?;
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(msg) = inner.messages.pop_front() {
                return Some(msg);
            }
            let now = Instant::now();
            if now >= deadline {
                return None;
            }
            let remaining = deadline.saturating_duration_since(now);
            let (new_inner, timeout_result) = cvar.wait_timeout(inner, remaining).ok()?;
            inner = new_inner;
            if timeout_result.timed_out() && inner.messages.is_empty() {
                return None;
            }
        }
    }

    /// Flushes all pending incoming messages from the buffer, discarding them.
    ///
    /// Useful for clearing stale events (e.g. initial mode switch acknowledgements
    /// or old pad presses) before entering an event loop.
    pub fn flush(&mut self) {
        let (lock, _) = &*self.queue;
        if let Ok(mut inner) = lock.lock() {
            inner.messages.clear();
        }
    }

    /// Drains and returns all currently pending incoming messages as a `Vec<Message>`.
    pub fn drain_messages(&mut self) -> Vec<Message> {
        let (lock, _) = &*self.queue;
        if let Ok(mut inner) = lock.lock() {
            inner.messages.drain(..).collect()
        } else {
            Vec::new()
        }
    }

    /// Returns the number of unread messages currently buffered.
    #[must_use]
    pub fn incoming_count(&self) -> usize {
        let (lock, _) = &*self.queue;
        lock.lock().map(|inner| inner.messages.len()).unwrap_or(0)
    }

    /// Returns `true` if there are no pending incoming messages.
    #[must_use]
    pub fn is_incoming_empty(&self) -> bool {
        let (lock, _) = &*self.queue;
        lock.lock().map(|inner| inner.messages.is_empty()).unwrap_or(true)
    }

    /// Sets the maximum capacity of the incoming message queue.
    ///
    /// If more messages arrive than this limit without being read, the oldest messages
    /// are dropped to avoid unbounded memory growth.
    pub fn set_incoming_capacity(&mut self, capacity: usize) {
        let (lock, _) = &*self.queue;
        if let Ok(mut inner) = lock.lock() {
            inner.capacity = capacity;
            while inner.messages.len() > capacity {
                inner.messages.pop_front();
            }
        }
    }

    /// Returns an iterator over all currently available incoming messages, consuming them.
    pub fn incoming_messages(&mut self) -> impl Iterator<Item = Message> + '_ {
        std::iter::from_fn(|| self.poll_message())
    }

    /// Sends a raw SysEx message wrapped in the standard Launchpad Mini MK3 header and footer.
    pub fn sys_ex_message(&mut self, message: &[u8]) -> Result<()> {
        let mut sysex_message = Vec::with_capacity(SYSEX_HEADER.len() + message.len() + 1);
        sysex_message.extend_from_slice(&SYSEX_HEADER);
        sysex_message.extend_from_slice(message);
        sysex_message.push(SYSEX_FOOTER);
        self.send_message(&sysex_message)
    }

    /// Sends an arbitrary raw MIDI byte buffer directly over the connection.
    pub fn send_message(&mut self, message: &[u8]) -> Result<()> {
        self.connection.send(message)?;
        Ok(())
    }

    /// Switches the device into Programmer Mode.
    pub fn programmer_mode(&mut self) -> Result<()> {
        let message = [command::MODE_SWITCH, mode::PROGRAMMER];
        self.sys_ex_message(&message)
    }

    /// Switches the device out of Programmer Mode back into Standalone (Live) Mode.
    pub fn standalone_mode(&mut self) -> Result<()> {
        let message = [command::MODE_SWITCH, mode::STANDALONE];
        self.sys_ex_message(&message)
    }

    /// Sets the color of a single button using a palette color index.
    pub fn set_led(&mut self, button: Button, palette: Palette) -> Result<()> {
        let message = [
            command::LED_SET,
            led_type::PALETTE,
            button.index(),
            palette.0,
        ];
        self.sys_ex_message(&message)
    }

    /// Sets the color of a single button using an RGB color (7-bit components: 0–127).
    pub fn set_led_rgb(&mut self, button: Button, rgb: RGB) -> Result<()> {
        let message = [
            command::LED_SET,
            led_type::RGB,
            button.index(),
            rgb.r & 0x7F,
            rgb.g & 0x7F,
            rgb.b & 0x7F,
        ];
        self.sys_ex_message(&message)
    }

    /// Sets multiple button colors using palette indices in a single SysEx message.
    pub fn set_leds<I>(&mut self, buttons: I) -> Result<()>
    where
        I: IntoIterator<Item = (Button, Palette)>,
    {
        let iter = buttons.into_iter();
        let (lower, _) = iter.size_hint();
        let mut message = Vec::with_capacity(1 + lower * 3);
        message.push(command::LED_SET);
        for (button, palette) in iter {
            message.extend([led_type::PALETTE, button.index(), palette.0]);
        }
        self.sys_ex_message(&message)
    }

    /// Sets multiple button colors using RGB colors in a single SysEx message.
    pub fn set_leds_rgb<I>(&mut self, buttons: I) -> Result<()>
    where
        I: IntoIterator<Item = (Button, RGB)>,
    {
        let iter = buttons.into_iter();
        let (lower, _) = iter.size_hint();
        let mut message = Vec::with_capacity(1 + lower * 5);
        message.push(command::LED_SET);
        for (button, rgb) in iter {
            message.extend([
                led_type::RGB,
                button.index(),
                rgb.r & 0x7F,
                rgb.g & 0x7F,
                rgb.b & 0x7F,
            ]);
        }
        self.sys_ex_message(&message)
    }

    /// Updates the entire 81-LED grid in a single message.
    ///
    /// The frame array is ordered in row-major order from top-left to bottom-right (`row * 9 + col`):
    /// - Row 0: Top control buttons (0..7) and Logo (8)
    /// - Rows 1..=8: 8x8 pad matrix (cols 0..7) and Scene buttons (col 8)
    pub fn set_frame(&mut self, frame: &[RGB; 81]) -> Result<()> {
        let mut msg = Vec::with_capacity(1 + 81 * 5);
        msg.push(command::LED_SET);
        for row in 0..9u8 {
            for col in 0..9u8 {
                let c = frame[(row * 9 + col) as usize];
                let position = (9 - row) * 10 + (col + 1);
                msg.extend([led_type::RGB, position, c.r & 0x7F, c.g & 0x7F, c.b & 0x7F]);
            }
        }
        self.sys_ex_message(&msg)
    }

    /// Turns off all LEDs across the entire 9x9 grid.
    pub fn clear(&mut self) -> Result<()> {
        let mut message = Vec::with_capacity(1 + 81 * 3);
        message.push(command::LED_SET);
        for row in 1..=9 {
            for col in 1..=9 {
                message.extend([led_type::PALETTE, row * 10 + col, 0x00]);
            }
        }
        self.sys_ex_message(&message)
    }

    /// Scrolls ASCII text across the Launchpad pads using the white palette color.
    pub fn text(&mut self, text: &[u8]) -> Result<()> {
        let mut message = Vec::with_capacity(5 + text.len());
        message.extend([
            command::TEXT_SCROLL,
            0x00,
            0x07,
            text_type::PALETTE,
            Palette::WHITE.0,
        ]);
        message.extend_from_slice(text);
        self.sys_ex_message(&message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pad_indices() {
        for y in 0..8 {
            for x in 0..8 {
                let btn = Button::Pad { x, y };
                let idx = btn.index();
                assert_eq!(Button::from_index(idx), Some(btn));
            }
        }
    }

    #[test]
    fn test_top_indices() {
        for i in 0..8 {
            let btn = Button::Top(i);
            let idx = btn.index();
            assert_eq!(Button::from_index(idx), Some(btn));
        }
    }

    #[test]
    fn test_side_indices() {
        for i in 0..8 {
            let btn = Button::Side(i);
            let idx = btn.index();
            assert_eq!(Button::from_index(idx), Some(btn));
        }
    }

    #[test]
    fn test_logo_index() {
        let btn = Button::Logo;
        assert_eq!(btn.index(), 99);
        assert_eq!(Button::from_index(99), Some(Button::Logo));
    }

    #[test]
    fn test_invalid_indices() {
        assert_eq!(Button::from_index(0), None);
        assert_eq!(Button::from_index(10), None);
        assert_eq!(Button::from_index(100), None);
        assert_eq!(Button::from_index(90), None);
    }

    #[test]
    fn test_rgb_valid() {
        let c = RGB::new(127, 0, 64);
        assert_eq!(c.r, 127);
        assert_eq!(c.g, 0);
        assert_eq!(c.b, 64);
        assert_eq!(RGB::try_new(127, 0, 64), Some(c));
    }

    #[test]
    fn test_rgb_try_new_out_of_bounds() {
        assert_eq!(RGB::try_new(128, 0, 0), None);
        assert_eq!(RGB::try_new(0, 128, 0), None);
        assert_eq!(RGB::try_new(0, 0, 128), None);
        assert_eq!(RGB::try_new(255, 255, 255), None);
    }

    #[test]
    fn test_rgb_from_8bit() {
        assert_eq!(RGB::from_8bit(255, 128, 0), RGB::new(127, 64, 0));
        assert_eq!(RGB::from_8bit(0, 0, 0), RGB::new(0, 0, 0));
    }

    #[test]
    #[should_panic(expected = "Red channel value must be 0–127")]
    fn test_rgb_new_panics_on_invalid_red() {
        let _ = RGB::new(128, 0, 0);
    }

    #[test]
    fn test_message_deref_and_bytes() {
        let msg = Message::new(12345, vec![0x90, 0x3C, 0x7F]);
        assert_eq!(msg.timestamp, 12345);
        assert_eq!(msg.as_bytes(), &[0x90, 0x3C, 0x7F]);
        assert_eq!(&msg[..], &[0x90, 0x3C, 0x7F]);
        assert_eq!(msg.len(), 3);
        assert_eq!(msg[0], 0x90);
    }

    #[test]
    fn test_inner_queue_capacity() {
        let mut q = InnerQueue {
            messages: VecDeque::new(),
            capacity: 2,
        };
        for i in 0..5 {
            if q.messages.len() >= q.capacity {
                q.messages.pop_front();
            }
            q.messages.push_back(Message::new(i, vec![i as u8]));
        }
        assert_eq!(q.messages.len(), 2);
        assert_eq!(q.messages.pop_front().unwrap().timestamp, 3);
        assert_eq!(q.messages.pop_front().unwrap().timestamp, 4);
        assert!(q.messages.is_empty());
    }
}
