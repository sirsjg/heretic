// Keep the console window from appearing alongside the app on Windows release
// builds. Harmless on macOS and Linux, which are the supported targets.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    accel_app::run();
}
