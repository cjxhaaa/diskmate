// Prevents an extra console window on Windows in release.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // `diskmate.exe --recycle <list>`: a short-lived helper, never the app (see clean::recycle).
    if let Some(code) = diskmate_lib::run_helper() {
        std::process::exit(code);
    }
    diskmate_lib::run()
}
