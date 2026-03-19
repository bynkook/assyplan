// In release builds on Windows, run as a GUI subsystem app so launching the
// executable does not open a second console window.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use assyplan_native::run;

fn main() {
    if let Err(e) = run() {
        eprintln!("Error running app: {}", e);
    }
}
