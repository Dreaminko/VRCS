//! OpenXR overlay session for Linux desktop compositors.

use std::collections::HashMap;
use std::ffi::{CString, OsStr};
use std::path::{Path, PathBuf};
use std::ptr;
use std::sync::{Mutex, OnceLock};

use ash::vk::{self, Handle as _};
use openxr as xr;
use openxr::sys::Handle as _;

use super::pose::{pose_from_quaternion, DeviceId, PointerId, PointerInput, SpaceId, TrackedPose};

const VIEW_TYPE: xr::ViewConfigurationType = xr::ViewConfigurationType::PRIMARY_STEREO;
const WIDTH: u32 = 1024;
const PANEL_Z: f32 = -1.2;
const MAX_TEXTURE: u32 = 2048;
const HEIGHT: u32 = 256;
const PANEL_COUNT: usize = 10;

/// Quad layer slots exposed by the runtime.
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
    Cursor,
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

/// One overlay session plus the Vulkan resources that feed it.
pub struct XrRuntime {
    /// Session and its child handles; taken first on drop so they are released
    /// while the Vulkan device still exists.
    handles: Option<Handles>,
    device: ash::Device,
    vk_instance: ash::Instance,
    pool: vk::CommandPool,
    command: vk::CommandBuffer,
    fence: vk::Fence,
    queue: vk::Queue,
    physical_device: vk::PhysicalDevice,
    stagings: [Option<Staging>; PANEL_COUNT],
    frame: Option<(xr::Time, bool)>,
    color_scale_bias: bool,
    last_layers: usize,
    last_layer_report: std::time::Instant,
    running: bool,
    tracking_reset: bool,
    /// Released after the Vulkan objects the runtime was using.
    instance: Option<xr::Instance>,
}

struct Panel {
    pose: xr::Posef,
    size: xr::Extent2Df,
    space: PanelSpace,
    visible: bool,
    swapchain: Option<xr::Swapchain<xr::Vulkan>>,
    images: Vec<u64>,
    acquired_image: Option<u32>,
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
            acquired_image: None,
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
    bgra: bool,
    actions: Actions,
    view: xr::Space,
    stage: xr::Space,
    stream: xr::FrameStream<xr::Vulkan>,
    waiter: xr::FrameWaiter,
    session: xr::Session<xr::Vulkan>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProbeStatus {
    pub runtime_installed: bool,
    pub hmd_present: bool,
}

fn xr_runtime_json_overridden() -> bool {
    std::env::var_os("XR_RUNTIME_JSON").is_some_and(|value| !value.is_empty())
}

fn load_loader_entry() -> Result<xr::Entry, String> {
    unsafe { xr::Entry::load(&()) }.map_err(|_| "vr.openxr_loader_unavailable".to_string())
}

fn push_active_runtime_pair(candidates: &mut Vec<PathBuf>, config_dir: &Path, arch: &str) {
    let base = config_dir.join("openxr/1");
    for file in [
        base.join(format!("active_runtime.{arch}.json")),
        base.join("active_runtime.json"),
    ] {
        if !candidates.contains(&file) {
            candidates.push(file);
        }
    }
}

fn active_manifest_candidates_from(
    xr_runtime_json: Option<&OsStr>,
    xdg_config_home: Option<&OsStr>,
    home: Option<&OsStr>,
    xdg_config_dirs: Option<&OsStr>,
    arch: &str,
) -> Vec<PathBuf> {
    if let Some(override_path) = xr_runtime_json.filter(|value| !value.is_empty()) {
        return vec![PathBuf::from(override_path)];
    }
    let mut candidates = Vec::new();
    if let Some(config_home) = xdg_config_home
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            home.filter(|value| !value.is_empty())
                .map(|home| PathBuf::from(home).join(".config"))
        })
    {
        push_active_runtime_pair(&mut candidates, &config_home, arch);
    }
    let config_dirs = xdg_config_dirs
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| OsStr::new("/etc/xdg"));
    for dir in std::env::split_paths(config_dirs).filter(|dir| !dir.as_os_str().is_empty()) {
        push_active_runtime_pair(&mut candidates, &dir, arch);
    }
    push_active_runtime_pair(&mut candidates, Path::new("/etc"), arch);
    candidates
}

fn active_manifest_candidates() -> Vec<PathBuf> {
    let xr_runtime_json = std::env::var_os("XR_RUNTIME_JSON");
    let xdg_config_home = std::env::var_os("XDG_CONFIG_HOME");
    let home = std::env::var_os("HOME");
    let xdg_config_dirs = std::env::var_os("XDG_CONFIG_DIRS");
    active_manifest_candidates_from(
        xr_runtime_json.as_deref(),
        xdg_config_home.as_deref(),
        home.as_deref(),
        xdg_config_dirs.as_deref(),
        std::env::consts::ARCH,
    )
    .into_iter()
    .filter(|path| path.is_file())
    .collect()
}

fn resolve_manifest_library_path(
    manifest_path: &Path,
    raw_json: &str,
) -> Option<(PathBuf, Option<String>)> {
    let value: serde_json::Value = serde_json::from_str(raw_json).ok()?;
    let runtime = value.get("runtime")?.as_object()?;
    let library = runtime
        .get("library_path")?
        .as_str()
        .map(str::trim)
        .filter(|path| !path.is_empty())?;
    let negotiate = runtime
        .get("functions")
        .and_then(|functions| functions.get("xrNegotiateLoaderRuntimeInterface"))
        .and_then(|name| name.as_str())
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(ToOwned::to_owned);
    let resolved = if library.starts_with('/') {
        PathBuf::from(library)
    } else if library.contains('/') {
        manifest_path.parent()?.join(library)
    } else {
        PathBuf::from(library)
    };
    Some((resolved, negotiate))
}

fn fallback_manifest_candidates_with(
    daemons: crate::vr_overlay::process::LinuxXrDaemons,
) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if daemons.wivrn {
        if let Some(home) = std::env::var_os("HOME").filter(|value| !value.is_empty()) {
            candidates.push(
                PathBuf::from(home)
                    .join(".var/app/io.github.wivrn.wivrn/config/openxr/1/active_runtime.json"),
            );
        }
        candidates.push(PathBuf::from("/usr/share/openxr/1/openxr_wivrn.json"));
        candidates.push(PathBuf::from("/usr/local/share/openxr/1/openxr_wivrn.json"));
    }
    if daemons.monado {
        candidates.push(PathBuf::from("/usr/share/openxr/1/openxr_monado.json"));
        candidates.push(PathBuf::from(
            "/usr/local/share/openxr/1/openxr_monado.json",
        ));
    }
    candidates
        .into_iter()
        .filter(|path| path.is_file())
        .collect()
}

fn manifest_requires_daemon(
    manifest_path: &Path,
    raw_json: &str,
    daemons: crate::vr_overlay::process::LinuxXrDaemons,
) -> bool {
    let Some((library_path, _)) = resolve_manifest_library_path(manifest_path, raw_json) else {
        return true;
    };
    let name = library_path
        .file_name()
        .and_then(OsStr::to_str)
        .unwrap_or_default()
        .to_ascii_lowercase();
    if name.contains("wivrn") {
        daemons.wivrn
    } else if name.contains("monado") {
        daemons.monado
    } else if name.contains("steamxr") {
        daemons.steamvr
    } else {
        true
    }
}

fn load_direct_runtime_entry(manifest_path: &Path) -> Result<xr::Entry, String> {
    // Keep loaded runtime shared objects open for the lifetime of cached Entry handles.
    static CACHE: OnceLock<Mutex<HashMap<PathBuf, xr::Entry>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let cache_key =
        std::fs::canonicalize(manifest_path).unwrap_or_else(|_| manifest_path.to_path_buf());
    if let Some(entry) = cache
        .lock()
        .map_err(|_| "OpenXR manifest cache poisoned".to_string())?
        .get(&cache_key)
        .cloned()
    {
        return Ok(entry);
    }

    let raw = std::fs::read_to_string(manifest_path)
        .map_err(|error| format!("Failed to read OpenXR manifest: {error}"))?;
    let (library_path, negotiate_symbol) = resolve_manifest_library_path(manifest_path, &raw)
        .ok_or_else(|| "Invalid OpenXR runtime manifest".to_string())?;
    let library_str = library_path
        .to_str()
        .ok_or_else(|| "Non-UTF-8 OpenXR runtime library path".to_string())?;
    if library_str.contains('/') && !library_path.is_file() {
        return Err("OpenXR runtime library does not exist".into());
    }
    let c_lib_path =
        CString::new(library_str).map_err(|_| "Invalid OpenXR runtime library path".to_string())?;
    let c_negotiate = CString::new(
        negotiate_symbol
            .as_deref()
            .unwrap_or("xrNegotiateLoaderRuntimeInterface"),
    )
    .map_err(|_| "Invalid OpenXR negotiate symbol".to_string())?;

    let handle = unsafe { libc::dlopen(c_lib_path.as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL) };
    if handle.is_null() {
        return Err("Failed to load OpenXR runtime library".into());
    }

    let negotiate_result = (|| -> Result<xr::Entry, String> {
        let symbol = unsafe { libc::dlsym(handle, c_negotiate.as_ptr()) };
        if symbol.is_null() {
            return Err("OpenXR runtime negotiate symbol missing".into());
        }
        let negotiate = unsafe {
            std::mem::transmute::<*mut libc::c_void, xr::sys::pfn::NegotiateLoaderRuntimeInterface>(
                symbol,
            )
        };
        let loader_info = xr::sys::NegotiateLoaderInfo {
            struct_type: xr::sys::NegotiateLoaderInfo::TYPE,
            struct_version: xr::sys::NegotiateLoaderInfo::VERSION,
            struct_size: std::mem::size_of::<xr::sys::NegotiateLoaderInfo>(),
            min_interface_version: 1,
            max_interface_version: 1,
            min_api_version: xr::Version::new(1, 0, 0),
            max_api_version: xr::Version::new(1, 0x3ff, 0xfff),
        };
        let mut runtime_request = xr::sys::NegotiateRuntimeRequest {
            struct_type: xr::sys::NegotiateRuntimeRequest::TYPE,
            struct_version: xr::sys::NegotiateRuntimeRequest::VERSION,
            struct_size: std::mem::size_of::<xr::sys::NegotiateRuntimeRequest>(),
            runtime_interface_version: 0,
            runtime_api_version: xr::Version::new(0, 0, 0),
            get_instance_proc_addr: None,
        };
        let result = unsafe { negotiate(&loader_info, &mut runtime_request) };
        if result != xr::sys::Result::SUCCESS {
            return Err(format!("OpenXR runtime negotiation failed: {result:?}"));
        }
        let get_proc = runtime_request
            .get_instance_proc_addr
            .ok_or_else(|| "OpenXR runtime returned null xrGetInstanceProcAddr".to_string())?;
        unsafe { xr::Entry::from_get_instance_proc_addr(get_proc, &()) }
            .map_err(|error| format!("OpenXR runtime entry initialization failed: {error:?}"))
    })();

    let entry = match negotiate_result {
        Ok(entry) => entry,
        Err(error) => {
            unsafe { libc::dlclose(handle) };
            return Err(error);
        }
    };

    let mut guard = cache
        .lock()
        .map_err(|_| "OpenXR manifest cache poisoned".to_string())?;
    if let Some(existing) = guard.get(&cache_key).cloned() {
        unsafe { libc::dlclose(handle) };
        return Ok(existing);
    }
    guard.insert(cache_key, entry.clone());
    Ok(entry)
}

fn check_required_extensions(entry: &xr::Entry) -> Result<xr::ExtensionSet, String> {
    let available = entry
        .enumerate_extensions()
        .map_err(|error| format!("OpenXR extension query failed: {error:?}"))?;
    if !available.extx_overlay {
        return Err("vr.openxr_overlay_unsupported".into());
    }
    if !available.khr_vulkan_enable2 {
        return Err("vr.openxr_vulkan_unavailable".into());
    }
    Ok(available)
}

fn create_instance_for_entry(
    entry: &xr::Entry,
    available: &xr::ExtensionSet,
    enable_optional: bool,
) -> xr::Result<xr::Instance> {
    let mut enabled = xr::ExtensionSet::default();
    enabled.extx_overlay = true;
    enabled.khr_vulkan_enable2 = true;
    if enable_optional {
        // Hand-tracked controllers expose the hand interaction profile.
        enabled.ext_hand_interaction = available.ext_hand_interaction;
        enabled.khr_composition_layer_color_scale_bias =
            available.khr_composition_layer_color_scale_bias;
    }
    entry.create_instance(
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
}

fn try_open_entry(
    entry: &xr::Entry,
    enable_optional: bool,
) -> Result<(xr::ExtensionSet, xr::Instance), String> {
    let available = check_required_extensions(entry)?;
    let instance = create_instance_for_entry(entry, &available, enable_optional)
        .map_err(|error| format!("OpenXR runtime unavailable: {error:?}"))?;
    Ok((available, instance))
}

fn open_overlay_instance(
    enable_optional: bool,
) -> Result<(xr::ExtensionSet, xr::Instance), String> {
    let overridden = xr_runtime_json_overridden();
    let active_manifests = active_manifest_candidates();
    let (active_runnable, fallback_manifests) = if overridden {
        if active_manifests.is_empty() {
            return Err("vr.openxr_loader_unavailable".into());
        }
        (true, Vec::new())
    } else {
        let daemons = crate::vr_overlay::process::linux_xr_daemons();
        let fallback_manifests = fallback_manifest_candidates_with(daemons);
        if active_manifests.is_empty() && fallback_manifests.is_empty() {
            return Err("vr.openxr_loader_unavailable".into());
        }
        let active_runnable = active_manifests.first().is_some_and(|manifest| {
            std::fs::read_to_string(manifest)
                .map(|raw| manifest_requires_daemon(manifest, &raw, daemons))
                .unwrap_or(true)
        });
        (active_runnable, fallback_manifests)
    };

    let mut tried = Vec::new();
    let mut primary_error = if active_runnable {
        match load_loader_entry() {
            Ok(entry) => match try_open_entry(&entry, enable_optional) {
                Ok(opened) => return Ok(opened),
                Err(error) => Some(error),
            },
            Err(loader_error) => {
                let mut active_error = None;
                for manifest in active_manifests {
                    let canon =
                        std::fs::canonicalize(&manifest).unwrap_or_else(|_| manifest.clone());
                    if !tried.contains(&canon) {
                        tried.push(canon);
                    }
                    match load_direct_runtime_entry(&manifest)
                        .and_then(|entry| try_open_entry(&entry, enable_optional))
                    {
                        Ok(opened) => return Ok(opened),
                        Err(error) if active_error.is_none() => active_error = Some(error),
                        Err(_) => {}
                    }
                    if overridden {
                        break;
                    }
                }
                Some(active_error.unwrap_or(loader_error))
            }
        }
    } else {
        Some("OpenXR runtime unavailable".into())
    };

    for manifest in fallback_manifests {
        let canon = std::fs::canonicalize(&manifest).unwrap_or_else(|_| manifest.clone());
        if tried.contains(&canon) {
            continue;
        }
        tried.push(canon);
        if let Ok(opened) = load_direct_runtime_entry(&manifest)
            .and_then(|entry| try_open_entry(&entry, enable_optional))
        {
            return Ok(opened);
        }
    }

    Err(primary_error
        .take()
        .unwrap_or_else(|| "vr.openxr_loader_unavailable".into()))
}

/// Check the installed loader and runtime for the two required extensions.
/// Creates and drops a short-lived instance so a loader with a missing runtime
/// service is not reported as usable.
pub fn probe_status() -> Result<ProbeStatus, String> {
    let (_, instance) = open_overlay_instance(false)?;
    let hmd_present = instance
        .system(xr::FormFactor::HEAD_MOUNTED_DISPLAY)
        .is_ok();
    drop(instance);
    Ok(ProbeStatus {
        runtime_installed: true,
        hmd_present,
    })
}

pub fn probe() -> Result<(), String> {
    let status = probe_status()?;
    if status.hmd_present {
        Ok(())
    } else {
        Err("vr.openxr_headset_unavailable".into())
    }
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
        let (available, instance) = match open_overlay_instance(true) {
            Ok(opened) => opened,
            Err(error) => {
                emit(serde_json::json!({"event":"entry","ok":false,"error":&error}));
                return Err(error);
            }
        };
        emit(serde_json::json!({
            "event": "extensions",
            "overlay": available.extx_overlay,
            "vulkan": available.khr_vulkan_enable2,
            "hand_tracking": available.ext_hand_tracking,
        }));

        struct InitCleanup {
            fence: Option<vk::Fence>,
            pool: Option<vk::CommandPool>,
            device: Option<ash::Device>,
            vk_instance: Option<ash::Instance>,
            instance: Option<xr::Instance>,
        }
        impl Drop for InitCleanup {
            fn drop(&mut self) {
                unsafe {
                    if let Some(device) = self.device.take() {
                        if let Some(fence) = self.fence.take() {
                            device.destroy_fence(fence, None);
                        }
                        if let Some(pool) = self.pool.take() {
                            device.destroy_command_pool(pool, None);
                        }
                        device.destroy_device(None);
                    }
                    if let Some(vk_instance) = self.vk_instance.take() {
                        vk_instance.destroy_instance(None);
                    }
                }
                drop(self.instance.take());
            }
        }
        let hand_interaction = available.ext_hand_interaction;
        let color_scale_bias = available.khr_composition_layer_color_scale_bias;
        let mut cleanup = InitCleanup {
            fence: None,
            pool: None,
            device: None,
            vk_instance: None,
            instance: Some(instance),
        };
        let instance = cleanup.instance.as_ref().expect("instance initialized");
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

        let requirements = <xr::Vulkan as xr::Graphics>::requirements(instance, system)
            .map_err(|error| format!("OpenXR Vulkan requirements failed: {error:?}"))?;
        let ash_entry = unsafe { ash::Entry::load() }
            .map_err(|error| format!("Vulkan loader unavailable: {error}"))?;
        // The runtime must create the VkInstance and VkDevice; Monado rejects an
        // application-created instance because it cannot resolve
        // vkGetInstanceProcAddr.
        let get_proc: xr::sys::platform::VkGetInstanceProcAddr =
            unsafe { std::mem::transmute(ash_entry.static_fn().get_instance_proc_addr) };
        let min_api = vk::make_api_version(
            0,
            u32::from(requirements.min_api_version_supported.major()),
            u32::from(requirements.min_api_version_supported.minor()),
            0,
        );
        let max_api = vk::make_api_version(
            0,
            u32::from(requirements.max_api_version_supported.major()),
            u32::from(requirements.max_api_version_supported.minor()),
            0,
        );
        let app_info = vk::ApplicationInfo::default()
            .application_name(c"vrcs")
            .api_version(max_api.min(vk::API_VERSION_1_1).max(min_api));
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
        cleanup.vk_instance = Some(unsafe {
            ash::Instance::load(
                ash_entry.static_fn(),
                vk::Instance::from_raw(raw_instance as _),
            )
        });
        let vk_instance = cleanup
            .vk_instance
            .as_ref()
            .expect("vk_instance initialized");
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
        cleanup.device = Some(unsafe {
            ash::Device::load(vk_instance.fp_v1_0(), vk::Device::from_raw(raw_device as _))
        });
        let device = cleanup.device.as_ref().expect("device initialized");
        let queue = unsafe { device.get_device_queue(queue_family, 0) };

        let (session, pool, waiter, stream) = unsafe {
            create_overlay_session(
                instance,
                system,
                vk_instance,
                physical_device,
                device,
                queue_family,
            )
        }
        .map_err(|error| format!("Overlay session creation failed: {error:?}"))?;
        cleanup.pool = Some(pool);
        let stage = session
            .create_reference_space(xr::ReferenceSpaceType::STAGE, xr::Posef::IDENTITY)
            .or_else(|_| {
                session.create_reference_space(xr::ReferenceSpaceType::LOCAL, xr::Posef::IDENTITY)
            })
            .map_err(|error| format!("Reference space creation failed: {error:?}"))?;
        let view = session
            .create_reference_space(xr::ReferenceSpaceType::VIEW, xr::Posef::IDENTITY)
            .map_err(|error| format!("View space creation failed: {error:?}"))?;
        let actions = Actions::new(instance, &session, &stage, hand_interaction)
            .map_err(|error| format!("Action setup failed: {error:?}"))?;

        let formats = session
            .enumerate_swapchain_formats()
            .map_err(|error| format!("Swapchain format query failed: {error:?}"))?;
        let rgba = vk::Format::R8G8B8A8_SRGB.as_raw() as u32;
        let bgra = vk::Format::B8G8R8A8_SRGB.as_raw() as u32;
        let format = if formats.contains(&rgba) {
            rgba
        } else if formats.contains(&bgra) {
            bgra
        } else {
            return Err("Runtime exposes no supported 8-bit sRGB swapchain format".into());
        };
        let command = unsafe {
            device.allocate_command_buffers(
                &vk::CommandBufferAllocateInfo::default()
                    .command_pool(pool)
                    .level(vk::CommandBufferLevel::PRIMARY)
                    .command_buffer_count(1),
            )
        }
        .map_err(|error| format!("Command buffer allocation failed: {error}"))?[0];
        let fence = unsafe { device.create_fence(&vk::FenceCreateInfo::default(), None) }
            .map_err(|error| format!("Fence creation failed: {error}"))?;
        cleanup.fence = Some(fence);
        emit(serde_json::json!({
            "event": "session", "ok": true,
            "format": format, "width": WIDTH, "height": HEIGHT,
        }));

        Ok(Self {
            handles: Some(Handles {
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
                bgra: format == bgra,
            }),
            device: cleanup.device.take().expect("device"),
            vk_instance: cleanup.vk_instance.take().expect("vk_instance"),
            pool: cleanup.pool.take().expect("pool"),
            command,
            fence: cleanup.fence.take().expect("fence"),
            queue,
            physical_device,
            stagings: std::array::from_fn(|_| None),
            frame: None,
            color_scale_bias,
            last_layers: usize::MAX,
            last_layer_report: std::time::Instant::now(),
            running: false,
            tracking_reset: false,
            instance: cleanup.instance.take(),
        })
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

    /// Set the physical width while preserving the aspect ratio of the last
    /// uploaded texture. Callers update this every tick, so the texture aspect
    /// must not be reset to a square here.
    pub fn set_panel_width(
        &mut self,
        kind: PanelId,
        space: PanelSpace,
        pose: xr::Posef,
        width: f32,
    ) {
        if let Some(handles) = self.handles.as_mut() {
            let panel = &mut handles.panels[panel_index(kind)];
            panel.space = space;
            panel.pose = pose;
            panel.size.width = width;
            let [texture_width, texture_height] = panel.texture_size;
            panel.size.height = if texture_width > 0 && texture_height > 0 {
                width * texture_height as f32 / texture_width as f32
            } else {
                width
            };
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
        })
    }

    /// Poll runtime events. Returns whether a frame should be produced now.
    pub fn pump(&mut self) -> Result<bool, String> {
        let instance = self.instance.as_ref().ok_or("XR instance closed")?;
        let xr = self.handles.as_mut().ok_or("XR session closed")?;
        let mut event = xr::EventDataBuffer::new();
        while let Some(event) = instance
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
                xr::Event::ReferenceSpaceChangePending(_) => {
                    // A recenter or stage change invalidates cached world poses.
                    // `pose_valid` only describes the optional previous-space
                    // delta, not whether the change itself happened.
                    self.tracking_reset = true;
                }
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
        let focused = match xr.session.sync_actions(&[(&xr.actions.set).into()]) {
            Ok(()) => true,
            Err(xr::sys::Result::SESSION_NOT_FOCUSED) => false,
            Err(error) => return Err(format!("Action sync failed: {error:?}")),
        };
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
    pub fn end_frame(&mut self) -> Result<bool, String> {
        let Some((time, render)) = self.frame.take() else {
            return Ok(false);
        };
        if render {
            self.submit(time)?;
            Ok(true)
        } else {
            self.skip(time)?;
            Ok(false)
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
                    usage_flags: xr::SwapchainUsageFlags::SAMPLED
                        | xr::SwapchainUsageFlags::COLOR_ATTACHMENT
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
            panel.acquired_image = None;
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
        let (opacity, bgra) = {
            let handles = self.handles.as_ref().ok_or("XR session closed")?;
            (handles.panels[index].opacity, handles.bgra)
        };
        staging
            .as_mut()
            .expect("staging buffer exists")
            .write_scaled(
                pixels,
                if self.color_scale_bias { 1.0 } else { opacity },
                bgra,
            );
        let panel = &mut self.handles.as_mut().ok_or("XR session closed")?.panels[index];
        panel.pixels.clear();
        if !self.color_scale_bias {
            panel.pixels.extend_from_slice(pixels);
        }
        panel.size.height = panel.size.width * height as f32 / width as f32;
        panel.pending = true;
        Ok(())
    }

    /// Set a panel's premultiplied opacity, restaging only when color scale bias is unsupported.
    pub fn set_opacity(&mut self, kind: PanelId, opacity: f32) -> Result<(), String> {
        let opacity = opacity.clamp(0.0, 1.0);
        let index = panel_index(kind);
        if self.color_scale_bias || self.stagings[index].is_none() {
            let panel = &self.handles.as_ref().ok_or("XR session closed")?.panels[index];
            if panel.opacity == opacity {
                return Ok(());
            }
            self.handles.as_mut().ok_or("XR session closed")?.panels[index].opacity = opacity;
            return Ok(());
        }
        let bgra = self.handles.as_ref().ok_or("XR session closed")?.bgra;
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
            .write_scaled(&pixels, opacity, bgra);
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
        let mut uploads = Vec::with_capacity(PANEL_COUNT);
        let mut waited = Vec::with_capacity(PANEL_COUNT);
        for index in 0..PANEL_COUNT {
            let panel = &mut handles.panels[index];
            if !panel.visible || !panel.pending {
                continue;
            }
            let Some(swapchain) = panel.swapchain.as_mut() else {
                continue;
            };
            let buffer = self.stagings[index]
                .as_ref()
                .ok_or("Panel staging buffer is missing")?
                .buffer;
            let image_index = match panel.acquired_image {
                Some(idx) => idx,
                None => {
                    let idx = swapchain
                        .acquire_image()
                        .map_err(|error| format!("Swapchain acquire failed: {error:?}"))?;
                    panel.acquired_image = Some(idx);
                    idx
                }
            };
            match swapchain.wait_image(xr::Duration::from_nanos(50_000_000)) {
                Ok(()) => {
                    uploads.push(PendingUpload {
                        buffer,
                        image: vk::Image::from_raw(panel.images[image_index as usize]),
                        width: panel.texture_size[0],
                        height: panel.texture_size[1],
                    });
                    waited.push(index);
                }
                Err(xr::sys::Result::TIMEOUT_EXPIRED) => {
                    tracing::debug!(index, "OpenXR swapchain wait timed out; deferring upload");
                }
                Err(error) => return Err(format!("Swapchain wait failed: {error:?}")),
            }
        }
        if !uploads.is_empty() {
            let upload_result = upload_images(
                &self.device,
                &self.queue,
                self.command,
                self.fence,
                &uploads,
            );
            for &index in &waited {
                let panel = &mut handles.panels[index];
                panel.acquired_image = None;
                panel
                    .swapchain
                    .as_mut()
                    .expect("swapchain")
                    .release_image()
                    .map_err(|error| format!("Swapchain release failed: {error:?}"))?;
            }
            upload_result?;
            for index in waited {
                let panel = &mut handles.panels[index];
                panel.has_image = true;
                panel.pending = false;
            }
        }
        let mut biases = [xr::sys::CompositionLayerColorScaleBiasKHR {
            ty: xr::sys::StructureType::COMPOSITION_LAYER_COLOR_SCALE_BIAS_KHR,
            next: ptr::null(),
            color_scale: xr::Color4f {
                r: 1.0,
                g: 1.0,
                b: 1.0,
                a: 1.0,
            },
            color_bias: xr::Color4f {
                r: 0.0,
                g: 0.0,
                b: 0.0,
                a: 0.0,
            },
        }; PANEL_COUNT];
        let mut layers = Vec::with_capacity(PANEL_COUNT);
        for (slot, panel) in handles.panels.iter().enumerate() {
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
                    // Match `controller_pose`: the wrist follows the grip when
                    // the profile provides it, and only falls back to aim.
                    let space = handles
                        .actions
                        .grip_spaces
                        .get(hand)
                        .and_then(Option::as_ref)
                        .or_else(|| handles.actions.spaces.get(hand).and_then(Option::as_ref));
                    let Some(space) = space else {
                        continue;
                    };
                    let valid = space.locate(&handles.stage, time).is_ok_and(|loc| {
                        loc.location_flags.contains(
                            xr::SpaceLocationFlags::POSITION_VALID
                                | xr::SpaceLocationFlags::ORIENTATION_VALID,
                        )
                    });
                    if !valid {
                        continue;
                    }
                    space
                }
            };
            let mut layer = xr::CompositionLayerQuad::new()
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
            if self.color_scale_bias && panel.opacity < 1.0 {
                let opacity = panel.opacity.clamp(0.0, 1.0);
                biases[slot].color_scale = xr::Color4f {
                    r: opacity,
                    g: opacity,
                    b: opacity,
                    a: opacity,
                };
                let mut raw = layer.into_raw();
                raw.next = (&raw const biases[slot]).cast();
                layer = unsafe { xr::CompositionLayerQuad::from_raw(raw) };
            }
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
                    .map(|state| {
                        if state.current_state.y.abs() < 0.20 {
                            0.0
                        } else {
                            state.current_state.y
                        }
                    })
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

    /// Rising edge of a controller menu button, used to toggle the settings quad.
    pub fn menu_clicked(&mut self) -> bool {
        let Some(xr) = self.handles.as_mut() else {
            return false;
        };
        let focused = xr.actions.focused;
        let pressed = focused
            && xr.actions.menu.iter().any(|action| {
                action
                    .state(&xr.session, xr::Path::NULL)
                    .ok()
                    .is_some_and(|state| state.is_active && state.current_state)
            });
        let edge = pressed && !xr.actions.menu_pressed;
        // Require a release after focus loss so a held menu button cannot
        // toggle the settings quad when the session becomes focused again.
        xr.actions.menu_pressed = pressed || !focused;
        edge
    }

    /// Ask the session to leave the frame loop and wait until the runtime has
    /// released it. Destroying the application's VkDevice while the runtime is
    /// still unwinding the session crashes inside the driver.
    fn end_session(&mut self) {
        if !self.running {
            return;
        }
        let Some(instance) = self.instance.as_ref() else {
            return;
        };
        let Some(handles) = self.handles.as_mut() else {
            return;
        };
        let _ = handles.session.request_exit();
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(500);
        let mut ended = false;
        let mut event = xr::EventDataBuffer::new();
        while std::time::Instant::now() < deadline {
            match instance.poll_event(&mut event) {
                Ok(Some(event)) => match event {
                    xr::Event::SessionStateChanged(event) => match event.state() {
                        xr::SessionState::STOPPING if !ended => {
                            let _ = handles.session.end();
                            ended = true;
                        }
                        xr::SessionState::EXITING | xr::SessionState::LOSS_PENDING => return,
                        _ => {}
                    },
                    xr::Event::InstanceLossPending(_) => return,
                    _ => {}
                },
                Ok(None) => std::thread::sleep(std::time::Duration::from_millis(2)),
                Err(_) => return,
            }
        }
    }

    pub fn focused(&self) -> bool {
        self.handles
            .as_ref()
            .is_some_and(|handles| handles.actions.focused)
    }

    pub fn take_tracking_reset(&mut self) -> bool {
        std::mem::take(&mut self.tracking_reset)
    }
}

impl Drop for XrRuntime {
    fn drop(&mut self) {
        if self.frame.is_some() {
            let _ = self.end_frame();
        }
        self.end_session();
        let Some(handles) = self.handles.take() else {
            return;
        };
        // The runtime can still have submissions in flight that reference the
        // swapchain images; drain them before tearing the session down.
        unsafe {
            let _ = self.device.device_wait_idle();
        }
        let instance = self.instance.take();
        // Children (swapchains, spaces, actions, frame stream) drop before the
        // session, which drops before the Vulkan objects it was bound to.
        drop(handles);
        for staging in &mut self.stagings {
            if let Some(mut staging) = staging.take() {
                staging.destroy(&self.device);
            }
        }
        unsafe {
            self.device.destroy_fence(self.fence, None);
            self.device.destroy_command_pool(self.pool, None);
            self.device.destroy_device(None);
            self.vk_instance.destroy_instance(None);
        }
        // WiVRn/Monado faults if vkDestroyInstance runs after xrDestroyInstance,
        // so the OpenXR instance is the last handle released.
        drop(instance);
    }
}

struct Actions {
    set: xr::ActionSet,
    // Keep OpenXR action handles alive so `spaces` and `grip_spaces` remain valid.
    #[allow(dead_code)]
    aim: [xr::Action<xr::Posef>; 2],
    #[allow(dead_code)]
    grip_pose: [xr::Action<xr::Posef>; 2],
    spaces: [Option<xr::Space>; 2],
    grip_spaces: [Option<xr::Space>; 2],
    trigger: [xr::Action<bool>; 2],
    grip: [xr::Action<bool>; 2],
    grip_value: [xr::Action<f32>; 2],
    scroll: [xr::Action<xr::Vector2f>; 2],
    menu: [xr::Action<bool>; 2],
    menu_pressed: bool,
    focused: bool,
}

impl Actions {
    fn new(
        instance: &xr::Instance,
        session: &xr::Session<xr::Vulkan>,
        stage: &xr::Space,
        hand_interaction_enabled: bool,
    ) -> xr::Result<Self> {
        let set = instance.create_action_set("vrcs", "VRCS", 0)?;
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
        let menu = [
            set.create_action::<bool>("left_menu", "Left menu", &[sides[0]])?,
            set.create_action::<bool>("right_menu", "Right menu", &[sides[1]])?,
        ];
        // Bind every profile the runtime advertises for hand controllers; a
        // profile that no device matches is ignored.
        let profiles: [(&str, &[&str]); 6] = [
            (
                "/interaction_profiles/khr/simple_controller",
                &["aim/pose", "select/click", "menu/click"],
            ),
            (
                "/interaction_profiles/ext/hand_interaction_ext",
                &[
                    "aim/pose",
                    "grip/pose",
                    "pinch_ext/value",
                    "grasp_ext/value",
                ],
            ),
            (
                "/interaction_profiles/oculus/touch_controller",
                &[
                    "aim/pose",
                    "grip/pose",
                    "trigger/value",
                    "squeeze/value",
                    "thumbstick",
                    "menu/click",
                ],
            ),
            (
                "/interaction_profiles/bytedance/pico4_controller",
                &[
                    "aim/pose",
                    "grip/pose",
                    "trigger/value",
                    "squeeze/value",
                    "thumbstick",
                    "menu/click",
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
                    "trackpad/force",
                ],
            ),
            (
                "/interaction_profiles/htc/vive_controller",
                &[
                    "aim/pose",
                    "grip/pose",
                    "trigger/value",
                    "squeeze/click",
                    "trackpad",
                    "menu/click",
                ],
            ),
        ];
        for (profile_name, inputs) in profiles {
            if !hand_interaction_enabled
                && profile_name == "/interaction_profiles/ext/hand_interaction_ext"
            {
                continue;
            }
            let profile = instance.string_to_path(profile_name)?;
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
                    } else if input.contains("trigger")
                        || input.contains("select")
                        || *input == "pinch_ext/value"
                    {
                        xr::Binding::new(&trigger[index], path)
                    } else if input.starts_with("squeeze/value") || *input == "grasp_ext/value" {
                        xr::Binding::new(&grip_value[index], path)
                    } else if input.starts_with("squeeze/click") {
                        xr::Binding::new(&grip[index], path)
                    } else if *input == "thumbstick" || *input == "trackpad" {
                        xr::Binding::new(&scroll[index], path)
                    } else if (*input == "menu/click" || *input == "trackpad/force") && index == 0 {
                        xr::Binding::new(&menu[index], path)
                    } else {
                        continue;
                    };
                    bindings.push(action);
                }
            }
            match instance.suggest_interaction_profile_bindings(profile, &bindings) {
                Ok(()) => {}
                Err(xr::sys::Result::ERROR_PATH_UNSUPPORTED) => {
                    tracing::debug!(
                        profile = profile_name,
                        "Skipping unsupported OpenXR interaction profile"
                    );
                }
                Err(error) => return Err(error),
            }
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
            menu,
            menu_pressed: false,
            focused: false,
        })
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

struct PendingUpload {
    buffer: vk::Buffer,
    image: vk::Image,
    width: u32,
    height: u32,
}

/// Batch-copy CPU RGBA frames into swapchain images with one command buffer submission.
fn upload_images(
    device: &ash::Device,
    queue: &vk::Queue,
    command: vk::CommandBuffer,
    fence: vk::Fence,
    uploads: &[PendingUpload],
) -> Result<(), String> {
    let barrier = |image: vk::Image,
                   old: vk::ImageLayout,
                   new: vk::ImageLayout,
                   src_access: vk::AccessFlags,
                   dst_access: vk::AccessFlags| {
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
            .src_access_mask(src_access)
            .dst_access_mask(dst_access)
    };
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
        for upload in uploads {
            let region = [vk::BufferImageCopy::default()
                .buffer_row_length(0)
                .buffer_image_height(0)
                .image_subresource(
                    vk::ImageSubresourceLayers::default()
                        .aspect_mask(vk::ImageAspectFlags::COLOR)
                        .layer_count(1),
                )
                .image_extent(vk::Extent3D {
                    width: upload.width,
                    height: upload.height,
                    depth: 1,
                })];
            device.cmd_pipeline_barrier(
                command,
                vk::PipelineStageFlags::TOP_OF_PIPE,
                vk::PipelineStageFlags::TRANSFER,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &[barrier(
                    upload.image,
                    vk::ImageLayout::UNDEFINED,
                    vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                    vk::AccessFlags::empty(),
                    vk::AccessFlags::TRANSFER_WRITE,
                )],
            );
            device.cmd_copy_buffer_to_image(
                command,
                upload.buffer,
                upload.image,
                vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                &region,
            );
            device.cmd_pipeline_barrier(
                command,
                vk::PipelineStageFlags::TRANSFER,
                vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT
                    | vk::PipelineStageFlags::FRAGMENT_SHADER,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &[barrier(
                    upload.image,
                    vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                    vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
                    vk::AccessFlags::TRANSFER_WRITE,
                    vk::AccessFlags::SHADER_READ | vk::AccessFlags::COLOR_ATTACHMENT_READ,
                )],
            );
        }
        device
            .end_command_buffer(command)
            .map_err(|error| format!("Command buffer end failed: {error}"))?;
        device
            .reset_fences(&[fence])
            .map_err(|error| format!("Fence reset failed: {error}"))?;
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

    fn write_scaled(&mut self, pixels: &[u8], opacity: f32, bgra: bool) {
        assert!(pixels.len() <= self.len);
        if opacity >= 1.0 && !bgra {
            unsafe { std::ptr::copy_nonoverlapping(pixels.as_ptr(), self.mapped, pixels.len()) };
            return;
        }
        let scale = opacity.clamp(0.0, 1.0);
        let destination = unsafe { std::slice::from_raw_parts_mut(self.mapped, pixels.len()) };
        for (target, source) in destination
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
            .zip(pixels.as_chunks::<4>().0)
        {
            let [r, g, b, a] = *source;
            let ordered = if bgra { [b, g, r, a] } else { [r, g, b, a] };
            for (slot, channel) in target.iter_mut().zip(ordered) {
                *slot = if opacity >= 1.0 {
                    channel
                } else {
                    (f32::from(channel) * scale).round() as u8
                };
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
            PanelId::Cursor,
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

    #[test]
    fn manifest_library_path_resolves_relative_and_absolute_paths() {
        let manifest = Path::new("/usr/share/openxr/1/openxr_wivrn.json");
        let relative = r#"{
            "file_format_version": "1.0.0",
            "runtime": {
                "name": "WiVRn",
                "library_path": "../../../lib/wivrn/libopenxr_wivrn.so"
            }
        }"#;
        assert_eq!(
            resolve_manifest_library_path(manifest, relative),
            Some((
                PathBuf::from("/usr/share/openxr/1/../../../lib/wivrn/libopenxr_wivrn.so"),
                None,
            ))
        );

        let absolute = r#"{
            "file_format_version": "1.0.0",
            "runtime": {
                "library_path": "/opt/xr/libcustom_xr.so",
                "functions": {
                    "xrNegotiateLoaderRuntimeInterface": "customNegotiate"
                }
            }
        }"#;
        assert_eq!(
            resolve_manifest_library_path(manifest, absolute),
            Some((
                PathBuf::from("/opt/xr/libcustom_xr.so"),
                Some("customNegotiate".into()),
            ))
        );
    }

    #[test]
    fn active_manifest_candidates_honor_override_and_xdg_order() {
        let overridden = active_manifest_candidates_from(
            Some(OsStr::new("/tmp/custom_runtime.json")),
            Some(OsStr::new("/home/u/.config")),
            Some(OsStr::new("/home/u")),
            Some(OsStr::new("/etc/xdg")),
            "x86_64",
        );
        assert_eq!(overridden, vec![PathBuf::from("/tmp/custom_runtime.json")]);

        let xdg = active_manifest_candidates_from(
            None,
            None,
            Some(OsStr::new("/home/u")),
            Some(OsStr::new("/etc/xdg:/opt/xdg")),
            "x86_64",
        );
        assert_eq!(
            xdg,
            vec![
                PathBuf::from("/home/u/.config/openxr/1/active_runtime.x86_64.json"),
                PathBuf::from("/home/u/.config/openxr/1/active_runtime.json"),
                PathBuf::from("/etc/xdg/openxr/1/active_runtime.x86_64.json"),
                PathBuf::from("/etc/xdg/openxr/1/active_runtime.json"),
                PathBuf::from("/opt/xdg/openxr/1/active_runtime.x86_64.json"),
                PathBuf::from("/opt/xdg/openxr/1/active_runtime.json"),
                PathBuf::from("/etc/openxr/1/active_runtime.x86_64.json"),
                PathBuf::from("/etc/openxr/1/active_runtime.json"),
            ]
        );
    }

    #[test]
    fn manifest_requires_daemon_gates_known_linux_runtimes_and_allows_custom() {
        let none = crate::vr_overlay::process::LinuxXrDaemons::default();
        let wivrn_manifest = Path::new("/etc/xdg/openxr/1/active_runtime.json");
        let wivrn_json = r#"{"runtime":{"library_path":"/usr/lib/wivrn/libopenxr_wivrn.so"}}"#;
        assert!(!manifest_requires_daemon(wivrn_manifest, wivrn_json, none));
        assert!(manifest_requires_daemon(
            wivrn_manifest,
            wivrn_json,
            crate::vr_overlay::process::LinuxXrDaemons {
                wivrn: true,
                ..none
            }
        ));

        let monado_json = r#"{"runtime":{"library_path":"/usr/lib/libopenxr_monado.so"}}"#;
        assert!(!manifest_requires_daemon(wivrn_manifest, monado_json, none));
        assert!(manifest_requires_daemon(
            wivrn_manifest,
            monado_json,
            crate::vr_overlay::process::LinuxXrDaemons {
                monado: true,
                ..none
            }
        ));

        let steam_json = r#"{"runtime":{"library_path":"/home/u/.local/share/Steam/steamapps/common/SteamVR/bin/linux64/steamxr_linux64.so"}}"#;
        assert!(!manifest_requires_daemon(wivrn_manifest, steam_json, none));
        assert!(manifest_requires_daemon(
            wivrn_manifest,
            steam_json,
            crate::vr_overlay::process::LinuxXrDaemons {
                steamvr: true,
                ..none
            }
        ));

        let custom_json = r#"{"runtime":{"library_path":"/opt/xr/libcustom_xr.so"}}"#;
        assert!(manifest_requires_daemon(wivrn_manifest, custom_json, none));
        assert!(manifest_requires_daemon(wivrn_manifest, "{bad json", none));
    }
}
