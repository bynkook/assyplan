// In release builds on Windows, attach a console so that debug/log output
// is visible when the binary is launched from a terminal.
// Remove this attribute (or invert the cfg) if you want a pure GUI-only binary.
#![cfg_attr(not(debug_assertions), windows_subsystem = "console")]

use assyplan_native::run;

fn main() {
    if let Err(e) = run() {
        eprintln!("Error running app: {}", e);
    }
}
