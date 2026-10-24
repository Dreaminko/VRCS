#[cfg(windows)]
mod platform {
    use openvr::overlay::OverlayHandle;
    use openvr::pose::Matrix3x4;
    use openvr::tracked_device_index;
    use openvr::{ApplicationType, Context, Overlay, System, TrackedControllerRole};
    use vrcs_core::{VrOcrWristConfig, VrOverlayHeadsetConfig, VrOverlayWristConfig};

    use super::{menu_config, ControllerBinding, OverlayKind};
    use crate::vr_overlay::d3d11_texture::Device;
    use crate::vr_overlay::dashboard::{
        pointer_from_openvr, DashboardPointerEvent, DASHBOARD_HEIGHT, DASHBOARD_WIDTH,
    };
    use crate::vr_overlay::ocr_capture::{
        center_bounds, crop_projection, pose_within_translation_limit_m, projection_from_openvr,
        EyeCapture, StereoCapture, OCR_MOVEMENT_LIMIT_M,
    };
    use crate::vr_overlay::renderer::Texture;
    use crate::vr_overlay::surface::SurfaceId;
    use crate::vr_overlay::transform;

    const HEADSET_KEY: &str = "org.vrcs.overlay.headset\0";
    const HEADSET_NAME: &str = "VRCS Headset Subtitles\0";
    const WRIST_KEY: &str = "org.vrcs.overlay.wrist\0";
    const WRIST_NAME: &str = "VRCS Wrist Subtitles\0";
    const DASHBOARD_KEY: &str = "org.vrcs.dashboard.settings\0";
    const DASHBOARD_NAME: &str = "VRCS\0";
    const DASHBOARD_THUMBNAIL: tauri::image::Image<'_> =
        tauri::include_image!("icons/128x128@2x.png");

    pub struct OpenVrBackend {
        context: Context,
        system: System,
        raw_system: *const openvr_sys::VR_IVRSystem_FnTable,
        overlay: Overlay,
        raw_overlay: *const openvr_sys::VR_IVROverlay_FnTable,
        texture_device: Option<Device>,
        ocr_mirrors: [Option<Mirror>; 2],
        headset: Option<OverlayHandle>,
        wrist: Option<OverlayHandle>,
        wrist_menu: Option<OverlayHandle>,
        tracking_reset: bool,
        ocr_wrist: Option<OverlayHandle>,
        ocr_progress: Option<OverlayHandle>,
        ocr_wrist_device: Option<u32>,
        ocr_frame: Option<OverlayHandle>,
        ocr_result: Option<OverlayHandle>,
        dashboard: DashboardOverlay,
        headset_state: SubmittedState,
        wrist_state: SubmittedState,
        wrist_menu_state: SubmittedState,
        ocr_wrist_state: SubmittedState,
        ocr_progress_state: SubmittedState,
        ocr_frame_state: SubmittedState,
        ocr_result_state: SubmittedState,
        dashboard_state: SubmittedState,
        ocr_input: Option<crate::vr_overlay::ocr_input::OcrInput>,
        cursor: crate::vr_overlay::cursor::CursorOverlay,
        cursor_interval: std::time::Duration,
    }

    #[derive(Default)]
    struct SubmittedState {
        visible: bool,
        opacity: Option<f32>,
        interactive: Option<bool>,
    }

    #[derive(Default)]
    struct DashboardOverlay {
        handles: Option<[OverlayHandle; 2]>,
    }

    impl DashboardOverlay {
        fn main(&self) -> Option<OverlayHandle> {
            self.handles.map(|handles| handles[0])
        }

        fn ensure(&mut self, table: &openvr_sys::VR_IVROverlay_FnTable) -> Result<(), String> {
            if self.handles.is_some() {
                return Ok(());
            }
            let check = |error, action| {
                if error == openvr_sys::EVROverlayError_VROverlayError_None {
                    Ok(())
                } else {
                    Err(format!("{action} failed with code {error}"))
                }
            };
            let mut main = 0;
            let mut thumbnail = 0;
            unsafe {
                check(
                    table
                        .CreateDashboardOverlay
                        .ok_or("SteamVR dashboard overlays unavailable")?(
                        DASHBOARD_KEY.as_ptr().cast_mut().cast(),
                        DASHBOARD_NAME.as_ptr().cast_mut().cast(),
                        &mut main,
                        &mut thumbnail,
                    ),
                    "Create dashboard overlay",
                )?;
            }
            self.handles = Some([OverlayHandle(main), OverlayHandle(thumbnail)]);
            let result = (|| unsafe {
                check(
                    table
                        .SetOverlayWidthInMeters
                        .ok_or("SteamVR dashboard width unavailable")?(
                        main, 2.25
                    ),
                    "Configure dashboard width",
                )?;
                check(
                    table
                        .SetOverlayInputMethod
                        .ok_or("SteamVR dashboard input unavailable")?(
                        main,
                        openvr_sys::VROverlayInputMethod_Mouse,
                    ),
                    "Configure dashboard input",
                )?;
                let mut scale = openvr_sys::HmdVector2_t {
                    v: [DASHBOARD_WIDTH as f32, DASHBOARD_HEIGHT as f32],
                };
                check(
                    table
                        .SetOverlayMouseScale
                        .ok_or("SteamVR dashboard pointer scale unavailable")?(
                        main, &mut scale
                    ),
                    "Configure dashboard pointer",
                )?;
                check(
                    table
                        .SetOverlayRaw
                        .ok_or("SteamVR dashboard thumbnail unavailable")?(
                        thumbnail,
                        DASHBOARD_THUMBNAIL.rgba().as_ptr().cast_mut().cast(),
                        DASHBOARD_THUMBNAIL.width(),
                        DASHBOARD_THUMBNAIL.height(),
                        4,
                    ),
                    "Submit dashboard thumbnail",
                )
            })();
            if result.is_err() {
                self.destroy(table);
            }
            result
        }

        fn clear_texture(&self, table: &openvr_sys::VR_IVROverlay_FnTable) {
            if let (Some(main), Some(clear)) = (self.main(), table.ClearOverlayTexture) {
                unsafe {
                    let _ = clear(main.0);
                }
            }
        }

        fn destroy(&mut self, table: &openvr_sys::VR_IVROverlay_FnTable) {
            if let Some(handles) = self.handles.take() {
                for handle in handles {
                    unsafe {
                        if let Some(clear) = table.ClearOverlayTexture {
                            let _ = clear(handle.0);
                        }
                        if let Some(destroy) = table.DestroyOverlay {
                            let _ = destroy(handle.0);
                        }
                    }
                }
            }
        }
    }

    struct Mirror {
        view: *mut std::ffi::c_void,
        release: unsafe extern "C" fn(*mut std::ffi::c_void),
    }

    impl Drop for Mirror {
        fn drop(&mut self) {
            if !self.view.is_null() {
                unsafe {
                    (self.release)(self.view);
                }
            }
        }
    }

    impl OpenVrBackend {
        pub fn hide_ocr(&mut self) {
            // Static WGC surfaces may not produce another frame when shown again.
            self.hide(OverlayKind::OcrFrame);
            self.hide(OverlayKind::OcrResult);
        }

        pub fn ocr_tracking(&self) -> Result<([[f32; 4]; 3], u32, i32), String> {
            let table = load_raw_interface(openvr_sys::IVRCompositor_Version, "compositor")?
                as *const openvr_sys::VR_IVRCompositor_FnTable;
            unsafe {
                let table = &*table;
                let mut pose: openvr_sys::TrackedDevicePose_t = std::mem::zeroed();
                let error = table
                    .GetLastPoseForTrackedDeviceIndex
                    .ok_or("Headset pose query unavailable")?(
                    tracked_device_index::HMD.0,
                    &mut pose,
                    std::ptr::null_mut(),
                );
                if error != 0 || !pose.bPoseIsValid || !pose.bDeviceIsConnected {
                    return Err("Headset tracking unavailable".into());
                }
                Ok((
                    pose.mDeviceToAbsoluteTracking.m,
                    table
                        .GetLastFrameRenderer
                        .ok_or("Scene query unavailable")?(),
                    table
                        .GetTrackingSpace
                        .ok_or("Tracking space query unavailable")?(),
                ))
            }
        }
        pub fn update_ocr_input(&mut self, path: &std::path::Path) -> Result<(), String> {
            if self.ocr_input.is_none() {
                let table = load_raw_interface(openvr_sys::IVRInput_Version, "input")?
                    as *const openvr_sys::VR_IVRInput_FnTable;
                self.ocr_input =
                    Some(unsafe { crate::vr_overlay::ocr_input::OcrInput::new(table, path)? });
            }
            self.ocr_input
                .as_mut()
                .expect("OCR input initialized")
                .update_actions()
        }

        pub fn ocr_input(
            &mut self,
            gesture_enabled: bool,
            reading_focus: bool,
            vision_enabled: bool,
        ) -> Result<crate::vr_overlay::ocr_input::InputActions, String> {
            let tracking = gesture_enabled
                .then(|| self.ocr_tracking().ok())
                .flatten()
                .map(|(head, _, origin)| (head, origin));
            self.ocr_input
                .as_mut()
                .ok_or("OCR input is unavailable")?
                .poll_with_vision(tracking, reading_focus, vision_enabled)
        }
        pub fn freeze_ocr_frame(&mut self) {
            if let Some(input) = &mut self.ocr_input {
                input.freeze_frame();
            }
        }
        pub fn open_ocr_bindings(&mut self) -> Result<(), String> {
            self.ocr_input
                .as_mut()
                .ok_or("Enable VR OCR before opening its bindings")?
                .open_bindings()
        }
        /// Projection metadata for crop validation; no mirror readback needed.
        pub fn ocr_view(&self) -> Result<([EyeCapture; 2], u32, i32), String> {
            let (head, pid, origin) = self.ocr_tracking()?;
            let system = load_raw_system()?;
            let mut eyes = Vec::with_capacity(2);
            for eye in [openvr_sys::EVREye_Eye_Left, openvr_sys::EVREye_Eye_Right] {
                let mut projection = [0.; 4];
                unsafe {
                    let system = &*system;
                    system
                        .GetProjectionRaw
                        .ok_or("Eye projection query unavailable")?(
                        eye,
                        &mut projection[0],
                        &mut projection[1],
                        &mut projection[2],
                        &mut projection[3],
                    );
                    let eye_to_head =
                        system
                            .GetEyeToHeadTransform
                            .ok_or("Eye transform query unavailable")?(eye)
                        .m;
                    eyes.push(EyeCapture {
                        image: Texture {
                            width: 1024,
                            height: 1024,
                            pixels: vec![],
                        },
                        projection: projection_from_openvr(projection),
                        eye_to_head,
                        head_pose: head,
                    });
                }
            }
            Ok((
                eyes.try_into().map_err(|_| "Missing OCR eye projection")?,
                pid,
                origin,
            ))
        }

        pub fn capture_ocr(
            &mut self,
            config: &vrcs_core::VrOcrConfig,
            selection: Option<&crate::vr_overlay::ocr_selection::Selection>,
        ) -> Result<StereoCapture, String> {
            let system = load_raw_system()?;
            let compositor = load_raw_interface(openvr_sys::IVRCompositor_Version, "compositor")?
                as *const openvr_sys::VR_IVRCompositor_FnTable;
            let device = self
                .texture_device
                .as_ref()
                .ok_or("OCR requires a D3D11 texture device")?;
            unsafe {
                let compositor = &*compositor;
                let system = &*system;
                let scene_pid = compositor
                    .GetLastFrameRenderer
                    .ok_or("Scene renderer query unavailable")?();
                if scene_pid == 0 || !crate::vr_overlay::process::vrchat_process(scene_pid) {
                    return Err("OCR requires an active VRChat VR scene".into());
                }
                let pose_fn = compositor
                    .GetLastPoseForTrackedDeviceIndex
                    .ok_or("Scene pose query unavailable")?;
                let mut pose: openvr_sys::TrackedDevicePose_t = std::mem::zeroed();
                if pose_fn(tracked_device_index::HMD.0, &mut pose, std::ptr::null_mut()) != 0
                    || !pose.bPoseIsValid
                    || !pose.bDeviceIsConnected
                {
                    return Err("Headset pose is unavailable".into());
                }
                let captured_at = std::time::Instant::now();
                let origin = compositor
                    .GetTrackingSpace
                    .ok_or("Tracking space query unavailable")?();
                let get_mirror = compositor
                    .GetMirrorTextureD3D11
                    .ok_or("D3D11 mirror capture unavailable")?;
                let release = compositor
                    .ReleaseMirrorTextureD3D11
                    .ok_or("D3D11 mirror release unavailable")?;
                let mut eyes = Vec::new();
                let eye_count = if config.display_mode == vrcs_core::VrOcrDisplayMode::Stereo {
                    2
                } else {
                    self.ocr_mirrors[1] = None;
                    1
                };
                for (index, eye) in [openvr_sys::EVREye_Eye_Left, openvr_sys::EVREye_Eye_Right]
                    .into_iter()
                    .take(eye_count)
                    .enumerate()
                {
                    // Releasing just after readback can make SteamVR return the previous image.
                    // Keep the mirror alive until the next acquisition (OpenVR issue #1888).
                    self.ocr_mirrors[index] = None;
                    let mut mirror = Mirror {
                        view: std::ptr::null_mut(),
                        release,
                    };
                    if get_mirror(eye, device.raw_device(), &mut mirror.view) != 0
                        || mirror.view.is_null()
                    {
                        return Err("Could not acquire SteamVR eye mirror".into());
                    }
                    let view = mirror.view;
                    self.ocr_mirrors[index] = Some(mirror);
                    let mut eye_pose: openvr_sys::TrackedDevicePose_t = std::mem::zeroed();
                    if pose_fn(
                        tracked_device_index::HMD.0,
                        &mut eye_pose,
                        std::ptr::null_mut(),
                    ) != 0
                        || !eye_pose.bPoseIsValid
                        || !eye_pose.bDeviceIsConnected
                    {
                        return Err(
                            "Headset tracking unavailable during capture; scan again".into()
                        );
                    }
                    if !pose_within_translation_limit_m(
                        pose.mDeviceToAbsoluteTracking.m,
                        eye_pose.mDeviceToAbsoluteTracking.m,
                        OCR_MOVEMENT_LIMIT_M,
                    ) {
                        return Err("Headset position changed during capture; scan again".into());
                    }
                    let mut projection = [0.; 4];
                    system
                        .GetProjectionRaw
                        .ok_or("Eye projection query unavailable")?(
                        eye,
                        &mut projection[0],
                        &mut projection[1],
                        &mut projection[2],
                        &mut projection[3],
                    );
                    let eye_to_head =
                        system
                            .GetEyeToHeadTransform
                            .ok_or("Eye transform query unavailable")?(eye)
                        .m;
                    let mut eye_capture = EyeCapture {
                        image: Texture {
                            width: 0,
                            height: 0,
                            pixels: Vec::new(),
                        },
                        projection: projection_from_openvr(projection),
                        eye_to_head,
                        head_pose: eye_pose.mDeviceToAbsoluteTracking.m,
                    };
                    if eye_capture
                        .projection
                        .iter()
                        .any(|value| !value.is_finite())
                        || eye_capture.projection[0] >= eye_capture.projection[1]
                        || eye_capture.projection[2] >= eye_capture.projection[3]
                        || eye_capture
                            .eye_to_head
                            .iter()
                            .flatten()
                            .any(|value| !value.is_finite())
                    {
                        return Err("Invalid eye projection".into());
                    }
                    let (image, bounds, size) =
                        device.read_shader_resource_region(view, |width, height| {
                            eye_capture.image.width = width;
                            eye_capture.image.height = height;
                            if let Some(selection) = selection {
                                selection
                                    .crop_bounds(&eye_capture)
                                    .ok_or_else(|| "OCR frame left the view before capture".into())
                            } else {
                                center_bounds(width, height, config.region_fraction)
                            }
                        })?;
                    eye_capture.projection = crop_projection(eye_capture.projection, size, bounds);
                    eye_capture.image = image;
                    eyes.push(eye_capture);
                }
                let mut after: openvr_sys::TrackedDevicePose_t = std::mem::zeroed();
                if pose_fn(
                    tracked_device_index::HMD.0,
                    &mut after,
                    std::ptr::null_mut(),
                ) != 0
                    || !after.bPoseIsValid
                    || !after.bDeviceIsConnected
                    || !pose_within_translation_limit_m(
                        pose.mDeviceToAbsoluteTracking.m,
                        after.mDeviceToAbsoluteTracking.m,
                        OCR_MOVEMENT_LIMIT_M,
                    )
                    || compositor
                        .GetLastFrameRenderer
                        .ok_or("Scene renderer query unavailable")?()
                        != scene_pid
                    || compositor
                        .GetTrackingSpace
                        .ok_or("Tracking space query unavailable")?()
                        != origin
                {
                    return Err(
                        "Headset position or scene changed during capture; scan again".into(),
                    );
                }
                Ok(StereoCapture {
                    eyes,
                    #[cfg(test)]
                    pose: after.mDeviceToAbsoluteTracking.m,
                    scene_pid,
                    captured_at,
                    origin,
                })
            }
        }
        pub fn runtime_installed() -> bool {
            openvr::is_runtime_installed()
        }

        pub fn hmd_present() -> bool {
            openvr::is_hmd_present()
        }

        pub fn connect() -> Result<Self, String> {
            let context = unsafe { openvr::init(ApplicationType::Overlay) }
                .map_err(|error| format!("OpenVR initialization failed: {error:?}"))?;
            let system = context
                .system()
                .map_err(|error| format!("OpenVR system interface failed: {error:?}"))?;
            let overlay = context
                .overlay()
                .map_err(|error| format!("OpenVR overlay interface failed: {error:?}"))?;
            let raw_overlay = load_raw_overlay()?;
            let raw_system = load_raw_system()?;
            let texture_device = create_texture_device();
            let cursor_interval = crate::vr_overlay::cursor::frame_interval(
                system
                    .float_tracked_device_property(
                        tracked_device_index::HMD,
                        openvr::property::DisplayFrequency_Float,
                    )
                    .unwrap_or(90.),
            );
            Ok(Self {
                context,
                system,
                raw_system,
                overlay,
                raw_overlay,
                texture_device,
                ocr_mirrors: [None, None],
                headset: None,
                wrist: None,
                wrist_menu: None,
                tracking_reset: false,
                ocr_wrist: None,
                ocr_progress: None,
                ocr_wrist_device: None,
                ocr_frame: None,
                ocr_result: None,
                dashboard: DashboardOverlay::default(),
                headset_state: SubmittedState::default(),
                wrist_state: SubmittedState::default(),
                wrist_menu_state: SubmittedState::default(),
                ocr_wrist_state: SubmittedState::default(),
                ocr_progress_state: SubmittedState::default(),
                ocr_frame_state: SubmittedState::default(),
                ocr_result_state: SubmittedState::default(),
                dashboard_state: SubmittedState::default(),
                ocr_input: None,
                cursor: Default::default(),
                cursor_interval,
            })
        }

        pub fn quit_requested(&mut self) -> bool {
            let (quit, reset) = poll_runtime_events(unsafe { &*self.raw_system });
            self.tracking_reset |= reset;
            quit
        }

        pub fn ensure_dashboard(&mut self) -> Result<(), String> {
            self.dashboard.ensure(unsafe { &*self.raw_overlay })
        }

        pub fn dashboard_visible(&self) -> bool {
            self.dashboard
                .main()
                .and_then(|handle| unsafe {
                    (&*self.raw_overlay).IsOverlayVisible.map(|f| f(handle.0))
                })
                .unwrap_or(false)
        }

        pub fn poll_dashboard_events(&self) -> Result<Vec<DashboardPointerEvent>, String> {
            let Some(handle) = self.dashboard.main() else {
                return Ok(Vec::new());
            };
            let poll = unsafe { (&*self.raw_overlay).PollNextOverlayEvent }
                .ok_or("SteamVR dashboard event polling unavailable")?;
            let mut events = Vec::new();
            loop {
                let mut event = openvr_sys::VREvent_t::default();
                if !unsafe {
                    poll(
                        handle.0,
                        &mut event,
                        std::mem::size_of::<openvr_sys::VREvent_t>() as u32,
                    )
                } {
                    break;
                }
                let pointer = match event.eventType as i32 {
                    openvr_sys::EVREventType_VREvent_MouseMove => {
                        let mouse = unsafe { event.data.mouse };
                        let (x, y) = pointer_from_openvr(mouse.x, mouse.y, DASHBOARD_HEIGHT);
                        Some(DashboardPointerEvent::Move { x, y })
                    }
                    openvr_sys::EVREventType_VREvent_MouseButtonDown => {
                        let mouse = unsafe { event.data.mouse };
                        let (x, y) = pointer_from_openvr(mouse.x, mouse.y, DASHBOARD_HEIGHT);
                        (mouse.button & openvr_sys::EVRMouseButton_VRMouseButton_Left as u32 != 0)
                            .then_some(DashboardPointerEvent::Down { x, y })
                    }
                    openvr_sys::EVREventType_VREvent_MouseButtonUp => {
                        let mouse = unsafe { event.data.mouse };
                        let (x, y) = pointer_from_openvr(mouse.x, mouse.y, DASHBOARD_HEIGHT);
                        (mouse.button & openvr_sys::EVRMouseButton_VRMouseButton_Left as u32 != 0)
                            .then_some(DashboardPointerEvent::Up { x, y })
                    }
                    openvr_sys::EVREventType_VREvent_OverlayShown => {
                        Some(DashboardPointerEvent::Shown)
                    }
                    openvr_sys::EVREventType_VREvent_OverlayHidden => {
                        Some(DashboardPointerEvent::Hidden)
                    }
                    _ => None,
                };
                if let Some(event) = pointer {
                    events.push(event);
                }
            }
            Ok(events)
        }

        pub fn ensure_headset(&mut self, config: &VrOverlayHeadsetConfig) -> Result<(), String> {
            if self.headset.is_none() {
                let handle = self
                    .overlay
                    .create_overlay(HEADSET_KEY, HEADSET_NAME)
                    .map_err(|error| format!("Create headset overlay failed: {error:?}"))?;
                self.headset = Some(handle);
            }
            let handle = self.headset.expect("headset overlay exists");
            self.overlay
                .set_width(handle, config.width_m)
                .map_err(|error| format!("Configure headset overlay failed: {error:?}"))?;
            let matrix = Matrix3x4(transform::headset(config));
            self.overlay
                .set_transform_tracked_device_relative(handle, tracked_device_index::HMD, &matrix)
                .map_err(|error| format!("Position headset overlay failed: {error:?}"))
        }

        pub fn ensure_ocr_progress(&mut self) -> Result<(), String> {
            if self.ocr_progress.is_none() {
                self.ocr_progress = Some(
                    self.overlay
                        .create_overlay("org.vrcs.overlay.ocr.progress\0", "VRCS OCR Progress\0")
                        .map_err(|error| {
                            format!("Create OCR progress overlay failed: {error:?}")
                        })?,
                );
                let handle = self.ocr_progress.expect("OCR progress overlay exists");
                self.overlay
                    .set_width(handle, 0.32)
                    .map_err(|error| format!("Configure OCR progress overlay failed: {error:?}"))?;
                let matrix = Matrix3x4(transform::headset(&VrOverlayHeadsetConfig {
                    distance_m: 1.2,
                    offset_y_m: -0.42,
                    pitch_deg: -19.,
                    ..Default::default()
                }));
                self.overlay
                    .set_transform_tracked_device_relative(
                        handle,
                        tracked_device_index::HMD,
                        &matrix,
                    )
                    .map_err(|error| format!("Position OCR progress overlay failed: {error:?}"))?;
            }
            Ok(())
        }

        pub fn ensure_ocr_plane(
            &mut self,
            kind: OverlayKind,
            plane: &crate::vr_overlay::ocr_plane::PlaneOverlay,
            origin: i32,
        ) -> Result<(), String> {
            let (slot, key, name) = match kind {
                OverlayKind::OcrFrame => (
                    &mut self.ocr_frame,
                    "org.vrcs.overlay.ocr.frame\0",
                    "VRCS OCR Frame\0",
                ),
                OverlayKind::OcrResult => (
                    &mut self.ocr_result,
                    "org.vrcs.overlay.ocr.result\0",
                    "VRCS OCR Translation\0",
                ),
                _ => return Err("Invalid OCR overlay kind".into()),
            };
            if slot.is_none() {
                *slot = Some(
                    self.overlay
                        .create_overlay(key, name)
                        .map_err(|error| format!("Create OCR overlay failed: {error:?}"))?,
                );
            }
            let handle = slot.expect("OCR overlay exists");
            self.overlay
                .set_width(handle, plane.width_m)
                .map_err(|error| format!("Configure OCR width failed: {error:?}"))?;
            self.overlay
                .set_texel_aspect(handle, plane.texel_aspect)
                .map_err(|error| format!("Configure OCR aspect failed: {error:?}"))?;
            let mut matrix = openvr_sys::HmdMatrix34_t { m: plane.pose };
            let error = unsafe {
                (*self.raw_overlay)
                    .SetOverlayTransformAbsolute
                    .ok_or("OCR absolute overlays are unavailable")?(
                    handle.0, origin, &mut matrix
                )
            };
            if error != openvr_sys::EVROverlayError_VROverlayError_None {
                return Err(format!("Position OCR overlay failed: {error}"));
            }
            Ok(())
        }

        pub fn ensure_wrist(
            &mut self,
            config: &VrOverlayWristConfig,
        ) -> Result<ControllerBinding, String> {
            self.ensure_controller_overlay(OverlayKind::Wrist, config)
        }

        pub fn ensure_ocr_wrist(
            &mut self,
            config: &VrOcrWristConfig,
            interactive: bool,
        ) -> Result<ControllerBinding, String> {
            let binding =
                self.ensure_controller_overlay(OverlayKind::OcrWrist, &config.overlay_config())?;
            if binding.available {
                if self.ocr_wrist_state.interactive.is_none() {
                    configure_ocr_wrist_input(
                        unsafe { &*self.raw_overlay },
                        self.ocr_wrist.ok_or("OCR wrist overlay unavailable")?,
                    )?;
                }
                self.ocr_wrist_state.interactive = Some(interactive);
            }
            Ok(binding)
        }

        pub fn ocr_wrist_on_same_hand(&self, config: &VrOverlayWristConfig) -> bool {
            self.system
                .tracked_device_index_for_controller_role(controller_role(config).1)
                .is_some_and(|device| Some(device.0) == self.ocr_wrist_device)
        }

        fn ensure_controller_overlay(
            &mut self,
            kind: OverlayKind,
            config: &VrOverlayWristConfig,
        ) -> Result<ControllerBinding, String> {
            let (role_name, role) = controller_role(config);
            let Some(device) = self.system.tracked_device_index_for_controller_role(role) else {
                self.hide(kind);
                return Ok(ControllerBinding {
                    role: Some(role_name),
                    available: false,
                });
            };
            if !self.system.is_tracked_device_connected(device)
                || self.controller_pose(device.0).is_none()
            {
                self.hide(kind);
                return Ok(ControllerBinding {
                    role: Some(role_name),
                    available: false,
                });
            }

            let (slot, key, name) = if kind == OverlayKind::OcrWrist {
                self.ocr_wrist_device = Some(device.0);
                (
                    &mut self.ocr_wrist,
                    "org.vrcs.overlay.ocr.wrist\0",
                    "VRCS OCR Reader\0",
                )
            } else if kind == OverlayKind::WristMenu {
                (
                    &mut self.wrist_menu,
                    "org.vrcs.overlay.wrist.menu\0",
                    "VRCS Wrist Menu\0",
                )
            } else {
                (&mut self.wrist, WRIST_KEY, WRIST_NAME)
            };
            if slot.is_none() {
                let handle = self
                    .overlay
                    .create_overlay(key, name)
                    .map_err(|error| format!("Create wrist overlay failed: {error:?}"))?;
                configure_ocr_wrist_input(unsafe { &*self.raw_overlay }, handle)?;
                *slot = Some(handle);
            }
            let handle = slot.expect("controller overlay exists");
            self.overlay
                .set_width(handle, config.width_m)
                .map_err(|error| format!("Configure wrist overlay failed: {error:?}"))?;
            let matrix = Matrix3x4(transform::wrist(config));
            self.overlay
                .set_transform_tracked_device_relative(handle, device, &matrix)
                .map_err(|error| format!("Position wrist overlay failed: {error:?}"))?;
            Ok(ControllerBinding {
                role: Some(role_name),
                available: true,
            })
        }

        pub fn controller_pose(&self, device: u32) -> Option<([[f32; 4]; 3], i32)> {
            let table = load_raw_interface(openvr_sys::IVRCompositor_Version, "compositor").ok()?
                as *const openvr_sys::VR_IVRCompositor_FnTable;
            let table = unsafe { &*table };
            let mut pose: openvr_sys::TrackedDevicePose_t = unsafe { std::mem::zeroed() };
            let error = unsafe {
                table.GetLastPoseForTrackedDeviceIndex?(device, &mut pose, std::ptr::null_mut())
            };
            if error != 0 || !pose.bPoseIsValid || !pose.bDeviceIsConnected {
                return None;
            }
            Some((pose.mDeviceToAbsoluteTracking.m, unsafe {
                table.GetTrackingSpace?()
            }))
        }

        pub fn controller_pointer_pose(
            &self,
            device: openvr::TrackedDeviceIndex,
            source: u64,
        ) -> Option<([[f32; 4]; 3], i32)> {
            let (pose, origin) = self.controller_pose(device.0)?;
            // The raw tracking axis is not the pointing axis on every controller.
            let tip = (|| {
                let model = self
                    .system
                    .string_tracked_device_property(
                        device,
                        openvr::property::RenderModelName_String,
                    )
                    .ok()?;
                let models =
                    load_raw_interface(openvr_sys::IVRRenderModels_Version, "render models").ok()?
                        as *const openvr_sys::VR_IVRRenderModels_FnTable;
                let mut mode: openvr_sys::RenderModel_ControllerMode_State_t =
                    unsafe { std::mem::zeroed() };
                let mut tip: openvr_sys::RenderModel_ComponentState_t =
                    unsafe { std::mem::zeroed() };
                let found = unsafe {
                    (*models).GetComponentStateForDevicePath?(
                        model.as_ptr().cast_mut(),
                        c"tip".as_ptr().cast_mut(),
                        source,
                        &mut mode,
                        &mut tip,
                    )
                };
                found.then_some(tip.mTrackingToComponentLocal.m)
            })();
            Some((
                tip.map(|tip| transform::compose(pose, tip)).unwrap_or(pose),
                origin,
            ))
        }

        pub fn tracking_reset(&mut self) -> bool {
            std::mem::take(&mut self.tracking_reset)
        }

        pub fn system_dashboard_visible(&mut self) -> bool {
            self.overlay.is_dashboard_visible()
        }

        pub fn visible(&self, kind: OverlayKind) -> bool {
            self.state(kind).visible
        }

        pub fn controller_device(&self, config: &VrOverlayWristConfig) -> Option<u32> {
            let device = self
                .system
                .tracked_device_index_for_controller_role(controller_role(config).1)?;
            self.controller_pose(device.0)?;
            Some(device.0)
        }

        pub fn wrist_pose(&self, config: &VrOverlayWristConfig) -> Option<[[f32; 4]; 3]> {
            let (controller, _) = self.controller_pose(self.controller_device(config)?)?;
            Some(transform::compose(controller, transform::wrist(config)))
        }

        pub fn wrist_angle(&self, config: &VrOverlayWristConfig) -> Option<f32> {
            let panel = self.wrist_pose(config)?;
            let (head, _, _) = self.ocr_tracking().ok()?;
            let toward: [f32; 3] = std::array::from_fn(|i| head[i][3] - panel[i][3]);
            let length = toward.iter().map(|v| v * v).sum::<f32>().sqrt();
            (length > 0.001).then(|| {
                (toward
                    .iter()
                    .enumerate()
                    .map(|(i, v)| v * panel[i][2])
                    .sum::<f32>()
                    / length)
                    .clamp(-1., 1.)
                    .acos()
                    .to_degrees()
            })
        }

        pub fn ensure_wrist_menu(&mut self, hand: &str) -> Result<(), String> {
            if self
                .ensure_controller_overlay(OverlayKind::WristMenu, &menu_config(hand))?
                .available
            {
                Ok(())
            } else {
                Err("Controller tracking unavailable".into())
            }
        }

        pub fn ensure_world_wrist(
            &mut self,
            config: &VrOverlayWristConfig,
            pose: [[f32; 4]; 3],
            origin: i32,
            head_relative: Option<[[f32; 4]; 3]>,
        ) -> Result<(), String> {
            if self.wrist.is_none() {
                let handle = self
                    .overlay
                    .create_overlay(WRIST_KEY, WRIST_NAME)
                    .map_err(|e| format!("Create dialogue overlay failed: {e:?}"))?;
                configure_ocr_wrist_input(unsafe { &*self.raw_overlay }, handle)?;
                self.wrist = Some(handle);
            }
            let handle = self.wrist.expect("dialogue overlay");
            self.overlay
                .set_width(handle, config.width_m)
                .map_err(|e| format!("Set dialogue width failed: {e:?}"))?;
            if let Some(relative) = head_relative {
                return self
                    .overlay
                    .set_transform_tracked_device_relative(
                        handle,
                        tracked_device_index::HMD,
                        &Matrix3x4(relative),
                    )
                    .map_err(|e| format!("Set dialogue follow transform failed: {e:?}"));
            }
            let mut matrix = openvr_sys::HmdMatrix34_t { m: pose };
            check_wrist_input(unsafe {
                (*self.raw_overlay)
                    .SetOverlayTransformAbsolute
                    .ok_or("Absolute overlays unavailable")?(
                    handle.0, origin, &mut matrix
                )
            })
        }

        pub fn ui_inputs(&self) -> Vec<crate::vr_overlay::ocr_input::UiInput> {
            self.ocr_input
                .as_ref()
                .map(|input| input.ui_samples())
                .unwrap_or_default()
        }

        pub fn intersect(
            &self,
            kind: OverlayKind,
            device: u32,
            ray: ([[f32; 4]; 3], i32),
        ) -> Option<([f32; 2], [f32; 3], f32)> {
            if !self.state(kind).visible {
                return None;
            }
            if kind == OverlayKind::OcrWrist
                && (self.ocr_wrist_device == Some(device)
                    || self.ocr_wrist_state.interactive != Some(true))
            {
                return None;
            }
            let handle = self.handle(kind)?;
            let (pose, origin) = ray;
            let size = match kind {
                OverlayKind::WristMenu => SurfaceId::Menu.size(),
                OverlayKind::Wrist => SurfaceId::Dialogue.size(),
                OverlayKind::OcrWrist => SurfaceId::Reader.size(),
                _ => return None,
            };
            wrist_ray_hit(unsafe { &*self.raw_overlay }, handle, origin, pose, size)
        }

        pub fn cursor_interval(&self) -> std::time::Duration {
            self.cursor_interval
        }

        pub fn update_cursor(
            &mut self,
            target: Option<(OverlayKind, [f32; 3], i32, u32)>,
        ) -> Result<(), String> {
            let api = unsafe { &*self.raw_overlay };
            if let Some((kind, position, origin, width)) = target {
                if self.visible(kind) {
                    if let Some(handle) = self.handle(kind) {
                        let diameter_px = if kind == OverlayKind::WristMenu {
                            30
                        } else {
                            24
                        };
                        self.cursor
                            .update(api, handle.0, position, origin, width, diameter_px)?;
                        return Ok(());
                    }
                }
            }
            self.cursor.hide(api)
        }

        pub fn surface_adapter_index(&self) -> Result<u32, String> {
            dxgi_adapter_index()
        }

        pub fn upload_surface(
            &mut self,
            kind: OverlayKind,
            shared_handle: usize,
        ) -> Result<(), String> {
            let handle = self.handle(kind).ok_or("Overlay is not created")?;
            submit_shared_texture(
                self.raw_overlay,
                handle,
                shared_handle as *mut std::ffi::c_void,
            )
        }

        pub fn clear_surface_texture(&mut self, kind: OverlayKind) -> Result<(), String> {
            let Some(handle) = self.handle(kind) else {
                return Ok(());
            };
            let table = unsafe { &*self.raw_overlay };
            let clear = table
                .ClearOverlayTexture
                .ok_or("ClearOverlayTexture is unavailable")?;
            let error = unsafe { clear(handle.0) };
            if error != openvr_sys::EVROverlayError_VROverlayError_None {
                return Err(format!("Clear surface texture failed: {error}"));
            }
            // Keep the handle, placement, and dashboard registration intact.
            Ok(())
        }

        pub fn set_opacity(&mut self, kind: OverlayKind, opacity: f32) -> Result<(), String> {
            let opacity = opacity.clamp(0.0, 1.0);
            if self.state(kind).opacity == Some(opacity) {
                return Ok(());
            }
            let handle = self
                .handle(kind)
                .ok_or_else(|| "Overlay is not created".to_string())?;
            self.overlay
                .set_opacity(handle, opacity)
                .map_err(|error| format!("Set overlay opacity failed: {error:?}"))?;
            self.state_mut(kind).opacity = Some(opacity);
            Ok(())
        }

        pub fn show(&mut self, kind: OverlayKind) -> Result<(), String> {
            if self.state(kind).visible {
                return Ok(());
            }
            let handle = self
                .handle(kind)
                .ok_or_else(|| "Overlay is not created".to_string())?;
            self.overlay
                .set_visibility(handle, true)
                .map_err(|error| format!("Show overlay failed: {error:?}"))?;
            self.state_mut(kind).visible = true;
            tracing::info!(?kind, "VR Overlay shown");
            Ok(())
        }

        pub fn hide(&mut self, kind: OverlayKind) {
            if self.cursor.target.is_some() && self.cursor.target == self.handle(kind).map(|h| h.0)
            {
                let _ = self.cursor.hide(unsafe { &*self.raw_overlay });
            }
            if !self.state(kind).visible {
                return;
            }
            if let Some(handle) = self.handle(kind) {
                if self.overlay.set_visibility(handle, false).is_ok() {
                    self.state_mut(kind).visible = false;
                }
            }
        }

        pub fn hide_all(&mut self) {
            let _ = self.cursor.hide(unsafe { &*self.raw_overlay });
            self.hide(OverlayKind::Headset);
            self.hide(OverlayKind::Wrist);
            self.hide(OverlayKind::WristMenu);
            self.hide(OverlayKind::OcrWrist);
            self.hide(OverlayKind::OcrProgress);
            self.hide(OverlayKind::OcrFrame);
            self.hide(OverlayKind::OcrResult);
        }

        pub fn reset(&mut self, kind: OverlayKind) {
            if kind == OverlayKind::Dashboard {
                // Surface recovery must not unregister the SteamVR dashboard tab.
                self.dashboard.clear_texture(unsafe { &*self.raw_overlay });
                self.dashboard_state = SubmittedState::default();
                return;
            }
            self.hide(kind);
            self.destroy(kind);
        }

        fn handle(&self, kind: OverlayKind) -> Option<OverlayHandle> {
            match kind {
                OverlayKind::Headset => self.headset,
                OverlayKind::Wrist => self.wrist,
                OverlayKind::WristMenu => self.wrist_menu,
                OverlayKind::OcrWrist => self.ocr_wrist,
                OverlayKind::OcrProgress => self.ocr_progress,
                OverlayKind::OcrFrame => self.ocr_frame,
                OverlayKind::OcrResult => self.ocr_result,
                OverlayKind::Dashboard => self.dashboard.main(),
            }
        }

        fn state(&self, kind: OverlayKind) -> &SubmittedState {
            match kind {
                OverlayKind::Headset => &self.headset_state,
                OverlayKind::Wrist => &self.wrist_state,
                OverlayKind::WristMenu => &self.wrist_menu_state,
                OverlayKind::OcrWrist => &self.ocr_wrist_state,
                OverlayKind::OcrProgress => &self.ocr_progress_state,
                OverlayKind::OcrFrame => &self.ocr_frame_state,
                OverlayKind::OcrResult => &self.ocr_result_state,
                OverlayKind::Dashboard => &self.dashboard_state,
            }
        }

        fn state_mut(&mut self, kind: OverlayKind) -> &mut SubmittedState {
            match kind {
                OverlayKind::Headset => &mut self.headset_state,
                OverlayKind::Wrist => &mut self.wrist_state,
                OverlayKind::WristMenu => &mut self.wrist_menu_state,
                OverlayKind::OcrWrist => &mut self.ocr_wrist_state,
                OverlayKind::OcrProgress => &mut self.ocr_progress_state,
                OverlayKind::OcrFrame => &mut self.ocr_frame_state,
                OverlayKind::OcrResult => &mut self.ocr_result_state,
                OverlayKind::Dashboard => &mut self.dashboard_state,
            }
        }

        fn destroy(&mut self, kind: OverlayKind) {
            let handle = match kind {
                OverlayKind::Headset => self.headset.take(),
                OverlayKind::Wrist => self.wrist.take(),
                OverlayKind::WristMenu => self.wrist_menu.take(),
                OverlayKind::OcrWrist => {
                    self.ocr_wrist_device = None;
                    self.ocr_wrist.take()
                }
                OverlayKind::OcrFrame => self.ocr_frame.take(),
                OverlayKind::OcrProgress => self.ocr_progress.take(),
                OverlayKind::OcrResult => self.ocr_result.take(),
                OverlayKind::Dashboard => {
                    self.dashboard.destroy(unsafe { &*self.raw_overlay });
                    None
                }
            };
            if let Some(handle) = handle {
                unsafe {
                    let table = &*self.raw_overlay;
                    if let Some(clear) = table.ClearOverlayTexture {
                        let _ = clear(handle.0);
                    }
                    if let Some(destroy) = table.DestroyOverlay {
                        let _ = destroy(handle.0);
                    }
                }
            }
            *self.state_mut(kind) = SubmittedState::default();
        }
    }

    impl Drop for OpenVrBackend {
        fn drop(&mut self) {
            self.ocr_mirrors = [None, None];
            self.hide_all();
            self.destroy(OverlayKind::Headset);
            self.destroy(OverlayKind::Wrist);
            self.destroy(OverlayKind::WristMenu);
            self.destroy(OverlayKind::OcrWrist);
            self.destroy(OverlayKind::OcrProgress);
            self.destroy(OverlayKind::OcrFrame);
            self.destroy(OverlayKind::OcrResult);
            self.destroy(OverlayKind::Dashboard);
            self.cursor.destroy(unsafe { &*self.raw_overlay });
            // Detach while graphics resources are still alive; keep the desktop process running.
            unsafe { self.context.shutdown() };
        }
    }

    fn poll_runtime_events(system: &openvr_sys::VR_IVRSystem_FnTable) -> (bool, bool) {
        let mut reset = false;
        let poll = system
            .PollNextEvent
            .expect("OpenVR event polling available");
        loop {
            let mut event = openvr_sys::VREvent_t::default();
            if !unsafe { poll(&mut event, std::mem::size_of_val(&event) as u32) } {
                return (false, reset);
            }
            reset |= matches!(
                event.eventType as i32,
                openvr_sys::EVREventType_VREvent_SeatedZeroPoseReset
                    | openvr_sys::EVREventType_VREvent_ChaperoneUniverseHasChanged
                    | openvr_sys::EVREventType_VREvent_ChaperoneDataHasChanged
            );
            if event.eventType == openvr_sys::EVREventType_VREvent_Quit as u32 {
                unsafe {
                    system
                        .AcknowledgeQuit_Exiting
                        .expect("OpenVR quit acknowledgement available")();
                }
                return (true, reset);
            }
        }
    }

    fn create_texture_device() -> Option<Device> {
        let adapter_index = match dxgi_adapter_index() {
            Ok(index) => index,
            Err(error) => {
                tracing::warn!(error, "Unable to select SteamVR GPU for OCR capture");
                return None;
            }
        };

        match Device::create(adapter_index) {
            Ok(device) => {
                tracing::info!(adapter_index, "OCR capture D3D11 device initialized");
                Some(device)
            }
            Err(error) => {
                tracing::warn!(error, adapter_index, "D3D11 OCR capture unavailable");
                None
            }
        }
    }

    fn set_wrist_interactive(
        api: &openvr_sys::VR_IVROverlay_FnTable,
        handle: OverlayHandle,
        focused: bool,
    ) -> Result<(), String> {
        let error = unsafe {
            api.SetOverlayFlag
                .ok_or("SteamVR OCR wrist flags unavailable")?(
                handle.0,
                openvr_sys::VROverlayFlags_MakeOverlaysInteractiveIfVisible,
                focused,
            )
        };
        check_wrist_input(error)
    }

    fn check_wrist_input(error: i32) -> Result<(), String> {
        if error == openvr_sys::EVROverlayError_VROverlayError_None {
            Ok(())
        } else {
            Err(format!(
                "Configure OCR wrist input failed with code {error}"
            ))
        }
    }

    fn configure_ocr_wrist_input(
        api: &openvr_sys::VR_IVROverlay_FnTable,
        handle: OverlayHandle,
    ) -> Result<(), String> {
        unsafe {
            check_wrist_input(api
                .SetOverlayInputMethod
                .ok_or("SteamVR OCR wrist input mode unavailable")?(
                handle.0,
                openvr_sys::VROverlayInputMethod_None,
            ))?;
        }
        set_wrist_interactive(api, handle, false)
    }

    fn wrist_ray_hit(
        overlay: &openvr_sys::VR_IVROverlay_FnTable,
        handle: OverlayHandle,
        origin: i32,
        matrix: [[f32; 4]; 3],
        texture_size: (u32, u32),
    ) -> Option<([f32; 2], [f32; 3], f32)> {
        let intersection = overlay.ComputeOverlayIntersection?;
        let mut ray = openvr_sys::VROverlayIntersectionParams_t {
            vSource: openvr_sys::HmdVector3_t {
                v: std::array::from_fn(|i| matrix[i][3]),
            },
            vDirection: openvr_sys::HmdVector3_t {
                v: std::array::from_fn(|i| -matrix[i][2]),
            },
            eOrigin: origin,
        };
        let mut hit: openvr_sys::VROverlayIntersectionResults_t = unsafe { std::mem::zeroed() };
        let found = unsafe { intersection(handle.0, &mut ray, &mut hit) };
        if !found
            || !hit.fDistance.is_finite()
            || hit.fDistance < 0.
            || !hit.vPoint.v.iter().all(|v| v.is_finite())
        {
            return None;
        }
        let mut panel = openvr_sys::HmdMatrix34_t { m: [[0.; 4]; 3] };
        let mut width = 0.;
        unsafe {
            check_wrist_input(overlay.GetTransformForOverlayCoordinates?(
                handle.0,
                origin,
                openvr_sys::HmdVector2_t { v: [0.5, 0.5] },
                &mut panel,
            ))
            .ok()?;
            check_wrist_input(overlay.GetOverlayWidthInMeters?(handle.0, &mut width)).ok()?;
        }
        if !width.is_finite() || width <= 0. || texture_size.0 == 0 || texture_size.1 == 0 {
            return None;
        }
        // OpenVR's input UVs can use a different aspect ratio from the visible texture.
        let local: [f32; 2] = std::array::from_fn(|axis| {
            (0..3)
                .map(|i| (hit.vPoint.v[i] - panel.m[i][3]) * panel.m[i][axis])
                .sum()
        });
        let height = width * texture_size.1 as f32 / texture_size.0 as f32;
        let point = [0.5 + local[0] / width, 0.5 - local[1] / height];
        point
            .iter()
            .all(|v| v.is_finite() && (0. ..=1.).contains(v))
            .then_some((point, hit.vPoint.v, hit.fDistance))
    }

    fn dxgi_adapter_index() -> Result<u32, String> {
        let raw_system = load_raw_system()?;
        let mut adapter_index = -1;
        unsafe {
            (&*raw_system)
                .GetDXGIOutputInfo
                .ok_or_else(|| "OpenVR GetDXGIOutputInfo is unavailable".to_string())?(
                &mut adapter_index,
            );
        }
        u32::try_from(adapter_index)
            .map_err(|_| "OpenVR did not provide a DXGI adapter".to_string())
    }

    fn submit_shared_texture(
        raw_overlay: *const openvr_sys::VR_IVROverlay_FnTable,
        handle: OverlayHandle,
        shared_handle: *mut std::ffi::c_void,
    ) -> Result<(), String> {
        let api = unsafe { &*raw_overlay };
        let error = unsafe {
            api.SetOverlayFlag
                .ok_or("OpenVR overlay alpha mode is unavailable")?(
                handle.0,
                openvr_sys::VROverlayFlags_IsPremultiplied,
                true,
            )
        };
        if error != openvr_sys::EVROverlayError_VROverlayError_None {
            return Err(format!("Set overlay alpha mode failed with code {error}"));
        }
        let mut texture = openvr_sys::Texture_t {
            handle: shared_handle,
            eType: openvr_sys::ETextureType_TextureType_DXGISharedHandle,
            eColorSpace: openvr_sys::EColorSpace_ColorSpace_Auto,
        };
        let error = unsafe {
            api.SetOverlayTexture
                .ok_or_else(|| "OpenVR SetOverlayTexture is unavailable".to_string())?(
                handle.0,
                &mut texture,
            )
        };
        if error == openvr_sys::EVROverlayError_VROverlayError_None {
            Ok(())
        } else {
            Err(format!("SetOverlayTexture failed with code {error}"))
        }
    }

    fn controller_role(config: &VrOverlayWristConfig) -> (String, TrackedControllerRole) {
        let role = if config.hand == "dominant" {
            config.dominant_hand.as_str()
        } else {
            config.hand.as_str()
        };
        match role {
            "right" => ("right".into(), TrackedControllerRole::RightHand),
            _ => ("left".into(), TrackedControllerRole::LeftHand),
        }
    }

    fn load_raw_system() -> Result<*const openvr_sys::VR_IVRSystem_FnTable, String> {
        load_raw_interface(openvr_sys::IVRSystem_Version, "system")
            .map(|pointer| pointer as *const openvr_sys::VR_IVRSystem_FnTable)
    }

    fn load_raw_overlay() -> Result<*const openvr_sys::VR_IVROverlay_FnTable, String> {
        load_raw_interface(openvr_sys::IVROverlay_Version, "overlay")
            .map(|pointer| pointer as *const openvr_sys::VR_IVROverlay_FnTable)
    }

    fn load_raw_interface(version: &[u8], name: &str) -> Result<isize, String> {
        let mut interface = Vec::from(b"FnTable:".as_ref());
        interface.extend(version);
        let mut error = openvr_sys::EVRInitError_VRInitError_None;
        let pointer =
            unsafe { openvr_sys::VR_GetGenericInterface(interface.as_ptr().cast(), &mut error) };
        if error != openvr_sys::EVRInitError_VRInitError_None || pointer == 0 {
            return Err(format!(
                "OpenVR raw {name} interface failed with code {error}"
            ));
        }
        Ok(pointer)
    }

    #[cfg(test)]
    mod tests {
        use super::Mirror;
        use std::sync::atomic::{AtomicBool, AtomicI32, AtomicUsize, Ordering};

        mod dashboard {
            use super::super::DashboardOverlay;
            use std::cell::{Cell, RefCell};

            thread_local! {
                static CALLS: RefCell<Vec<(&'static str, u64)>> = const { RefCell::new(Vec::new()) };
                static FAIL: Cell<&'static str> = const { Cell::new("") };
            }

            fn call(name: &'static str, handle: u64) -> i32 {
                CALLS.with_borrow_mut(|calls| calls.push((name, handle)));
                if FAIL.get() == name {
                    10
                } else {
                    0
                }
            }

            fn api() -> openvr_sys::VR_IVROverlay_FnTable {
                unsafe extern "C" fn create(
                    key: *mut std::ffi::c_char,
                    name: *mut std::ffi::c_char,
                    main: *mut u64,
                    thumbnail: *mut u64,
                ) -> i32 {
                    assert_eq!(
                        unsafe { std::ffi::CStr::from_ptr(key) }.to_bytes(),
                        b"org.vrcs.dashboard.settings"
                    );
                    assert_eq!(
                        unsafe { std::ffi::CStr::from_ptr(name) }.to_bytes(),
                        b"VRCS"
                    );
                    unsafe {
                        *main = 73;
                        *thumbnail = 74;
                    }
                    call("create", 73)
                }
                unsafe extern "C" fn width(handle: u64, width: f32) -> i32 {
                    assert_eq!(width, 2.25);
                    call("width", handle)
                }
                unsafe extern "C" fn input(handle: u64, method: i32) -> i32 {
                    assert_eq!(method, openvr_sys::VROverlayInputMethod_Mouse);
                    call("input", handle)
                }
                unsafe extern "C" fn scale(
                    handle: u64,
                    scale: *mut openvr_sys::HmdVector2_t,
                ) -> i32 {
                    assert_eq!(unsafe { (*scale).v }, [1440., 900.]);
                    call("scale", handle)
                }
                unsafe extern "C" fn thumbnail(
                    handle: u64,
                    data: *mut std::ffi::c_void,
                    width: u32,
                    height: u32,
                    depth: u32,
                ) -> i32 {
                    assert_eq!((width, height, depth), (256, 256, 4));
                    let pixels =
                        unsafe { std::slice::from_raw_parts(data.cast::<u8>(), 256 * 256 * 4) };
                    let (pixels, remainder) = pixels.as_chunks::<4>();
                    assert!(remainder.is_empty());
                    assert!(pixels.iter().any(|pixel| pixel[3] > 0));
                    assert!(pixels.iter().any(|pixel| pixel[3] == 0));
                    call("thumbnail", handle)
                }
                unsafe extern "C" fn clear(handle: u64) -> i32 {
                    call("clear", handle)
                }
                unsafe extern "C" fn destroy(handle: u64) -> i32 {
                    call("destroy", handle)
                }

                CALLS.with_borrow_mut(Vec::clear);
                FAIL.set("");
                let mut api: openvr_sys::VR_IVROverlay_FnTable = unsafe { std::mem::zeroed() };
                api.CreateDashboardOverlay = Some(create);
                api.SetOverlayWidthInMeters = Some(width);
                api.SetOverlayInputMethod = Some(input);
                api.SetOverlayMouseScale = Some(scale);
                api.SetOverlayRaw = Some(thumbnail);
                api.ClearOverlayTexture = Some(clear);
                api.DestroyOverlay = Some(destroy);
                api
            }

            fn calls() -> Vec<(&'static str, u64)> {
                CALLS.with_borrow_mut(std::mem::take)
            }

            #[test]
            fn registers_static_thumbnail_once_and_keeps_tab_during_surface_recovery() {
                let api = api();
                let mut dashboard = DashboardOverlay::default();
                dashboard.ensure(&api).unwrap();
                dashboard.ensure(&api).unwrap();
                assert_eq!(
                    calls(),
                    [
                        ("create", 73),
                        ("width", 73),
                        ("input", 73),
                        ("scale", 73),
                        ("thumbnail", 74)
                    ]
                );

                dashboard.clear_texture(&api);
                dashboard.ensure(&api).unwrap();
                assert_eq!(dashboard.main().unwrap().0, 73);
                assert_eq!(calls(), [("clear", 73)]);

                dashboard.destroy(&api);
                assert!(dashboard.main().is_none());
                assert_eq!(
                    calls(),
                    [
                        ("clear", 73),
                        ("destroy", 73),
                        ("clear", 74),
                        ("destroy", 74)
                    ]
                );
                dashboard.destroy(&api);
                assert!(calls().is_empty());
            }

            #[test]
            fn initialization_failures_release_both_handles_and_retry_registration() {
                for step in ["width", "input", "scale", "thumbnail"] {
                    let api = api();
                    let mut dashboard = DashboardOverlay::default();
                    FAIL.set(step);
                    assert!(dashboard.ensure(&api).is_err());
                    assert!(dashboard.main().is_none());
                    assert!(calls().ends_with(&[
                        ("clear", 73),
                        ("destroy", 73),
                        ("clear", 74),
                        ("destroy", 74)
                    ]));

                    FAIL.set("");
                    dashboard.ensure(&api).unwrap();
                    assert_eq!(
                        calls(),
                        [
                            ("create", 73),
                            ("width", 73),
                            ("input", 73),
                            ("scale", 73),
                            ("thumbnail", 74)
                        ]
                    );
                    dashboard.destroy(&api);
                }
            }
        }

        #[test]
        fn shared_textures_configure_alpha_before_submission() {
            use std::cell::Cell;
            thread_local! {
                static ALPHA: Cell<Option<bool>> = const { Cell::new(None) };
                static SUBMITTED: Cell<usize> = const { Cell::new(0) };
            }
            unsafe extern "C" fn flag(handle: u64, flag: i32, enabled: bool) -> i32 {
                assert_eq!(handle, 73);
                assert_eq!(flag, openvr_sys::VROverlayFlags_IsPremultiplied);
                ALPHA.set(Some(enabled));
                0
            }
            unsafe extern "C" fn submit(handle: u64, texture: *mut openvr_sys::Texture_t) -> i32 {
                assert_eq!(handle, 73);
                if ALPHA.get().is_none() {
                    return 10;
                }
                assert_eq!(
                    unsafe { (*texture).eType },
                    openvr_sys::ETextureType_TextureType_DXGISharedHandle
                );
                SUBMITTED.set(SUBMITTED.get() + 1);
                0
            }
            unsafe extern "C" fn rejected(_: u64, _: i32, _: bool) -> i32 {
                10
            }
            let mut api: openvr_sys::VR_IVROverlay_FnTable = unsafe { std::mem::zeroed() };
            api.SetOverlayFlag = Some(flag);
            api.SetOverlayTexture = Some(submit);
            let handle = openvr::overlay::OverlayHandle(73);
            let texture = std::ptr::without_provenance_mut(42);
            ALPHA.set(None);
            super::submit_shared_texture(&api, handle, texture).unwrap();
            assert_eq!(ALPHA.get(), Some(true));
            api.SetOverlayFlag = Some(rejected);
            assert!(super::submit_shared_texture(&api, handle, texture).is_err());
            assert_eq!(SUBMITTED.get(), 1);
        }

        #[test]
        fn quit_is_acknowledged_but_another_process_quitting_is_not() {
            use std::cell::{Cell, RefCell};
            use std::collections::VecDeque;

            thread_local! {
                static EVENTS: RefCell<VecDeque<u32>> = const { RefCell::new(VecDeque::new()) };
                static ACKNOWLEDGED: Cell<usize> = const { Cell::new(0) };
            }
            unsafe extern "C" fn poll(event: *mut openvr_sys::VREvent_t, size: u32) -> bool {
                assert_eq!(size as usize, std::mem::size_of::<openvr_sys::VREvent_t>());
                EVENTS.with_borrow_mut(|events| {
                    let Some(kind) = events.pop_front() else {
                        return false;
                    };
                    unsafe { (*event).eventType = kind };
                    true
                })
            }
            unsafe extern "C" fn acknowledge() {
                ACKNOWLEDGED.set(ACKNOWLEDGED.get() + 1);
            }

            let mut api: openvr_sys::VR_IVRSystem_FnTable = unsafe { std::mem::zeroed() };
            api.PollNextEvent = Some(poll);
            api.AcknowledgeQuit_Exiting = Some(acknowledge);
            EVENTS.with_borrow_mut(|events| {
                events.push_back(openvr_sys::EVREventType_VREvent_ProcessQuit as u32);
            });
            assert!(!super::poll_runtime_events(&api).0);
            assert_eq!(ACKNOWLEDGED.get(), 0);

            EVENTS.with_borrow_mut(|events| {
                events.push_back(openvr_sys::EVREventType_VREvent_TrackedDeviceUpdated as u32);
                events.push_back(openvr_sys::EVREventType_VREvent_Quit as u32);
            });
            assert!(super::poll_runtime_events(&api).0);
            assert_eq!(ACKNOWLEDGED.get(), 1);
            assert!(!super::poll_runtime_events(&api).0);
        }

        #[test]
        fn ordinary_overlay_disables_system_mouse_and_focus() {
            static CONFIGURED: AtomicUsize = AtomicUsize::new(0);
            static METHOD: AtomicI32 = AtomicI32::new(-1);
            static INTERACTIVE: AtomicBool = AtomicBool::new(true);
            unsafe extern "C" fn method(handle: u64, method: i32) -> i32 {
                assert_eq!(handle, 73);
                METHOD.store(method, Ordering::Relaxed);
                CONFIGURED.fetch_or(1, Ordering::Relaxed);
                0
            }
            unsafe extern "C" fn flag(handle: u64, flag: i32, enabled: bool) -> i32 {
                assert_eq!(handle, 73);
                match flag {
                    openvr_sys::VROverlayFlags_MakeOverlaysInteractiveIfVisible => {
                        INTERACTIVE.store(enabled, Ordering::Relaxed);
                        CONFIGURED.fetch_or(2, Ordering::Relaxed);
                    }
                    _ => panic!("unexpected overlay flag"),
                }
                0
            }
            unsafe extern "C" fn rejected(_: u64, _: i32, _: bool) -> i32 {
                10
            }
            let mut api: openvr_sys::VR_IVROverlay_FnTable = unsafe { std::mem::zeroed() };
            api.SetOverlayInputMethod = Some(method);
            api.SetOverlayFlag = Some(flag);
            super::configure_ocr_wrist_input(&api, openvr::overlay::OverlayHandle(73)).unwrap();
            assert_eq!(CONFIGURED.load(Ordering::Relaxed), 3);
            assert!(!INTERACTIVE.load(Ordering::Relaxed));
            assert_eq!(
                METHOD.load(Ordering::Relaxed),
                openvr_sys::VROverlayInputMethod_None
            );
            api.SetOverlayFlag = Some(rejected);
            assert!(
                super::configure_ocr_wrist_input(&api, openvr::overlay::OverlayHandle(73)).is_err()
            );
            api.SetOverlayFlag = None;
            assert!(
                super::configure_ocr_wrist_input(&api, openvr::overlay::OverlayHandle(73)).is_err()
            );
        }

        #[test]
        fn wrist_ray_matches_texture_aspect_and_visible_menu() {
            use crate::vr_overlay::surface::{SurfaceId, SurfaceLayout};
            use std::cell::Cell;
            thread_local! {
                static PANEL: Cell<[[f32; 4]; 3]> = const { Cell::new([[0.; 4]; 3]) };
            }
            unsafe extern "C" fn coordinates(
                _: u64,
                _: i32,
                uv: openvr_sys::HmdVector2_t,
                pose: *mut openvr_sys::HmdMatrix34_t,
            ) -> i32 {
                assert_eq!(uv.v, [0.5, 0.5]);
                unsafe { (*pose).m = PANEL.get() };
                0
            }
            unsafe extern "C" fn width(_: u64, width: *mut f32) -> i32 {
                unsafe { *width = 0.18 };
                0
            }
            unsafe extern "C" fn intersection(
                _: u64,
                ray: *mut openvr_sys::VROverlayIntersectionParams_t,
                hit: *mut openvr_sys::VROverlayIntersectionResults_t,
            ) -> bool {
                let ray = unsafe { &*ray };
                let panel = PANEL.get();
                let dot =
                    |v: [f32; 3], axis: usize| (0..3).map(|i| v[i] * panel[i][axis]).sum::<f32>();
                let offset = std::array::from_fn(|i| panel[i][3] - ray.vSource.v[i]);
                let distance = dot(offset, 2) / dot(ray.vDirection.v, 2);
                let point =
                    std::array::from_fn(|i| ray.vSource.v[i] + distance * ray.vDirection.v[i]);
                let local = std::array::from_fn(|i| point[i] - panel[i][3]);
                // Reproduce SteamVR's square input geometry with default mouse scale.
                let uv = [0.5 + dot(local, 0) / 0.18, 0.5 + dot(local, 1) / 0.18];
                unsafe {
                    (*hit).vPoint.v = point;
                    (*hit).vUVs.v = uv;
                    (*hit).fDistance = distance;
                }
                distance >= 0. && uv.iter().all(|v| (0. ..=1.).contains(v))
            }
            let mut api: openvr_sys::VR_IVROverlay_FnTable = unsafe { std::mem::zeroed() };
            api.GetTransformForOverlayCoordinates = Some(coordinates);
            api.GetOverlayWidthInMeters = Some(width);
            api.ComputeOverlayIntersection = Some(intersection);
            let layout: SurfaceLayout = serde_json::from_value(serde_json::json!({
                "key": { "generation": 1, "revision": 1 },
                "regions": [{ "id": 1, "bounds": [314./1024., 48./256., 474./1024., 208./256.], "circle": true }],
                "menu_bounds": [16./1024., 8./256., 1008./1024., 248./256.],
                "page_count": 1
            })).unwrap();
            layout.validate(SurfaceId::Menu).unwrap();
            for panel in [
                super::transform::matrix(0., 0., 0., [1., 2., 3.]),
                super::transform::matrix(-90., 0., 0., [1., 2., 3.]),
                super::transform::matrix(-35., 25., -10., [1., 2., 3.]),
            ] {
                PANEL.set(panel);
                for (x, y, visible, button) in [
                    (394., -64., false, None),
                    (394., 4., false, None),
                    (394., 16., true, None),
                    (394., 128., true, Some(1)),
                    (394., 224., true, None),
                    (394., 252., false, None),
                    (394., 320., false, None),
                    (20., 16., false, None),
                    (32., 128., true, None),
                ] {
                    let expected = [x / 1024., y / 256.];
                    let position: [f32; 3] = std::array::from_fn(|i| {
                        panel[i][3]
                            + panel[i][0] * (expected[0] - 0.5) * 0.18
                            + panel[i][1] * (0.5 - expected[1]) * 0.045
                    });
                    let mut ray = panel;
                    for i in 0..3 {
                        ray[i][3] = position[i] + panel[i][2] * 0.2;
                    }
                    let hit = super::wrist_ray_hit(
                        &api,
                        super::OverlayHandle(73),
                        1,
                        ray,
                        SurfaceId::Menu.size(),
                    );
                    if !(0. ..=1.).contains(&expected[1]) {
                        assert!(hit.is_none(), "outside texture: {x}, {y}");
                        continue;
                    }
                    let (uv, point, distance) = hit.expect("hit inside the texture");
                    for i in 0..2 {
                        assert!(
                            (uv[i] - expected[i]).abs() < 0.0001,
                            "pixel {x}, {y}: {uv:?} != {expected:?}"
                        );
                    }
                    for i in 0..3 {
                        assert!((point[i] - position[i]).abs() < 0.00001);
                    }
                    assert!((distance - 0.2).abs() < 0.00001);
                    assert_eq!(layout.pointer_contains(uv), visible, "pixel {x}, {y}");
                    assert_eq!(layout.hit(uv), button, "pixel {x}, {y}");
                }
            }
        }

        #[test]
        fn wrist_ray_uses_the_pointer_pose_and_rejects_misses() {
            use std::cell::Cell;
            thread_local! {
                static HIT: Cell<bool> = const { Cell::new(true) };
                static POSITION: Cell<[f32; 3]> = const { Cell::new([1., 2., 2.25]) };
            }
            unsafe extern "C" fn intersection(
                _: u64,
                ray: *mut openvr_sys::VROverlayIntersectionParams_t,
                hit: *mut openvr_sys::VROverlayIntersectionResults_t,
            ) -> bool {
                let ray = unsafe { &*ray };
                if ray.vSource.v != [1., 2., 3.]
                    || ray.vDirection.v != [0., 0., -1.]
                    || ray.eOrigin != 1
                {
                    return false;
                }
                unsafe {
                    (*hit).vUVs.v = [0.75, 0.25];
                    (*hit).vPoint.v = POSITION.get();
                    (*hit).vNormal.v = [0., 0., 1.];
                    (*hit).fDistance = 0.75;
                }
                HIT.get()
            }
            unsafe extern "C" fn coordinates(
                _: u64,
                _: i32,
                _: openvr_sys::HmdVector2_t,
                pose: *mut openvr_sys::HmdMatrix34_t,
            ) -> i32 {
                unsafe { (*pose).m = super::transform::matrix(0., 0., 0., [0.75, 2.25, 2.25]) };
                0
            }
            unsafe extern "C" fn width(_: u64, width: *mut f32) -> i32 {
                unsafe { *width = 1. };
                0
            }
            let mut overlay: openvr_sys::VR_IVROverlay_FnTable = unsafe { std::mem::zeroed() };
            overlay.ComputeOverlayIntersection = Some(intersection);
            overlay.GetTransformForOverlayCoordinates = Some(coordinates);
            overlay.GetOverlayWidthInMeters = Some(width);
            let sample = || {
                super::wrist_ray_hit(
                    &overlay,
                    openvr::overlay::OverlayHandle(73),
                    1,
                    super::transform::matrix(0., 0., 0., [1., 2., 3.]),
                    (1024, 1024),
                )
            };
            assert_eq!(sample(), Some(([0.75, 0.75], [1., 2., 2.25], 0.75)));
            POSITION.set([f32::NAN, 2., 2.25]);
            assert!(sample().is_none());
            POSITION.set([1., 2., 2.25]);
            HIT.set(false);
            assert!(sample().is_none());
        }

        #[test]
        fn mirrors_remain_owned_until_reacquisition_or_shutdown() {
            unsafe extern "C" fn release(view: *mut std::ffi::c_void) {
                let count = &*view.cast::<AtomicUsize>();
                count.fetch_add(1, Ordering::Relaxed);
            }
            let released = AtomicUsize::new(0);
            let mirror = || Mirror {
                view: std::ptr::from_ref(&released).cast_mut().cast(),
                release,
            };
            let mut mirrors = [Some(mirror()), Some(mirror())];
            assert_eq!(released.load(Ordering::Relaxed), 0);
            mirrors[0] = None;
            assert_eq!(released.load(Ordering::Relaxed), 1);
            mirrors[0] = Some(mirror());
            assert_eq!(released.load(Ordering::Relaxed), 1);
            mirrors = [None, None];
            assert_eq!(released.load(Ordering::Relaxed), 3);
            drop(mirrors);
            assert_eq!(released.load(Ordering::Relaxed), 3);
        }
    }
}

#[cfg(not(windows))]
mod platform {
    use vrcs_core::{VrOverlayHeadsetConfig, VrOverlayWristConfig};

    use super::{ControllerBinding, OverlayKind};

    pub struct OpenVrBackend;

    impl OpenVrBackend {
        pub fn quit_requested(&self) -> bool {
            false
        }

        pub fn runtime_installed() -> bool {
            false
        }
        #[allow(dead_code)]
        pub fn ensure_ocr_progress(&mut self) -> Result<(), String> {
            Err("VR Overlay is only supported on Windows".into())
        }
        pub fn hmd_present() -> bool {
            false
        }
        pub fn connect() -> Result<Self, String> {
            Err("VR Overlay is only supported on Windows".into())
        }
        pub fn ensure_headset(&mut self, _: &VrOverlayHeadsetConfig) -> Result<(), String> {
            Err("VR Overlay is only supported on Windows".into())
        }
        pub fn ensure_wrist(
            &mut self,
            _: &VrOverlayWristConfig,
        ) -> Result<ControllerBinding, String> {
            Err("VR Overlay is only supported on Windows".into())
        }
        pub fn set_opacity(&mut self, _: OverlayKind, _: f32) -> Result<(), String> {
            Err("VR Overlay is only supported on Windows".into())
        }
        pub fn show(&mut self, _: OverlayKind) -> Result<(), String> {
            Err("VR Overlay is only supported on Windows".into())
        }
        pub fn hide(&mut self, _: OverlayKind) {}
        pub fn hide_all(&mut self) {}
        pub fn reset(&mut self, _: OverlayKind) {}
        pub fn ensure_dashboard(&mut self) -> Result<(), String> {
            Err("VR Overlay is only supported on Windows".into())
        }
        pub fn dashboard_visible(&self) -> bool {
            false
        }
        pub fn poll_dashboard_events(
            &self,
        ) -> Result<Vec<crate::vr_overlay::dashboard::DashboardPointerEvent>, String> {
            Ok(Vec::new())
        }
    }
}

pub use platform::OpenVrBackend;

#[cfg(windows)]
pub fn menu_config(hand: &str) -> vrcs_core::VrOverlayWristConfig {
    vrcs_core::VrOverlayWristConfig {
        hand: hand.into(),
        width_m: 0.18,
        // Place the menu above the forearm, behind the controller in local +Z.
        offset_x_m: 0.0,
        offset_y_m: 0.06,
        offset_z_m: 0.18,
        pitch_deg: -90.0,
        yaw_deg: 0.0,
        roll_deg: 0.0,
        ..Default::default()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverlayKind {
    Headset,
    Wrist,
    WristMenu,
    OcrWrist,
    OcrProgress,
    OcrFrame,
    OcrResult,
    Dashboard,
}

#[derive(Debug, Clone)]
pub struct ControllerBinding {
    pub role: Option<String>,
    pub available: bool,
}
