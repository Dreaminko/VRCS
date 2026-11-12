//! XR0 probe: independent overlay session on the active OpenXR runtime.
//!
//! Proves, without touching the production Manager, that an overlay session can
//! be created, that a quad layer reaches the compositor while a game runs, and
//! that controller actions stay readable. Each stage prints one JSON event so a
//! failure names the exact API boundary instead of "VR unavailable".

use std::ptr;

use ash::vk::{self, Handle as _};
use openxr as xr;
use openxr::sys::Handle as _;

use super::pose::{pose_from_quaternion, DeviceId, PointerId, PointerInput, SpaceId, TrackedPose};

const VIEW_TYPE: xr::ViewConfigurationType = xr::ViewConfigurationType::PRIMARY_STEREO;
const WIDTH: u32 = 1024;
const PANEL_Z: f32 = -1.2;
const PANEL_W: f32 = 0.4;
const PANEL_H: f32 = 0.1;
const MAX_TEXTURE: u32 = 2048;
const HEIGHT: u32 = 256;
const PANEL_COUNT: usize = 9;

/// Quad layer slots exposed by the runtime. The production backend maps its
/// overlay kinds onto these; the prototype only uses `Headset`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanelId {
    Headset,
    Wrist,
    WristMenu,
    OcrWrist,
    OcrProgress,
    PlayerAudio,
    OcrFrame,
    OcrResult,
    Dashboard,
}

fn panel_index(kind: PanelId) -> usize {
    kind as usize
}

/// Reference space a quad layer is anchored to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanelSpace {
    /// Head-locked.
    View,
    /// Room-scale world anchor.
    Stage,
    /// One controller's grip space (0 left, 1 right).
    Grip(usize),
}

#[derive(Debug, Clone, Copy)]
pub struct PanelInfo {
    pub space: PanelSpace,
    pub pose: xr::Posef,
    pub size: xr::Extent2Df,
    pub visible: bool,
    pub has_image: bool,
}

fn emit(value: serde_json::Value) {
    println!("{value}");
}

fn locate_in_stage(space: &xr::Space, stage: &xr::Space, time: xr::Time) -> Option<TrackedPose> {
    let location = space.locate(stage, time).ok()?;
    let flags = location.location_flags;
    if !flags.contains(
        xr::SpaceLocationFlags::POSITION_VALID
            | xr::SpaceLocationFlags::ORIENTATION_VALID
            | xr::SpaceLocationFlags::POSITION_TRACKED
            | xr::SpaceLocationFlags::ORIENTATION_TRACKED,
    ) {
        return None;
    }
    let position = location.pose.position;
    let orientation = location.pose.orientation;
    Some(TrackedPose {
        pose: pose_from_quaternion(
            [position.x, position.y, position.z],
            [orientation.x, orientation.y, orientation.z, orientation.w],
        ),
        space: SpaceId(0),
    })
}

/// One overlay session plus the Vulkan resources that feed it. Production code
/// drives this through `pump`/`wait_frame`/`upload`/`submit`; the prototype
/// below is only a driver.
pub struct XrRuntime {
    /// Session and its child handles; taken first on drop so they are released
    /// while the Vulkan device still exists.
    handles: Option<Handles>,
    device: ash::Device,
    vk_instance: ash::Instance,
    pool: vk::CommandPool,
    command: vk::CommandBuffer,
    queue: vk::Queue,
    physical_device: vk::PhysicalDevice,
    stagings: [Option<Staging>; PANEL_COUNT],
    frame: Option<(xr::Time, bool)>,
    last_layers: usize,
    last_layer_report: std::time::Instant,
    running: bool,
}

struct Panel {
    pose: xr::Posef,
    size: xr::Extent2Df,
    space: PanelSpace,
    visible: bool,
    swapchain: Option<xr::Swapchain<xr::Vulkan>>,
    images: Vec<u64>,
    texture_size: [u32; 2],
    pixels: Vec<u8>,
    opacity: f32,
    has_image: bool,
    pending: bool,
}

impl Panel {
    fn new(visible: bool) -> Self {
        Self {
            pose: xr::Posef {
                orientation: xr::Quaternionf::IDENTITY,
                position: xr::Vector3f {
                    x: 0.0,
                    y: 0.0,
                    z: PANEL_Z,
                },
            },
            size: xr::Extent2Df {
                width: 0.8,
                height: 0.2,
            },
            space: PanelSpace::View,
            visible,
            swapchain: None,
            images: Vec::new(),
            texture_size: [0, 0],
            pixels: Vec::new(),
            opacity: 1.0,
            has_image: false,
            pending: false,
        }
    }
}

struct Handles {
    // Field order is drop order: children before the session, session before
    // the instance the runtime destroys last.
    panels: [Panel; PANEL_COUNT],
    format: u32,
    actions: Actions,
    view: xr::Space,
    stage: xr::Space,
    stream: xr::FrameStream<xr::Vulkan>,
    waiter: xr::FrameWaiter,
    session: xr::Session<xr::Vulkan>,
    instance: xr::Instance,
}

/// Check the installed loader and runtime for the two required extensions.
/// Creates and drops a short-lived instance so a loader with a missing runtime
/// service is not reported as usable.
pub fn probe() -> Result<(), String> {
    let entry = unsafe { xr::Entry::load(&()) }.map_err(|_| "vr.openxr_loader_unavailable")?;
    let available = entry
        .enumerate_extensions()
        .map_err(|error| format!("OpenXR extension query failed: {error:?}"))?;
    if !available.extx_overlay {
        return Err("vr.openxr_overlay_unsupported".into());
    }
    if !available.khr_vulkan_enable2 {
        return Err("vr.openxr_vulkan_unavailable".into());
    }
    let mut enabled = xr::ExtensionSet::default();
    enabled.extx_overlay = true;
    enabled.khr_vulkan_enable2 = true;
    let instance = entry
        .create_instance(
            &xr::ApplicationInfo {
                api_version: xr::Version::new(1, 0, 32),
                application_name: "vrcs",
                application_version: 0,
                engine_name: "vrcs",
                engine_version: 0,
            },
            &enabled,
            &[],
            &(),
        )
        .map_err(|error| format!("OpenXR runtime unavailable: {error:?}"))?;
    drop(instance);
    Ok(())
}

impl XrRuntime {
    /// Overlay instance, Vulkan device and session. Fails at the first API step
    /// that rejects us so the caller can report an exact reason.
    pub fn open(verbose: bool) -> Result<Self, String> {
        if !cfg!(target_arch = "x86_64") {
            return Err("vr.runtime_arch_unverified".into());
        }
        let emit = |value: serde_json::Value| {
            if verbose {
                println!("{value}");
            }
        };
        let entry = unsafe { xr::Entry::load(&()) }.map_err(|error| {
            emit(serde_json::json!({"event":"entry","ok":false,"error":format!("{error:?}")}));
            "vr.openxr_loader_unavailable"
        })?;
        let available = entry
            .enumerate_extensions()
            .map_err(|error| format!("OpenXR extension query failed: {error:?}"))?;
        emit(serde_json::json!({
            "event": "extensions",
            "overlay": available.extx_overlay,
            "vulkan": available.khr_vulkan_enable2,
            "hand_tracking": available.ext_hand_tracking,
        }));
        if !available.extx_overlay {
            return Err("vr.openxr_overlay_unsupported".into());
        }
        if !available.khr_vulkan_enable2 {
            return Err("vr.openxr_vulkan_unavailable".into());
        }

        let mut enabled = xr::ExtensionSet::default();
        enabled.extx_overlay = true;
        enabled.khr_vulkan_enable2 = true;
        // Hand-tracked controllers expose the hand interaction profile.
        enabled.ext_hand_interaction = available.ext_hand_interaction;
        let instance = entry
            .create_instance(
                &xr::ApplicationInfo {
                    api_version: xr::Version::new(1, 0, 32),
                    application_name: "vrcs",
                    application_version: 0,
                    engine_name: "vrcs",
                    engine_version: 0,
                },
                &enabled,
                &[],
                &(),
            )
            .map_err(|error| format!("OpenXR instance creation failed: {error:?}"))?;
        let properties = instance
            .properties()
            .map_err(|error| format!("OpenXR runtime query failed: {error:?}"))?;
        emit(serde_json::json!({
            "event": "instance",
            "runtime": properties.runtime_name,
            "version": format!("{}.{}.{}",
                properties.runtime_version.major(),
                properties.runtime_version.minor(),
                properties.runtime_version.patch()),
        }));

        let system = match instance.system(xr::FormFactor::HEAD_MOUNTED_DISPLAY) {
            Ok(system) => system,
            Err(error) => {
                emit(serde_json::json!({
                    "event":"system","ok":false,"error":format!("{error:?}"),
                }));
                return Err("vr.openxr_headset_unavailable".into());
            }
        };
        emit(serde_json::json!({"event":"system","ok":true}));

        let requirements = <xr::Vulkan as xr::Graphics>::requirements(&instance, system)
            .map_err(|error| format!("OpenXR Vulkan requirements failed: {error:?}"))?;
        let ash_entry = unsafe { ash::Entry::load() }
            .map_err(|error| format!("Vulkan loader unavailable: {error}"))?;
        // The runtime must create the VkInstance and VkDevice; Monado rejects an
        // application-created instance because it cannot resolve
        // vkGetInstanceProcAddr.
        let get_proc: xr::sys::platform::VkGetInstanceProcAddr =
            unsafe { std::mem::transmute(ash_entry.static_fn().get_instance_proc_addr) };
        let app_info = vk::ApplicationInfo::default()
            .application_name(c"vrcs")
            .api_version(vk::make_api_version(
                0,
                u32::from(requirements.max_api_version_supported.major()),
                u32::from(requirements.max_api_version_supported.minor()),
                0,
            ));
        let instance_info = vk::InstanceCreateInfo::default().application_info(&app_info);
        let raw_instance = unsafe {
            instance.create_vulkan_instance(
                system,
                get_proc,
                &instance_info as *const _ as *const _,
            )
        }
        .map_err(|error| format!("OpenXR Vulkan instance request failed: {error:?}"))?
        .map_err(|error| format!("Vulkan instance creation failed: {error}"))?;
        let vk_instance = unsafe {
            ash::Instance::load(
                ash_entry.static_fn(),
                vk::Instance::from_raw(raw_instance as _),
            )
        };
        let raw_device =
            unsafe { instance.vulkan_graphics_device(system, vk_instance.handle().as_raw() as _) }
                .map_err(|error| format!("OpenXR graphics device query failed: {error:?}"))?;
        let physical_device = vk::PhysicalDevice::from_raw(raw_device as _);
        let queue_family =
            unsafe { vk_instance.get_physical_device_queue_family_properties(physical_device) }
                .iter()
                .position(|family| family.queue_flags.contains(vk::QueueFlags::GRAPHICS))
                .ok_or("Selected Vulkan device has no graphics queue")? as u32;
        let priorities = [1.0f32];
        let queues = [vk::DeviceQueueCreateInfo::default()
            .queue_family_index(queue_family)
            .queue_priorities(&priorities)];
        let device_info = vk::DeviceCreateInfo::default().queue_create_infos(&queues);
        let raw_device = unsafe {
            instance.create_vulkan_device(
                system,
                get_proc,
                physical_device.as_raw() as _,
                &device_info as *const _ as *const _,
            )
        }
        .map_err(|error| format!("OpenXR Vulkan device request failed: {error:?}"))?
        .map_err(|error| format!("Vulkan device creation failed: {error}"))?;
        let device = unsafe {
            ash::Device::load(vk_instance.fp_v1_0(), vk::Device::from_raw(raw_device as _))
        };
        let queue = unsafe { device.get_device_queue(queue_family, 0) };

        let (session, pool, waiter, stream) = unsafe {
            create_overlay_session(
                &instance,
                system,
                &vk_instance,
                physical_device,
                &device,
                queue_family,
            )
        }
        .map_err(|error| format!("Overlay session creation failed: {error:?}"))?;
        let stage = session
            .create_reference_space(xr::ReferenceSpaceType::STAGE, xr::Posef::IDENTITY)
            .or_else(|_| {
                session.create_reference_space(xr::ReferenceSpaceType::LOCAL, xr::Posef::IDENTITY)
            })
            .map_err(|error| format!("Reference space creation failed: {error:?}"))?;
        let view = session
            .create_reference_space(xr::ReferenceSpaceType::VIEW, xr::Posef::IDENTITY)
            .map_err(|error| format!("View space creation failed: {error:?}"))?;
        let actions = Actions::new(&instance, &session, &stage)
            .map_err(|error| format!("Action setup failed: {error:?}"))?;

        let formats = session
            .enumerate_swapchain_formats()
            .map_err(|error| format!("Swapchain format query failed: {error:?}"))?;
        let format = formats
            .iter()
            .copied()
            .find(|format| {
                *format == vk::Format::R8G8B8A8_SRGB.as_raw() as u32
                    || *format == vk::Format::B8G8R8A8_SRGB.as_raw() as u32
            })
            .or_else(|| formats.first().copied())
            .ok_or("Runtime exposes no swapchain format")?;
        let command = unsafe {
            device.allocate_command_buffers(
                &vk::CommandBufferAllocateInfo::default()
                    .command_pool(pool)
                    .level(vk::CommandBufferLevel::PRIMARY)
                    .command_buffer_count(1),
            )
        }
        .map_err(|error| format!("Command buffer allocation failed: {error}"))?[0];
        emit(serde_json::json!({
            "event": "session", "ok": true,
            "format": format, "width": WIDTH, "height": HEIGHT,
        }));

        Ok(Self {
            handles: Some(Handles {
                instance,
                session,
                waiter,
                stream,
                stage,
                view,
                actions,
                panels: std::array::from_fn(|index| {
                    Panel::new(index == panel_index(PanelId::Headset))
                }),
                format,
            }),
            device,
            vk_instance,
            pool,
            command,
            queue,
            physical_device,
            stagings: std::array::from_fn(|_| None),
            frame: None,
            last_layers: usize::MAX,
            last_layer_report: std::time::Instant::now(),
            running: false,
        })
    }

    pub fn texture_size() -> [u32; 2] {
        [WIDTH, HEIGHT]
    }

    /// Panel pose and physical size in the backend's head-locked view space.
    pub fn set_panel(
        &mut self,
        kind: PanelId,
        space: PanelSpace,
        pose: xr::Posef,
        size: xr::Extent2Df,
    ) {
        if let Some(handles) = self.handles.as_mut() {
            let panel = &mut handles.panels[panel_index(kind)];
            panel.space = space;
            panel.pose = pose;
            panel.size = size;
        }
    }

    pub fn set_visible(&mut self, kind: PanelId, visible: bool) {
        if let Some(handles) = self.handles.as_mut() {
            handles.panels[panel_index(kind)].visible = visible;
        }
    }

    pub fn visible(&self, kind: PanelId) -> bool {
        self.handles
            .as_ref()
            .is_some_and(|handles| handles.panels[panel_index(kind)].visible)
    }

    pub fn panel(&self, kind: PanelId) -> Option<PanelInfo> {
        let panel = &self.handles.as_ref()?.panels[panel_index(kind)];
        Some(PanelInfo {
            space: panel.space,
            pose: panel.pose,
            size: panel.size,
            visible: panel.visible,
            has_image: panel.has_image,
        })
    }

    /// Poll runtime events. Returns whether a frame should be produced now.
    pub fn pump(&mut self) -> Result<bool, String> {
        let xr = self.handles.as_mut().ok_or("XR session closed")?;
        let mut event = xr::EventDataBuffer::new();
        while let Some(event) = xr
            .instance
            .poll_event(&mut event)
            .map_err(|error| format!("OpenXR event poll failed: {error:?}"))?
        {
            match event {
                xr::Event::SessionStateChanged(event) => match event.state() {
                    xr::SessionState::READY => {
                        xr.session
                            .begin(VIEW_TYPE)
                            .map_err(|error| format!("Session begin failed: {error:?}"))?;
                        self.running = true;
                    }
                    xr::SessionState::STOPPING => {
                        let _ = xr.session.end();
                        self.running = false;
                    }
                    xr::SessionState::EXITING | xr::SessionState::LOSS_PENDING => {
                        self.running = false;
                        return Err("vr.openxr_session_ended".into());
                    }
                    _ => {}
                },
                xr::Event::InstanceLossPending(_) => {
                    self.running = false;
                    return Err("vr.openxr_instance_lost".into());
                }
                _ => {}
            }
        }
        Ok(self.running)
    }

    /// Block until the runtime is ready for a new frame.
    pub fn wait_frame(&mut self) -> Result<(xr::Time, bool), String> {
        let xr = self.handles.as_mut().ok_or("XR session closed")?;
        let state = xr
            .waiter
            .wait()
            .map_err(|error| format!("Frame wait failed: {error:?}"))?;
        xr.stream
            .begin()
            .map_err(|error| format!("Frame begin failed: {error:?}"))?;
        let focused = !matches!(
            xr.session.sync_actions(&[(&xr.actions.set).into()]),
            Err(xr::sys::Result::SESSION_NOT_FOCUSED)
        );
        xr.actions.focused = focused;
        Ok((state.predicted_display_time, state.should_render))
    }

    /// Block until the runtime offers the next frame. Returns false when the
    /// session is not running yet; the caller should retry later.
    pub fn begin_frame(&mut self) -> Result<bool, String> {
        if !self.pump()? {
            return Ok(false);
        }
        let (time, render) = self.wait_frame()?;
        self.frame = Some((time, render));
        Ok(true)
    }

    /// Submit staged panels when the runtime asked for a frame; otherwise end
    /// the frame without layers.
    pub fn end_frame(&mut self) -> Result<(), String> {
        let Some((time, render)) = self.frame.take() else {
            return Ok(());
        };
        if render {
            self.submit(time)
        } else {
            self.skip(time)
        }
    }

    /// Copy one RGBA frame into the staging buffer; applied on the next submit.
    ///
    /// Panels differ in size (headset 1024x1360, OCR result 2048x2048), so each
    /// panel owns its swapchain and staging buffer and recreates them only when
    /// its incoming surface dimensions change.
    pub fn upload(
        &mut self,
        kind: PanelId,
        pixels: &[u8],
        width: u32,
        height: u32,
    ) -> Result<(), String> {
        if pixels.len() != width as usize * height as usize * 4 {
            return Err("XR overlay frame has the wrong size".into());
        }
        if width > MAX_TEXTURE || height > MAX_TEXTURE {
            return Err("XR overlay frame exceeds the supported texture size".into());
        }
        tracing::info!(?kind, width, height, "OpenXR surface upload");
        let index = panel_index(kind);
        let recreate = {
            let handles = self.handles.as_ref().ok_or("XR session closed")?;
            let panel = &handles.panels[index];
            panel.swapchain.is_none() || panel.texture_size != [width, height]
        };
        if recreate {
            let handles = self.handles.as_mut().ok_or("XR session closed")?;
            let format = handles.format;
            let swapchain = handles
                .session
                .create_swapchain(&xr::SwapchainCreateInfo {
                    create_flags: xr::SwapchainCreateFlags::EMPTY,
                    usage_flags: xr::SwapchainUsageFlags::COLOR_ATTACHMENT
                        | xr::SwapchainUsageFlags::TRANSFER_DST,
                    format,
                    sample_count: 1,
                    width,
                    height,
                    face_count: 1,
                    array_size: 1,
                    mip_count: 1,
                })
                .map_err(|error| format!("Swapchain recreation failed: {error:?}"))?;
            let images = swapchain
                .enumerate_images()
                .map_err(|error| format!("Swapchain image query failed: {error:?}"))?;
            let panel = &mut handles.panels[index];
            panel.swapchain = Some(swapchain);
            panel.images = images;
            panel.texture_size = [width, height];
            panel.has_image = false;
            panel.pending = false;
        }
        let staging = &mut self.stagings[index];
        if staging
            .as_ref()
            .is_none_or(|staging| staging.len != pixels.len())
        {
            if let Some(mut old) = staging.take() {
                old.destroy(&self.device);
            }
            *staging = Some(Staging::new(
                &self.device,
                &self.vk_instance,
                self.physical_device,
                pixels.len() as u64,
            )?);
        }
        let opacity = self.handles.as_ref().ok_or("XR session closed")?.panels[index].opacity;
        staging
            .as_mut()
            .expect("staging buffer exists")
            .write_scaled(pixels, opacity);
        let panel = &mut self.handles.as_mut().ok_or("XR session closed")?.panels[index];
        panel.pixels.clear();
        panel.pixels.extend_from_slice(pixels);
        panel.size.height = panel.size.width * height as f32 / width as f32;
        panel.pending = true;
        Ok(())
    }

    /// Set a panel's premultiplied opacity, restaging the last uploaded frame.
    pub fn set_opacity(&mut self, kind: PanelId, opacity: f32) -> Result<(), String> {
        let opacity = opacity.clamp(0.0, 1.0);
        let index = panel_index(kind);
        if self.stagings[index].is_none() {
            let panel = &self.handles.as_ref().ok_or("XR session closed")?.panels[index];
            if panel.opacity == opacity {
                return Ok(());
            }
            self.handles.as_mut().ok_or("XR session closed")?.panels[index].opacity = opacity;
            return Ok(());
        }
        let panel = &mut self.handles.as_mut().ok_or("XR session closed")?.panels[index];
        if panel.opacity == opacity {
            return Ok(());
        }
        panel.opacity = opacity;
        let pixels = std::mem::take(&mut panel.pixels);
        if pixels.is_empty() {
            return Ok(());
        }
        self.stagings[index]
            .as_mut()
            .expect("staging buffer exists")
            .write_scaled(&pixels, opacity);
        self.handles.as_mut().ok_or("XR session closed")?.panels[index].pixels = pixels;
        self.handles.as_mut().ok_or("XR session closed")?.panels[index].pending = true;
        Ok(())
    }

    pub fn has_image(&self, kind: PanelId) -> bool {
        self.handles
            .as_ref()
            .is_some_and(|handles| handles.panels[panel_index(kind)].has_image)
    }

    pub fn clear(&mut self, kind: PanelId) {
        if let Some(handles) = self.handles.as_mut() {
            let panel = &mut handles.panels[panel_index(kind)];
            panel.has_image = false;
            panel.pending = false;
            panel.pixels.clear();
        }
    }

    /// Upload pending panel pixels and submit every visible quad layer.
    pub fn submit(&mut self, time: xr::Time) -> Result<(), String> {
        let handles = self.handles.as_mut().ok_or("XR session closed")?;
        for index in 0..PANEL_COUNT {
            let panel = &mut handles.panels[index];
            if !panel.visible || !panel.pending {
                continue;
            }
            let Some(swapchain) = panel.swapchain.as_mut() else {
                continue;
            };
            let image_index = swapchain
                .acquire_image()
                .map_err(|error| format!("Swapchain acquire failed: {error:?}"))?;
            swapchain
                .wait_image(xr::Duration::INFINITE)
                .map_err(|error| format!("Swapchain wait failed: {error:?}"))?;
            upload_image(
                &self.device,
                &self.queue,
                self.command,
                self.stagings[index]
                    .as_ref()
                    .ok_or("Panel staging buffer is missing")?
                    .buffer,
                vk::Image::from_raw(panel.images[image_index as usize]),
                panel.texture_size[0],
                panel.texture_size[1],
            )?;
            swapchain
                .release_image()
                .map_err(|error| format!("Swapchain release failed: {error:?}"))?;
            panel.has_image = true;
            panel.pending = false;
        }
        let mut layers = Vec::new();
        for panel in &handles.panels {
            if !panel.visible || !panel.has_image {
                continue;
            }
            let Some(swapchain) = panel.swapchain.as_ref() else {
                continue;
            };
            let space = match panel.space {
                PanelSpace::View => &handles.view,
                PanelSpace::Stage => &handles.stage,
                PanelSpace::Grip(hand) => {
                    let Some(space) = handles.actions.spaces.get(hand).and_then(Option::as_ref)
                    else {
                        continue;
                    };
                    space
                }
            };
            let layer = xr::CompositionLayerQuad::new()
                .layer_flags(xr::CompositionLayerFlags::BLEND_TEXTURE_SOURCE_ALPHA)
                .space(space)
                .eye_visibility(xr::EyeVisibility::BOTH)
                .sub_image(
                    xr::SwapchainSubImage::new()
                        .swapchain(swapchain)
                        .image_array_index(0)
                        .image_rect(xr::Rect2Di {
                            offset: xr::Offset2Di { x: 0, y: 0 },
                            extent: xr::Extent2Di {
                                width: panel.texture_size[0] as i32,
                                height: panel.texture_size[1] as i32,
                            },
                        }),
                )
                .pose(panel.pose)
                .size(panel.size);
            layers.push(layer);
        }
        let layers: Vec<&xr::CompositionLayerBase<'_, xr::Vulkan>> =
            layers.iter().map(|layer| &**layer).collect();
        if layers.len() != self.last_layers
            || self.last_layer_report.elapsed() >= std::time::Duration::from_secs(5)
        {
            let visible = handles.panels.iter().filter(|panel| panel.visible).count();
            let images = handles
                .panels
                .iter()
                .filter(|panel| panel.has_image)
                .count();
            tracing::info!(
                layers = layers.len(),
                visible,
                images,
                "OpenXR frame layers"
            );
            self.last_layers = layers.len();
            self.last_layer_report = std::time::Instant::now();
        }
        handles
            .stream
            .end(time, xr::EnvironmentBlendMode::OPAQUE, &layers)
            .map_err(|error| format!("Frame submission failed: {error:?}"))?;
        Ok(())
    }

    /// End a frame the runtime asked us not to render.
    pub fn skip(&mut self, time: xr::Time) -> Result<(), String> {
        let xr = self.handles.as_mut().ok_or("XR session closed")?;
        xr.stream
            .end(time, xr::EnvironmentBlendMode::OPAQUE, &[])
            .map_err(|error| format!("Frame end failed: {error:?}"))?;
        Ok(())
    }

    pub fn click(&mut self, time: xr::Time) -> bool {
        let Some(xr) = self.handles.as_mut() else {
            return false;
        };
        xr.actions.click(&xr.session, &xr.view, time)
    }

    /// Head pose in the stage space for the current frame.
    pub fn head_pose(&self) -> Option<TrackedPose> {
        let (time, _) = self.frame?;
        let handles = self.handles.as_ref()?;
        locate_in_stage(&handles.view, &handles.stage, time)
    }

    /// Grip pose for a hand (0 left, 1 right) in the stage space.
    pub fn controller_pose(&self, hand: usize) -> Option<TrackedPose> {
        let (time, _) = self.frame?;
        let handles = self.handles.as_ref()?;
        if let Some(pose) = handles
            .actions
            .grip_spaces
            .get(hand)
            .and_then(Option::as_ref)
            .and_then(|space| locate_in_stage(space, &handles.stage, time))
        {
            return Some(pose);
        }
        let space = handles.actions.spaces.get(hand)?.as_ref()?;
        locate_in_stage(space, &handles.stage, time)
    }

    /// Aim pose for a pointer in the stage space.
    pub fn pointer_pose(&self, pointer: PointerId) -> Option<TrackedPose> {
        let (time, _) = self.frame?;
        let handles = self.handles.as_ref()?;
        let space = handles
            .actions
            .spaces
            .get(pointer.source as usize)?
            .as_ref()?;
        locate_in_stage(space, &handles.stage, time)
    }

    pub fn pointer_inputs(&self) -> Vec<PointerInput> {
        let Some(handles) = self.handles.as_ref() else {
            return Vec::new();
        };
        let focused = handles.actions.focused;
        (0..2)
            .map(|hand| {
                let button = |action: &xr::Action<bool>| {
                    action
                        .state(&handles.session, xr::Path::NULL)
                        .ok()
                        .filter(|state| state.is_active && focused)
                        .map(|state| state.current_state)
                };
                let scroll = handles.actions.scroll[hand]
                    .state(&handles.session, xr::Path::NULL)
                    .ok()
                    .filter(|state| state.is_active && focused)
                    .map(|state| state.current_state.y)
                    .unwrap_or(0.0);
                PointerInput {
                    pointer: PointerId::new(DeviceId(hand as u32), hand as u64),
                    device: DeviceId(hand as u32),
                    trigger: button(&handles.actions.trigger[hand]),
                    grip: button(&handles.actions.grip[hand]).or_else(|| {
                        handles.actions.grip_value[hand]
                            .state(&handles.session, xr::Path::NULL)
                            .ok()
                            .filter(|state| state.is_active && focused)
                            .map(|state| state.current_state > 0.5)
                    }),
                    scroll_y: scroll,
                }
            })
            .collect()
    }

    /// Rate-limited JSON input sample for the prototype.
    pub fn report(&mut self, time: xr::Time) -> Option<serde_json::Value> {
        let xr = self.handles.as_mut()?;
        let focused = xr.actions.focused;
        let (instance, session, stage) = (&xr.instance, &xr.session, &xr.stage);
        xr.actions.poll(instance, session, stage, time, focused)
    }

    pub fn focused(&self) -> bool {
        self.handles
            .as_ref()
            .is_some_and(|handles| handles.actions.focused)
    }
}

impl Drop for XrRuntime {
    fn drop(&mut self) {
        // Session, swapchain and actions go first; the runtime touches the
        // Vulkan device during their teardown.
        drop(self.handles.take());
        for staging in &mut self.stagings {
            if let Some(mut staging) = staging.take() {
                staging.destroy(&self.device);
            }
        }
        unsafe {
            let _ = self.device.device_wait_idle();
            self.device.destroy_command_pool(self.pool, None);
        }
        // `vkDestroyDevice`/`vkDestroyInstance` are intentionally not called:
        // WiVRn/Monado still references the application's device while it tears
        // the session down asynchronously, which segfaults here. The handles are
        // process-lifetime until the reconnect path can prove a safe ordering.
        let _ = (&self.device, &self.vk_instance);
    }
}

/// Prototype driver: open a session, show one textured panel, report input.
pub fn run(seconds: u64) -> Result<(), String> {
    let mut runtime = XrRuntime::open(true)?;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(seconds);
    let mut background = [0.02f64, 0.05, 0.10];
    let mut pixels = render_sample(WIDTH, HEIGHT, background)?;
    runtime.upload(PanelId::Headset, &pixels, WIDTH, HEIGHT)?;
    let mut frames = 0u32;
    while std::time::Instant::now() < deadline {
        if !runtime.pump()? {
            std::thread::sleep(std::time::Duration::from_millis(100));
            continue;
        }
        let (time, should_render) = runtime.wait_frame()?;
        if !should_render {
            runtime.skip(time)?;
            continue;
        }
        if runtime.click(time) {
            background = if background[1] > 0.5 {
                [0.02, 0.05, 0.10]
            } else {
                [0.05, 0.45, 0.15]
            };
            pixels = render_sample(WIDTH, HEIGHT, background)?;
            runtime.upload(PanelId::Headset, &pixels, WIDTH, HEIGHT)?;
            println!("{}", serde_json::json!({"event":"click"}));
        }
        if let Some(sample) = runtime.report(time) {
            println!("{sample}");
        }
        runtime.submit(time)?;
        frames += 1;
    }
    println!(
        "{}",
        serde_json::json!({"event":"frames","submitted":frames})
    );
    Ok(())
}

struct Actions {
    set: xr::ActionSet,
    aim: [xr::Action<xr::Posef>; 2],
    grip_pose: [xr::Action<xr::Posef>; 2],
    spaces: [Option<xr::Space>; 2],
    grip_spaces: [Option<xr::Space>; 2],
    trigger: [xr::Action<bool>; 2],
    grip: [xr::Action<bool>; 2],
    grip_value: [xr::Action<f32>; 2],
    scroll: [xr::Action<xr::Vector2f>; 2],
    last_report: std::time::Instant,
    was_pressed: [bool; 2],
    focused: bool,
}

impl Actions {
    fn new(
        instance: &xr::Instance,
        session: &xr::Session<xr::Vulkan>,
        stage: &xr::Space,
    ) -> xr::Result<Self> {
        let set = instance.create_action_set("vrcs_probe", "VRCS probe", 0)?;
        let sides = [
            instance.string_to_path("/user/hand/left")?,
            instance.string_to_path("/user/hand/right")?,
        ];
        let aim = [
            set.create_action::<xr::Posef>("left_aim", "Left aim", &[sides[0]])?,
            set.create_action::<xr::Posef>("right_aim", "Right aim", &[sides[1]])?,
        ];
        let grip_pose = [
            set.create_action::<xr::Posef>("left_grip_pose", "Left grip", &[sides[0]])?,
            set.create_action::<xr::Posef>("right_grip_pose", "Right grip", &[sides[1]])?,
        ];
        let trigger = [
            set.create_action::<bool>("left_trigger", "Left trigger", &[sides[0]])?,
            set.create_action::<bool>("right_trigger", "Right trigger", &[sides[1]])?,
        ];
        let grip = [
            set.create_action::<bool>("left_grip", "Left grip press", &[sides[0]])?,
            set.create_action::<bool>("right_grip", "Right grip press", &[sides[1]])?,
        ];
        let grip_value = [
            set.create_action::<f32>("left_grip_value", "Left grip value", &[sides[0]])?,
            set.create_action::<f32>("right_grip_value", "Right grip value", &[sides[1]])?,
        ];
        let scroll = [
            set.create_action::<xr::Vector2f>("left_scroll", "Left stick", &[sides[0]])?,
            set.create_action::<xr::Vector2f>("right_scroll", "Right stick", &[sides[1]])?,
        ];
        // Bind every profile the runtime advertises for hand controllers; a
        // profile that no device matches is ignored.
        let profiles: [(&str, &[&str]); 5] = [
            (
                "/interaction_profiles/khr/simple_controller",
                &["select/click"],
            ),
            (
                "/interaction_profiles/ext/hand_interaction_ext",
                &["aim/pose"],
            ),
            (
                "/interaction_profiles/oculus/touch_controller",
                &[
                    "aim/pose",
                    "grip/pose",
                    "trigger/value",
                    "squeeze/value",
                    "thumbstick",
                ],
            ),
            (
                "/interaction_profiles/valve/index_controller",
                &[
                    "aim/pose",
                    "grip/pose",
                    "trigger/value",
                    "squeeze/value",
                    "thumbstick",
                ],
            ),
            (
                "/interaction_profiles/htc/vive_controller",
                &["grip/pose", "trigger/value", "squeeze/click", "trackpad"],
            ),
        ];
        for (profile, inputs) in profiles {
            let profile = instance.string_to_path(profile)?;
            let mut bindings = Vec::new();
            for (side, index) in [("left", 0), ("right", 1)] {
                for input in inputs {
                    let path =
                        instance.string_to_path(&format!("/user/hand/{side}/input/{input}"))?;
                    let action = if input.ends_with("pose") {
                        if input.starts_with("grip") {
                            xr::Binding::new(&grip_pose[index], path)
                        } else {
                            xr::Binding::new(&aim[index], path)
                        }
                    } else if input.contains("trigger") || input.contains("select") {
                        xr::Binding::new(&trigger[index], path)
                    } else if input.starts_with("squeeze/value") {
                        xr::Binding::new(&grip_value[index], path)
                    } else if input.starts_with("squeeze/click") {
                        xr::Binding::new(&grip[index], path)
                    } else if *input == "thumbstick" || *input == "trackpad" {
                        xr::Binding::new(&scroll[index], path)
                    } else {
                        continue;
                    };
                    bindings.push(action);
                }
            }
            instance.suggest_interaction_profile_bindings(profile, &bindings)?;
        }
        session.attach_action_sets(&[&set])?;
        let _ = stage;
        let spaces = [
            aim[0]
                .create_space(session, xr::Path::NULL, xr::Posef::IDENTITY)
                .ok(),
            aim[1]
                .create_space(session, xr::Path::NULL, xr::Posef::IDENTITY)
                .ok(),
        ];
        let grip_spaces = [
            grip_pose[0]
                .create_space(session, xr::Path::NULL, xr::Posef::IDENTITY)
                .ok(),
            grip_pose[1]
                .create_space(session, xr::Path::NULL, xr::Posef::IDENTITY)
                .ok(),
        ];
        Ok(Self {
            set,
            aim,
            grip_pose,
            spaces,
            grip_spaces,
            trigger,
            grip,
            grip_value,
            scroll,
            last_report: std::time::Instant::now() - std::time::Duration::from_secs(1),
            was_pressed: [false; 2],
            focused: false,
        })
    }

    /// Ray/plane test in the panel's space plus a trigger rising edge: the same
    /// geometry the production `Router` will use.
    fn click(
        &mut self,
        session: &xr::Session<xr::Vulkan>,
        view: &xr::Space,
        time: xr::Time,
    ) -> bool {
        let mut clicked = false;
        for index in 0..2 {
            let pressed = self.trigger[index]
                .state(session, xr::Path::NULL)
                .map(|state| state.is_active && state.current_state)
                .unwrap_or(false);
            // Update the latch before any early return, otherwise an untracked
            // hand re-fires the edge on every frame and produces repeat clicks.
            let edge = pressed && !self.was_pressed[index];
            self.was_pressed[index] = pressed;
            if !edge {
                continue;
            }
            let Some(space) = self.spaces[index].as_ref() else {
                continue;
            };
            let Ok(location) = space.locate(view, time) else {
                continue;
            };
            let p = location.pose.position;
            let q = location.pose.orientation;
            // Rotate (0, 0, -1) by q to get the aim direction.
            let (x, y, z, w) = (q.x, q.y, q.z, q.w);
            let forward = [
                -2.0 * (x * z + w * y),
                -2.0 * (y * z - w * x),
                -(1.0 - 2.0 * (x * x + y * y)),
            ];
            if forward[2].abs() <= 1e-4 {
                continue;
            }
            let t = (PANEL_Z - p.z) / forward[2];
            if t <= 0.0 {
                continue;
            }
            let hit = [p.x + forward[0] * t, p.y + forward[1] * t];
            clicked |= hit[0].abs() <= PANEL_W && hit[1].abs() <= PANEL_H;
        }
        clicked
    }

    fn poll(
        &mut self,
        instance: &xr::Instance,
        session: &xr::Session<xr::Vulkan>,
        stage: &xr::Space,
        time: xr::Time,
        focused: bool,
    ) -> Option<serde_json::Value> {
        if self.last_report.elapsed() < std::time::Duration::from_secs(1) {
            return None;
        }
        self.last_report = std::time::Instant::now();
        let locate = |index: usize| -> (Option<[f32; 3]>, bool, bool) {
            let Some(space) = self.spaces[index].as_ref() else {
                return (None, false, false);
            };
            match space.locate(stage, time) {
                Ok(location) => {
                    let position = location.pose.position;
                    let valid = location
                        .location_flags
                        .contains(xr::SpaceLocationFlags::POSITION_VALID);
                    let tracked = location
                        .location_flags
                        .contains(xr::SpaceLocationFlags::POSITION_TRACKED);
                    (
                        valid.then_some([position.x, position.y, position.z]),
                        valid,
                        tracked,
                    )
                }
                Err(_) => (None, false, false),
            }
        };
        let pressed = |index: usize| -> bool {
            self.trigger[index]
                .state(session, xr::Path::NULL)
                .map(|state| state.is_active && state.current_state)
                .unwrap_or(false)
        };
        let profile = |hand: &str| -> String {
            let Ok(path) = instance.string_to_path(&format!("/user/hand/{hand}")) else {
                return "<path>".into();
            };
            session
                .current_interaction_profile(path)
                .ok()
                .filter(|profile| *profile != xr::Path::NULL)
                .and_then(|profile| instance.path_to_string(profile).ok())
                .unwrap_or_else(|| "<none>".into())
        };
        let _ = &self.aim;
        Some(serde_json::json!({
            "left_profile": profile("left"),
            "right_profile": profile("right"),
            "event": "input",
            "focused": focused,
            "left": locate(0).0,
            "left_valid": locate(0).1,
            "left_tracked": locate(0).2,
            "right": locate(1).0,
            "right_valid": locate(1).1,
            "right_tracked": locate(1).2,
            "left_trigger": pressed(0),
            "right_trigger": pressed(1),
        }))
    }
}

/// The high-level helper chains only the graphics binding, so an overlay
/// session is created through the raw entry point with `XR_EXTX_overlay` first.
unsafe fn create_overlay_session(
    instance: &xr::Instance,
    system: xr::SystemId,
    vk_instance: &ash::Instance,
    physical_device: vk::PhysicalDevice,
    device: &ash::Device,
    queue_family_index: u32,
) -> Result<
    (
        xr::Session<xr::Vulkan>,
        vk::CommandPool,
        xr::FrameWaiter,
        xr::FrameStream<xr::Vulkan>,
    ),
    xr::sys::Result,
> {
    let overlay = xr::sys::SessionCreateInfoOverlayEXTX {
        ty: xr::sys::SessionCreateInfoOverlayEXTX::TYPE,
        next: ptr::null(),
        create_flags: xr::sys::OverlaySessionCreateFlagsEXTX::EMPTY,
        session_layers_placement: 5,
    };
    let binding = xr::sys::GraphicsBindingVulkanKHR {
        ty: xr::sys::GraphicsBindingVulkanKHR::TYPE,
        next: (&raw const overlay).cast(),
        instance: vk_instance.handle().as_raw() as _,
        physical_device: physical_device.as_raw() as _,
        device: device.handle().as_raw() as _,
        queue_family_index,
        queue_index: 0,
    };
    let info = xr::sys::SessionCreateInfo {
        ty: xr::sys::SessionCreateInfo::TYPE,
        next: (&raw const binding).cast(),
        create_flags: xr::sys::SessionCreateFlags::EMPTY,
        system_id: system,
    };
    let mut raw = xr::sys::Session::NULL;
    let result =
        unsafe { (instance.fp().create_session)(instance.as_raw(), &raw const info, &mut raw) };
    if result.into_raw() < 0 {
        return Err(result);
    }
    let (session, waiter, stream) =
        unsafe { xr::Session::from_raw(instance.clone(), raw, Box::new(())) };
    let pool = unsafe {
        device.create_command_pool(
            &vk::CommandPoolCreateInfo::default()
                .queue_family_index(queue_family_index)
                .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER),
            None,
        )
    }
    .map_err(|_| xr::sys::Result::ERROR_RUNTIME_FAILURE)?;
    Ok((session, pool, waiter, stream))
}

/// Copy the CPU RGBA frame into the swapchain image: one staging buffer and a
/// buffer→image copy, no shader or descriptor pipeline.
fn upload_image(
    device: &ash::Device,
    queue: &vk::Queue,
    command: vk::CommandBuffer,
    buffer: vk::Buffer,
    image: vk::Image,
    width: u32,
    height: u32,
) -> Result<(), String> {
    let barrier = |old: vk::ImageLayout, new: vk::ImageLayout| {
        vk::ImageMemoryBarrier::default()
            .old_layout(old)
            .new_layout(new)
            .image(image)
            .subresource_range(
                vk::ImageSubresourceRange::default()
                    .aspect_mask(vk::ImageAspectFlags::COLOR)
                    .level_count(1)
                    .layer_count(1),
            )
            .src_access_mask(vk::AccessFlags::MEMORY_WRITE)
            .dst_access_mask(vk::AccessFlags::TRANSFER_WRITE)
    };
    let region = [vk::BufferImageCopy::default()
        .buffer_row_length(0)
        .buffer_image_height(0)
        .image_subresource(
            vk::ImageSubresourceLayers::default()
                .aspect_mask(vk::ImageAspectFlags::COLOR)
                .layer_count(1),
        )
        .image_extent(vk::Extent3D {
            width,
            height,
            depth: 1,
        })];
    unsafe {
        device
            .reset_command_buffer(command, vk::CommandBufferResetFlags::empty())
            .map_err(|error| format!("Command buffer reset failed: {error}"))?;
        device
            .begin_command_buffer(
                command,
                &vk::CommandBufferBeginInfo::default()
                    .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT),
            )
            .map_err(|error| format!("Command buffer begin failed: {error}"))?;
        device.cmd_pipeline_barrier(
            command,
            vk::PipelineStageFlags::ALL_COMMANDS,
            vk::PipelineStageFlags::TRANSFER,
            vk::DependencyFlags::empty(),
            &[],
            &[],
            &[barrier(
                vk::ImageLayout::UNDEFINED,
                vk::ImageLayout::TRANSFER_DST_OPTIMAL,
            )],
        );
        device.cmd_copy_buffer_to_image(
            command,
            buffer,
            image,
            vk::ImageLayout::TRANSFER_DST_OPTIMAL,
            &region,
        );
        device.cmd_pipeline_barrier(
            command,
            vk::PipelineStageFlags::TRANSFER,
            vk::PipelineStageFlags::BOTTOM_OF_PIPE,
            vk::DependencyFlags::empty(),
            &[],
            &[],
            &[barrier(
                vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
            )],
        );
        device
            .end_command_buffer(command)
            .map_err(|error| format!("Command buffer end failed: {error}"))?;
        let fence = device
            .create_fence(&vk::FenceCreateInfo::default(), None)
            .map_err(|error| format!("Fence creation failed: {error}"))?;
        device
            .queue_submit(
                *queue,
                &[vk::SubmitInfo::default().command_buffers(&[command])],
                fence,
            )
            .map_err(|error| format!("Queue submit failed: {error}"))?;
        device
            .wait_for_fences(&[fence], true, u64::MAX)
            .map_err(|error| format!("Queue wait failed: {error}"))?;
        device.destroy_fence(fence, None);
    }
    Ok(())
}

struct Staging {
    buffer: vk::Buffer,
    memory: vk::DeviceMemory,
    mapped: *mut u8,
    len: usize,
}

impl Staging {
    fn new(
        device: &ash::Device,
        instance: &ash::Instance,
        physical_device: vk::PhysicalDevice,
        len: u64,
    ) -> Result<Self, String> {
        let buffer = unsafe {
            device.create_buffer(
                &vk::BufferCreateInfo::default()
                    .size(len)
                    .usage(vk::BufferUsageFlags::TRANSFER_SRC),
                None,
            )
        }
        .map_err(|error| format!("Staging buffer creation failed: {error}"))?;
        let requirements = unsafe { device.get_buffer_memory_requirements(buffer) };
        let memory_type = unsafe { instance.get_physical_device_memory_properties(physical_device) }
            .memory_types
            .iter()
            .enumerate()
            .position(|(index, memory)| {
                requirements.memory_type_bits & (1 << index) != 0
                    && memory.property_flags.contains(
                        vk::MemoryPropertyFlags::HOST_VISIBLE
                            | vk::MemoryPropertyFlags::HOST_COHERENT,
                    )
            })
            .ok_or("No host-visible staging memory")? as u32;
        let memory = unsafe {
            device.allocate_memory(
                &vk::MemoryAllocateInfo::default()
                    .allocation_size(requirements.size)
                    .memory_type_index(memory_type),
                None,
            )
        }
        .map_err(|error| format!("Staging memory allocation failed: {error}"))?;
        unsafe { device.bind_buffer_memory(buffer, memory, 0) }
            .map_err(|error| format!("Staging bind failed: {error}"))?;
        let mapped = unsafe { device.map_memory(memory, 0, len, vk::MemoryMapFlags::empty()) }
            .map_err(|error| format!("Staging map failed: {error}"))?
            as *mut u8;
        Ok(Self {
            buffer,
            memory,
            mapped,
            len: len as usize,
        })
    }

    fn write_scaled(&mut self, pixels: &[u8], opacity: f32) {
        assert!(pixels.len() <= self.len);
        if opacity >= 1.0 {
            unsafe { std::ptr::copy_nonoverlapping(pixels.as_ptr(), self.mapped, pixels.len()) };
            return;
        }
        let scale = opacity.clamp(0.0, 1.0);
        let destination = unsafe { std::slice::from_raw_parts_mut(self.mapped, pixels.len()) };
        for (target, source) in destination.chunks_exact_mut(4).zip(pixels.chunks_exact(4)) {
            for channel in 0..4 {
                target[channel] = (f32::from(source[channel]) * scale).round() as u8;
            }
        }
    }

    fn destroy(&mut self, device: &ash::Device) {
        unsafe {
            device.unmap_memory(self.memory);
            device.destroy_buffer(self.buffer, None);
            device.free_memory(self.memory, None);
        }
    }
}

/// Opaque text sample so alpha, channel order and orientation are all visible.
fn render_sample(width: u32, height: u32, background: [f64; 3]) -> Result<Vec<u8>, String> {
    let mut surface =
        cairo::ImageSurface::create(cairo::Format::ARgb32, width as i32, height as i32)
            .map_err(|error| error.to_string())?;
    let context = cairo::Context::new(&surface).map_err(|error| error.to_string())?;
    context.set_source_rgba(background[0], background[1], background[2], 0.7);
    context.rectangle(0., 0., width as f64, height as f64);
    context.fill().map_err(|error| error.to_string())?;
    context.set_source_rgba(1., 1., 1., 1.);
    context.select_font_face(
        "sans-serif",
        cairo::FontSlant::Normal,
        cairo::FontWeight::Normal,
    );
    context.set_font_size(56.);
    context.move_to(32., 96.);
    context
        .show_text("VRCS OpenXR / WiVRN")
        .map_err(|error| error.to_string())?;
    context.set_font_size(40.);
    context.move_to(32., 176.);
    context
        .show_text("字幕测试 こんにちは 안녕하세요")
        .map_err(|error| error.to_string())?;
    drop(context);
    surface.flush();
    let stride = surface.stride() as usize;
    let data = surface.data().map_err(|error| error.to_string())?;
    // Cairo ARGB32 is premultiplied BGRA in memory on little-endian; the
    // swapchain format is R8G8B8A8_SRGB.
    let mut rgba = Vec::with_capacity(width as usize * height as usize * 4);
    for row in 0..height as usize {
        let line = &data[row * stride..row * stride + width as usize * 4];
        for pixel in line.chunks_exact(4) {
            rgba.extend_from_slice(&[pixel[2], pixel[1], pixel[0], pixel[3]]);
        }
    }
    Ok(rgba)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn panel_slots_are_distinct_and_headset_is_first() {
        let kinds = [
            PanelId::Headset,
            PanelId::Wrist,
            PanelId::WristMenu,
            PanelId::OcrWrist,
            PanelId::OcrProgress,
            PanelId::PlayerAudio,
            PanelId::OcrFrame,
            PanelId::OcrResult,
            PanelId::Dashboard,
        ];
        let mut seen = [false; PANEL_COUNT];
        for kind in kinds {
            let index = panel_index(kind);
            assert!(index < PANEL_COUNT);
            assert!(!seen[index], "{kind:?} shares a slot");
            seen[index] = true;
        }
        assert!(seen.into_iter().all(|used| used));
        assert_eq!(panel_index(PanelId::Headset), 0);
    }
}
