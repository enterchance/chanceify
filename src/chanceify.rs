//! Chanceify desktop command.

#![cfg_attr(
    all(target_os = "windows", not(feature = "console")),
    windows_subsystem = "windows"
)]

mod entrypoint;

fn main() -> eframe::Result<()> {
    entrypoint::run()
}
