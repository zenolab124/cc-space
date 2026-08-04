// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    if let Some(code) = app_lib::run_internal_command() {
        std::process::exit(code);
    }
    app_lib::run();
}
