#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    codeterm_lib::run();
}

extern crate domios_lib as codeterm_lib; // rename-bridge
