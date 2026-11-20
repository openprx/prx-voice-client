//! Prints the input devices with UID and transport type and the device the input policy would open.
//!
//! Usage: `cargo run -p prxvc-audio --example list_inputs -- [CONFIGURED_UID_OR_NAME]`
//! Read-only: it opens no audio stream.

use prxvc_audio::input_devices::list_input_devices;
use prxvc_audio::input_policy::choose_input;

fn main() {
    let configured = std::env::args().nth(1).unwrap_or_default();
    let devices = match list_input_devices() {
        Ok(d) => d,
        Err(e) => {
            eprintln!("enumeration failed: {e}");
            std::process::exit(1);
        }
    };
    for d in &devices {
        println!(
            "{} uid={:?} name={:?} transport={:?}",
            if d.is_default { "*" } else { " " },
            d.uid,
            d.name,
            d.transport
        );
    }
    match choose_input(&devices, &configured) {
        Ok(sel) => println!("policy: {:?} ({:?})", sel.device.name, sel.reason),
        Err(e) => println!("policy error: {e}"),
    }
}
