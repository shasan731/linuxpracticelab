// Hides the console window on Windows release builds. Debug builds keep it, because the
// tracing output is exactly what you want while working on the VM lifecycle.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    linux_practice_lab_lib::run()
}
