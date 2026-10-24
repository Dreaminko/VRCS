use super::ocr_gesture::{camera_frame, gesture_frame, point_in_head, FrameCorners, HandSample};
use super::ocr_input_state::{FrameSession, FrameTracking, HoldAction, TriggerEdge, TriggerUpdate};
use super::ocr_wrist::Action;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::{ffi::CString, mem::size_of, path::Path, time::Instant};

pub struct UiInput {
    pub device: u32,
    pub source: u64,
    pub trigger: Option<bool>,
    pub grip: Option<bool>,
    pub scroll_y: f32,
}

pub struct InputActions {
    pub vision_trigger: TriggerUpdate,
    pub cancel: bool,
    pub clear: bool,
    pub available: bool,
    pub gesture_available: bool,
    pub frame: Option<Frame>,
    pub frame_confirmed: bool,
    pub frame_cancelled: bool,
    pub navigation: Vec<Action>,
}

pub struct Frame {
    /// Hand anchors in the tracking space, sampled using head_pose.
    pub corners: [[f32; 3]; 2],
    pub head_pose: [[f32; 4]; 3],
    pub origin: i32,
}

impl Frame {
    fn from_head_corners(corners: FrameCorners, head_pose: [[f32; 4]; 3], origin: i32) -> Self {
        Self {
            corners: corners.corners.map(|point| {
                std::array::from_fn(|r| {
                    head_pose[r][3] + (0..3).map(|c| head_pose[r][c] * point[c]).sum::<f32>()
                })
            }),
            head_pose,
            origin,
        }
    }
}

pub struct OcrInput {
    table: *const openvr_sys::VR_IVRInput_FnTable,
    action_set: u64,
    scan: u64,
    clear: u64,
    cancel: u64,
    vision_trigger: TriggerEdge,
    vision_confirm: TriggerEdge,
    cancel_trigger: TriggerEdge,
    reading: [u64; 4],
    scan_hold: HoldAction,
    clear_hold: HoldAction,
    hands: [u64; 2],
    grips: [u64; 2],
    scrolls: [u64; 2],
    poses: [u64; 2],
    frame_session: FrameSession,
    grips_blocked: bool,
    last_digital_state: Option<(bool, bool, bool, bool, bool, bool)>,
}

fn check(error: openvr_sys::EVRInputError) -> Result<(), String> {
    if error == 0 {
        Ok(())
    } else {
        Err(format!("SteamVR OCR input error {error}"))
    }
}

pub fn install_manifest(directory: &Path) -> Result<std::path::PathBuf, String> {
    std::fs::create_dir_all(directory).map_err(|_| "Could not create SteamVR input directory")?;
    let mut bindings = Vec::new();
    for controller in ["knuckles", "oculus_touch", "vive_controller"] {
        let name = format!("{controller}.json");
        let grip = |side: &str| {
            let (mode, component) = if controller == "knuckles" {
                ("grab", "grab")
            } else {
                ("button", "click")
            };
            let mut inputs = serde_json::Map::new();
            inputs.insert(
                component.into(),
                serde_json::json!({"output":format!("/actions/ocr/in/{side}_grip")}),
            );
            serde_json::json!({"path":format!("/user/hand/{side}/input/grip"),"mode":mode,"inputs":inputs})
        };
        let scroll = |side: &str| {
            let (axis, mode) = match controller {
                "knuckles" => ("thumbstick", "joystick"),
                "oculus_touch" => ("joystick", "joystick"),
                _ => ("trackpad", "trackpad"),
            };
            serde_json::json!({"path":format!("/user/hand/{side}/input/{axis}"),"mode":mode,
                "inputs":{"position":{"output":format!("/actions/ocr/in/{side}_scroll")}}})
        };
        let binding = serde_json::json!({"controller_type":controller,"name":"VRCS OCR",
        "description":"Hold both grips to drag a frame. After the frame appears, hold the right trigger to confirm OCR. With the visual assistant enabled, hold the left trigger in a valid frame to speak, then release to ask; otherwise hold to cancel / clear. If a trigger was already held before the frame appeared, release it before confirming. Triggers cannot scan or start a question without a frame. Bind the optional cancel action as needed. Customize to avoid game input conflicts.",
        "bindings":{"/actions/ocr":{"poses":[
            {"path":"/user/hand/left/pose/raw","output":"/actions/ocr/in/left_pose"},
            {"path":"/user/hand/right/pose/raw","output":"/actions/ocr/in/right_pose"}
        ],"skeleton":[
            {"path":"/user/hand/left/input/skeleton/left","output":"/actions/ocr/in/left_hand"},
            {"path":"/user/hand/right/input/skeleton/right","output":"/actions/ocr/in/right_hand"}
        ],"sources":[
            {"path":"/user/hand/right/input/trigger","mode":"button","inputs":{"click":{"output":"/actions/ocr/in/scan"}}},
            {"path":"/user/hand/left/input/trigger","mode":"button","inputs":{"click":{"output":"/actions/ocr/in/clear"}}},
            grip("left"), grip("right"), scroll("left"), scroll("right")
        ]}}});
        write_if_changed(&directory.join(&name), &binding)?;
        bindings.push(serde_json::json!({"controller_type":controller,"binding_url":name}));
    }
    let path = directory.join("actions.json");
    write_if_changed(
        &path,
        &serde_json::json!({
            "action_sets":[{"name":"/actions/ocr","usage":"leftright"}],
            "actions":[{"name":"/actions/ocr/in/scan","type":"boolean","requirement":"optional"},
                {"name":"/actions/ocr/in/clear","type":"boolean","requirement":"optional"},
                                {"name":"/actions/ocr/in/cancel","type":"boolean","requirement":"optional"},
                {"name":"/actions/ocr/in/previous_block","type":"boolean","requirement":"optional"},
                {"name":"/actions/ocr/in/next_block","type":"boolean","requirement":"optional"},
                {"name":"/actions/ocr/in/previous_page","type":"boolean","requirement":"optional"},
                {"name":"/actions/ocr/in/next_page","type":"boolean","requirement":"optional"},
                {"name":"/actions/ocr/in/left_grip","type":"boolean","requirement":"optional"},
                {"name":"/actions/ocr/in/right_grip","type":"boolean","requirement":"optional"},
                {"name":"/actions/ocr/in/left_scroll","type":"vector2","requirement":"optional"},
                {"name":"/actions/ocr/in/right_scroll","type":"vector2","requirement":"optional"},
                {"name":"/actions/ocr/in/left_pose","type":"pose","requirement":"optional"},
                {"name":"/actions/ocr/in/right_pose","type":"pose","requirement":"optional"},
                {"name":"/actions/ocr/in/left_hand","type":"skeleton","skeleton":"/skeleton/hand/left","requirement":"optional"},
                {"name":"/actions/ocr/in/right_hand","type":"skeleton","skeleton":"/skeleton/hand/right","requirement":"optional"}],
            "default_bindings":bindings,
            "localization":[{"language_tag":"en_US","/actions/ocr":"VRCS OCR",
                "/actions/ocr/in/scan":"Confirm active frame (hold)","/actions/ocr/in/clear":"Visual question in active frame (press / release) or cancel / clear (hold)",
                "/actions/ocr/in/cancel":"Cancel visual question (custom binding)",
                                "/actions/ocr/in/previous_block":"Previous OCR block (point at panel)",
                "/actions/ocr/in/next_block":"Next OCR block (point at panel)",
                "/actions/ocr/in/previous_page":"Previous OCR page (point at panel)",
                "/actions/ocr/in/next_page":"Next OCR page (point at panel)",
                "/actions/ocr/in/left_grip":"Frame drag left grip", "/actions/ocr/in/right_grip":"Frame drag right grip",
                "/actions/ocr/in/left_scroll":"Scroll reader with left stick (point at panel)",
                "/actions/ocr/in/right_scroll":"Scroll reader with right stick (point at panel)",
                "/actions/ocr/in/left_pose":"Frame left corner", "/actions/ocr/in/right_pose":"Frame right corner",
                "/actions/ocr/in/left_hand":"Left hand camera frame", "/actions/ocr/in/right_hand":"Right hand camera frame"}]
        }),
    )?;
    Ok(path)
}

fn write_if_changed(path: &Path, value: &serde_json::Value) -> Result<(), String> {
    let body = serde_json::to_string_pretty(value)
        .map_err(|_| "Could not encode SteamVR input manifest")?;
    if std::fs::read_to_string(path).ok().as_deref() != Some(&body) {
        std::fs::write(path, body).map_err(|_| "Could not write SteamVR input manifest")?;
    }
    Ok(())
}

impl OcrInput {
    /// Resolve the physical hand source for pointer pose components.
    pub fn pointer_source(&self, right: bool) -> Option<u64> {
        let api = unsafe { &*self.table };
        let path = if right {
            c"/user/hand/right"
        } else {
            c"/user/hand/left"
        };
        let mut source = 0;
        let error = unsafe { api.GetInputSourceHandle?(path.as_ptr().cast_mut(), &mut source) };
        (error == 0 && source != 0).then_some(source)
    }

    /// Read controller actions without requesting global input focus.
    pub fn ui_samples(&self) -> Vec<UiInput> {
        let api = unsafe { &*self.table };
        [false, true]
            .into_iter()
            .filter_map(|right| {
                let side = usize::from(right);
                let source = self.pointer_source(right)?;
                let read = |action| {
                    let mut data: openvr_sys::InputDigitalActionData_t =
                        unsafe { std::mem::zeroed() };
                    let error = unsafe {
                        api.GetDigitalActionData?(
                            action,
                            &mut data,
                            size_of::<openvr_sys::InputDigitalActionData_t>() as u32,
                            source,
                        )
                    };
                    (error == 0 && data.bActive && data.activeOrigin != 0).then_some(data)
                };
                let trigger = read(if right { self.scan } else { self.clear });
                let grip = read(self.grips[side]);
                let scroll = api.GetAnalogActionData.and_then(|read| {
                    let action = self.scrolls[side];
                    if action == 0 {
                        return None;
                    }
                    let mut data: openvr_sys::InputAnalogActionData_t =
                        unsafe { std::mem::zeroed() };
                    let error = unsafe {
                        read(
                            action,
                            &mut data,
                            size_of::<openvr_sys::InputAnalogActionData_t>() as u32,
                            source,
                        )
                    };
                    (error == 0 && data.bActive && data.activeOrigin != 0 && data.y.is_finite())
                        .then_some(data)
                });
                let device = |origin| {
                    let mut info: openvr_sys::InputOriginInfo_t = unsafe { std::mem::zeroed() };
                    let error = unsafe {
                        api.GetOriginTrackedDeviceInfo?(
                            origin,
                            &mut info,
                            size_of::<openvr_sys::InputOriginInfo_t>() as u32,
                        )
                    };
                    (error == 0 && info.devicePath == source).then_some(info.trackedDeviceIndex)
                };
                let origin = trigger
                    .as_ref()
                    .or(grip.as_ref())
                    .map(|data| data.activeOrigin)
                    .or_else(|| scroll.as_ref().map(|data| data.activeOrigin))?;
                let tracked_device = device(origin)?;
                let scroll_y = scroll
                    .filter(|data| device(data.activeOrigin) == Some(tracked_device))
                    .map(|data| data.y.signum() * ((data.y.abs().min(1.0) - 0.2) / 0.8).max(0.0))
                    .unwrap_or(0.0);
                Some(UiInput {
                    device: tracked_device,
                    source,
                    trigger: trigger.map(|v| v.bState),
                    grip: grip.map(|v| v.bState),
                    scroll_y,
                })
            })
            .collect()
    }

    pub fn freeze_frame(&mut self) {
        self.frame_session = FrameSession::default();
        self.scan_hold = HoldAction::default();
        self.vision_confirm = TriggerEdge::default();
    }

    pub fn open_bindings(&mut self) -> Result<(), String> {
        let api = unsafe { &*self.table };
        check(unsafe {
            api.OpenBindingUI.ok_or("SteamVR binding UI unavailable")?(
                std::ptr::null_mut(),
                self.action_set,
                0,
                false,
            )
        })?;
        self.scan_hold = HoldAction::default();
        self.clear_hold = HoldAction::default();
        self.vision_trigger = TriggerEdge::default();
        self.vision_confirm = TriggerEdge::default();
        self.cancel_trigger = TriggerEdge::default();
        self.frame_session = FrameSession::default();
        self.last_digital_state = None;
        Ok(())
    }
    /// The input table must remain valid while the owning OpenVR context is alive.
    pub unsafe fn new(
        table: *const openvr_sys::VR_IVRInput_FnTable,
        path: &Path,
    ) -> Result<Self, String> {
        let api = unsafe { &*table };
        let path = CString::new(path.to_string_lossy().as_bytes())
            .map_err(|_| "Invalid input manifest path")?;
        check(unsafe {
            api.SetActionManifestPath
                .ok_or("Action manifest API unavailable")?(path.as_ptr().cast_mut())
        })?;
        let mut action_set = 0;
        let set = CString::new("/actions/ocr").unwrap();
        check(unsafe {
            api.GetActionSetHandle.ok_or("Action set API unavailable")?(
                set.as_ptr().cast_mut(),
                &mut action_set,
            )
        })?;
        let action = |name: &str| -> Result<u64, String> {
            let mut handle = 0;
            let name = CString::new(name).map_err(|_| "Invalid OCR action name")?;
            check(unsafe {
                api.GetActionHandle.ok_or("Action handle API unavailable")?(
                    name.as_ptr().cast_mut(),
                    &mut handle,
                )
            })?;
            Ok(handle)
        };
        Ok(Self {
            table,
            action_set,
            scan: action("/actions/ocr/in/scan")?,
            clear: action("/actions/ocr/in/clear")?,
            cancel: action("/actions/ocr/in/cancel").unwrap_or(0),
            vision_trigger: TriggerEdge::default(),
            vision_confirm: TriggerEdge::default(),
            cancel_trigger: TriggerEdge::default(),
            reading: ["previous_block", "next_block", "previous_page", "next_page"]
                .map(|name| action(&format!("/actions/ocr/in/{name}")).unwrap_or(0)),
            scan_hold: HoldAction::default(),
            clear_hold: HoldAction::default(),
            hands: [
                action("/actions/ocr/in/left_hand").unwrap_or(0),
                action("/actions/ocr/in/right_hand").unwrap_or(0),
            ],
            grips: [
                action("/actions/ocr/in/left_grip").unwrap_or(0),
                action("/actions/ocr/in/right_grip").unwrap_or(0),
            ],
            scrolls: [
                action("/actions/ocr/in/left_scroll").unwrap_or(0),
                action("/actions/ocr/in/right_scroll").unwrap_or(0),
            ],
            poses: [
                action("/actions/ocr/in/left_pose").unwrap_or(0),
                action("/actions/ocr/in/right_pose").unwrap_or(0),
            ],
            frame_session: FrameSession::default(),
            grips_blocked: false,
            last_digital_state: None,
        })
    }

    pub fn update_actions(&mut self) -> Result<(), String> {
        let api = unsafe { &*self.table };
        let mut set: openvr_sys::VRActiveActionSet_t = unsafe { std::mem::zeroed() };
        set.ulActionSet = self.action_set;
        check(unsafe {
            api.UpdateActionState
                .ok_or("Action state API unavailable")?(
                &mut set,
                size_of::<openvr_sys::VRActiveActionSet_t>() as u32,
                1,
            )
        })
    }

    pub fn poll_with_vision(
        &mut self,
        tracking: Option<([[f32; 4]; 3], i32)>,
        reading_focus: bool,
        vision_enabled: bool,
    ) -> Result<InputActions, String> {
        let api = unsafe { &*self.table };
        let read = |handle| -> Result<openvr_sys::InputDigitalActionData_t, String> {
            let mut data = unsafe { std::mem::zeroed() };
            check(unsafe {
                api.GetDigitalActionData
                    .ok_or("Digital input API unavailable")?(
                    handle,
                    &mut data,
                    size_of::<openvr_sys::InputDigitalActionData_t>() as u32,
                    0,
                )
            })?;
            Ok(data)
        };
        // Frame bindings are optional; basic scan/clear must still work without them.
        let grips = self
            .grips
            .map(|handle| read(handle).unwrap_or_else(|_| unsafe { std::mem::zeroed() }));
        if reading_focus {
            self.grips_blocked = true;
        } else if grips.iter().all(|g| g.bActive && !g.bState) {
            self.grips_blocked = false;
        }
        let tracking = tracking.filter(|_| !reading_focus && !self.grips_blocked);
        let scan = read(self.scan)?;
        let clear = read(self.clear)?;
        let navigation = if reading_focus {
            self.reading
                .iter()
                .copied()
                .zip([
                    Action::PreviousBlock,
                    Action::NextBlock,
                    Action::PreviousPage,
                    Action::NextPage,
                ])
                .filter_map(|(handle, action)| {
                    (handle != 0)
                        .then(|| read(handle).ok())
                        .flatten()
                        .filter(|data| data.bActive && data.bChanged && data.bState)
                        .map(|_| action)
                })
                .collect()
        } else {
            Vec::new()
        };
        let state = (
            scan.bActive,
            clear.bActive,
            scan.bState,
            clear.bState,
            scan.activeOrigin != 0,
            clear.activeOrigin != 0,
        );
        if self.last_digital_state != Some(state) {
            tracing::info!(
                scan_active = state.0, clear_active = state.1,
                scan_pressed = state.2, clear_pressed = state.3,
                scan_has_origin = state.4, clear_has_origin = state.5,
                scan_bindings = ?binding_count(api, self.scan),
                clear_bindings = ?binding_count(api, self.clear),
                "SteamVR OCR digital input state"
            );
            self.last_digital_state = Some(state);
        }
        let origin = |data: &openvr_sys::InputDigitalActionData_t| {
            (data.bActive && data.activeOrigin != 0 && !reading_focus).then_some(data.activeOrigin)
        };
        // Do not mask the recording trigger with wrist focus: a real release must close audio.
        let raw_origin = |data: &openvr_sys::InputDigitalActionData_t| {
            if !data.bActive || data.activeOrigin == 0 {
                return None;
            }
            let Some(query) = api.GetOriginTrackedDeviceInfo else {
                return Some(data.activeOrigin);
            };
            let mut info: openvr_sys::InputOriginInfo_t = unsafe { std::mem::zeroed() };
            let error = unsafe {
                query(
                    data.activeOrigin,
                    &mut info,
                    size_of::<openvr_sys::InputOriginInfo_t>() as u32,
                )
            };
            // An action origin identifies a component; devicePath identifies the controller.
            (error == 0 && info.devicePath != 0).then_some(info.devicePath)
        };
        let mut vision_trigger = self.vision_trigger.update(raw_origin(&clear), clear.bState);
        let cancel = (self.cancel != 0).then(|| read(self.cancel).ok()).flatten();
        let cancel = self
            .cancel_trigger
            .update(
                cancel.as_ref().and_then(raw_origin),
                cancel.as_ref().is_some_and(|data| data.bState),
            )
            .pressed;
        let now = Instant::now();
        let hands = tracking.and_then(|(head, origin)| {
            Some([
                read_hand(api, self.hands[0], head, origin).ok()?,
                read_hand(api, self.hands[1], head, origin).ok()?,
            ])
        });
        let gesture_origin = hands.as_ref().map(|hands| {
            let mut hasher = DefaultHasher::new();
            (hands[0].origin, hands[1].origin).hash(&mut hasher);
            hasher.finish()
        });
        let gesture_tracking = hands.as_ref().map(|hands| {
            let frame = gesture_frame(
                hands,
                (scan.bActive && scan.activeOrigin != 0 && scan.bState)
                    || (vision_enabled && vision_trigger.down),
            );
            FrameTracking {
                origin: gesture_origin.unwrap_or(0),
                corners: frame.unwrap_or(FrameCorners {
                    corners: [hands[0].wrist, hands[1].wrist],
                }),
                activating: camera_frame(hands),
                dragging: frame.is_some(),
            }
        });
        let controllers = tracking.and_then(|(head, tracking_origin)| {
            let poses = [
                read_controller(api, self.poses[0], head, tracking_origin).ok()?,
                read_controller(api, self.poses[1], head, tracking_origin).ok()?,
            ];
            if poses[0].0 == poses[1].0
                || grips
                    .iter()
                    .any(|grip| !grip.bActive || grip.activeOrigin == 0)
            {
                return None;
            }
            let mut hasher = DefaultHasher::new();
            (
                poses[0].0,
                poses[1].0,
                grips[0].activeOrigin,
                grips[1].activeOrigin,
            )
                .hash(&mut hasher);
            let pressed = grips.iter().all(|grip| grip.bState);
            Some(FrameTracking {
                origin: hasher.finish(),
                corners: FrameCorners {
                    corners: [poses[0].1, poses[1].1],
                },
                activating: pressed,
                dragging: pressed,
            })
        });
        let available = scan.bActive && clear.bActive;
        // Arm confirmation only after a frame was active in a previous poll.
        // Keep raw left-trigger releases available after the recording frame is frozen.
        let frame_active = self.frame_session.active();
        let scan = self
            .scan_hold
            .update(origin(&scan).filter(|_| frame_active), scan.bState, now);
        vision_trigger.pressed = self
            .vision_confirm
            .update(
                raw_origin(&clear).filter(|_| frame_active && !reading_focus && vision_enabled),
                clear.bState,
            )
            .pressed;
        let clear = self.clear_hold.update(origin(&clear), clear.bState, now);
        let frame = self.frame_session.update(
            [gesture_tracking, controllers],
            scan && !(vision_enabled && vision_trigger.down),
            cancel || (clear && !vision_enabled),
            now,
        );
        vision_trigger.pressed &= frame.frame.is_some() && !frame.cancelled;
        Ok(InputActions {
            vision_trigger,
            cancel,
            clear,
            available,
            gesture_available: hands.is_some() || controllers.is_some(),
            frame: frame
                .frame
                .zip(tracking)
                .map(|(corners, (head, origin))| Frame::from_head_corners(corners, head, origin)),
            frame_confirmed: frame.confirmed,
            frame_cancelled: frame.cancelled,
            navigation,
        })
    }
}

fn binding_count(api: &openvr_sys::VR_IVRInput_FnTable, action: u64) -> Option<Result<u32, i32>> {
    let query = api.GetActionBindingInfo?;
    let mut bindings: [openvr_sys::InputBindingInfo_t; 8] = unsafe { std::mem::zeroed() };
    let mut count = 0;
    let error = unsafe {
        query(
            action,
            bindings.as_mut_ptr(),
            size_of::<openvr_sys::InputBindingInfo_t>() as u32,
            8,
            &mut count,
        )
    };
    Some(if error == 0 { Ok(count) } else { Err(error) })
}

fn read_controller(
    api: &openvr_sys::VR_IVRInput_FnTable,
    action: u64,
    head: [[f32; 4]; 3],
    origin: i32,
) -> Result<(u64, [f32; 3]), String> {
    let data = read_controller_pose(api, action, origin)?;
    if head.iter().flatten().any(|value| !value.is_finite()) {
        return Err("Controller tracking inactive".into());
    }
    let point = point_in_head(head, data.pose.mDeviceToAbsoluteTracking.m, [0.; 3]);
    if point.iter().any(|value| !value.is_finite()) {
        return Err("Invalid controller position".into());
    }
    Ok((data.activeOrigin, point))
}

fn read_controller_pose(
    api: &openvr_sys::VR_IVRInput_FnTable,
    action: u64,
    origin: i32,
) -> Result<openvr_sys::InputPoseActionData_t, String> {
    if action == 0 {
        return Err("Controller pose unbound".into());
    }
    let mut data: openvr_sys::InputPoseActionData_t = unsafe { std::mem::zeroed() };
    check(unsafe {
        api.GetPoseActionDataRelativeToNow
            .ok_or("Controller pose unavailable")?(
            action,
            origin,
            0.,
            &mut data,
            size_of::<openvr_sys::InputPoseActionData_t>() as u32,
            0,
        )
    })?;
    let device = data.pose.mDeviceToAbsoluteTracking.m;
    if !data.bActive
        || data.activeOrigin == 0
        || !data.pose.bPoseIsValid
        || !data.pose.bDeviceIsConnected
        || device.iter().flatten().any(|value| !value.is_finite())
    {
        return Err("Controller tracking inactive".into());
    }
    Ok(data)
}

fn read_hand(
    api: &openvr_sys::VR_IVRInput_FnTable,
    action: u64,
    head: [[f32; 4]; 3],
    origin: i32,
) -> Result<HandSample, String> {
    let mut data: openvr_sys::InputSkeletalActionData_t = unsafe { std::mem::zeroed() };
    let mut level = 0;
    let mut count = 0;
    let mut pose: openvr_sys::InputPoseActionData_t = unsafe { std::mem::zeroed() };
    let mut bones: [openvr_sys::VRBoneTransform_t; 31] = unsafe { std::mem::zeroed() };
    let mut summary: openvr_sys::VRSkeletalSummaryData_t = unsafe { std::mem::zeroed() };
    unsafe {
        check(api
            .GetSkeletalActionData
            .ok_or("Skeletal input unavailable")?(
            action,
            &mut data,
            size_of::<openvr_sys::InputSkeletalActionData_t>() as u32,
        ))?;
        if !data.bActive || data.activeOrigin == 0 {
            return Err("Hand input inactive".into());
        }
        check(api
            .GetSkeletalTrackingLevel
            .ok_or("Skeletal tracking unavailable")?(
            action, &mut level
        ))?;
        // Estimated or partially measured bones cannot establish the finger frame orientation.
        if level != openvr_sys::EVRSkeletalTrackingLevel_VRSkeletalTracking_Full {
            return Err("Full finger tracking required".into());
        }
        check(api.GetBoneCount.ok_or("Hand skeleton unavailable")?(
            action, &mut count,
        ))?;
        if count != 31 {
            return Err("Unsupported hand skeleton".into());
        }
        check(api
            .GetPoseActionDataRelativeToNow
            .ok_or("Hand pose unavailable")?(
            action,
            origin,
            0.,
            &mut pose,
            size_of::<openvr_sys::InputPoseActionData_t>() as u32,
            0,
        ))?;
        if !pose.bActive
            || pose.activeOrigin != data.activeOrigin
            || !pose.pose.bPoseIsValid
            || !pose.pose.bDeviceIsConnected
        {
            return Err("Hand pose inactive".into());
        }
        check(api.GetSkeletalBoneData.ok_or("Hand bones unavailable")?(
            action,
            openvr_sys::EVRSkeletalTransformSpace_VRSkeletalTransformSpace_Model,
            openvr_sys::EVRSkeletalMotionRange_VRSkeletalMotionRange_WithController,
            bones.as_mut_ptr(),
            count,
        ))?;
        check(api
            .GetSkeletalSummaryData
            .ok_or("Finger curl unavailable")?(
            action,
            openvr_sys::EVRSummaryType_VRSummaryType_FromAnimation,
            &mut summary,
        ))?;
    }
    let device = pose.pose.mDeviceToAbsoluteTracking.m;
    if head
        .iter()
        .chain(device.iter())
        .flatten()
        .any(|value| !value.is_finite())
        || bones
            .iter()
            .flat_map(|bone| bone.position.v)
            .any(|value| !value.is_finite())
        || summary
            .flFingerCurl
            .iter()
            .any(|curl| !curl.is_finite() || !(0.0..=1.0).contains(curl))
    {
        return Err("Invalid hand tracking data".into());
    }
    let point = |index: usize| {
        let [x, y, z, _] = bones[index].position.v;
        point_in_head(head, device, [x, y, z])
    };
    Ok(HandSample {
        origin: data.activeOrigin,
        curls: summary.flFingerCurl,
        wrist: point(1),
        thumb_base: point(2),
        thumb_tip: point(5),
        index_base: point(6),
        index_tip: point(10),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    thread_local! {
        static CONTROLLER_BUTTONS: std::cell::Cell<[bool; 4]> = const {
            std::cell::Cell::new([false; 4])
        };
    }

    fn controller_api() -> openvr_sys::VR_IVRInput_FnTable {
        unsafe extern "C" fn digital(
            action: u64,
            data: *mut openvr_sys::InputDigitalActionData_t,
            _: u32,
            _: u64,
        ) -> i32 {
            if !(1..=4).contains(&action) {
                return 3;
            }
            unsafe {
                (*data).bActive = true;
                (*data).activeOrigin = action;
                (*data).bState = CONTROLLER_BUTTONS.get()[action as usize - 1];
            }
            0
        }
        unsafe extern "C" fn pose(
            action: u64,
            _: i32,
            _: f32,
            data: *mut openvr_sys::InputPoseActionData_t,
            _: u32,
            _: u64,
        ) -> i32 {
            let position = if action == 5 {
                [-0.2, -0.1, -0.5]
            } else {
                [0.2, 0.1, -0.5]
            };
            unsafe {
                (*data).bActive = true;
                (*data).activeOrigin = action;
                (*data).pose.bPoseIsValid = true;
                (*data).pose.bDeviceIsConnected = true;
                (*data).pose.mDeviceToAbsoluteTracking.m =
                    super::super::transform::matrix(0., 0., 0., position);
            }
            0
        }
        CONTROLLER_BUTTONS.set([false; 4]);
        let mut api: openvr_sys::VR_IVRInput_FnTable = unsafe { std::mem::zeroed() };
        api.GetDigitalActionData = Some(digital);
        api.GetPoseActionDataRelativeToNow = Some(pose);
        api
    }

    #[test]
    fn right_grip_and_trigger_without_a_frame_cannot_scan() {
        let api = controller_api();
        let mut input = input(&api);
        input.poll_with_vision(None, false, false).unwrap();
        CONTROLLER_BUTTONS.set([true, false, false, true]);
        let started = input.poll_with_vision(None, false, false).unwrap();
        assert!(!started.frame_confirmed);
        std::thread::sleep(std::time::Duration::from_millis(700));
        let held = input.poll_with_vision(None, false, false).unwrap();
        assert!(!held.frame_confirmed);
        assert!(held.frame.is_none());
    }

    #[test]
    fn ocr_requires_a_new_trigger_hold_after_entering_the_frame() {
        let api = controller_api();
        let mut input = input(&api);
        let tracking = Some((super::super::transform::matrix(0., 0., 0., [0.; 3]), 1));
        input.poll_with_vision(tracking, false, false).unwrap();
        CONTROLLER_BUTTONS.set([true, false, true, true]);
        input.poll_with_vision(tracking, false, false).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(700));
        let entered = input.poll_with_vision(tracking, false, false).unwrap();
        assert!(entered.frame.is_some());
        assert!(!entered.frame_confirmed);
        assert!(
            !input
                .poll_with_vision(tracking, false, false)
                .unwrap()
                .frame_confirmed
        );
        CONTROLLER_BUTTONS.set([false, false, true, true]);
        input.poll_with_vision(tracking, false, false).unwrap();
        CONTROLLER_BUTTONS.set([true, false, true, true]);
        assert!(
            !input
                .poll_with_vision(tracking, false, false)
                .unwrap()
                .frame_confirmed
        );
        std::thread::sleep(std::time::Duration::from_millis(700));
        let confirmed = input.poll_with_vision(tracking, false, false).unwrap();
        assert!(confirmed.frame_confirmed);
        assert!(confirmed.frame.is_some());
        let held = input.poll_with_vision(tracking, false, false).unwrap();
        assert!(!held.frame_confirmed);
    }

    #[test]
    fn visual_question_requires_a_new_trigger_press_after_entering_the_frame() {
        let api = controller_api();
        let mut input = input(&api);
        let tracking = Some((super::super::transform::matrix(0., 0., 0., [0.; 3]), 1));
        input.poll_with_vision(tracking, false, true).unwrap();
        CONTROLLER_BUTTONS.set([false, true, true, true]);
        let simultaneous = input.poll_with_vision(tracking, false, true).unwrap();
        assert!(!simultaneous.vision_trigger.pressed);
        std::thread::sleep(std::time::Duration::from_millis(700));
        let entered = input.poll_with_vision(tracking, false, true).unwrap();
        assert!(entered.frame.is_some());
        assert!(!entered.vision_trigger.pressed);
        CONTROLLER_BUTTONS.set([false, false, true, true]);
        input.poll_with_vision(tracking, false, true).unwrap();
        CONTROLLER_BUTTONS.set([false, true, true, true]);
        let confirmed = input.poll_with_vision(tracking, false, true).unwrap();
        assert!(confirmed.vision_trigger.pressed);
        assert!(confirmed.frame.is_some());
        input.freeze_frame();
        CONTROLLER_BUTTONS.set([false; 4]);
        let released = input.poll_with_vision(None, true, true).unwrap();
        assert!(released.vision_trigger.released);
        assert!(!released.vision_trigger.pressed);
    }

    #[test]
    fn releasing_a_grip_on_visual_confirmation_cancels_the_frame() {
        let api = controller_api();
        let mut input = input(&api);
        let tracking = Some((super::super::transform::matrix(0., 0., 0., [0.; 3]), 1));
        input.poll_with_vision(tracking, false, true).unwrap();
        CONTROLLER_BUTTONS.set([false, false, true, true]);
        input.poll_with_vision(tracking, false, true).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(700));
        assert!(input
            .poll_with_vision(tracking, false, true)
            .unwrap()
            .frame
            .is_some());
        input.poll_with_vision(tracking, false, true).unwrap();
        CONTROLLER_BUTTONS.set([false, true, false, true]);
        let cancelled = input.poll_with_vision(tracking, false, true).unwrap();
        assert!(cancelled.frame_cancelled);
        assert!(!cancelled.vision_trigger.pressed);
    }

    fn input(api: &openvr_sys::VR_IVRInput_FnTable) -> OcrInput {
        OcrInput {
            table: api,
            action_set: 42,
            scan: 1,
            clear: 2,
            cancel: 0,
            vision_trigger: TriggerEdge::default(),
            vision_confirm: TriggerEdge::default(),
            cancel_trigger: TriggerEdge::default(),
            reading: [0; 4],
            hands: [72, 73],
            grips: [3, 4],
            scrolls: [7, 8],
            poses: [5, 6],
            scan_hold: Default::default(),
            clear_hold: Default::default(),
            frame_session: Default::default(),
            grips_blocked: false,
            last_digital_state: None,
        }
    }

    #[test]
    fn wrist_pointer_source_is_available_without_active_action_data() {
        unsafe extern "C" fn source(path: *mut std::ffi::c_char, handle: *mut u64) -> i32 {
            let path = unsafe { std::ffi::CStr::from_ptr(path) };
            unsafe {
                *handle = if path == c"/user/hand/right" { 99 } else { 100 };
            }
            0
        }
        let mut api: openvr_sys::VR_IVRInput_FnTable = unsafe { std::mem::zeroed() };
        api.GetInputSourceHandle = Some(source);
        assert_eq!(input(&api).pointer_source(true), Some(99));
        assert_eq!(input(&api).pointer_source(false), Some(100));
        assert!(input(&api).ui_samples().is_empty());
        api.GetInputSourceHandle = None;
        assert!(input(&api).pointer_source(true).is_none());
    }

    #[test]
    fn wrist_pointer_works_without_optional_frame_pose_bindings() {
        unsafe extern "C" fn source(_: *mut std::ffi::c_char, handle: *mut u64) -> i32 {
            unsafe {
                *handle = 99;
            }
            0
        }
        unsafe extern "C" fn digital(
            _: u64,
            data: *mut openvr_sys::InputDigitalActionData_t,
            _: u32,
            _: u64,
        ) -> i32 {
            unsafe {
                (*data).bActive = true;
                (*data).activeOrigin = 11;
                (*data).bState = true;
            }
            0
        }
        unsafe extern "C" fn origin(
            _: u64,
            info: *mut openvr_sys::InputOriginInfo_t,
            _: u32,
        ) -> i32 {
            unsafe {
                (*info).trackedDeviceIndex = 2;
                (*info).devicePath = 99;
            }
            0
        }
        let mut api: openvr_sys::VR_IVRInput_FnTable = unsafe { std::mem::zeroed() };
        api.GetInputSourceHandle = Some(source);
        api.GetDigitalActionData = Some(digital);
        api.GetOriginTrackedDeviceInfo = Some(origin);
        // Menu clicks must work even when grip-based frame poses are not bound.
        let samples = input(&api).ui_samples();
        assert_eq!(samples.len(), 2);
        assert!(samples.iter().all(|sample| sample.device == 2
            && sample.source == 99
            && sample.trigger == Some(true)));
    }

    #[test]
    fn wrist_pointer_uses_the_current_trigger_snapshot_and_controller_identity() {
        use std::cell::Cell;
        thread_local! { static UPDATES: Cell<u32> = const { Cell::new(0) }; }
        unsafe extern "C" fn source(path: *mut std::ffi::c_char, handle: *mut u64) -> i32 {
            let path = unsafe { std::ffi::CStr::from_ptr(path) };
            unsafe {
                *handle = if path == c"/user/hand/right" {
                    101
                } else {
                    102
                };
            }
            0
        }
        unsafe extern "C" fn update(
            _: *mut openvr_sys::VRActiveActionSet_t,
            _: u32,
            _: u32,
        ) -> i32 {
            UPDATES.set(UPDATES.get() + 1);
            0
        }
        unsafe extern "C" fn digital(
            action: u64,
            data: *mut openvr_sys::InputDigitalActionData_t,
            _: u32,
            _: u64,
        ) -> i32 {
            unsafe {
                (*data).bActive = true;
                (*data).activeOrigin = action;
                (*data).bState = UPDATES.get() == 1;
            }
            0
        }
        unsafe extern "C" fn origin(
            origin: u64,
            info: *mut openvr_sys::InputOriginInfo_t,
            _: u32,
        ) -> i32 {
            unsafe {
                (*info).trackedDeviceIndex = origin as u32;
                (*info).devicePath = origin + 100;
            }
            0
        }
        let mut api: openvr_sys::VR_IVRInput_FnTable = unsafe { std::mem::zeroed() };
        api.GetInputSourceHandle = Some(source);
        api.UpdateActionState = Some(update);
        api.GetDigitalActionData = Some(digital);
        api.GetOriginTrackedDeviceInfo = Some(origin);
        let mut controller = input(&api);
        controller.update_actions().unwrap();
        let samples = controller.ui_samples();
        assert_eq!(samples.len(), 2);
        assert_eq!(
            (samples[0].device, samples[0].source, samples[0].trigger),
            (2, 102, Some(true))
        );
        assert_eq!(
            (samples[1].device, samples[1].source, samples[1].trigger),
            (1, 101, Some(true))
        );
        controller.poll_with_vision(None, true, false).unwrap();
        assert_eq!(
            UPDATES.get(),
            1,
            "Menu and OCR must share one action snapshot"
        );
        controller.update_actions().unwrap();
        let samples = controller.ui_samples();
        assert_eq!(
            (samples[1].device, samples[1].source, samples[1].trigger),
            (1, 101, Some(false))
        );
        api.GetOriginTrackedDeviceInfo = None;
        assert!(input(&api).ui_samples().is_empty());
    }

    #[test]
    fn reader_scroll_uses_each_hands_axis_and_ignores_invalid_samples() {
        use std::cell::{Cell, RefCell};
        thread_local! {
            static AXIS: Cell<(i32, bool, u64, f32)> = const { Cell::new((0, true, 0, 1.0)) };
            static READS: RefCell<Vec<(u64, u64)>> = const { RefCell::new(Vec::new()) };
        }
        unsafe extern "C" fn source(path: *mut std::ffi::c_char, handle: *mut u64) -> i32 {
            unsafe {
                *handle = if std::ffi::CStr::from_ptr(path) == c"/user/hand/left" {
                    101
                } else {
                    102
                };
            }
            0
        }
        unsafe extern "C" fn digital(
            _: u64,
            data: *mut openvr_sys::InputDigitalActionData_t,
            _: u32,
            source: u64,
        ) -> i32 {
            unsafe {
                (*data).bActive = true;
                (*data).activeOrigin = source - 100;
                (*data).bState = true;
            }
            0
        }
        unsafe extern "C" fn analog(
            action: u64,
            data: *mut openvr_sys::InputAnalogActionData_t,
            _: u32,
            source: u64,
        ) -> i32 {
            READS.with_borrow_mut(|reads| reads.push((action, source)));
            let (error, active, origin, y) = AXIS.get();
            unsafe {
                (*data).bActive = active;
                (*data).activeOrigin = if origin == 0 { source - 100 } else { origin };
                (*data).y = if action == 7 { y } else { -y };
            }
            error
        }
        unsafe extern "C" fn origin(
            origin: u64,
            info: *mut openvr_sys::InputOriginInfo_t,
            _: u32,
        ) -> i32 {
            unsafe {
                (*info).trackedDeviceIndex = origin as u32;
                (*info).devicePath = origin + 100;
            }
            0
        }
        let mut api: openvr_sys::VR_IVRInput_FnTable = unsafe { std::mem::zeroed() };
        api.GetInputSourceHandle = Some(source);
        api.GetDigitalActionData = Some(digital);
        api.GetAnalogActionData = Some(analog);
        api.GetOriginTrackedDeviceInfo = Some(origin);
        let samples = input(&api).ui_samples();
        assert_eq!(
            READS.with_borrow(|reads| reads.clone()),
            [(7, 101), (8, 102)]
        );
        assert_eq!((samples[0].scroll_y, samples[1].scroll_y), (1.0, -1.0));
        for value in [0.0, 0.1, -0.2, f32::NAN, f32::INFINITY] {
            AXIS.set((0, true, 0, value));
            assert!(input(&api)
                .ui_samples()
                .iter()
                .all(|sample| sample.scroll_y == 0.0));
        }
        for (error, active) in [(1, true), (0, false)] {
            AXIS.set((error, active, 0, 1.0));
            let samples = input(&api).ui_samples();
            assert_eq!(samples.len(), 2);
            assert!(samples
                .iter()
                .all(|sample| sample.scroll_y == 0.0 && sample.trigger == Some(true)));
        }
        AXIS.set((0, true, 2, 1.0));
        let samples = input(&api).ui_samples();
        assert_eq!((samples[0].scroll_y, samples[1].scroll_y), (0.0, -1.0));
        api.GetAnalogActionData = None;
        assert!(input(&api)
            .ui_samples()
            .iter()
            .all(|sample| sample.scroll_y == 0.0 && sample.trigger == Some(true)));
        api.GetAnalogActionData = Some(analog);
        api.GetDigitalActionData = None;
        AXIS.set((0, true, 0, 0.6));
        let samples = input(&api).ui_samples();
        assert_eq!(samples.len(), 2);
        assert!((samples[0].scroll_y - 0.5).abs() < 0.0001);
        assert!((samples[1].scroll_y + 0.5).abs() < 0.0001);
    }

    #[test]
    fn vision_trigger_uses_device_identity_and_releases_with_reading_focus() {
        use std::cell::Cell;
        thread_local! { static SAMPLE: Cell<(u64, bool)> = const { Cell::new((11, false)) }; }
        unsafe extern "C" fn update(
            _: *mut openvr_sys::VRActiveActionSet_t,
            _: u32,
            _: u32,
        ) -> i32 {
            0
        }
        unsafe extern "C" fn digital(
            _: u64,
            data: *mut openvr_sys::InputDigitalActionData_t,
            _: u32,
            _: u64,
        ) -> i32 {
            let (origin, down) = SAMPLE.get();
            unsafe {
                (*data).bActive = true;
                (*data).activeOrigin = origin;
                (*data).bState = down;
            }
            0
        }
        unsafe extern "C" fn device(
            _: u64,
            data: *mut openvr_sys::InputOriginInfo_t,
            _: u32,
        ) -> i32 {
            unsafe {
                (*data).devicePath = 7;
            }
            0
        }
        let mut api = api();
        api.UpdateActionState = Some(update);
        api.GetDigitalActionData = Some(digital);
        api.GetOriginTrackedDeviceInfo = Some(device);
        let mut input = input(&api);
        input.poll_with_vision(None, false, true).unwrap();
        SAMPLE.set((12, true));
        let pressed = input.poll_with_vision(None, false, true).unwrap();
        assert!(!pressed.vision_trigger.pressed);
        assert!(!pressed.vision_trigger.lost);
        SAMPLE.set((13, false));
        let released = input.poll_with_vision(None, true, true).unwrap();
        assert!(released.vision_trigger.released);
        assert!(!released.vision_trigger.lost);
    }

    #[test]
    fn left_trigger_keeps_the_skeletal_frame_when_the_index_finger_curls() {
        unsafe extern "C" fn update(
            _: *mut openvr_sys::VRActiveActionSet_t,
            _: u32,
            _: u32,
        ) -> i32 {
            0
        }
        unsafe extern "C" fn digital(
            action: u64,
            data: *mut openvr_sys::InputDigitalActionData_t,
            _: u32,
            _: u64,
        ) -> i32 {
            unsafe {
                (*data).bActive = action <= 2;
                (*data).activeOrigin = action;
                (*data).bState = action == 2;
            }
            0
        }
        unsafe extern "C" fn skeletal(
            action: u64,
            data: *mut openvr_sys::InputSkeletalActionData_t,
            _: u32,
        ) -> i32 {
            unsafe {
                (*data).bActive = true;
                (*data).activeOrigin = action;
            }
            0
        }
        unsafe extern "C" fn summary(
            _: u64,
            _: i32,
            data: *mut openvr_sys::VRSkeletalSummaryData_t,
        ) -> i32 {
            unsafe {
                (*data).flFingerCurl = [0.1, 0.9, 0.8, 0.8, 0.8];
            }
            0
        }
        unsafe extern "C" fn pose(
            action: u64,
            _: i32,
            _: f32,
            data: *mut openvr_sys::InputPoseActionData_t,
            _: u32,
            _: u64,
        ) -> i32 {
            unsafe {
                (*data).bActive = true;
                (*data).activeOrigin = action;
                (*data).pose.bPoseIsValid = true;
                (*data).pose.bDeviceIsConnected = true;
                let position = if action == 72 {
                    [-0.2, -0.1, -0.5]
                } else {
                    [0.2, 0.1, -0.5]
                };
                (*data).pose.mDeviceToAbsoluteTracking.m =
                    super::super::transform::matrix(0., 0., 0., position);
            }
            0
        }
        let mut api = api();
        api.UpdateActionState = Some(update);
        api.GetDigitalActionData = Some(digital);
        api.GetSkeletalActionData = Some(skeletal);
        api.GetSkeletalSummaryData = Some(summary);
        api.GetPoseActionDataRelativeToNow = Some(pose);
        let head = super::super::transform::matrix(0., 0., 0., [0.; 3]);
        let mut input = input(&api);
        input.vision_trigger.update(Some(2), false);
        let actions = input
            .poll_with_vision(Some((head, 1)), false, true)
            .unwrap();
        assert!(!actions.vision_trigger.pressed);
        let mut hasher = DefaultHasher::new();
        (72u64, 73u64).hash(&mut hasher);
        let mut sample = FrameTracking {
            origin: hasher.finish(),
            corners: FrameCorners {
                corners: [[-0.2, -0.1, -0.5], [0.2, 0.1, -0.5]],
            },
            activating: false,
            dragging: false,
        };
        let now = Instant::now();
        input
            .frame_session
            .update([Some(sample), None], false, false, now);
        sample.activating = true;
        sample.dragging = true;
        input
            .frame_session
            .update([Some(sample), None], false, false, now);
        input.frame_session.update(
            [Some(sample), None],
            false,
            false,
            now + std::time::Duration::from_millis(650),
        );
        let actions = input
            .poll_with_vision(Some((head, 1)), false, true)
            .unwrap();
        assert!(!actions.frame_cancelled);
        assert!(actions.frame.is_some());
        let actions = input
            .poll_with_vision(Some((head, 1)), false, false)
            .unwrap();
        assert!(actions.frame_cancelled);
    }

    #[test]
    fn missing_frame_apis_keep_controller_actions_available_without_confirmation() {
        unsafe extern "C" fn update(
            _: *mut openvr_sys::VRActiveActionSet_t,
            _: u32,
            _: u32,
        ) -> i32 {
            0
        }
        unsafe extern "C" fn digital(
            action: u64,
            data: *mut openvr_sys::InputDigitalActionData_t,
            _: u32,
            _: u64,
        ) -> i32 {
            if action > 2 {
                return 3;
            }
            unsafe {
                (*data).bActive = true;
                (*data).activeOrigin = action;
            }
            0
        }
        let mut api: openvr_sys::VR_IVRInput_FnTable = unsafe { std::mem::zeroed() };
        api.UpdateActionState = Some(update);
        api.GetDigitalActionData = Some(digital);
        let mut input = input(&api);
        let head = super::super::transform::matrix(0., 0., 0., [0.; 3]);
        let actions = input
            .poll_with_vision(Some((head, 1)), false, false)
            .unwrap();
        assert!(actions.available);
        assert!(!actions.gesture_available);
        assert!(actions.frame.is_none());
        assert!(!actions.frame_confirmed);
    }

    #[test]
    fn reading_navigation_requires_focus_and_does_not_trigger_scan_or_clear() {
        unsafe extern "C" fn update(
            _: *mut openvr_sys::VRActiveActionSet_t,
            _: u32,
            _: u32,
        ) -> i32 {
            0
        }
        unsafe extern "C" fn digital(
            action: u64,
            data: *mut openvr_sys::InputDigitalActionData_t,
            _: u32,
            _: u64,
        ) -> i32 {
            unsafe {
                (*data).bActive = true;
                (*data).bChanged = true;
                (*data).bState = true;
                (*data).activeOrigin = action;
            }
            0
        }
        let mut api: openvr_sys::VR_IVRInput_FnTable = unsafe { std::mem::zeroed() };
        api.UpdateActionState = Some(update);
        api.GetDigitalActionData = Some(digital);
        let mut input = input(&api);
        input.reading = [7, 8, 9, 10];
        let held_since = Instant::now() - std::time::Duration::from_secs(1);
        input.scan_hold.update(Some(1), true, held_since);
        input.clear_hold.update(Some(2), true, held_since);
        let focused = input.poll_with_vision(None, true, false).unwrap();
        assert!(!focused.frame_confirmed && !focused.clear);
        assert_eq!(
            focused.navigation,
            [
                Action::PreviousBlock,
                Action::NextBlock,
                Action::PreviousPage,
                Action::NextPage
            ]
        );
        assert!(input
            .poll_with_vision(None, false, false)
            .unwrap()
            .navigation
            .is_empty());
    }

    #[test]
    fn manifests_bind_two_controller_corners_and_grips_to_optional_actions() {
        let directory =
            std::env::temp_dir().join(format!("vrcs-frame-bindings-{}", std::process::id()));
        let manifest = install_manifest(&directory).unwrap();
        let manifest: serde_json::Value =
            serde_json::from_slice(&std::fs::read(manifest).unwrap()).unwrap();
        assert!(manifest["actions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|action| action["name"] == "/actions/ocr/in/cancel"
                && action["requirement"] == "optional"));
        for controller in ["knuckles", "oculus_touch", "vive_controller"] {
            let binding: serde_json::Value = serde_json::from_slice(
                &std::fs::read(directory.join(format!("{controller}.json"))).unwrap(),
            )
            .unwrap();
            assert!(!serde_json::to_string(&binding)
                .unwrap()
                .contains("/actions/ocr/in/cancel"));
            let binding = &binding["bindings"]["/actions/ocr"];
            for side in ["left", "right"] {
                let pose = binding["poses"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|pose| pose["path"] == format!("/user/hand/{side}/pose/raw"))
                    .unwrap();
                assert_eq!(pose["output"], format!("/actions/ocr/in/{side}_pose"));
                let grip = binding["sources"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|source| source["path"] == format!("/user/hand/{side}/input/grip"))
                    .unwrap();
                let component = if controller == "knuckles" {
                    "grab"
                } else {
                    "click"
                };
                assert_eq!(
                    grip["inputs"][component]["output"],
                    format!("/actions/ocr/in/{side}_grip")
                );
                let axis = match controller {
                    "knuckles" => "thumbstick",
                    "oculus_touch" => "joystick",
                    _ => "trackpad",
                };
                let scroll = binding["sources"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|source| source["path"] == format!("/user/hand/{side}/input/{axis}"))
                    .expect("Each hand must have a default reader scroll binding");
                assert_eq!(
                    scroll["inputs"]["position"]["output"],
                    format!("/actions/ocr/in/{side}_scroll")
                );
                assert_eq!(
                    scroll["mode"],
                    if axis == "trackpad" {
                        "trackpad"
                    } else {
                        "joystick"
                    }
                );
                for (name, kind) in [("pose", "pose"), ("grip", "boolean"), ("scroll", "vector2")] {
                    let action = manifest["actions"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .find(|action| action["name"] == format!("/actions/ocr/in/{side}_{name}"))
                        .unwrap();
                    assert_eq!(action["type"], kind);
                    assert_eq!(action["requirement"], "optional");
                }
            }
        }
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn controller_corner_uses_head_coordinates_and_rejects_invalid_poses() {
        let mut api = api();
        let head = super::super::transform::matrix(0., 0., 0., [1., 2., 3.45]);
        let (origin, point) = read_controller(&api, 72, head, 1).unwrap();
        assert_eq!(origin, 22);
        for (actual, expected) in point.into_iter().zip([0., 0., -0.45]) {
            assert!((actual - expected).abs() < 0.0001);
        }
        unsafe extern "C" fn lost(
            _: u64,
            _: i32,
            _: f32,
            _: *mut openvr_sys::InputPoseActionData_t,
            _: u32,
            _: u64,
        ) -> i32 {
            0
        }
        api.GetPoseActionDataRelativeToNow = Some(lost);
        assert!(read_controller(&api, 72, head, 1).is_err());
        api.GetPoseActionDataRelativeToNow = None;
        assert!(read_controller(&api, 72, head, 1).is_err());
    }

    #[test]
    fn turning_away_does_not_reject_a_valid_controller_pose() {
        let api = api();
        let head = super::super::transform::matrix(0., 180., 0., [1., 2., 3.45]);
        let (origin, point) = read_controller(&api, 72, head, 1).unwrap();
        assert_eq!(origin, 22);
        for (actual, expected) in point.into_iter().zip([0., 0., 0.45]) {
            assert!((actual - expected).abs() < 0.0001);
        }
    }

    #[test]
    fn frame_anchors_use_the_head_pose_from_the_input_sample() {
        let head = super::super::transform::matrix(0., 90., 0., [1., 2., 3.]);
        let frame = Frame::from_head_corners(
            FrameCorners {
                corners: [[-0.2, 0.1, -0.5], [0.2, -0.1, -0.5]],
            },
            head,
            1,
        );
        assert_eq!(frame.head_pose, head);
        assert_eq!(frame.origin, 1);
        for (actual, expected) in frame
            .corners
            .into_iter()
            .zip([[0.5, 2.1, 3.2], [0.5, 1.9, 2.8]])
        {
            for (a, b) in actual.into_iter().zip(expected) {
                assert!((a - b).abs() < 0.0001, "{a} != {b}");
            }
        }
    }

    #[test]
    fn fixed_world_hands_keep_the_initial_frame_geometry_during_head_motion() {
        use super::super::{ocr_selection::Selection, transform};
        let head = transform::matrix(15., -25., 30., [1., 1.6, -2.]);
        let frame = Frame::from_head_corners(
            FrameCorners {
                corners: [[-0.2, 0.1, -0.4], [0.2, -0.1, -0.7]],
            },
            head,
            1,
        );
        let initial = Selection::from_corners(frame.corners, head, 7, 1).unwrap();
        let expected = initial.preview();
        let identity = transform::matrix(0., 0., 0., [0.; 3]);
        let mut selection = initial;
        for moved_head in [
            transform::matrix(-20., 10., -40., [1.1, 1.65, -1.9]),
            transform::matrix(0., 180., 0., [1., 1.6, -2.]),
            head,
        ] {
            let observed = FrameCorners {
                corners: frame
                    .corners
                    .map(|point| point_in_head(moved_head, identity, point)),
            };
            let updated = Frame::from_head_corners(observed, moved_head, 1);
            selection = selection.with_corners(updated.corners).unwrap();
            let plane = selection.preview();
            for (a, b) in plane
                .pose
                .iter()
                .flatten()
                .zip(expected.pose.iter().flatten())
            {
                assert!((a - b).abs() < 0.0001, "{a} != {b}");
            }
            assert!((plane.width_m - expected.width_m).abs() < 0.0001);
            assert!((plane.texel_aspect - expected.texel_aspect).abs() < 0.0001);
        }
    }

    unsafe extern "C" fn skeletal(
        action: u64,
        data: *mut openvr_sys::InputSkeletalActionData_t,
        size: u32,
    ) -> i32 {
        assert_eq!(action, 72);
        assert_eq!(
            size as usize,
            size_of::<openvr_sys::InputSkeletalActionData_t>()
        );
        unsafe {
            (*data).bActive = true;
            (*data).activeOrigin = 22;
        }
        0
    }
    unsafe extern "C" fn full(_: u64, level: *mut i32) -> i32 {
        unsafe {
            *level = 2;
        }
        0
    }
    unsafe extern "C" fn partial(_: u64, level: *mut i32) -> i32 {
        unsafe {
            *level = 1;
        }
        0
    }
    unsafe extern "C" fn bone_count(_: u64, count: *mut u32) -> i32 {
        unsafe {
            *count = 31;
        }
        0
    }
    unsafe extern "C" fn oversized(_: u64, count: *mut u32) -> i32 {
        unsafe {
            *count = 32;
        }
        0
    }
    unsafe extern "C" fn pose(
        action: u64,
        origin: i32,
        prediction: f32,
        data: *mut openvr_sys::InputPoseActionData_t,
        _: u32,
        device: u64,
    ) -> i32 {
        assert_eq!((action, origin, device), (72, 1, 0));
        assert_eq!(prediction, 0.);
        unsafe {
            (*data).bActive = true;
            (*data).activeOrigin = 22;
            (*data).pose.bPoseIsValid = true;
            (*data).pose.bDeviceIsConnected = true;
            (*data).pose.mDeviceToAbsoluteTracking.m =
                super::super::transform::matrix(0., 0., 0., [1., 2., 3.]);
        }
        0
    }
    unsafe extern "C" fn bones(
        _: u64,
        space: i32,
        range: i32,
        data: *mut openvr_sys::VRBoneTransform_t,
        count: u32,
    ) -> i32 {
        assert_eq!((space, range, count), (0, 0, 31));
        unsafe {
            for bone in std::slice::from_raw_parts_mut(data, 31) {
                bone.position.v = [0., 0., 0., 1.];
            }
            for (index, point) in [
                (1, [-0.18, -0.12, -0.45, 1.]),
                (2, [-0.18, -0.12, -0.45, 1.]),
                (5, [-0.11, -0.12, -0.45, 1.]),
                (6, [-0.18, -0.12, -0.45, 1.]),
                (10, [-0.18, -0.02, -0.45, 1.]),
            ] {
                (*data.add(index)).position.v = point;
            }
        }
        0
    }
    unsafe extern "C" fn summary(
        _: u64,
        kind: i32,
        data: *mut openvr_sys::VRSkeletalSummaryData_t,
    ) -> i32 {
        assert_eq!(kind, 0);
        unsafe {
            (*data).flFingerCurl = [0.1, 0.1, 0.8, 0.8, 0.8];
        }
        0
    }
    fn api() -> openvr_sys::VR_IVRInput_FnTable {
        let mut api: openvr_sys::VR_IVRInput_FnTable = unsafe { std::mem::zeroed() };
        api.GetSkeletalActionData = Some(skeletal);
        api.GetSkeletalTrackingLevel = Some(full);
        api.GetBoneCount = Some(bone_count);
        api.GetPoseActionDataRelativeToNow = Some(pose);
        api.GetSkeletalBoneData = Some(bones);
        api.GetSkeletalSummaryData = Some(summary);
        api
    }

    #[test]
    fn hand_input_uses_the_skeletal_action_pose_and_validates_its_fidelity_and_bone_count() {
        let head = super::super::transform::matrix(0., 0., 0., [1., 2., 3.]);
        let mut api = api();
        let hand = read_hand(&api, 72, head, 1).unwrap();
        assert_eq!(hand.origin, 22);
        for (actual, expected) in hand.wrist.into_iter().zip([-0.18, -0.12, -0.45]) {
            assert!((actual - expected).abs() < 0.0001);
        }
        assert!(hand.thumb_tip[0] > hand.thumb_base[0]);
        assert!(hand.index_tip[1] > hand.index_base[1]);
        assert_eq!(hand.curls, [0.1, 0.1, 0.8, 0.8, 0.8]);
        api.GetSkeletalTrackingLevel = Some(partial);
        api.GetBoneCount = None;
        assert_eq!(
            read_hand(&api, 72, head, 1).err().as_deref(),
            Some("Full finger tracking required")
        );
        api.GetSkeletalTrackingLevel = Some(full);
        api.GetBoneCount = Some(oversized);
        api.GetPoseActionDataRelativeToNow = None;
        assert_eq!(
            read_hand(&api, 72, head, 1).err().as_deref(),
            Some("Unsupported hand skeleton")
        );
    }

    #[test]
    fn opening_bindings_uses_the_current_app_and_resets_held_inputs() {
        unsafe extern "C" fn open(
            key: *mut std::ffi::c_char,
            set: u64,
            device: u64,
            desktop: bool,
        ) -> i32 {
            assert!(key.is_null());
            assert_eq!((set, device, desktop), (42, 0, false));
            0
        }
        let mut api = api();
        api.OpenBindingUI = Some(open);
        let mut input = input(&api);
        let now = Instant::now();
        for hold in [&mut input.scan_hold, &mut input.clear_hold] {
            hold.update(Some(1), false, now);
            hold.update(Some(1), true, now);
        }
        input.open_bindings().unwrap();
        for hold in [&mut input.scan_hold, &mut input.clear_hold] {
            assert!(!hold.update(Some(1), true, now + std::time::Duration::from_secs(2)));
        }
    }
}
