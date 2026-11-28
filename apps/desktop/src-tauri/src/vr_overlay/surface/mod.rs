//! Synchronous GPU-rendered VR overlay surfaces backed by `gpu_ui`.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tauri::{AppHandle, Manager};

use super::backend::{Backend, OverlayKind};
use super::gpu_ui::{GpuUiEngine, SurfaceRuntimeState};
use super::surface_protocol::{SurfaceId, SurfaceKey, SurfaceLayout, SurfaceState, SurfaceView};

#[cfg(windows)]
pub(crate) mod frame {
    use crate::vr_overlay::gpu_ui::painter_d3d11::D3d11SurfaceTarget;

    pub struct Capture {
        pub(super) target: D3d11SurfaceTarget,
    }

    pub(crate) struct Captured {
        pub shared_handle: usize,
        pub bounds: [u32; 4],
    }

    impl Capture {
        pub(super) fn new(width: u32, height: u32) -> Self {
            Self {
                target: D3d11SurfaceTarget::new(width, height),
            }
        }

        pub(crate) fn submitted(&mut self, frame: &Captured) -> Result<(), String> {
            self.target.submitted_handle(frame.shared_handle)
        }
    }
}

#[cfg(linux_vr)]
pub(crate) mod frame {
    use crate::vr_overlay::renderer::Texture;

    pub struct Capture;

    pub(crate) struct Captured {
        pub texture: Texture,
        pub bounds: [u32; 4],
    }

    impl Capture {
        pub(super) fn new(_width: u32, _height: u32) -> Self {
            Self
        }

        pub(crate) fn submitted(&mut self, _frame: &Captured) -> Result<(), String> {
            Ok(())
        }
    }
}

#[cfg(windows)]
thread_local! {
    static D3D11_PAINTER: RefCell<Option<(u32, crate::vr_overlay::gpu_ui::painter_d3d11::D3d11Painter)>> =
        const { RefCell::new(None) };
}

#[cfg(all(linux_vr, target_arch = "x86_64"))]
thread_local! {
    static VULKAN_PAINTER: RefCell<Option<crate::vr_overlay::gpu_ui::painter_vulkan::VulkanPainter>> =
        const { RefCell::new(None) };
}

#[cfg(windows)]
pub(super) fn warm_gpu(adapter_index: u32) {
    D3D11_PAINTER.with(|cell| {
        let mut slot = cell.borrow_mut();
        if slot.as_ref().is_none_or(|(idx, _)| *idx != adapter_index)
            && let Ok(painter) =
                crate::vr_overlay::gpu_ui::painter_d3d11::D3d11Painter::new(adapter_index)
        {
            *slot = Some((adapter_index, painter));
        }
    });
}

struct Entry {
    active: bool,
    generation: u32,
    state: Option<SurfaceState>,
    layout: Option<Arc<SurfaceLayout>>,
    runtime: SurfaceRuntimeState,
    dirty: bool,
    #[cfg(windows)]
    last_anim_paint: Option<Instant>,
}

impl Default for Entry {
    fn default() -> Self {
        Self {
            active: false,
            generation: 1,
            state: None,
            layout: None,
            runtime: SurfaceRuntimeState::default(),
            dirty: false,
            #[cfg(windows)]
            last_anim_paint: None,
        }
    }
}

impl Entry {
    fn set_state(
        &mut self,
        view: SurfaceView,
        hovered: Option<u8>,
        pressed: Option<u8>,
    ) -> SurfaceKey {
        let content_changed = self
            .state
            .as_ref()
            .is_none_or(|state| !state.view.same_layout(&view));
        let feedback_changed = self.state.as_ref().is_some_and(|state| {
            state.hovered != hovered
                || state.pressed != pressed
                || menu_progress_changed(&state.view, &view)
        });

        if content_changed {
            let revision = self
                .state
                .as_ref()
                .map_or(1, |state| state.key.revision.wrapping_add(1).max(1));
            let key = SurfaceKey {
                generation: self.generation,
                revision,
            };
            let scroll_key = self
                .state
                .as_ref()
                .filter(|state| state.view.same_scroll_view(&view))
                .map_or(key, |state| state.scroll_key);
            self.state = Some(SurfaceState {
                key,
                scroll_key,
                view,
                hovered,
                pressed,
                pointer: self.state.as_ref().and_then(|s| s.pointer),
            });
            self.dirty = true;
            key
        } else {
            let state = self.state.as_mut().expect("state exists");
            state.view = view;
            state.hovered = hovered;
            state.pressed = pressed;
            if feedback_changed {
                self.dirty = true;
            }
            state.key
        }
    }

    #[cfg(windows)]
    fn needs_animation_tick(&self, now: Instant) -> bool {
        if !self.active {
            return false;
        }
        let animated = matches!(
            self.state.as_ref().map(|s| &s.view),
            Some(SurfaceView::OcrProgress {
                progress: Some(_),
                ..
            })
        );
        animated
            && self
                .last_anim_paint
                .is_none_or(|at| now.saturating_duration_since(at) >= Duration::from_millis(16))
    }

    #[cfg(not(windows))]
    fn needs_animation_tick(&self, _now: Instant) -> bool {
        false
    }
}

fn menu_progress_changed(a: &SurfaceView, b: &SurfaceView) -> bool {
    match (a, b) {
        (
            SurfaceView::Menu {
                hold_progress: pa, ..
            },
            SurfaceView::Menu {
                hold_progress: pb, ..
            },
        ) => pa != pb,
        _ => false,
    }
}

struct HubInner {
    entries: [Entry; 10],
    engine: GpuUiEngine,
}

impl Default for HubInner {
    fn default() -> Self {
        Self {
            entries: std::array::from_fn(|_| Entry::default()),
            engine: GpuUiEngine::new(),
        }
    }
}

#[derive(Default)]
pub struct SurfaceHub(Mutex<HubInner>);

impl SurfaceHub {
    pub(super) fn needs_fast_tick(&self) -> bool {
        let now = Instant::now();
        let inner = crate::vr_overlay::runtime::lock_unpoisoned(&self.0);
        inner
            .entries
            .iter()
            .any(|entry| entry.active && (entry.dirty || entry.needs_animation_tick(now)))
    }

    pub fn generation(&self, surface: SurfaceId) -> u32 {
        crate::vr_overlay::runtime::lock_unpoisoned(&self.0).entries[surface.index()].generation
    }

    pub fn layout(&self, surface: SurfaceId) -> Option<Arc<SurfaceLayout>> {
        let inner = crate::vr_overlay::runtime::lock_unpoisoned(&self.0);
        let entry = &inner.entries[surface.index()];
        entry.active.then(|| entry.layout.clone()).flatten()
    }

    pub fn pointer_layout(&self, surface: SurfaceId) -> Option<Arc<SurfaceLayout>> {
        self.layout(surface)
    }

    pub fn point(
        &self,
        _app: &AppHandle,
        surface: SurfaceId,
        point: Option<[f32; 2]>,
    ) -> Result<(), String> {
        let mut inner = crate::vr_overlay::runtime::lock_unpoisoned(&self.0);
        let entry = &mut inner.entries[surface.index()];
        let point = point.filter(|p| {
            entry.layout.is_some() && p.iter().all(|v| v.is_finite() && (0.0..=1.0).contains(v))
        });
        if let Some(state) = &mut entry.state {
            state.pointer = point;
        }
        Ok(())
    }

    pub fn scroll_reader(&self, app: &AppHandle, delta: f32) -> Result<(), String> {
        self.scroll(app, SurfaceId::Reader, delta)
    }

    pub fn scroll(&self, _app: &AppHandle, surface: SurfaceId, delta: f32) -> Result<(), String> {
        if !delta.is_finite() || delta == 0.0 {
            return Ok(());
        }
        let mut inner = crate::vr_overlay::runtime::lock_unpoisoned(&self.0);
        let entry = &mut inner.entries[surface.index()];
        if GpuUiEngine::apply_scroll(
            surface,
            delta,
            entry.state.as_ref().map(|s| &s.view),
            &mut entry.runtime,
        ) {
            entry.dirty = true;
        }
        Ok(())
    }

    pub(super) fn set_locale(&self, locale: &str) {
        let mut inner = crate::vr_overlay::runtime::lock_unpoisoned(&self.0);
        if inner.engine.text.set_locale(locale) {
            for entry in &mut inner.entries {
                if entry.active {
                    entry.dirty = true;
                }
            }
        }
    }

    pub(super) fn publish(
        &self,
        _app: &AppHandle,
        surface: SurfaceId,
        view: SurfaceView,
        hovered: Option<u8>,
        pressed: Option<u8>,
    ) -> Result<Option<SurfaceKey>, String> {
        let mut inner = crate::vr_overlay::runtime::lock_unpoisoned(&self.0);
        let entry = &mut inner.entries[surface.index()];
        let key = entry.set_state(view, hovered, pressed);
        Ok(Some(key))
    }

    fn set_active(&self, surface: SurfaceId, active: bool) {
        let mut inner = crate::vr_overlay::runtime::lock_unpoisoned(&self.0);
        let entry = &mut inner.entries[surface.index()];
        if entry.active != active {
            entry.active = active;
            if active {
                entry.dirty = true;
            }
        }
    }
}

pub(super) struct PendingFrame {
    key: SurfaceKey,
    capture: Rc<RefCell<frame::Capture>>,
    frame: frame::Captured,
}

pub struct SurfaceRenderer {
    surface: SurfaceId,
    requested: bool,
    visible_frame: bool,
    hidden_since: Option<Instant>,
    capture: Option<Rc<RefCell<frame::Capture>>>,
    pending_frame: Option<PendingFrame>,
    published_key: Option<SurfaceKey>,
    retained_key: Option<SurfaceKey>,
    output_bounds: Option<[u32; 4]>,
}

impl SurfaceRenderer {
    pub fn new(surface: SurfaceId) -> Self {
        Self {
            surface,
            requested: false,
            visible_frame: false,
            hidden_since: None,
            capture: None,
            pending_frame: None,
            published_key: None,
            retained_key: None,
            output_bounds: None,
        }
    }

    pub fn reset(&mut self) {
        self.capture = None;
        self.pending_frame = None;
        self.visible_frame = false;
        self.hidden_since = None;
        self.published_key = None;
        self.retained_key = None;
        self.output_bounds = None;
    }

    pub fn finish_tick(
        &mut self,
        app: &AppHandle,
        mut backend: Option<&mut Backend>,
        kind: OverlayKind,
    ) {
        let requested = std::mem::take(&mut self.requested) && backend.is_some();
        if requested {
            self.hidden_since = None;
            return;
        }
        if let Some(backend) = backend.as_deref_mut() {
            backend.hide(kind);
        }
        app.state::<SurfaceHub>().set_active(self.surface, false);
        self.visible_frame = false;
        self.pending_frame = None;
        let now = Instant::now();
        let hidden_at = *self.hidden_since.get_or_insert(now);
        let expired =
            backend.is_none() || now.saturating_duration_since(hidden_at) >= Duration::from_secs(5);
        if expired
            && self.capture.is_some()
            && backend.is_none_or(|b| b.clear_surface_texture(kind).is_ok())
        {
            self.reset();
        }
    }

    pub fn stop(&mut self, app: &AppHandle, backend: Option<&mut Backend>, kind: OverlayKind) {
        self.requested = false;
        self.finish_tick(app, backend, kind);
    }

    pub(super) fn request(&mut self, app: &AppHandle) -> Result<(), String> {
        self.requested = true;
        app.state::<SurfaceHub>().set_active(self.surface, true);
        Ok(())
    }

    pub fn present(
        &mut self,
        app: &AppHandle,
        backend: &mut Backend,
        kind: OverlayKind,
        view: SurfaceView,
        hovered: Option<u8>,
        pressed: Option<u8>,
    ) -> Result<bool, String> {
        self.requested = true;
        let hub = app.state::<SurfaceHub>();
        hub.set_active(self.surface, true);
        self.published_key = hub.publish(app, self.surface, view, hovered, pressed)?;
        self.service(app, backend, kind)
    }

    pub fn refresh(
        &mut self,
        app: &AppHandle,
        backend: &mut Backend,
        kind: OverlayKind,
        feedback: Option<(Option<u8>, Option<u8>, Option<f32>)>,
    ) -> Result<bool, String> {
        let hub = app.state::<SurfaceHub>();
        {
            let mut inner = crate::vr_overlay::runtime::lock_unpoisoned(&hub.0);
            let entry = &mut inner.entries[self.surface.index()];
            if !entry.active {
                return Ok(false);
            }
            self.requested = true;
            if let Some((hovered, pressed, progress)) = feedback
                && let Some(state) = entry.state.as_mut()
            {
                let mut changed = state.hovered != hovered || state.pressed != pressed;
                state.hovered = hovered;
                state.pressed = pressed;
                if let SurfaceView::Menu { hold_progress, .. } = &mut state.view {
                    changed |= *hold_progress != progress;
                    *hold_progress = progress;
                }
                if changed {
                    entry.dirty = true;
                }
            }
        }
        self.service(app, backend, kind)
    }

    fn service(
        &mut self,
        app: &AppHandle,
        backend: &mut Backend,
        kind: OverlayKind,
    ) -> Result<bool, String> {
        let now = Instant::now();
        let hub = app.state::<SurfaceHub>();
        let needs_render = {
            let inner = crate::vr_overlay::runtime::lock_unpoisoned(&hub.0);
            let entry = &inner.entries[self.surface.index()];
            entry.active
                && entry.state.is_some()
                && (entry.dirty || !self.visible_frame || entry.needs_animation_tick(now))
        };
        if !needs_render {
            return Ok(self.visible_frame);
        }

        let (w, h) = self.surface.size();
        let capture = self
            .capture
            .get_or_insert_with(|| Rc::new(RefCell::new(frame::Capture::new(w, h))))
            .clone();

        #[cfg(windows)]
        let adapter_index = backend.surface_adapter_index()?;

        let (key, captured) = {
            let mut inner = crate::vr_overlay::runtime::lock_unpoisoned(&hub.0);
            let HubInner { entries, engine } = &mut *inner;
            let entry = &mut entries[self.surface.index()];
            let Some(state) = entry.state.as_ref() else {
                return Ok(false);
            };
            let key = state.key;
            let (layout, draw) = engine.render_surface(state, &mut entry.runtime, now);
            layout.validate(self.surface)?;
            entry.layout = Some(Arc::new(layout));
            entry.dirty = false;
            #[cfg(windows)]
            {
                entry.last_anim_paint = Some(now);
            }

            #[cfg(windows)]
            let captured = {
                let mut cap = capture.borrow_mut();
                D3D11_PAINTER.with(|cell| {
                    let mut slot = cell.borrow_mut();
                    let need_new = slot.as_ref().is_none_or(|(idx, _)| *idx != adapter_index);
                    if need_new {
                        let painter = crate::vr_overlay::gpu_ui::painter_d3d11::D3d11Painter::new(
                            adapter_index,
                        )?;
                        *slot = Some((adapter_index, painter));
                    }
                    let (_, painter) = slot.as_mut().expect("painter initialized");
                    let (shared_handle, bounds) =
                        painter.render(&mut cap.target, &engine.text.atlas, &draw)?;
                    Ok::<_, String>(frame::Captured {
                        shared_handle,
                        bounds,
                    })
                })?
            };

            #[cfg(all(linux_vr, target_arch = "x86_64"))]
            let captured = VULKAN_PAINTER.with(|cell| {
                let mut slot = cell.borrow_mut();
                if slot.is_none() {
                    *slot = Some(crate::vr_overlay::gpu_ui::painter_vulkan::VulkanPainter::new()?);
                }
                let painter = slot.as_mut().expect("vulkan painter");
                let texture = painter.render_rgba(&engine.text.atlas, &draw)?;
                let bounds = draw.capture_bounds;
                Ok::<_, String>(frame::Captured { texture, bounds })
            })?;

            (key, captured)
        };

        backend.upload_surface(kind, &captured, Rc::clone(&capture))?;
        let bounds = captured.bounds;
        if backend.defers_submission() {
            self.pending_frame = Some(PendingFrame {
                key,
                capture,
                frame: captured,
            });
            return Ok(self.visible_frame);
        }
        capture.borrow_mut().submitted(&captured)?;
        self.visible_frame = true;
        self.retained_key = Some(key);
        self.output_bounds = Some(bounds);
        Ok(true)
    }

    pub fn current_frame(&self, app: &AppHandle) -> bool {
        let hub = app.state::<SurfaceHub>();
        let inner = crate::vr_overlay::runtime::lock_unpoisoned(&hub.0);
        let entry = &inner.entries[self.surface.index()];
        entry.active
            && self.visible_frame
            && self.published_key.is_some()
            && self.published_key == self.retained_key
    }

    pub fn has_published_key(&self) -> bool {
        self.published_key.is_some()
    }

    pub fn hold_for_fade(
        &mut self,
        _app: &AppHandle,
        backend: &Backend,
        kind: OverlayKind,
    ) -> bool {
        let ready =
            self.visible_frame && self.retained_key.is_some() && backend.has_surface_texture(kind);
        if ready {
            self.requested = true;
        }
        ready
    }

    pub fn confirm_frame(&mut self, _app: &AppHandle, accepted: bool) {
        let Some(pending) = self.pending_frame.take() else {
            return;
        };
        if !accepted {
            self.visible_frame = false;
            return;
        }
        if pending
            .capture
            .borrow_mut()
            .submitted(&pending.frame)
            .is_ok()
        {
            self.visible_frame = true;
            self.retained_key = Some(pending.key);
            self.output_bounds = Some(pending.frame.bounds);
        } else {
            self.visible_frame = false;
        }
    }

    #[cfg(windows)]
    pub fn output_bounds(&self) -> [u32; 4] {
        self.output_bounds
            .unwrap_or([0, 0, self.surface.size().0, self.surface.size().1])
    }
}

#[cfg(all(windows, debug_assertions, feature = "vr-preview"))]
pub(crate) fn smoke(app: AppHandle, views: Vec<(SurfaceId, SurfaceView)>) {
    use crate::vr_overlay::gpu_ui::painter_d3d11::{D3d11Painter, D3d11SurfaceTarget};

    let dump_dir = std::env::var_os("VRCS_SURFACE_SMOKE_DIR").map(std::path::PathBuf::from);
    if let Some(dir) = &dump_dir {
        let _ = std::fs::create_dir_all(dir);
    }

    let mut engine = GpuUiEngine::new();
    let mut painter = match D3d11Painter::new(0) {
        Ok(p) => p,
        Err(err) => {
            tracing::error!("GPU surface smoke failed to init D3D11 painter: {err}");
            app.exit(1);
            return;
        }
    };

    for (surface, view) in views {
        let (w, h) = surface.size();
        let state = SurfaceState {
            key: SurfaceKey {
                generation: 1,
                revision: 1,
            },
            scroll_key: SurfaceKey {
                generation: 1,
                revision: 1,
            },
            view,
            hovered: None,
            pressed: None,
            pointer: None,
        };
        let mut rt = SurfaceRuntimeState::default();
        let (layout, draw) = engine.render_surface(&state, &mut rt, Instant::now());
        if let Err(err) = layout.validate(surface) {
            tracing::error!(surface = surface.name(), "Smoke layout invalid: {err}");
            app.exit(1);
            return;
        }
        let mut target = D3d11SurfaceTarget::new(w, h);
        let (handle, bounds) = match painter.render(&mut target, &engine.text.atlas, &draw) {
            Ok(res) => res,
            Err(err) => {
                tracing::error!(surface = surface.name(), "Smoke D3D11 render failed: {err}");
                app.exit(1);
                return;
            }
        };
        let rgba = match painter.readback_rgba(&target, handle, bounds) {
            Ok(px) => px,
            Err(err) => {
                tracing::error!(surface = surface.name(), "Smoke readback failed: {err}");
                app.exit(1);
                return;
            }
        };
        if !rgba.as_chunks::<4>().0.iter().any(|px| px[3] > 0) {
            tracing::error!(
                surface = surface.name(),
                "Smoke frame had no visible pixels"
            );
            app.exit(1);
            return;
        }
        if let Some(dir) = &dump_dir {
            let rw = bounds[2] - bounds[0];
            let rh = bounds[3] - bounds[1];
            let path = dir.join(format!("{}.png", surface.name()));
            if let Ok(file) = std::fs::File::create(&path) {
                let mut enc = png::Encoder::new(file, rw, rh);
                enc.set_color(png::ColorType::Rgba);
                enc.set_depth(png::BitDepth::Eight);
                if let Ok(mut writer) = enc.write_header() {
                    let _ = writer.write_image_data(&rgba);
                }
            }
        }
    }
    app.exit(0);
}
