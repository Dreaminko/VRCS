//! One authorized source, one frozen frame. No global PipeWire connection or desktop fallback.
use super::image::{Image, PixelFormat};
use ashpd::desktop::{screencast::{CursorMode, Screencast, SourceType}, Session, PersistMode};
use futures_util::StreamExt;
use std::{cell::RefCell, os::fd::OwnedFd, rc::Rc, sync::{Arc, atomic::{AtomicBool, Ordering}}, time::{Duration, Instant}};

struct SessionGuard {
    session: Option<Session<'static, Screencast<'static>>>,
    cancelled: Arc<AtomicBool>,
}
impl Drop for SessionGuard {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Release);
        if let Some(session) = self.session.take() {
            tauri::async_runtime::spawn(async move { let _ = session.close().await; });
        }
    }
}

fn portal_error(error: ashpd::Error) -> String {
    use ashpd::{Error, PortalError};
    match error {
        Error::Response(ashpd::desktop::ResponseError::Cancelled) | Error::Portal(PortalError::Cancelled(_)) => "capture.cancelled".into(),
        Error::Response(_) | Error::Portal(PortalError::NotAllowed(_)) => "capture.rejected".into(),
        Error::PortalNotFound(_) | Error::RequiresVersion(_, _) => "capture.no_portal".into(),
        other => { tracing::warn!(%other, "ScreenCast Portal failed"); "capture.portal_failed".into() }
    }
}

pub async fn available() -> bool {
    match Screencast::new().await {
        Ok(proxy) => proxy.available_source_types().await.is_ok_and(|types| types.contains(SourceType::Monitor)),
        Err(_) => false,
    }
}

pub async fn freeze(cancelled: Arc<AtomicBool>) -> Result<Option<Image>, String> {
    // A dropped caller must not abandon CreateSession between the DBus reply and
    // installing its close guard. The transaction owns cleanup independently.
    tauri::async_runtime::spawn(freeze_inner(cancelled)).await.map_err(|_| "capture.portal_failed".to_string())?
}

async fn freeze_inner(cancelled: Arc<AtomicBool>) -> Result<Option<Image>, String> {
    let proxy = Screencast::new().await.map_err(|_| "capture.no_portal".to_string())?;
    let session = proxy.create_session().await.map_err(portal_error)?;
    let worker_cancelled = Arc::new(AtomicBool::new(false));
    let mut guard = SessionGuard { session: Some(session), cancelled: worker_cancelled.clone() };
    let session = guard.session.as_ref().unwrap();
    if cancelled.load(Ordering::Acquire) { return Ok(None); }
    let result = async {
        let mut closed = session.receive_closed().await.map_err(portal_error)?;
        let work = async {
            proxy.select_sources(session, CursorMode::Hidden, SourceType::Monitor.into(), false, None, PersistMode::DoNot)
                .await.map_err(portal_error)?.response().map_err(portal_error)?;
            let streams = proxy.start(session, None).await.map_err(portal_error)?.response().map_err(portal_error)?;
            // Do not silently pick among multiple granted screens.
            if streams.streams().len() != 1 { return Err("capture.invalid_sources".into()); }
            let node = streams.streams()[0].pipe_wire_node_id();
            let fd = proxy.open_pipe_wire_remote(session).await.map_err(portal_error)?;
            let flag = worker_cancelled.clone();
            tauri::async_runtime::spawn_blocking(move || receive_frame(fd, node, flag)).await
                .map_err(|_| "capture.pipewire_failed".to_string())?
        };
        tokio::pin!(work);
        tokio::select! {
            result = &mut work => result,
            _ = closed.next() => Err("capture.session_closed".into()),
            _ = async { while !cancelled.load(Ordering::Acquire) { tokio::time::sleep(Duration::from_millis(40)).await; } } => Ok(None),
        }
    }.await;
    worker_cancelled.store(true, Ordering::Release);
    let _ = session.close().await;
    guard.session.take();
    drop(guard);
    match result { Err(error) if error == "capture.cancelled" => Ok(None), result => result }
}

fn receive_frame(fd: OwnedFd, node: u32, cancelled: Arc<AtomicBool>) -> Result<Option<Image>, String> {
    use pipewire::{self as pw, spa, properties::properties};
    use spa::{pod::Pod, param::video::{VideoFormat, VideoInfoRaw}};
    pw::init();
    let mainloop = pw::main_loop::MainLoopRc::new(None).map_err(|e| e.to_string())?;
    let context = pw::context::ContextRc::new(&mainloop, None).map_err(|e| e.to_string())?;
    let core = context.connect_fd_rc(fd, None).map_err(|e| e.to_string())?;
    let stream = pw::stream::StreamBox::new(&core, "VRCS authorized image", properties! {
        *pw::keys::MEDIA_TYPE => "Video", *pw::keys::MEDIA_CATEGORY => "Capture", *pw::keys::MEDIA_ROLE => "Screen"
    }).map_err(|e| e.to_string())?;
    struct Data { format: VideoInfoRaw, result: Rc<RefCell<Option<Result<Image, String>>>> }
    let result = Rc::new(RefCell::new(None));
    let _listener = stream.add_local_listener_with_user_data(Data { format: Default::default(), result: result.clone() })
        .state_changed(|_, data, _, state| {
            if let pw::stream::StreamState::Error(error) = state {
                *data.result.borrow_mut() = Some(Err(if error.contains("no more input formats") || error.contains("no more output formats") {
                    "capture.unsupported_format".into()
                } else { format!("capture.pipewire_failed: {error}") }));
            }
        })
        .param_changed(|stream, data, id, param| {
            if id != spa::param::ParamType::Format.as_raw() { return; }
            if let Some(param) = param {
                if data.format.parse(param).is_err() { *data.result.borrow_mut() = Some(Err("capture.unsupported_format".into())); return; }
                // CPU readback only understands explicit linear DMA-BUFs.
                let dma = data.format.flags().contains(spa::param::video::VideoFlags::MODIFIER);
                if dma && data.format.modifier() != 0 { *data.result.borrow_mut() = Some(Err("capture.unsupported_format".into())); return; }
                let metadata = [
                    (spa::sys::SPA_META_VideoTransform, std::mem::size_of::<spa::sys::spa_meta_videotransform>()),
                    (spa::sys::SPA_META_VideoCrop, std::mem::size_of::<spa::sys::spa_meta_region>()),
                    (spa::sys::SPA_META_Header, std::mem::size_of::<spa::sys::spa_meta_header>()),
                ];
                let pods: Result<Vec<_>, _> = metadata.into_iter().map(|(kind, size)| {
                    let object = spa::pod::Object {
                        type_: spa::utils::SpaTypes::ObjectParamMeta.as_raw(), id: spa::param::ParamType::Meta.as_raw(),
                        properties: vec![
                            spa::pod::Property::new(spa::sys::SPA_PARAM_META_type, spa::pod::Value::Id(spa::utils::Id(kind))),
                            spa::pod::Property::new(spa::sys::SPA_PARAM_META_size, spa::pod::Value::Int(size as i32)),
                        ],
                    };
                    spa::pod::serialize::PodSerializer::serialize(std::io::Cursor::new(Vec::new()), &spa::pod::Value::Object(object)).map(|value| value.0.into_inner())
                }).collect();
                let pods = pods.and_then(|mut pods| {
                    let types = if dma { 1 << spa::sys::SPA_DATA_DmaBuf } else { (1 << spa::sys::SPA_DATA_MemFd) | (1 << spa::sys::SPA_DATA_MemPtr) };
                    let buffers = spa::pod::Object {
                        type_: spa::utils::SpaTypes::ObjectParamBuffers.as_raw(), id: spa::param::ParamType::Buffers.as_raw(),
                        properties: vec![
                            spa::pod::Property::new(spa::sys::SPA_PARAM_BUFFERS_blocks, spa::pod::Value::Int(1)),
                            spa::pod::Property::new(spa::sys::SPA_PARAM_BUFFERS_dataType, spa::pod::Value::Choice(spa::pod::ChoiceValue::Int(spa::utils::Choice(
                                spa::utils::ChoiceFlags::empty(), spa::utils::ChoiceEnum::Flags { default: types, flags: vec![types] },
                            )))),
                        ],
                    };
                    pods.push(spa::pod::serialize::PodSerializer::serialize(std::io::Cursor::new(Vec::new()), &spa::pod::Value::Object(buffers))?.0.into_inner());
                    Ok(pods)
                });
                match pods {
                    Ok(pods) => {
                        let mut params: Vec<_> = pods.iter().filter_map(|bytes| Pod::from_bytes(bytes)).collect();
                        if stream.update_params(&mut params).is_err() { *data.result.borrow_mut() = Some(Err("capture.metadata_failed".into())); }
                    }
                    Err(_) => *data.result.borrow_mut() = Some(Err("capture.metadata_failed".into())),
                }
            }
        })
        .process(|stream, data| {
            if data.result.borrow().is_some() { return; }
            let Some(mut buffer) = stream.dequeue_buffer() else { return; };
            if buffer.find_meta::<spa::buffer::meta::MetaHeader>().is_some_and(|header| header.flags().contains(spa::buffer::meta::MetaHeaderFlags::CORRUPTED)) { return; }
            let crop = buffer.find_meta::<spa::buffer::meta::MetaVideoCrop>().and_then(|crop| {
                let region = crop.meta_region();
                region.is_valid().then(|| (region.position(), region.size()))
            });
            let transform = buffer.find_meta::<spa::buffer::meta::MetaVideoTransform>().map(|meta| meta.transform().as_raw()).unwrap_or(0);
            let planes = buffer.datas_mut();
            if planes.len() != 1 { *data.result.borrow_mut() = Some(Err("capture.unsupported_format".into())); return; }
            let plane = &mut planes[0];
            if plane.as_raw().chunk.is_null() || plane.chunk().size() == 0 { return; }
            if plane.chunk().flags().contains(spa::buffer::ChunkFlags::CORRUPTED) { return; }
            let format = match data.format.format() {
                VideoFormat::RGBA => PixelFormat::Rgba, VideoFormat::BGRA => PixelFormat::Bgra,
                VideoFormat::RGBx => PixelFormat::Rgbx, VideoFormat::BGRx => PixelFormat::Bgrx,
                _ => { *data.result.borrow_mut() = Some(Err("capture.unsupported_format".into())); return; }
            };
            let frame = read_plane(plane, data.format, format).and_then(|image| {
                let image = if let Some((position, size)) = crop {
                    if position.x < 0 || position.y < 0 { return Err("capture.invalid_frame".into()); }
                    image.crop(super::selection::Region { left: position.x as u32, top: position.y as u32, width: size.width, height: size.height })?
                } else { image };
                image.transform(transform)
            });
            *data.result.borrow_mut() = Some(frame);
        }).register().map_err(|e| e.to_string())?;
    let obj = spa::pod::object!(
        spa::utils::SpaTypes::ObjectParamFormat, spa::param::ParamType::EnumFormat,
        spa::pod::property!(spa::param::format::FormatProperties::MediaType, Id, spa::param::format::MediaType::Video),
        spa::pod::property!(spa::param::format::FormatProperties::MediaSubtype, Id, spa::param::format::MediaSubtype::Raw),
        spa::pod::property!(spa::param::format::FormatProperties::VideoFormat, Choice, Enum, Id, VideoFormat::BGRx, VideoFormat::BGRx, VideoFormat::BGRA, VideoFormat::RGBA, VideoFormat::RGBx)
    );
    // A modifier is mandatory on DMA-BUF-only compositors (e.g. niri 26.04).
    // Offer linear DMA-BUF separately; leaving the original offer unmodified
    // preserves shared-memory negotiation on GNOME and other Portal backends.
    let mut dma_obj = obj.clone();
    dma_obj.properties.push(spa::pod::Property {
        key: spa::param::format::FormatProperties::VideoModifier.as_raw(),
        flags: spa::pod::PropertyFlags::MANDATORY,
        value: spa::pod::Value::Long(0), // DRM_FORMAT_MOD_LINEAR
    });
    let dma_values = spa::pod::serialize::PodSerializer::serialize(std::io::Cursor::new(Vec::new()), &spa::pod::Value::Object(dma_obj))
        .map_err(|e| e.to_string())?.0.into_inner();
    let values = spa::pod::serialize::PodSerializer::serialize(std::io::Cursor::new(Vec::new()), &spa::pod::Value::Object(obj))
        .map_err(|e| e.to_string())?.0.into_inner();
    stream.connect(spa::utils::Direction::Input, Some(node), pw::stream::StreamFlags::AUTOCONNECT | pw::stream::StreamFlags::MAP_BUFFERS, &mut [
        Pod::from_bytes(&values).ok_or("capture.invalid_format")?,
        Pod::from_bytes(&dma_values).ok_or("capture.invalid_format")?,
    ])
        .map_err(|e| e.to_string())?;
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if cancelled.load(Ordering::Acquire) { return Ok(None); }
        if let Some(result) = result.borrow_mut().take() { return result.map(Some); }
        if Instant::now() >= deadline { return Err("capture.frame_timeout".into()); }
        mainloop.loop_().iterate(pw::loop_::Timeout::Finite(Duration::from_millis(40)));
    }
}


/// Read only packed single-plane frames negotiated on the authorized remote.
fn read_plane(plane: &mut pipewire::spa::buffer::Data, info: pipewire::spa::param::video::VideoInfoRaw, format: PixelFormat) -> Result<Image, String> {
    use pipewire::spa::{buffer::DataType, param::video::VideoFlags};
    let size = info.size();
    let offset = plane.chunk().offset() as usize;
    let stride = plane.chunk().stride();
    if plane.type_() != DataType::DmaBuf {
        if info.flags().contains(VideoFlags::MODIFIER) { return Err("capture.unsupported_format".into()); }
        let length = plane.chunk().size() as usize;
        let bytes = plane.data().ok_or("capture.invalid_frame")?;
        let bytes = offset.checked_add(length).and_then(|end| bytes.get(offset..end)).ok_or("capture.invalid_frame")?;
        return Image::from_rows(size.width, size.height, stride, bytes, format);
    }
    if !info.flags().contains(VideoFlags::MODIFIER) || info.modifier() != 0 { return Err("capture.unsupported_format".into()); }
    let row = (size.width as usize).checked_mul(4).ok_or("capture.invalid_frame")?;
    let pitch = stride.unsigned_abs() as usize;
    if size.width == 0 || size.height == 0 || pitch < row { return Err("capture.invalid_frame".into()); }
    let length = pitch.checked_mul(size.height as usize - 1).and_then(|n| n.checked_add(row)).ok_or("capture.invalid_frame")?;
    let start = (plane.as_raw().mapoffset as usize).checked_add(offset).ok_or("capture.invalid_frame")?;
    let mapped_length = start.checked_add(length).filter(|n| *n <= 512 * 1024 * 1024).ok_or("capture.invalid_frame")?;
    let fd = plane.fd();
    if fd < 0 { return Err("capture.invalid_frame".into()); }
    // DMA-BUF maxsize is not meaningful (niri sets it to 1). Query the actual
    // allocation length before mmap so malformed offsets cannot cause SIGBUS.
    let allocation_length = unsafe { libc::lseek(fd, 0, libc::SEEK_END) };
    if allocation_length < 0 || mapped_length as u64 > allocation_length as u64 { return Err("capture.invalid_frame".into()); }
    // No explicit-sync metadata is negotiated. Wait for the producer's
    // implicit GPU fence before entering the CPU cache-coherency bracket.
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        let mut poll = libc::pollfd { fd, events: libc::POLLIN, revents: 0 };
        let ready = unsafe { libc::poll(&mut poll, 1, 40) };
        if ready > 0 {
            if poll.revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL) != 0 { return Err("capture.pipewire_failed".into()); }
            if poll.revents & libc::POLLIN != 0 { break; }
        } else if ready < 0 && std::io::Error::last_os_error().kind() != std::io::ErrorKind::Interrupted { return Err("capture.pipewire_failed".into()); }
        if Instant::now() >= deadline { return Err("capture.frame_timeout".into()); }
    }
    let address = unsafe { libc::mmap(std::ptr::null_mut(), mapped_length, libc::PROT_READ, libc::MAP_SHARED, fd, 0) };
    if address == libc::MAP_FAILED { return Err("capture.unsupported_format".into()); }
    let mut mapping = DmaReadMapping { address, length: mapped_length, fd, started: false };
    dma_sync(fd, 1)?; // DMA_BUF_SYNC_START | DMA_BUF_SYNC_READ
    mapping.started = true;
    // Keep the PipeWire buffer dequeued until copy and END sync are complete.
    let bytes = unsafe { std::slice::from_raw_parts(address.cast::<u8>().add(start), length) };
    let image = Image::from_rows(size.width, size.height, stride, bytes, format)?;
    dma_sync(fd, 1 | 4)?; // DMA_BUF_SYNC_END | DMA_BUF_SYNC_READ
    mapping.started = false;
    Ok(image)
}

fn dma_sync(fd: std::os::fd::RawFd, flags: u64) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        // Linux UAPI _IOW('b', 0, struct dma_buf_sync { __u64 flags; }).
        if unsafe { libc::ioctl(fd, libc::_IOW::<u64>(b'b' as u32, 0), &flags) } == 0 { return Ok(()); }
        let error = std::io::Error::last_os_error();
        if !matches!(error.raw_os_error(), Some(libc::EINTR) | Some(libc::EAGAIN)) || Instant::now() >= deadline { return Err("capture.pipewire_failed".into()); }
        std::thread::sleep(Duration::from_millis(1));
    }
}

struct DmaReadMapping { address: *mut libc::c_void, length: usize, fd: std::os::fd::RawFd, started: bool }
impl Drop for DmaReadMapping {
    fn drop(&mut self) {
        if self.started { let _ = dma_sync(self.fd, 1 | 4); }
        unsafe { libc::munmap(self.address, self.length); }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ending_capture_stops_worker_without_cancelling_region_selection() {
        let request_cancelled = Arc::new(AtomicBool::new(false));
        let worker_cancelled = Arc::new(AtomicBool::new(false));
        let guard = SessionGuard { session: None, cancelled: worker_cancelled.clone() };
        drop(guard);
        assert!(worker_cancelled.load(Ordering::Acquire));
        assert!(!request_cancelled.load(Ordering::Acquire));
    }
}
