use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::time::{Duration, Instant};

use ::pipewire as pw;
use pw::types::ObjectType;

use crate::models::AudioDevice;
use super::super::AudioError;
use super::DeviceDirection;

const DISCOVERY_TIMEOUT: Duration = Duration::from_secs(3);

pub(super) fn error(detail: impl std::fmt::Display) -> AudioError {
    AudioError::with_code("audio.pipewire_failed", detail.to_string())
}

#[derive(Clone, Debug)]
pub(super) struct Node {
    pub id: u32,
    pub name: String,
    pub label: String,
    pub direction: DeviceDirection,
    pub rate: u32,
    pub channels: u32,
}

impl Node {
    pub fn key(&self) -> String {
        let class = match self.direction {
            DeviceDirection::Render => "Audio/Sink",
            DeviceDirection::Capture => "Audio/Source",
        };
        format!("pipewire:{class}:{}", self.name)
    }

    pub fn device(&self, is_default: bool) -> AudioDevice {
        let key = self.key();
        // Retained for the common response schema, not accepted as a selection
        // identifier on Linux. Only device_key is persistent backend identity.
        AudioDevice {
            id: crate::audio::device_id(&key),
            device_key: Some(key),
            name: self.label.clone(),
            is_default,
            is_loopback: self.direction == DeviceDirection::Render,
            sample_rate: self.rate,
            channels: self.channels,
        }
    }
}

#[derive(Default)]
pub(super) struct Inventory {
    pub nodes: HashMap<u32, Node>,
    pub default_sink: Option<String>,
    pub default_source: Option<String>,
    pub failure: Option<AudioError>,
}

impl Inventory {
    pub fn default_name(&self, direction: DeviceDirection) -> Option<&str> {
        match direction {
            DeviceDirection::Render => self.default_sink.as_deref(),
            DeviceDirection::Capture => self.default_source.as_deref(),
        }
    }

    pub fn resolve(&self, key: Option<&str>, direction: DeviceDirection) -> Result<Node, AudioError> {
        let default = self.default_name(direction);
        if key.is_none() && default.is_none() {
            return Err(AudioError::with_code(
                "audio.device_unavailable", "PipeWire default device metadata is unavailable",
            ));
        }
        let mut matches = self.nodes.values().filter(|node| {
            node.direction == direction && match key {
                Some(key) => node.key() == key,
                None => Some(node.name.as_str()) == default,
            }
        });
        let node = matches.next().ok_or_else(|| AudioError::with_code(
            "audio.device_unavailable", "Selected PipeWire audio device is no longer available; select it again",
        ))?;
        if matches.next().is_some() {
            return Err(AudioError::with_code(
                "audio.device_ambiguous", "Multiple PipeWire devices have the selected identity; select a uniquely named device",
            ));
        }
        Ok(node.clone())
    }
}

// Keep the loop entered across non-PipeWire work too. Otherwise an invoke
// arriving while the loop has no owning thread may execute on the data thread.
struct EnteredLoop(pw::main_loop::MainLoopRc);

impl EnteredLoop {
    fn new(main_loop: &pw::main_loop::MainLoopRc) -> Self {
        // SAFETY: Session and all its Rc-backed objects remain on this thread.
        unsafe { main_loop.loop_().enter(); }
        Self(main_loop.clone())
    }
}

impl Drop for EnteredLoop {
    fn drop(&mut self) {
        // SAFETY: paired with new(), after listeners/proxies/core are dropped.
        unsafe { self.0.loop_().leave(); }
    }
}

// Field order keeps all listeners alive only while their proxies/core exist.
// Metadata proxies are retained because default changes arrive after discovery.
pub(super) struct Session {
    _registry_listener: pw::registry::Listener,
    _core_listener: pw::core::Listener,
    _metadata: Rc<RefCell<HashMap<u32, (pw::metadata::MetadataListener, pw::metadata::Metadata)>>>,
    _nodes: Rc<RefCell<HashMap<u32, (pw::node::NodeListener, pw::node::Node)>>>,
    _registry: pw::registry::RegistryRc,
    pub core: pw::core::CoreRc,
    _entered: EnteredLoop,
    pub main_loop: pw::main_loop::MainLoopRc,
    pub inventory: Rc<RefCell<Inventory>>,
}

impl Session {
    pub fn connect(stop: Option<&std::sync::atomic::AtomicBool>) -> Result<Self, AudioError> {
        pw::init();
        let main_loop = pw::main_loop::MainLoopRc::new(None).map_err(error)?;
        let entered = EnteredLoop::new(&main_loop);
        let context = pw::context::ContextRc::new(&main_loop, None).map_err(error)?;
        let core = context.connect_rc(None).map_err(|detail| AudioError::with_code(
            "audio.service_not_running", format!("Cannot connect to the user PipeWire service: {detail}"),
        ))?;
        let registry = core.get_registry_rc().map_err(error)?;
        let inventory = Rc::new(RefCell::new(Inventory::default()));
        let metadata = Rc::new(RefCell::new(HashMap::new()));
        let nodes = Rc::new(RefCell::new(HashMap::new()));
        let core_inventory = inventory.clone();
        let core_listener = core.add_listener_local().error(move |id, _, result, detail| {
            let denied = result == -libc::EACCES || result == -libc::EPERM;
            if id == pw::core::PW_ID_CORE || denied {
                core_inventory.borrow_mut().failure = Some(AudioError::with_code(
                    if denied { "audio.permission_denied" } else { "audio.service_disconnected" },
                    format!("PipeWire error {result}: {detail}"),
                ));
            }
        }).register();
        let reg_inventory = inventory.clone();
        let remove_inventory = inventory.clone();
        let reg_metadata = metadata.clone();
        let remove_metadata = metadata.clone();
        let reg_nodes = nodes.clone();
        let remove_nodes = nodes.clone();
        let weak_registry = registry.downgrade();
        let registry_listener = registry.add_listener_local().global(move |object| {
            let Some(props) = object.props.as_ref() else { return; };
            if object.type_ == ObjectType::Node {
                let direction = match props.get("media.class") {
                    Some("Audio/Sink") => DeviceDirection::Render,
                    Some("Audio/Source") => DeviceDirection::Capture,
                    _ => return,
                };
                let Some(name) = props.get("node.name").filter(|name| !name.is_empty()) else { return; };
                reg_inventory.borrow_mut().nodes.insert(object.id, Node {
                    id: object.id,
                    name: name.to_owned(),
                    label: props.get("node.description").or_else(|| props.get("node.nick")).unwrap_or(name).to_owned(),
                    direction,
                    rate: props.get("audio.rate").and_then(|rate| rate.parse().ok()).unwrap_or(0),
                    channels: props.get("audio.channels").and_then(|channels| channels.parse().ok()).unwrap_or(0),
                });
                // Registry globals only advertise routing identity. Device
                // format properties are generally delivered by Node::info.
                let Some(registry) = weak_registry.upgrade() else { return; };
                let proxy: pw::node::Node = match registry.bind(object) {
                    Ok(proxy) => proxy,
                    Err(detail) => { reg_inventory.borrow_mut().failure = Some(error(detail)); return; }
                };
                let node_inventory = reg_inventory.clone();
                let node_id = object.id;
                let listener = proxy.add_listener_local().info(move |info| {
                    let Some(props) = info.props() else { return; };
                    let mut inventory = node_inventory.borrow_mut();
                    let Some(node) = inventory.nodes.get_mut(&node_id) else { return; };
                    if let Some(rate) = props.get("audio.rate").and_then(|rate| rate.parse().ok()) {
                        node.rate = rate;
                    }
                    if let Some(channels) = props.get("audio.channels").and_then(|channels| channels.parse().ok()) {
                        node.channels = channels;
                    }
                }).register();
                reg_nodes.borrow_mut().insert(object.id, (listener, proxy));
            } else if object.type_ == ObjectType::Metadata && props.get("metadata.name") == Some("default") {
                let Some(registry) = weak_registry.upgrade() else { return; };
                let proxy: pw::metadata::Metadata = match registry.bind(object) {
                    Ok(proxy) => proxy,
                    Err(detail) => { reg_inventory.borrow_mut().failure = Some(error(detail)); return; }
                };
                let defaults = reg_inventory.clone();
                let listener = proxy.add_listener_local().property(move |subject, key, _, value| {
                    if subject != pw::core::PW_ID_CORE { return 0; }
                    let mut inventory = defaults.borrow_mut();
                    let destination = match key {
                        Some("default.audio.sink") => &mut inventory.default_sink,
                        Some("default.audio.source") => &mut inventory.default_source,
                        None => { inventory.default_sink = None; inventory.default_source = None; return 0; }
                        _ => return 0,
                    };
                    *destination = value.and_then(|value| serde_json::from_str::<serde_json::Value>(value).ok())
                        .and_then(|value| value.get("name").and_then(|name| name.as_str()).map(str::to_owned));
                    0
                }).register();
                reg_metadata.borrow_mut().insert(object.id, (listener, proxy));
            }
        }).global_remove(move |id| {
            remove_inventory.borrow_mut().nodes.remove(&id);
            remove_nodes.borrow_mut().remove(&id);
            if remove_metadata.borrow_mut().remove(&id).is_some() {
                let mut inventory = remove_inventory.borrow_mut();
                inventory.default_sink = None;
                inventory.default_source = None;
            }
        }).register();
        let session = Self {
            _registry_listener: registry_listener,
            _core_listener: core_listener,
            _metadata: metadata,
            _nodes: nodes,
            _registry: registry,
            core,
            _entered: entered,
            main_loop,
            inventory,
        };
        // First barrier discovers globals; second includes initial events from
        // node and metadata proxies bound by the registry callbacks.
        let deadline = Instant::now() + DISCOVERY_TIMEOUT;
        for _ in 0..2 {
            let sequence = session.core.sync(0).map_err(|detail| AudioError::retryable_with_code("audio.service_disconnected", detail.to_string()))?;
            let done = Rc::new(Cell::new(false));
            let done_callback = done.clone();
            let _listener = session.core.add_listener_local().done(move |id, seq| {
                if id == pw::core::PW_ID_CORE && seq == sequence { done_callback.set(true); }
            }).register();
            while !done.get() {
                if stop.is_some_and(|stop| stop.load(std::sync::atomic::Ordering::Relaxed)) {
                    return Err(AudioError::with_code("capture.stopped", "PipeWire capture stopped during discovery"));
                }
                if Instant::now() >= deadline {
                    return Err(AudioError::with_code("audio.service_timeout", "PipeWire device discovery timed out"));
                }
                session.pump(pw::loop_::Timeout::Finite(Duration::from_millis(50)))?;
            }
        }
        Ok(session)
    }

    pub fn pump(&self, timeout: pw::loop_::Timeout) -> Result<(), AudioError> {
        // SAFETY: _entered keeps this loop owned by this same thread.
        if unsafe { self.main_loop.loop_().iterate_unguarded(timeout) } < 0 {
            return Err(AudioError::retryable_with_code("audio.service_disconnected", "PipeWire main loop failed"));
        }
        if let Some(failure) = self.inventory.borrow_mut().failure.take() { return Err(failure); }
        Ok(())
    }
}

pub(crate) fn list_devices() -> Result<Vec<AudioDevice>, AudioError> {
    let session = Session::connect(None)?;
    let inventory = session.inventory.borrow();
    let mut devices: Vec<_> = inventory.nodes.values().map(|node| {
        node.device(inventory.default_name(node.direction) == Some(node.name.as_str()))
    }).collect();
    devices.sort_by(|a, b| a.is_loopback.cmp(&b.is_loopback).then(a.name.cmp(&b.name)).then(a.device_key.cmp(&b.device_key)));
    Ok(devices)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(id: u32, name: &str, direction: DeviceDirection) -> Node {
        Node { id, name: name.into(), label: name.into(), direction, rate: 48_000, channels: 2 }
    }

    #[test]
    fn persistent_selection_survives_node_id_changes_but_never_falls_back() {
        let original = node(12, "usb-microphone", DeviceDirection::Capture);
        let key = original.key();
        let mut inventory = Inventory::default();
        inventory.nodes.insert(88, node(88, "usb-microphone", DeviceDirection::Capture));
        inventory.nodes.insert(9, node(9, "other", DeviceDirection::Capture));
        inventory.default_source = Some("other".into());
        assert_eq!(inventory.resolve(Some(&key), DeviceDirection::Capture).unwrap().id, 88);
        inventory.nodes.remove(&88);
        assert_eq!(inventory.resolve(Some(&key), DeviceDirection::Capture).unwrap_err().code(), "audio.device_unavailable");
        inventory.nodes.insert(20, node(20, "usb-microphone", DeviceDirection::Capture));
        inventory.nodes.insert(21, node(21, "usb-microphone", DeviceDirection::Capture));
        assert_eq!(inventory.resolve(Some(&key), DeviceDirection::Capture).unwrap_err().code(), "audio.device_ambiguous");
    }

    #[test]
    fn default_selection_uses_current_metadata_and_direction() {
        let mut inventory = Inventory::default();
        inventory.nodes.insert(1, node(1, "first", DeviceDirection::Render));
        inventory.nodes.insert(2, node(2, "second", DeviceDirection::Render));
        inventory.nodes.insert(3, node(3, "second", DeviceDirection::Capture));
        inventory.default_sink = Some("first".into());
        assert_eq!(inventory.resolve(None, DeviceDirection::Render).unwrap().id, 1);
        inventory.default_sink = Some("second".into());
        assert_eq!(inventory.resolve(None, DeviceDirection::Render).unwrap().id, 2);
    }
}
