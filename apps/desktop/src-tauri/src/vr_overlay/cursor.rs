//! One visual cursor shared by the in-game panels. It never requests input focus.
use openvr_sys::{HmdMatrix34_t, HmdVector2_t, VR_IVROverlay_FnTable};

const CURSOR_SORT_ORDER: u32 = 100;
const CURSOR_SURFACE_OFFSET_M: f32 = 0.003;

pub fn frame_interval(refresh_hz: f32) -> std::time::Duration {
    let hz = if refresh_hz.is_finite() && refresh_hz > 0. {
        refresh_hz.clamp(60., 240.)
    } else {
        90.
    };
    std::time::Duration::from_secs_f32(1. / hz)
}

#[derive(Default)]
pub struct CursorOverlay {
    handle: Option<u64>,
    pub target: Option<u64>,
    visible: bool,
}

fn check(error: i32) -> Result<(), String> {
    if error == openvr_sys::EVROverlayError_VROverlayError_None {
        Ok(())
    } else {
        Err(format!("Cursor overlay failed with code {error}"))
    }
}

impl CursorOverlay {
    fn ensure(&mut self, api: &VR_IVROverlay_FnTable) -> Result<u64, String> {
        if let Some(handle) = self.handle {
            return Ok(handle);
        }
        let mut handle = 0;
        unsafe {
            check(api.CreateOverlay.ok_or("Cursor overlays unavailable")?(
                c"org.vrcs.overlay.cursor".as_ptr().cast_mut(),
                c"VRCS Cursor".as_ptr().cast_mut(),
                &mut handle,
            ))?;
        }
        self.handle = Some(handle);
        let result = (|| unsafe {
            check(api
                .SetOverlayInputMethod
                .ok_or("Cursor input configuration unavailable")?(
                handle,
                openvr_sys::VROverlayInputMethod_None,
            ))?;
            check(api
                .SetOverlaySortOrder
                .ok_or("Cursor sort order unavailable")?(
                handle,
                CURSOR_SORT_ORDER,
            ))?;
            let pixels = texture();
            check(api
                .SetOverlayRaw
                .ok_or("Cursor texture upload unavailable")?(
                handle,
                pixels.as_ptr().cast_mut().cast(),
                64,
                64,
                4,
            ))
        })();
        if let Err(error) = result {
            self.destroy(api);
            return Err(error);
        }
        Ok(handle)
    }

    pub fn update(
        &mut self,
        api: &VR_IVROverlay_FnTable,
        target: u64,
        position: [f32; 3],
        origin: i32,
        texture_width: u32,
        diameter_px: u32,
    ) -> Result<(), String> {
        let result = (|| unsafe {
            if texture_width == 0 || !position.iter().all(|v| v.is_finite()) {
                return Err("Invalid cursor coordinates".into());
            }
            let handle = self.ensure(api)?;
            let mut pose = HmdMatrix34_t { m: [[0.; 4]; 3] };
            // Only use the panel orientation; the ray hit supplies the position.
            check(api
                .GetTransformForOverlayCoordinates
                .ok_or("Cursor coordinates unavailable")?(
                target,
                origin,
                HmdVector2_t { v: [0.5, 0.5] },
                &mut pose,
            ))?;
            let mut width = 0.;
            check(api
                .GetOverlayWidthInMeters
                .ok_or("Cursor target width unavailable")?(
                target, &mut width
            ))?;
            // Match the preview's ring and outline at the requested diameter.
            check(api
                .SetOverlayWidthInMeters
                .ok_or("Cursor width unavailable")?(
                handle,
                width * diameter_px as f32 / texture_width as f32,
            ))?;
            // SteamVR returns the panel's +Z-facing normal here.
            for (row, coordinate) in pose.m.iter_mut().zip(position) {
                row[3] = coordinate + row[2] * CURSOR_SURFACE_OFFSET_M;
            }
            check(api
                .SetOverlayTransformAbsolute
                .ok_or("Cursor transform unavailable")?(
                handle, origin, &mut pose,
            ))?;
            if !self.visible {
                check(api.ShowOverlay.ok_or("Cursor visibility unavailable")?(
                    handle,
                ))?;
                self.visible = true;
            }
            self.target = Some(target);
            Ok(())
        })();
        if result.is_err() {
            let _ = self.hide(api);
        }
        result
    }

    pub fn hide(&mut self, api: &VR_IVROverlay_FnTable) -> Result<(), String> {
        if self.visible {
            unsafe {
                check(api.HideOverlay.ok_or("Cursor visibility unavailable")?(
                    self.handle.expect("visible cursor"),
                ))?;
            }
            self.visible = false;
        }
        self.target = None;
        Ok(())
    }

    pub fn destroy(&mut self, api: &VR_IVROverlay_FnTable) {
        if let Some(handle) = self.handle.take() {
            if let Some(destroy) = api.DestroyOverlay {
                unsafe {
                    let _ = destroy(handle);
                }
            }
        }
        self.target = None;
        self.visible = false;
    }
}

fn texture() -> Vec<u8> {
    (0..64 * 64)
        .flat_map(|i| {
            let x = (i % 64) as f32 + 0.5 - 32.;
            let y = (i / 64) as f32 + 0.5 - 32.;
            let distance = x.hypot(y) * 24. / 64.;
            let coverage = |radius: f32| ((radius - distance) * 64. / 24. + 0.5).clamp(0., 1.);
            let fill = coverage(7.) * 0.35;
            let ring = coverage(10.) - coverage(7.);
            let outline = (coverage(12.) - coverage(10.)) * 0.6;
            let alpha = fill + ring + outline;
            let color = |fill_color: f32, outline_color: f32| {
                if alpha > 0. {
                    ((fill_color * fill + 255. * ring + outline_color * outline) / alpha).round()
                        as u8
                } else {
                    0
                }
            };
            [
                color(116., 15.),
                color(214., 23.),
                color(255., 42.),
                (alpha * 255.).round() as u8,
            ]
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};

    thread_local! {
        static CALLS: RefCell<Vec<&'static str>> = const { RefCell::new(Vec::new()) };
        static FAIL: Cell<&'static str> = const { Cell::new("") };
        static POSE: Cell<[[f32; 4]; 3]> = const { Cell::new([[0.; 4]; 3]) };
        static TARGET_POSE: Cell<[[f32; 4]; 3]> = const { Cell::new([[0.; 4]; 3]) };
        static WIDTH: Cell<f32> = const { Cell::new(0.) };
        static INPUT: Cell<i32> = const { Cell::new(-1) };
        static SORT: Cell<u32> = const { Cell::new(0) };
    }

    fn call(name: &'static str) -> i32 {
        CALLS.with_borrow_mut(|calls| calls.push(name));
        if FAIL.get() == name {
            10
        } else {
            0
        }
    }

    fn api() -> VR_IVROverlay_FnTable {
        unsafe extern "C" fn create(
            _: *mut std::ffi::c_char,
            _: *mut std::ffi::c_char,
            handle: *mut u64,
        ) -> i32 {
            unsafe {
                *handle = 91;
            }
            call("create")
        }
        unsafe extern "C" fn raw(_: u64, _: *mut std::ffi::c_void, _: u32, _: u32, _: u32) -> i32 {
            call("texture")
        }
        unsafe extern "C" fn input(_: u64, method: i32) -> i32 {
            INPUT.set(method);
            call("input")
        }
        unsafe extern "C" fn sort(_: u64, sort_order: u32) -> i32 {
            SORT.set(sort_order);
            call("sort")
        }
        unsafe extern "C" fn width(_: u64, width: f32) -> i32 {
            WIDTH.set(width);
            call("width")
        }
        unsafe extern "C" fn get_width(_: u64, width: *mut f32) -> i32 {
            unsafe {
                *width = 0.18;
            }
            call("get_width")
        }
        unsafe extern "C" fn coordinates(
            _: u64,
            _: i32,
            _: HmdVector2_t,
            pose: *mut HmdMatrix34_t,
        ) -> i32 {
            unsafe {
                (*pose).m = TARGET_POSE.get();
            }
            call("coordinates")
        }
        unsafe extern "C" fn transform(_: u64, _: i32, pose: *mut HmdMatrix34_t) -> i32 {
            POSE.set(unsafe { (*pose).m });
            call("transform")
        }
        unsafe extern "C" fn show(_: u64) -> i32 {
            call("show")
        }
        unsafe extern "C" fn hide(_: u64) -> i32 {
            call("hide")
        }
        unsafe extern "C" fn destroy(_: u64) -> i32 {
            call("destroy")
        }
        let mut api: VR_IVROverlay_FnTable = unsafe { std::mem::zeroed() };
        api.CreateOverlay = Some(create);
        api.SetOverlayRaw = Some(raw);
        api.SetOverlayInputMethod = Some(input);
        api.SetOverlaySortOrder = Some(sort);
        api.SetOverlayWidthInMeters = Some(width);
        api.GetOverlayWidthInMeters = Some(get_width);
        api.GetTransformForOverlayCoordinates = Some(coordinates);
        api.SetOverlayTransformAbsolute = Some(transform);
        api.ShowOverlay = Some(show);
        api.HideOverlay = Some(hide);
        api.DestroyOverlay = Some(destroy);
        CALLS.with_borrow_mut(Vec::clear);
        FAIL.set("");
        SORT.set(0);
        TARGET_POSE.set([[1., 0., 0., 1.], [0., 1., 0., 2.], [0., 0., 1., 3.]]);
        api
    }

    #[test]
    fn switching_panels_reuses_one_handle_and_texture_without_input_focus() {
        let api = api();
        let mut cursor = CursorOverlay::default();
        cursor.update(&api, 1, [1.2, 2.3, 3.], 1, 1024, 30).unwrap();
        assert!((WIDTH.get() - 0.18 * 30. / 1024.).abs() < 0.00001);
        cursor.update(&api, 2, [-0.4, 1.7, 3.], 1, 768, 24).unwrap();
        assert_eq!(cursor.target, Some(2));
        assert_eq!(POSE.get()[0][3], -0.4);
        assert_eq!(POSE.get()[1][3], 1.7);
        assert!((WIDTH.get() - 0.18 * 24. / 768.).abs() < 0.00001);
        assert!((POSE.get()[2][3] - 3.003).abs() < 0.00001);
        assert_eq!(INPUT.get(), openvr_sys::VROverlayInputMethod_None);
        assert_eq!(SORT.get(), CURSOR_SORT_ORDER);
        CALLS.with_borrow(|calls| {
            for name in ["create", "texture", "show"] {
                assert_eq!(calls.iter().filter(|&&call| call == name).count(), 1);
            }
        });
        cursor.hide(&api).unwrap();
        assert!(!cursor.visible);
        assert!(cursor.target.is_none());
        cursor.destroy(&api);
        assert!(cursor.handle.is_none());
    }

    #[test]
    fn cursor_stays_in_front_of_the_target_plane() {
        let api = api();
        let mut cursor = CursorOverlay::default();
        for target in [
            super::super::transform::matrix(0., 0., 0., [0., 0., -1.]),
            super::super::transform::matrix(-90., 0., 0., [0., 0.5, -0.5]),
            super::super::transform::matrix(-35., 25., -10., [1., 2., 3.]),
        ] {
            TARGET_POSE.set(target);
            let position = std::array::from_fn(|i| target[i][3]);
            cursor.update(&api, 1, position, 1, 1024, 24).unwrap();
            let cursor_pose = POSE.get();
            let distance: f32 = (0..3)
                .map(|i| (cursor_pose[i][3] - target[i][3]) * target[i][2])
                .sum();
            assert!(
                distance > 0.,
                "cursor is behind the target plane: {distance}"
            );
            assert!((distance - CURSOR_SURFACE_OFFSET_M).abs() < 0.00001);
        }
    }

    #[test]
    fn cursor_matches_ray_hits_across_a_wide_and_tilted_panel() {
        let api = api();
        let mut cursor = CursorOverlay::default();
        let front = [[1., 0., 0., 1.], [0., 1., 0., 2.], [0., 0., 1., 3.]];
        let wrist = [[1., 0., 0., 1.], [0., 0., 1., 2.], [0., -1., 0., 3.]];
        let tilted = [[1., 0., 0., 1.], [0., 0.8, 0.6, 2.], [0., -0.6, 0.8, 3.]];
        // Hand-derived hits on a 0.18 m wide, 1024 x 256 panel.
        for (target, position, expected) in [
            (front, [0.946, 2.018, 3.], [0.946, 2.018, 3.003]),
            (front, [1.054, 1.982, 3.], [1.054, 1.982, 3.003]),
            (wrist, [0.946, 2., 2.982], [0.946, 2.003, 2.982]),
            (tilted, [1.054, 1.9856, 3.0108], [1.054, 1.9874, 3.0132]),
        ] {
            TARGET_POSE.set(target);
            cursor.update(&api, 1, position, 1, 1024, 30).unwrap();
            let actual = POSE.get();
            for i in 0..3 {
                assert!(
                    (actual[i][3] - expected[i]).abs() < 0.00001,
                    "cursor does not match ray hit {position:?}: {actual:?}"
                );
                assert_eq!(&actual[i][..3], &target[i][..3]);
            }
        }
    }

    #[test]
    fn failed_position_hides_the_previous_cursor_and_can_recover() {
        let api = api();
        let mut cursor = CursorOverlay::default();
        cursor.update(&api, 1, [1., 2., 3.], 1, 1024, 24).unwrap();
        FAIL.set("coordinates");
        assert!(cursor.update(&api, 2, [1., 2., 3.], 1, 1024, 24).is_err());
        assert!(!cursor.visible);
        assert!(cursor.target.is_none());
        FAIL.set("");
        cursor.update(&api, 2, [1., 2., 3.], 1, 1024, 24).unwrap();
        assert!(cursor.visible);
        FAIL.set("hide");
        assert!(cursor.hide(&api).is_err());
        assert!(cursor.visible, "a failed hide must be retried");
        FAIL.set("");
        cursor.hide(&api).unwrap();
        assert!(!cursor.visible);
    }

    #[test]
    fn failed_creation_cleans_up_and_invalid_coordinates_hide() {
        let api = api();
        let mut cursor = CursorOverlay::default();
        FAIL.set("texture");
        assert!(cursor.update(&api, 1, [1., 2., 3.], 1, 1024, 24).is_err());
        assert!(cursor.handle.is_none());
        CALLS.with_borrow(|calls| assert!(calls.contains(&"destroy")));
        FAIL.set("");
        cursor.update(&api, 1, [1., 2., 3.], 1, 1024, 24).unwrap();
        for position in [
            [f32::NAN, 0., 0.],
            [0., f32::INFINITY, 0.],
            [0., 0., f32::NEG_INFINITY],
        ] {
            assert!(cursor.update(&api, 1, position, 1, 1024, 24).is_err());
            assert!(!cursor.visible);
        }
    }

    #[test]
    fn cursor_interval_follows_the_headset_and_has_a_finite_fallback() {
        for hz in [72., 90., 120., 144.] {
            assert!((frame_interval(hz).as_secs_f32() - 1. / hz).abs() < 0.000001);
        }
        for hz in [0., -1., f32::NAN, f32::INFINITY] {
            assert_eq!(frame_interval(hz), frame_interval(90.));
        }
    }
}
