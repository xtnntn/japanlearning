// Prevents a console window on Windows in release, do not remove.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
  nihongo_daily_reader_lib::run()
}
