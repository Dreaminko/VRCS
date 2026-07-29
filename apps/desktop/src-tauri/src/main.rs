#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    if std::env::args().any(|argument| argument == "--release-self-test") {
        if let Err(error) = vrcs_desktop_lib::release_self_test() {
            eprintln!("{error}");
            std::process::exit(1);
        }
        return;
    }
    vrcs_desktop_lib::run();
}
