#[cfg(target_os = "linux")]
#[path = "../vr_overlay/linux/mod.rs"]
mod linux;

fn main() {
    #[cfg(target_os = "linux")]
    let result = linux::run();
    #[cfg(not(target_os = "linux"))]
    let result: Result<(), String> = Err("Linux VR prototype requires Linux".into());
    if let Err(error) = result {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
