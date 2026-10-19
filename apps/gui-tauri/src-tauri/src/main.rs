// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use clap::Parser;

use copyforge_gui_tauri_lib::{run, LaunchArgs};

fn main() {
    run(LaunchArgs::parse());
}
