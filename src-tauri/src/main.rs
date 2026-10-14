#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    codeterm_lib::register_socket();
    codeterm_lib::run();
}
