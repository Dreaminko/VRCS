//! CPU raw submission intentionally isolated from the Windows GPU backend.
use std::{
    ffi::CString,
    time::{Duration, Instant},
};

pub enum Input {
    Move([f64; 2]),
    Down([f64; 2]),
    Up([f64; 2]),
    Quit,
}

#[cfg(target_arch = "x86_64")]
mod native {
    use super::*;
    pub struct Overlay {
        table: *const openvr_sys::VR_IVROverlay_FnTable,
        system: *const openvr_sys::VR_IVRSystem_FnTable,
        handle: u64,
        _context: openvr::Context,
    }
    fn check(code: openvr_sys::EVROverlayError) -> Result<(), String> {
        if code == openvr_sys::EVROverlayError_VROverlayError_None {
            Ok(())
        } else {
            Err(format!("OpenVR overlay error {code}"))
        }
    }
    impl Overlay {
        pub fn connect() -> Result<Self, String> {
            let context = unsafe { openvr::init(openvr::ApplicationType::Overlay) }
                .map_err(|error| format!("vr.runtime_unavailable: {error:?}"))?;
            let mut version = b"FnTable:".to_vec();
            version.extend(openvr_sys::IVROverlay_Version);
            let mut error = openvr_sys::EVRInitError_VRInitError_None;
            let pointer =
                unsafe { openvr_sys::VR_GetGenericInterface(version.as_ptr().cast(), &mut error) };
            if pointer == 0 || error != openvr_sys::EVRInitError_VRInitError_None {
                return Err(format!("OpenVR overlay interface error {error}"));
            }
            let mut system_version = b"FnTable:".to_vec();
            system_version.extend(openvr_sys::IVRSystem_Version);
            let system = unsafe {
                openvr_sys::VR_GetGenericInterface(system_version.as_ptr().cast(), &mut error)
            };
            if system == 0 || error != openvr_sys::EVRInitError_VRInitError_None {
                return Err(format!("OpenVR system interface error {error}"));
            }
            let mut overlay = Self {
                table: pointer as _,
                system: system as _,
                handle: 0,
                _context: context,
            };
            let api = unsafe { &*overlay.table };
            let name =
                CString::new(format!("vrcs.linux.prototype.{}", std::process::id())).unwrap();
            unsafe {
                check(api.CreateOverlay.ok_or("CreateOverlay unavailable")?(
                    name.as_ptr().cast_mut(),
                    name.as_ptr().cast_mut(),
                    &mut overlay.handle,
                ))?;
                check(api
                    .SetOverlayWidthInMeters
                    .ok_or("SetOverlayWidthInMeters unavailable")?(
                    overlay.handle,
                    1.2,
                ))?;
                let mut transform = openvr_sys::HmdMatrix34_t {
                    m: [[1., 0., 0., 0.], [0., 1., 0., -0.15], [0., 0., 1., -1.5]],
                };
                check(api
                    .SetOverlayTransformTrackedDeviceRelative
                    .ok_or("Tracked transform unavailable")?(
                    overlay.handle,
                    0,
                    &mut transform,
                ))?;
                check(api
                    .SetOverlayInputMethod
                    .ok_or("SetOverlayInputMethod unavailable")?(
                    overlay.handle,
                    openvr_sys::VROverlayInputMethod_Mouse,
                ))?;
                let mut scale = openvr_sys::HmdVector2_t { v: [1., 1.] };
                check(api
                    .SetOverlayMouseScale
                    .ok_or("SetOverlayMouseScale unavailable")?(
                    overlay.handle,
                    &mut scale,
                ))?;
            }
            Ok(overlay)
        }
        pub fn submit(&self, rgba: &[u8], width: u32, height: u32) -> Result<(), String> {
            if rgba.len() != width as usize * height as usize * 4 {
                return Err("Invalid RGBA buffer".into());
            }
            unsafe {
                check((&*self.table)
                    .SetOverlayRaw
                    .ok_or("SetOverlayRaw unavailable")?(
                    self.handle,
                    rgba.as_ptr().cast_mut().cast(),
                    width,
                    height,
                    4,
                ))
            }
        }
        pub fn visibility(&self, alpha: f32) -> Result<(), String> {
            let api = unsafe { &*self.table };
            unsafe {
                check(api.SetOverlayAlpha.ok_or("SetOverlayAlpha unavailable")?(
                    self.handle,
                    alpha,
                ))?;
                if alpha > 0. {
                    check(api.ShowOverlay.ok_or("ShowOverlay unavailable")?(
                        self.handle,
                    ))
                } else {
                    check(api.HideOverlay.ok_or("HideOverlay unavailable")?(
                        self.handle,
                    ))
                }
            }
        }
        pub fn events(&self) -> Result<Vec<Input>, String> {
            let api = unsafe { &*self.table };
            let poll = api
                .PollNextOverlayEvent
                .ok_or("PollNextOverlayEvent unavailable")?;
            let mut inputs = Vec::new();
            let system = unsafe { &*self.system };
            if let Some(poll) = system.PollNextEvent {
                for _ in 0..128 {
                    let mut event: openvr_sys::VREvent_t = unsafe { std::mem::zeroed() };
                    if !unsafe { poll(&mut event, std::mem::size_of_val(&event) as u32) } {
                        break;
                    }
                    if event.eventType == openvr_sys::EVREventType_VREvent_Quit {
                        if let Some(acknowledge) = system.AcknowledgeQuit_Exiting {
                            unsafe {
                                acknowledge();
                            }
                        }
                        inputs.push(Input::Quit);
                    }
                }
            }
            for _ in 0..128 {
                let mut event: openvr_sys::VREvent_t = unsafe { std::mem::zeroed() };
                if !unsafe {
                    poll(
                        self.handle,
                        &mut event,
                        std::mem::size_of_val(&event) as u32,
                    )
                } {
                    break;
                }
                let kind = event.eventType;
                if kind == openvr_sys::EVREventType_VREvent_Quit {
                    inputs.push(Input::Quit);
                } else if [
                    openvr_sys::EVREventType_VREvent_MouseMove,
                    openvr_sys::EVREventType_VREvent_MouseButtonDown,
                    openvr_sys::EVREventType_VREvent_MouseButtonUp,
                ]
                .contains(&kind)
                {
                    let mouse = unsafe { event.data.mouse };
                    let point = [f64::from(mouse.x), 1. - f64::from(mouse.y)];
                    if kind == openvr_sys::EVREventType_VREvent_MouseMove {
                        inputs.push(Input::Move(point));
                    } else if mouse.button & openvr_sys::EVRMouseButton_VRMouseButton_Left as u32
                        != 0
                    {
                        inputs.push(
                            if kind == openvr_sys::EVREventType_VREvent_MouseButtonDown {
                                Input::Down(point)
                            } else {
                                Input::Up(point)
                            },
                        );
                    }
                }
            }
            Ok(inputs)
        }
    }
    impl Drop for Overlay {
        fn drop(&mut self) {
            if self.handle == 0 {
                return;
            }
            let api = unsafe { &*self.table };
            unsafe {
                if let Some(hide) = api.HideOverlay {
                    hide(self.handle);
                }
                if let Some(clear) = api.ClearOverlayTexture {
                    clear(self.handle);
                }
                if let Some(destroy) = api.DestroyOverlay {
                    destroy(self.handle);
                }
            }
            // Context is dropped after the handle and submitted texture detach.
        }
    }
}
#[cfg(target_arch = "x86_64")]
pub use native::Overlay;

#[cfg(not(target_arch = "x86_64"))]
pub struct Overlay;
#[cfg(not(target_arch = "x86_64"))]
impl Overlay {
    pub fn connect() -> Result<Self, String> {
        Err("vr.runtime_arch_unverified".into())
    }
    pub fn submit(&self, _: &[u8], _: u32, _: u32) -> Result<(), String> {
        Err("vr.runtime_arch_unverified".into())
    }
    pub fn visibility(&self, _: f32) -> Result<(), String> {
        Err("vr.runtime_arch_unverified".into())
    }
    pub fn events(&self) -> Result<Vec<Input>, String> {
        Err("vr.runtime_arch_unverified".into())
    }
}

pub fn static_sample(seconds: u64) -> Result<(), String> {
    if !cfg!(target_arch = "x86_64") {
        return Err("vr.runtime_arch_unverified".into());
    }
    let image =
        cairo::ImageSurface::create(cairo::Format::ARgb32, 1024, 256).map_err(|e| e.to_string())?;
    let context = cairo::Context::new(&image).map_err(|e| e.to_string())?;
    context.set_source_rgba(0.02, 0.05, 0.10, 0.65);
    context.rectangle(16., 16., 992., 224.);
    context.fill().map_err(|e| e.to_string())?;
    context.set_source_rgba(1., 1., 1., 1.);
    context.select_font_face(
        "sans-serif",
        cairo::FontSlant::Normal,
        cairo::FontWeight::Normal,
    );
    context.set_font_size(52.);
    context.move_to(48., 112.);
    context
        .show_text("VRCS Linux / RGBA subtitle")
        .map_err(|e| e.to_string())?;
    context.set_font_size(28.);
    context.move_to(48., 180.);
    context
        .show_text("TOP LEFT   -   transparent border   -   bottom right")
        .map_err(|e| e.to_string())?;
    drop(context);
    let pixels = super::host::rgba(&image)?;
    let started = Instant::now();
    let mut runtime: Option<Overlay> = None;
    let mut next_attempt = Instant::now();
    let mut submissions = 0;
    while started.elapsed() < Duration::from_secs(seconds) {
        if runtime.is_none() && Instant::now() >= next_attempt {
            match Overlay::connect().and_then(|overlay| {
                overlay.submit(&pixels, 1024, 256)?;
                overlay.visibility(1.)?;
                Ok(overlay)
            }) {
                Ok(overlay) => {
                    runtime = Some(overlay);
                    submissions += 1;
                    println!(
                        "{}",
                        serde_json::json!({"event":"static_submitted","session":submissions})
                    );
                }
                Err(error) => eprintln!("{error}"),
            }
            next_attempt = Instant::now() + Duration::from_secs(2);
        }
        if let Some(overlay) = runtime.as_ref() {
            match overlay.events() {
                Ok(events) if !events.iter().any(|input| matches!(input, Input::Quit)) => {
                    if let Err(error) = overlay.visibility(1.) {
                        eprintln!("{error}");
                        runtime = None;
                    }
                }
                _ => runtime = None,
            }
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    drop(runtime);
    if submissions == 0 {
        return Err(
            "No static frame reached OpenVR; runtime/hardware acceptance remains blocked".into(),
        );
    }
    Ok(())
}
