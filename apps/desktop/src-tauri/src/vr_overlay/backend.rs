#[cfg(windows)]
mod platform {
    use openvr::overlay::OverlayHandle;
    use openvr::pose::Matrix3x4;
    use openvr::tracked_device_index;
    use openvr::{ApplicationType, Context, Overlay, System, TrackedControllerRole};
    use vrcs_core::{VrOverlayHeadsetConfig, VrOverlayWristConfig};

    use super::{ControllerBinding, OverlayKind};
    use crate::vr_overlay::d3d11_texture::{Device, OverlayTexture};
    use crate::vr_overlay::renderer::Texture;
    use crate::vr_overlay::transform;

    const HEADSET_KEY: &str = "org.vrcs.overlay.headset\0";
    const HEADSET_NAME: &str = "VRCS Headset Subtitles\0";
    const WRIST_KEY: &str = "org.vrcs.overlay.wrist\0";
    const WRIST_NAME: &str = "VRCS Wrist Subtitles\0";

    pub struct OpenVrBackend {
        context: Context,
        system: System,
        overlay: Overlay,
        raw_overlay: *const openvr_sys::VR_IVROverlay_FnTable,
        texture_device: Option<Device>,
        headset: Option<OverlayHandle>,
        wrist: Option<OverlayHandle>,
        headset_state: SubmittedState,
        wrist_state: SubmittedState,
    }

    #[derive(Default)]
    struct SubmittedState {
        visible: bool,
        opacity: Option<f32>,
        texture: Option<OverlayTexture>,
        d3d11_disabled: bool,
    }

    impl OpenVrBackend {
        pub fn runtime_installed() -> bool {
            openvr::is_runtime_installed()
        }

        pub fn hmd_present() -> bool {
            openvr::is_hmd_present()
        }

        pub fn connect() -> Result<Self, String> {
            let context = unsafe { openvr::init(ApplicationType::Background) }
                .map_err(|error| format!("OpenVR initialization failed: {error:?}"))?;
            let system = context
                .system()
                .map_err(|error| format!("OpenVR system interface failed: {error:?}"))?;
            let overlay = context
                .overlay()
                .map_err(|error| format!("OpenVR overlay interface failed: {error:?}"))?;
            let raw_overlay = load_raw_overlay()?;
            let texture_device = create_texture_device();
            Ok(Self {
                context,
                system,
                overlay,
                raw_overlay,
                texture_device,
                headset: None,
                wrist: None,
                headset_state: SubmittedState::default(),
                wrist_state: SubmittedState::default(),
            })
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

        pub fn ensure_wrist(
            &mut self,
            config: &VrOverlayWristConfig,
        ) -> Result<ControllerBinding, String> {
            let (role_name, role) = controller_role(config);
            let Some(device) = self.system.tracked_device_index_for_controller_role(role) else {
                self.hide(OverlayKind::Wrist);
                return Ok(ControllerBinding {
                    role: Some(role_name),
                    available: false,
                });
            };
            if !self.system.is_tracked_device_connected(device) {
                self.hide(OverlayKind::Wrist);
                return Ok(ControllerBinding {
                    role: Some(role_name),
                    available: false,
                });
            }

            if self.wrist.is_none() {
                let handle = self
                    .overlay
                    .create_overlay(WRIST_KEY, WRIST_NAME)
                    .map_err(|error| format!("Create wrist overlay failed: {error:?}"))?;
                self.wrist = Some(handle);
            }
            let handle = self.wrist.expect("wrist overlay exists");
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

        pub fn upload(&mut self, kind: OverlayKind, texture: &Texture) -> Result<(), String> {
            let handle = self
                .handle(kind)
                .ok_or_else(|| "Overlay is not created".to_string())?;
            if self.texture_device.is_some() && !self.state(kind).d3d11_disabled {
                if let Err(error) = self.upload_d3d11(kind, handle, texture) {
                    tracing::warn!(error, "D3D11 overlay upload failed; using raw uploads");
                    self.upload_raw(handle, texture)?;
                    let state = self.state_mut(kind);
                    state.texture = None;
                    state.d3d11_disabled = true;
                }
                return Ok(());
            }
            self.upload_raw(handle, texture)
        }

        fn upload_d3d11(
            &mut self,
            kind: OverlayKind,
            handle: OverlayHandle,
            source: &Texture,
        ) -> Result<(), String> {
            let device = self.texture_device.as_ref().expect("D3D11 device exists");
            if let Some(texture) = self.state(kind).texture.as_ref() {
                device.copy_texture(texture, source)?;
                tracing::info!(
                    ?kind,
                    shared_handle = texture.shared_handle() as usize,
                    submitted = false,
                    "VR Overlay diagnostic: shared texture updated"
                );
            } else {
                let texture = device.create_shared_texture(source)?;
                submit_shared_texture(self.raw_overlay, handle, texture.shared_handle())?;
                let shared_handle = texture.shared_handle() as usize;
                self.state_mut(kind).texture = Some(texture);
                tracing::info!(
                    ?kind,
                    shared_handle,
                    submitted = true,
                    width = source.width,
                    height = source.height,
                    format = "BGRA8",
                    "VR Overlay diagnostic: shared texture submitted"
                );
            }
            Ok(())
        }

        fn upload_raw(&mut self, handle: OverlayHandle, texture: &Texture) -> Result<(), String> {
            self.overlay
                .set_raw_data(
                    handle,
                    &texture.pixels,
                    texture.width as usize,
                    texture.height as usize,
                    4,
                )
                .map_err(|error| format!("Upload overlay texture failed: {error:?}"))
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
            self.hide(OverlayKind::Headset);
            self.hide(OverlayKind::Wrist);
        }

        pub fn reset(&mut self, kind: OverlayKind) {
            self.hide(kind);
            self.destroy(kind);
        }

        fn handle(&self, kind: OverlayKind) -> Option<OverlayHandle> {
            match kind {
                OverlayKind::Headset => self.headset,
                OverlayKind::Wrist => self.wrist,
            }
        }

        fn state(&self, kind: OverlayKind) -> &SubmittedState {
            match kind {
                OverlayKind::Headset => &self.headset_state,
                OverlayKind::Wrist => &self.wrist_state,
            }
        }

        fn state_mut(&mut self, kind: OverlayKind) -> &mut SubmittedState {
            match kind {
                OverlayKind::Headset => &mut self.headset_state,
                OverlayKind::Wrist => &mut self.wrist_state,
            }
        }

        fn destroy(&mut self, kind: OverlayKind) {
            let handle = match kind {
                OverlayKind::Headset => self.headset.take(),
                OverlayKind::Wrist => self.wrist.take(),
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
            self.hide_all();
            self.destroy(OverlayKind::Headset);
            self.destroy(OverlayKind::Wrist);
            let _ = &self.context;
        }
    }

    fn create_texture_device() -> Option<Device> {
        let adapter_index = match dxgi_adapter_index() {
            Ok(index) => index,
            Err(error) => {
                tracing::warn!(error, "Unable to select SteamVR GPU; using raw uploads");
                return None;
            }
        };

        match Device::create(adapter_index) {
            Ok(device) => {
                tracing::info!(adapter_index, "VR Overlay D3D11 device initialized");
                Some(device)
            }
            Err(error) => {
                tracing::warn!(error, adapter_index, "D3D11 unavailable; using raw uploads");
                None
            }
        }
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
        let mut texture = openvr_sys::Texture_t {
            handle: shared_handle,
            eType: openvr_sys::ETextureType_TextureType_DXGISharedHandle,
            eColorSpace: openvr_sys::EColorSpace_ColorSpace_Auto,
        };
        let error = unsafe {
            (&*raw_overlay)
                .SetOverlayTexture
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
}

#[cfg(not(windows))]
mod platform {
    use vrcs_core::{VrOverlayHeadsetConfig, VrOverlayWristConfig};

    use super::{ControllerBinding, OverlayKind};
    use crate::vr_overlay::renderer::Texture;

    pub struct OpenVrBackend;

    impl OpenVrBackend {
        pub fn runtime_installed() -> bool {
            false
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
        pub fn upload(&mut self, _: OverlayKind, _: &Texture) -> Result<(), String> {
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
    }
}

pub use platform::OpenVrBackend;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverlayKind {
    Headset,
    Wrist,
}

#[derive(Debug, Clone)]
pub struct ControllerBinding {
    pub role: Option<String>,
    pub available: bool,
}
