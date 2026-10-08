use super::ocr_gesture::{camera_frame, gesture_frame, point_in_head, FrameCorners, HandSample};
use super::ocr_input_state::{FrameSession, FrameTracking, HoldAction};
use super::ocr_wrist::Action;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::{ffi::CString, mem::size_of, path::Path, time::Instant};

pub struct InputActions {
    pub scan: bool,
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
    reading: [u64; 4],
    scan_hold: HoldAction,
    clear_hold: HoldAction,
    hands: [u64; 2],
    grips: [u64; 2],
    poses: [u64; 2],
    frame_session: FrameSession,
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
        let binding = serde_json::json!({"controller_type":controller,"name":"VRCS OCR",
        "description":"Hold both grips to drag a frame. Hold right trigger to confirm / scan, left trigger to cancel / clear. Customize to avoid game input conflicts.",
        "bindings":{"/actions/ocr":{"poses":[
            {"path":"/user/hand/left/pose/raw","output":"/actions/ocr/in/left_pose"},
            {"path":"/user/hand/right/pose/raw","output":"/actions/ocr/in/right_pose"}
        ],"skeleton":[
            {"path":"/user/hand/left/input/skeleton/left","output":"/actions/ocr/in/left_hand"},
            {"path":"/user/hand/right/input/skeleton/right","output":"/actions/ocr/in/right_hand"}
        ],"sources":[
            {"path":"/user/hand/right/input/trigger","mode":"button","inputs":{"click":{"output":"/actions/ocr/in/scan"}}},
            {"path":"/user/hand/left/input/trigger","mode":"button","inputs":{"click":{"output":"/actions/ocr/in/clear"}}},
            grip("left"), grip("right")
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
                {"name":"/actions/ocr/in/previous_block","type":"boolean","requirement":"optional"},
                {"name":"/actions/ocr/in/next_block","type":"boolean","requirement":"optional"},
                {"name":"/actions/ocr/in/previous_page","type":"boolean","requirement":"optional"},
                {"name":"/actions/ocr/in/next_page","type":"boolean","requirement":"optional"},
                {"name":"/actions/ocr/in/left_grip","type":"boolean","requirement":"optional"},
                {"name":"/actions/ocr/in/right_grip","type":"boolean","requirement":"optional"},
                {"name":"/actions/ocr/in/left_pose","type":"pose","requirement":"optional"},
                {"name":"/actions/ocr/in/right_pose","type":"pose","requirement":"optional"},
                {"name":"/actions/ocr/in/left_hand","type":"skeleton","skeleton":"/skeleton/hand/left","requirement":"optional"},
                {"name":"/actions/ocr/in/right_hand","type":"skeleton","skeleton":"/skeleton/hand/right","requirement":"optional"}],
            "default_bindings":bindings,
            "localization":[{"language_tag":"en_US","/actions/ocr":"VRCS OCR",
                "/actions/ocr/in/scan":"Confirm frame / scan (hold)","/actions/ocr/in/clear":"Cancel frame / clear (hold)",
                "/actions/ocr/in/previous_block":"Previous OCR block (point at panel)",
                "/actions/ocr/in/next_block":"Next OCR block (point at panel)",
                "/actions/ocr/in/previous_page":"Previous OCR page (point at panel)",
                "/actions/ocr/in/next_page":"Next OCR page (point at panel)",
                "/actions/ocr/in/left_grip":"Frame drag left grip", "/actions/ocr/in/right_grip":"Frame drag right grip",
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
            poses: [
                action("/actions/ocr/in/left_pose").unwrap_or(0),
                action("/actions/ocr/in/right_pose").unwrap_or(0),
            ],
            frame_session: FrameSession::default(),
            last_digital_state: None,
        })
    }

    pub fn poll(
        &mut self,
        tracking: Option<([[f32; 4]; 3], i32)>,
        reading_focus: bool,
    ) -> Result<InputActions, String> {
        let tracking = tracking.filter(|_| !reading_focus);
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
        })?;
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
            let frame = gesture_frame(hands, scan.bActive && scan.activeOrigin != 0 && scan.bState);
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
            let grips = [read(self.grips[0]).ok()?, read(self.grips[1]).ok()?];
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
        let scan = self.scan_hold.update(origin(&scan), scan.bState, now);
        let clear = self.clear_hold.update(origin(&clear), clear.bState, now);
        let frame = self
            .frame_session
            .update([gesture_tracking, controllers], scan, clear, now);
        Ok(InputActions {
            scan,
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
        || head
            .iter()
            .chain(device.iter())
            .flatten()
            .any(|value| !value.is_finite())
    {
        return Err("Controller tracking inactive".into());
    }
    let point = point_in_head(head, device, [0.; 3]);
    if point.iter().any(|value| !value.is_finite()) {
        return Err("Invalid controller position".into());
    }
    Ok((data.activeOrigin, point))
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

    fn input(api: &openvr_sys::VR_IVRInput_FnTable) -> OcrInput {
        OcrInput {
            table: api,
            action_set: 42,
            scan: 1,
            clear: 2,
            reading: [0; 4],
            hands: [72, 73],
            grips: [3, 4],
            poses: [5, 6],
            scan_hold: Default::default(),
            clear_hold: Default::default(),
            frame_session: Default::default(),
            last_digital_state: None,
        }
    }

    #[test]
    fn optional_frame_apis_do_not_disable_scan_and_clear() {
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
        let actions = input.poll(Some((head, 1)), false).unwrap();
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
        let focused = input.poll(None, true).unwrap();
        assert!(!focused.scan && !focused.clear);
        assert_eq!(
            focused.navigation,
            [
                Action::PreviousBlock,
                Action::NextBlock,
                Action::PreviousPage,
                Action::NextPage
            ]
        );
        assert!(input.poll(None, false).unwrap().navigation.is_empty());
    }

    #[test]
    fn manifests_bind_two_controller_corners_and_grips_to_optional_actions() {
        let directory =
            std::env::temp_dir().join(format!("vrcs-frame-bindings-{}", std::process::id()));
        let manifest = install_manifest(&directory).unwrap();
        let manifest: serde_json::Value =
            serde_json::from_slice(&std::fs::read(manifest).unwrap()).unwrap();
        for controller in ["knuckles", "oculus_touch", "vive_controller"] {
            let binding: serde_json::Value = serde_json::from_slice(
                &std::fs::read(directory.join(format!("{controller}.json"))).unwrap(),
            )
            .unwrap();
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
                for (name, kind) in [("pose", "pose"), ("grip", "boolean")] {
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
