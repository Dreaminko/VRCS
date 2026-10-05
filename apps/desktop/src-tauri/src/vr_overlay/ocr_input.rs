use super::ocr_gesture::{camera_frame, point_in_head, HandSample};
use super::ocr_input_state::HoldAction;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::{ffi::CString, mem::size_of, path::Path, time::Instant};

pub struct InputActions {
    pub scan: bool,
    pub clear: bool,
    pub available: bool,
    pub gesture_available: bool,
    pub gesture_scan: bool,
    pub hands_in_view: bool,
}

pub struct OcrInput {
    table: *const openvr_sys::VR_IVRInput_FnTable,
    action_set: u64,
    scan: u64,
    clear: u64,
    scan_hold: HoldAction,
    clear_hold: HoldAction,
    hands: [u64; 2],
    gesture_hold: HoldAction,
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
        let binding = serde_json::json!({"controller_type":controller,"name":"VRCS OCR",
        "description":"Hold right trigger to scan; hold left trigger to clear. Customize to avoid game input conflicts.",
        "bindings":{"/actions/ocr":{"skeleton":[
            {"path":"/user/hand/left/input/skeleton/left","output":"/actions/ocr/in/left_hand"},
            {"path":"/user/hand/right/input/skeleton/right","output":"/actions/ocr/in/right_hand"}
        ],"sources":[
            {"path":"/user/hand/right/input/trigger","mode":"button","inputs":{"click":{"output":"/actions/ocr/in/scan"}}},
            {"path":"/user/hand/left/input/trigger","mode":"button","inputs":{"click":{"output":"/actions/ocr/in/clear"}}}
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
                {"name":"/actions/ocr/in/left_hand","type":"skeleton","skeleton":"/skeleton/hand/left","requirement":"optional"},
                {"name":"/actions/ocr/in/right_hand","type":"skeleton","skeleton":"/skeleton/hand/right","requirement":"optional"}],
            "default_bindings":bindings,
            "localization":[{"language_tag":"en_US","/actions/ocr":"VRCS OCR",
                "/actions/ocr/in/scan":"Scan / rescan (hold)","/actions/ocr/in/clear":"Clear OCR (hold)",
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
        self.gesture_hold = HoldAction::default();
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
            scan_hold: HoldAction::default(),
            clear_hold: HoldAction::default(),
            hands: [
                action("/actions/ocr/in/left_hand")?,
                action("/actions/ocr/in/right_hand")?,
            ],
            gesture_hold: HoldAction::default(),
            last_digital_state: None,
        })
    }

    pub fn poll(&mut self, tracking: Option<([[f32; 4]; 3], i32)>) -> Result<InputActions, String> {
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
            (data.bActive && data.activeOrigin != 0).then_some(data.activeOrigin)
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
        let gesture = hands.as_ref().is_some_and(camera_frame);
        let hands_in_view = hands.as_ref().is_some_and(|hands| {
            hands.iter().any(|hand| {
                [
                    hand.wrist,
                    hand.thumb_base,
                    hand.thumb_tip,
                    hand.index_base,
                    hand.index_tip,
                ]
                .iter()
                .any(|point| {
                    (-1.5..=-0.05).contains(&point[2])
                        && point[0].abs() < 0.75
                        && point[1].abs() < 0.65
                })
            })
        });
        Ok(InputActions {
            scan: self.scan_hold.update(origin(&scan), scan.bState, now),
            clear: self.clear_hold.update(origin(&clear), clear.bState, now),
            available: scan.bActive && clear.bActive,
            gesture_available: hands.is_some(),
            gesture_scan: self.gesture_hold.update(gesture_origin, gesture, now),
            hands_in_view,
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
        let mut input = OcrInput {
            table: &api,
            action_set: 42,
            scan: 1,
            clear: 2,
            hands: [72, 73],
            scan_hold: Default::default(),
            clear_hold: Default::default(),
            gesture_hold: Default::default(),
            last_digital_state: None,
        };
        let now = Instant::now();
        for hold in [
            &mut input.scan_hold,
            &mut input.clear_hold,
            &mut input.gesture_hold,
        ] {
            hold.update(Some(1), false, now);
            hold.update(Some(1), true, now);
        }
        input.open_bindings().unwrap();
        for hold in [
            &mut input.scan_hold,
            &mut input.clear_hold,
            &mut input.gesture_hold,
        ] {
            assert!(!hold.update(Some(1), true, now + std::time::Duration::from_secs(2)));
        }
    }
}
