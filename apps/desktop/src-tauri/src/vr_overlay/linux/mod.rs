//! M0 route experiment. Not wired into production SurfaceHub/runtime capabilities.
mod host;
mod openvr;
mod protocol;
#[path = "../frame_receipt.rs"]
mod frame_receipt;
#[path = "../visibility.rs"]
mod visibility;

pub fn run() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let mut mode = "web".to_owned();
    let mut options = host::Options {
        assets: std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../dist"),
        output: std::env::temp_dir().join(format!("vrcs-linux-vr-prototype-{}", std::process::id())),
        seconds: 16, submit_vr: false,
    };
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--mode" => mode = args.next().ok_or("--mode requires web or static")?,
            "--assets" => options.assets = args.next().ok_or("--assets requires the built frontend dist directory")?.into(),
            "--output" => options.output = args.next().ok_or("--output requires a directory")?.into(),
            "--seconds" => options.seconds = args.next().ok_or("--seconds requires a duration")?.parse().map_err(|_| "invalid duration")?,
            "--vr" => options.submit_vr = true,
            "--help" => {
                println!("vrcs-linux-vr-prototype [--mode web|static] [--assets apps/desktop/dist] [--output DIR] [--seconds 16] [--vr]\nweb: real offscreen WebKitGTK React reader, CJK updates, layout/paint receipts, pin interaction, scrolling, hide/pause/resume/reload; add --vr for SetOverlayRaw.\nstatic: independent RGBA sample, no GTK window, reconnects runtime until duration ends.\nJSON events and PNGs are measurements, not hardware/visual acceptance. Use /usr/bin/time -v for CPU/RSS; inspect PNGs and the real headset. ARM64 --vr/static: vr.runtime_arch_unverified.");
                return Ok(());
            }
            _ => return Err(format!("Unknown argument: {arg}")),
        }
    }
    if options.seconds == 0 || options.seconds > 3600 { return Err("--seconds must be 1..3600".into()); }
    if options.submit_vr && !cfg!(target_arch = "x86_64") { return Err("vr.runtime_arch_unverified".into()); }
    println!("{}", serde_json::json!({"event":"environment","arch":std::env::consts::ARCH,"session":std::env::var("XDG_SESSION_TYPE").ok(),
        "mode":mode,"output":options.output,"vr_requested":options.submit_vr,"production_capability":false}));
    match mode.as_str() {
        "web" => host::run(options),
        "static" => openvr::static_sample(options.seconds),
        _ => Err("--mode must be web or static".into()),
    }
}
