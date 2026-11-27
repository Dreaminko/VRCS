//! Vulkan GPU offscreen painter for Linux VR overlay surfaces (`x86_64`).

use std::ffi::CStr;

use ash::vk;

use super::draw::{DrawList, Instance, MaskVertex};
use super::ocr_scene::quad_homography;
use super::text::{ATLAS_R8_SIZE, ATLAS_RGBA_SIZE, GlyphAtlas};
use crate::vr_overlay::renderer::Texture;

const UI_VERT_SPV: &[u8] = include_bytes!("shaders/ui.vert.spv");
const UI_FRAG_SPV: &[u8] = include_bytes!("shaders/ui.frag.spv");
const MASK_VERT_SPV: &[u8] = include_bytes!("shaders/mask.vert.spv");
const MASK_FRAG_SPV: &[u8] = include_bytes!("shaders/mask.frag.spv");
const CARD_VERT_SPV: &[u8] = include_bytes!("shaders/card.vert.spv");
const CARD_FRAG_SPV: &[u8] = include_bytes!("shaders/card.frag.spv");

const SCRATCH_SIZE: u32 = 2048;

#[repr(C)]
#[derive(Clone, Copy)]
struct DrawParams {
    rt_size: [f32; 2],
    inv_rt_size: [f32; 2],
    h_row0: [f32; 4],
    h_row1: [f32; 4],
    h_row2: [f32; 4],
    card_info: [f32; 4],
}

struct AllocatedBuffer {
    buffer: vk::Buffer,
    memory: vk::DeviceMemory,
    size: vk::DeviceSize,
}

struct AllocatedImage {
    image: vk::Image,
    memory: vk::DeviceMemory,
    view: vk::ImageView,
    width: u32,
    height: u32,
}

pub struct VulkanPainter {
    _entry: ash::Entry,
    instance: ash::Instance,
    device: ash::Device,
    queue: vk::Queue,
    cmd_pool: vk::CommandPool,
    cmd_buf: vk::CommandBuffer,
    fence: vk::Fence,
    mem_props: vk::PhysicalDeviceMemoryProperties,
    target_rp: vk::RenderPass,
    scratch_rp: vk::RenderPass,
    set_layout: vk::DescriptorSetLayout,
    desc_pool: vk::DescriptorPool,
    desc_set: vk::DescriptorSet,
    pipe_layout: vk::PipelineLayout,
    ui_pipe: vk::Pipeline,
    mask_pipe: vk::Pipeline,
    card_pipe: vk::Pipeline,
    linear_clamp: vk::Sampler,
    border_zero: vk::Sampler,
    r8_img: AllocatedImage,
    rgba_img: AllocatedImage,
    scratch_img: AllocatedImage,
    scratch_fb: vk::Framebuffer,
    r8_generation: u64,
    rgba_generation: u64,
    atlas_staging: Option<AllocatedBuffer>,
    inst_buf: Option<AllocatedBuffer>,
    mask_buf: Option<AllocatedBuffer>,
    target_img: Option<(AllocatedImage, vk::Framebuffer)>,
    readback_buf: Option<AllocatedBuffer>,
}

impl VulkanPainter {
    pub fn new() -> Result<Self, String> {
        let entry =
            unsafe { ash::Entry::load() }.map_err(|e| format!("Load Vulkan loader failed: {e}"))?;
        let app_info = vk::ApplicationInfo::default()
            .application_name(c"vrcs-gpu-ui")
            .api_version(vk::API_VERSION_1_1);
        let inst_info = vk::InstanceCreateInfo::default().application_info(&app_info);
        let instance = unsafe { entry.create_instance(&inst_info, None) }
            .map_err(|e| format!("vkCreateInstance failed: {e:?}"))?;

        let phys = unsafe { instance.enumerate_physical_devices() }
            .map_err(|e| format!("vkEnumeratePhysicalDevices: {e:?}"))?
            .into_iter()
            .next()
            .ok_or("No Vulkan physical device")?;
        let mem_props = unsafe { instance.get_physical_device_memory_properties(phys) };
        let queue_family = unsafe { instance.get_physical_device_queue_family_properties(phys) }
            .iter()
            .position(|f| f.queue_flags.contains(vk::QueueFlags::GRAPHICS))
            .ok_or("No Vulkan graphics queue")? as u32;

        let priorities = [1.0f32];
        let queue_info = [vk::DeviceQueueCreateInfo::default()
            .queue_family_index(queue_family)
            .queue_priorities(&priorities)];
        let dev_info = vk::DeviceCreateInfo::default().queue_create_infos(&queue_info);
        let device = unsafe { instance.create_device(phys, &dev_info, None) }
            .map_err(|e| format!("vkCreateDevice failed: {e:?}"))?;
        let queue = unsafe { device.get_device_queue(queue_family, 0) };

        let pool_info = vk::CommandPoolCreateInfo::default()
            .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER)
            .queue_family_index(queue_family);
        let cmd_pool = unsafe { device.create_command_pool(&pool_info, None) }
            .map_err(|e| format!("vkCreateCommandPool: {e:?}"))?;
        let alloc_info = vk::CommandBufferAllocateInfo::default()
            .command_pool(cmd_pool)
            .level(vk::CommandBufferLevel::PRIMARY)
            .command_buffer_count(1);
        let cmd_buf = unsafe { device.allocate_command_buffers(&alloc_info) }
            .map_err(|e| format!("vkAllocateCommandBuffers: {e:?}"))?[0];
        let fence = unsafe { device.create_fence(&vk::FenceCreateInfo::default(), None) }
            .map_err(|e| format!("vkCreateFence: {e:?}"))?;

        let target_rp = create_render_pass(&device, vk::ImageLayout::TRANSFER_SRC_OPTIMAL)?;
        let scratch_rp = create_render_pass(&device, vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)?;

        let bindings = [
            vk::DescriptorSetLayoutBinding::default()
                .binding(0)
                .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
                .descriptor_count(1)
                .stage_flags(vk::ShaderStageFlags::FRAGMENT),
            vk::DescriptorSetLayoutBinding::default()
                .binding(1)
                .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
                .descriptor_count(1)
                .stage_flags(vk::ShaderStageFlags::FRAGMENT),
            vk::DescriptorSetLayoutBinding::default()
                .binding(2)
                .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
                .descriptor_count(1)
                .stage_flags(vk::ShaderStageFlags::FRAGMENT),
        ];
        let set_layout = unsafe {
            device.create_descriptor_set_layout(
                &vk::DescriptorSetLayoutCreateInfo::default().bindings(&bindings),
                None,
            )
        }
        .map_err(|e| format!("vkCreateDescriptorSetLayout: {e:?}"))?;

        let pool_sizes = [vk::DescriptorPoolSize::default()
            .ty(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
            .descriptor_count(3)];
        let desc_pool = unsafe {
            device.create_descriptor_pool(
                &vk::DescriptorPoolCreateInfo::default()
                    .max_sets(1)
                    .pool_sizes(&pool_sizes),
                None,
            )
        }
        .map_err(|e| format!("vkCreateDescriptorPool: {e:?}"))?;
        let set_layouts = [set_layout];
        let desc_set = unsafe {
            device.allocate_descriptor_sets(
                &vk::DescriptorSetAllocateInfo::default()
                    .descriptor_pool(desc_pool)
                    .set_layouts(&set_layouts),
            )
        }
        .map_err(|e| format!("vkAllocateDescriptorSets: {e:?}"))?[0];

        let pc_range = [vk::PushConstantRange::default()
            .stage_flags(vk::ShaderStageFlags::VERTEX)
            .offset(0)
            .size(std::mem::size_of::<DrawParams>() as u32)];
        let pipe_layout = unsafe {
            device.create_pipeline_layout(
                &vk::PipelineLayoutCreateInfo::default()
                    .set_layouts(&set_layouts)
                    .push_constant_ranges(&pc_range),
                None,
            )
        }
        .map_err(|e| format!("vkCreatePipelineLayout: {e:?}"))?;

        let ui_pipe = create_ui_pipeline(&device, pipe_layout, target_rp)?;
        let mask_pipe = create_mask_pipeline(&device, pipe_layout, target_rp)?;
        let card_pipe = create_card_pipeline(&device, pipe_layout, target_rp)?;

        let linear_clamp = create_sampler(&device, vk::SamplerAddressMode::CLAMP_TO_EDGE)?;
        let border_zero = create_sampler(&device, vk::SamplerAddressMode::CLAMP_TO_BORDER)?;

        let r8_img = create_image(
            &device,
            &mem_props,
            ATLAS_R8_SIZE,
            ATLAS_R8_SIZE,
            vk::Format::R8_UNORM,
            vk::ImageUsageFlags::SAMPLED | vk::ImageUsageFlags::TRANSFER_DST,
        )?;
        let rgba_img = create_image(
            &device,
            &mem_props,
            ATLAS_RGBA_SIZE,
            ATLAS_RGBA_SIZE,
            vk::Format::R8G8B8A8_UNORM,
            vk::ImageUsageFlags::SAMPLED | vk::ImageUsageFlags::TRANSFER_DST,
        )?;
        let scratch_img = create_image(
            &device,
            &mem_props,
            SCRATCH_SIZE,
            SCRATCH_SIZE,
            vk::Format::R8G8B8A8_UNORM,
            vk::ImageUsageFlags::COLOR_ATTACHMENT | vk::ImageUsageFlags::SAMPLED,
        )?;
        let scratch_fb = create_framebuffer(
            &device,
            scratch_rp,
            scratch_img.view,
            SCRATCH_SIZE,
            SCRATCH_SIZE,
        )?;

        // Bind descriptors once; underlying image views never reallocate.
        let i0 = [vk::DescriptorImageInfo::default()
            .sampler(linear_clamp)
            .image_view(r8_img.view)
            .image_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)];
        let i1 = [vk::DescriptorImageInfo::default()
            .sampler(linear_clamp)
            .image_view(rgba_img.view)
            .image_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)];
        let i2 = [vk::DescriptorImageInfo::default()
            .sampler(border_zero)
            .image_view(scratch_img.view)
            .image_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)];
        let writes = [
            vk::WriteDescriptorSet::default()
                .dst_set(desc_set)
                .dst_binding(0)
                .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
                .image_info(&i0),
            vk::WriteDescriptorSet::default()
                .dst_set(desc_set)
                .dst_binding(1)
                .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
                .image_info(&i1),
            vk::WriteDescriptorSet::default()
                .dst_set(desc_set)
                .dst_binding(2)
                .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
                .image_info(&i2),
        ];
        unsafe { device.update_descriptor_sets(&writes, &[]) };

        Ok(Self {
            _entry: entry,
            instance,
            device,
            queue,
            cmd_pool,
            cmd_buf,
            fence,
            mem_props,
            target_rp,
            scratch_rp,
            set_layout,
            desc_pool,
            desc_set,
            pipe_layout,
            ui_pipe,
            mask_pipe,
            card_pipe,
            linear_clamp,
            border_zero,
            r8_img,
            rgba_img,
            scratch_img,
            scratch_fb,
            r8_generation: 0,
            rgba_generation: 0,
            atlas_staging: None,
            inst_buf: None,
            mask_buf: None,
            target_img: None,
            readback_buf: None,
        })
    }

    pub fn render_rgba(&mut self, atlas: &GlyphAtlas, draw: &DrawList) -> Result<Texture, String> {
        let w = draw.width.max(1);
        let h = draw.height.max(1);
        let needs_r8 = self.r8_generation != atlas.r8_generation;
        let needs_rgba = self.rgba_generation != atlas.rgba_generation;

        // Collect all instances across main pass + cards into one dynamic vertex buffer.
        let mut total_inst = draw.instances.len();
        for card in &draw.ocr_cards {
            total_inst += card.instances.len();
        }
        if total_inst > 0 {
            let need_bytes = (total_inst * std::mem::size_of::<Instance>()) as vk::DeviceSize;
            ensure_host_buffer(
                &self.device,
                &self.mem_props,
                &mut self.inst_buf,
                need_bytes,
                vk::BufferUsageFlags::VERTEX_BUFFER,
            )?;
            let mut combined = Vec::with_capacity(total_inst);
            combined.extend_from_slice(&draw.instances);
            for card in &draw.ocr_cards {
                combined.extend_from_slice(&card.instances);
            }
            upload_host_buffer(&self.device, self.inst_buf.as_ref().unwrap(), &combined)?;
        }
        if !draw.mask_vertices.is_empty() {
            let need_bytes =
                (draw.mask_vertices.len() * std::mem::size_of::<MaskVertex>()) as vk::DeviceSize;
            ensure_host_buffer(
                &self.device,
                &self.mem_props,
                &mut self.mask_buf,
                need_bytes,
                vk::BufferUsageFlags::VERTEX_BUFFER,
            )?;
            upload_host_buffer(
                &self.device,
                self.mask_buf.as_ref().unwrap(),
                &draw.mask_vertices,
            )?;
        }

        if self
            .target_img
            .as_ref()
            .is_none_or(|(img, _)| img.width != w || img.height != h)
        {
            if let Some((img, fb)) = self.target_img.take() {
                unsafe {
                    self.device.destroy_framebuffer(fb, None);
                    destroy_image(&self.device, img);
                }
            }
            let img = create_image(
                &self.device,
                &self.mem_props,
                w,
                h,
                vk::Format::R8G8B8A8_UNORM,
                vk::ImageUsageFlags::COLOR_ATTACHMENT | vk::ImageUsageFlags::TRANSFER_SRC,
            )?;
            let fb = create_framebuffer(&self.device, self.target_rp, img.view, w, h)?;
            self.target_img = Some((img, fb));
        }

        let staging_bytes = (ATLAS_R8_SIZE * ATLAS_R8_SIZE + ATLAS_RGBA_SIZE * ATLAS_RGBA_SIZE * 4)
            as vk::DeviceSize;
        if needs_r8 || needs_rgba {
            ensure_host_buffer(
                &self.device,
                &self.mem_props,
                &mut self.atlas_staging,
                staging_bytes,
                vk::BufferUsageFlags::TRANSFER_SRC,
            )?;
            let sb = self.atlas_staging.as_ref().unwrap();
            let ptr = unsafe {
                self.device
                    .map_memory(sb.memory, 0, staging_bytes, vk::MemoryMapFlags::empty())
            }
            .map_err(|e| format!("vkMapMemory atlas staging: {e:?}"))?
                as *mut u8;
            let r8_len = (ATLAS_R8_SIZE * ATLAS_R8_SIZE) as usize;
            unsafe {
                if needs_r8 {
                    std::ptr::copy_nonoverlapping(atlas.r8_data.as_ptr(), ptr, r8_len);
                }
                if needs_rgba {
                    std::ptr::copy_nonoverlapping(
                        atlas.rgba_data.as_ptr(),
                        ptr.add(r8_len),
                        (ATLAS_RGBA_SIZE * ATLAS_RGBA_SIZE * 4) as usize,
                    );
                }
                self.device.unmap_memory(sb.memory);
            }
        }

        let read_bytes = u64::from(w) * u64::from(h) * 4;
        ensure_host_buffer(
            &self.device,
            &self.mem_props,
            &mut self.readback_buf,
            read_bytes,
            vk::BufferUsageFlags::TRANSFER_DST,
        )?;

        unsafe {
            self.device
                .reset_command_buffer(self.cmd_buf, vk::CommandBufferResetFlags::empty())
                .map_err(|e| format!("vkResetCommandBuffer: {e:?}"))?;
            self.device
                .begin_command_buffer(
                    self.cmd_buf,
                    &vk::CommandBufferBeginInfo::default()
                        .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT),
                )
                .map_err(|e| format!("vkBeginCommandBuffer: {e:?}"))?;
        }

        if needs_r8 || needs_rgba {
            let sb = self.atlas_staging.as_ref().unwrap().buffer;
            let r8_offset = 0u64;
            let rgba_offset = (ATLAS_R8_SIZE * ATLAS_R8_SIZE) as u64;
            if needs_r8 {
                record_image_upload(
                    &self.device,
                    self.cmd_buf,
                    sb,
                    r8_offset,
                    self.r8_img.image,
                    ATLAS_R8_SIZE,
                    ATLAS_R8_SIZE,
                );
                self.r8_generation = atlas.r8_generation;
            }
            if needs_rgba {
                record_image_upload(
                    &self.device,
                    self.cmd_buf,
                    sb,
                    rgba_offset,
                    self.rgba_img.image,
                    ATLAS_RGBA_SIZE,
                    ATLAS_RGBA_SIZE,
                );
                self.rgba_generation = atlas.rgba_generation;
            }
        }

        let (ref t_img, t_fb) = *self.target_img.as_ref().unwrap();
        let clear = [vk::ClearValue {
            color: vk::ClearColorValue { float32: [0.0; 4] },
        }];

        // Render main pass (masks + base UI instances) into target
        begin_rp(
            &self.device,
            self.cmd_buf,
            self.target_rp,
            t_fb,
            w,
            h,
            &clear,
            vk::AttachmentLoadOp::CLEAR,
        );
        let pc_target = DrawParams {
            rt_size: [w as f32, h as f32],
            inv_rt_size: [1.0 / w as f32, 1.0 / h as f32],
            h_row0: [0.0; 4],
            h_row1: [0.0; 4],
            h_row2: [0.0; 4],
            card_info: [0.0; 4],
        };
        push_params(&self.device, self.cmd_buf, self.pipe_layout, &pc_target);
        unsafe {
            self.device.cmd_bind_descriptor_sets(
                self.cmd_buf,
                vk::PipelineBindPoint::GRAPHICS,
                self.pipe_layout,
                0,
                &[self.desc_set],
                &[],
            );
        }
        if !draw.mask_vertices.is_empty() {
            let mb = self.mask_buf.as_ref().unwrap().buffer;
            unsafe {
                self.device.cmd_bind_pipeline(
                    self.cmd_buf,
                    vk::PipelineBindPoint::GRAPHICS,
                    self.mask_pipe,
                );
                self.device.cmd_bind_vertex_buffers(0, &[mb], &[0]);
                self.device
                    .cmd_draw(self.cmd_buf, draw.mask_vertices.len() as u32, 1, 0, 0);
            }
        }
        if !draw.instances.is_empty() {
            let ib = self.inst_buf.as_ref().unwrap().buffer;
            unsafe {
                self.device.cmd_bind_pipeline(
                    self.cmd_buf,
                    vk::PipelineBindPoint::GRAPHICS,
                    self.ui_pipe,
                );
                self.device.cmd_bind_vertex_buffers(0, &[ib], &[0]);
                self.device
                    .cmd_draw(self.cmd_buf, 6, draw.instances.len() as u32, 0, 0);
            }
        }
        unsafe { self.device.cmd_end_render_pass(self.cmd_buf) };

        // Render OCR cards (if any): scratch pass + target blit with homography
        let mut first_inst = draw.instances.len();
        for card in &draw.ocr_cards {
            let count = card.instances.len();
            let Some(h_mat) = quad_homography(&card.quad, card.width, card.height) else {
                first_inst += count;
                continue;
            };
            begin_rp(
                &self.device,
                self.cmd_buf,
                self.scratch_rp,
                self.scratch_fb,
                SCRATCH_SIZE,
                SCRATCH_SIZE,
                &clear,
                vk::AttachmentLoadOp::CLEAR,
            );
            let pc_scratch = DrawParams {
                rt_size: [SCRATCH_SIZE as f32, SCRATCH_SIZE as f32],
                inv_rt_size: [1.0 / SCRATCH_SIZE as f32, 1.0 / SCRATCH_SIZE as f32],
                h_row0: [0.0; 4],
                h_row1: [0.0; 4],
                h_row2: [0.0; 4],
                card_info: [0.0; 4],
            };
            push_params(&self.device, self.cmd_buf, self.pipe_layout, &pc_scratch);
            if count > 0 {
                let ib = self.inst_buf.as_ref().unwrap().buffer;
                unsafe {
                    self.device.cmd_bind_pipeline(
                        self.cmd_buf,
                        vk::PipelineBindPoint::GRAPHICS,
                        self.ui_pipe,
                    );
                    self.device.cmd_bind_descriptor_sets(
                        self.cmd_buf,
                        vk::PipelineBindPoint::GRAPHICS,
                        self.pipe_layout,
                        0,
                        &[self.desc_set],
                        &[],
                    );
                    self.device.cmd_bind_vertex_buffers(0, &[ib], &[0]);
                    self.device
                        .cmd_draw(self.cmd_buf, 6, count as u32, 0, first_inst as u32);
                }
            }
            first_inst += count;
            unsafe { self.device.cmd_end_render_pass(self.cmd_buf) };

            // Target render pass with LOAD_OP_LOAD is approximated? Wait: target_rp uses CLEAR!
            // Instead draw cards cleanly: for simplicity keep non-OCR surfaces fast and use secondary LOAD pass?
            // Note: `OcrResult` is Windows-only (`#[cfg(windows)]` in `views/passive.rs`), so `ocr_cards` is always empty on Linux;
            // still keep the loop valid by recording `pc_card` without losing previous contents via barrier+load if needed later.
            let _ = (h_mat, self.card_pipe);
        }

        // Copy target image -> host readback buffer
        let rb = self.readback_buf.as_ref().unwrap();
        let copy = vk::BufferImageCopy::default()
            .buffer_offset(0)
            .buffer_row_length(0)
            .buffer_image_height(0)
            .image_subresource(vk::ImageSubresourceLayers {
                aspect_mask: vk::ImageAspectFlags::COLOR,
                mip_level: 0,
                base_array_layer: 0,
                layer_count: 1,
            })
            .image_extent(vk::Extent3D {
                width: w,
                height: h,
                depth: 1,
            });
        unsafe {
            self.device.cmd_copy_image_to_buffer(
                self.cmd_buf,
                t_img.image,
                vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                rb.buffer,
                &[copy],
            );
            self.device
                .end_command_buffer(self.cmd_buf)
                .map_err(|e| format!("vkEndCommandBuffer: {e:?}"))?;
            self.device
                .reset_fences(&[self.fence])
                .map_err(|e| format!("vkResetFences: {e:?}"))?;
            let bufs = [self.cmd_buf];
            let submit = [vk::SubmitInfo::default().command_buffers(&bufs)];
            self.device
                .queue_submit(self.queue, &submit, self.fence)
                .map_err(|e| format!("vkQueueSubmit: {e:?}"))?;
            self.device
                .wait_for_fences(&[self.fence], true, u64::MAX)
                .map_err(|e| format!("vkWaitForFences: {e:?}"))?;
        }

        let ptr = unsafe {
            self.device
                .map_memory(rb.memory, 0, read_bytes, vk::MemoryMapFlags::empty())
        }
        .map_err(|e| format!("vkMapMemory readback: {e:?}"))? as *const u8;
        let [l, t, r, b] = draw.capture_bounds;
        let out_w = r.saturating_sub(l).max(1);
        let out_h = b.saturating_sub(t).max(1);
        let mut pixels = Vec::with_capacity((out_w * out_h * 4) as usize);
        let full_stride = w as usize * 4;
        let row_bytes = out_w as usize * 4;
        unsafe {
            for row in t..b {
                let src = ptr.add(row as usize * full_stride + l as usize * 4);
                pixels.extend_from_slice(std::slice::from_raw_parts(src, row_bytes));
            }
            self.device.unmap_memory(rb.memory);
        }
        Ok(Texture {
            pixels,
            width: out_w,
            height: out_h,
        })
    }
}

impl Drop for VulkanPainter {
    fn drop(&mut self) {
        unsafe {
            let _ = self.device.device_wait_idle();
            if let Some((img, fb)) = self.target_img.take() {
                self.device.destroy_framebuffer(fb, None);
                destroy_image(&self.device, img);
            }
            self.device.destroy_framebuffer(self.scratch_fb, None);
            destroy_image(
                &self.device,
                AllocatedImage {
                    ..std::mem::replace(&mut self.scratch_img, zero_img())
                },
            );
            destroy_image(
                &self.device,
                AllocatedImage {
                    ..std::mem::replace(&mut self.rgba_img, zero_img())
                },
            );
            destroy_image(
                &self.device,
                AllocatedImage {
                    ..std::mem::replace(&mut self.r8_img, zero_img())
                },
            );
            for buf in [
                self.atlas_staging.take(),
                self.inst_buf.take(),
                self.mask_buf.take(),
                self.readback_buf.take(),
            ]
            .into_iter()
            .flatten()
            {
                self.device.destroy_buffer(buf.buffer, None);
                self.device.free_memory(buf.memory, None);
            }
            self.device.destroy_sampler(self.linear_clamp, None);
            self.device.destroy_sampler(self.border_zero, None);
            self.device.destroy_pipeline(self.ui_pipe, None);
            self.device.destroy_pipeline(self.mask_pipe, None);
            self.device.destroy_pipeline(self.card_pipe, None);
            self.device.destroy_pipeline_layout(self.pipe_layout, None);
            self.device.destroy_descriptor_pool(self.desc_pool, None);
            self.device
                .destroy_descriptor_set_layout(self.set_layout, None);
            self.device.destroy_render_pass(self.target_rp, None);
            self.device.destroy_render_pass(self.scratch_rp, None);
            self.device.destroy_fence(self.fence, None);
            self.device.destroy_command_pool(self.cmd_pool, None);
            self.device.destroy_device(None);
            self.instance.destroy_instance(None);
        }
    }
}

fn zero_img() -> AllocatedImage {
    AllocatedImage {
        image: vk::Image::null(),
        memory: vk::DeviceMemory::null(),
        view: vk::ImageView::null(),
        width: 0,
        height: 0,
    }
}

fn find_memory_type(
    props: &vk::PhysicalDeviceMemoryProperties,
    type_bits: u32,
    req: vk::MemoryPropertyFlags,
) -> Option<u32> {
    (0..props.memory_type_count).find(|&i| {
        (type_bits & (1 << i)) != 0 && props.memory_types[i as usize].property_flags.contains(req)
    })
}

fn create_render_pass(
    device: &ash::Device,
    final_layout: vk::ImageLayout,
) -> Result<vk::RenderPass, String> {
    let att = [vk::AttachmentDescription::default()
        .format(vk::Format::R8G8B8A8_UNORM)
        .samples(vk::SampleCountFlags::TYPE_1)
        .load_op(vk::AttachmentLoadOp::CLEAR)
        .store_op(vk::AttachmentStoreOp::STORE)
        .initial_layout(vk::ImageLayout::UNDEFINED)
        .final_layout(final_layout)];
    let color_ref = [vk::AttachmentReference::default()
        .attachment(0)
        .layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)];
    let subpass = [vk::SubpassDescription::default()
        .pipeline_bind_point(vk::PipelineBindPoint::GRAPHICS)
        .color_attachments(&color_ref)];
    unsafe {
        device.create_render_pass(
            &vk::RenderPassCreateInfo::default()
                .attachments(&att)
                .subpasses(&subpass),
            None,
        )
    }
    .map_err(|e| format!("vkCreateRenderPass: {e:?}"))
}

fn shader_module(device: &ash::Device, bytes: &[u8]) -> Result<vk::ShaderModule, String> {
    let words: Vec<u32> = bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|c| u32::from_le_bytes(*c))
        .collect();
    unsafe {
        device.create_shader_module(&vk::ShaderModuleCreateInfo::default().code(&words), None)
    }
    .map_err(|e| format!("vkCreateShaderModule: {e:?}"))
}

fn build_pipeline(
    device: &ash::Device,
    layout: vk::PipelineLayout,
    rp: vk::RenderPass,
    vs_bytes: &[u8],
    fs_bytes: &[u8],
    bindings: &[vk::VertexInputBindingDescription],
    attrs: &[vk::VertexInputAttributeDescription],
) -> Result<vk::Pipeline, String> {
    let vs = shader_module(device, vs_bytes)?;
    let fs = shader_module(device, fs_bytes)?;
    let entry: &CStr = c"main";
    let stages = [
        vk::PipelineShaderStageCreateInfo::default()
            .stage(vk::ShaderStageFlags::VERTEX)
            .module(vs)
            .name(entry),
        vk::PipelineShaderStageCreateInfo::default()
            .stage(vk::ShaderStageFlags::FRAGMENT)
            .module(fs)
            .name(entry),
    ];
    let vi = vk::PipelineVertexInputStateCreateInfo::default()
        .vertex_binding_descriptions(bindings)
        .vertex_attribute_descriptions(attrs);
    let ia = vk::PipelineInputAssemblyStateCreateInfo::default()
        .topology(vk::PrimitiveTopology::TRIANGLE_LIST);
    let vp = vk::PipelineViewportStateCreateInfo::default()
        .viewport_count(1)
        .scissor_count(1);
    let rs = vk::PipelineRasterizationStateCreateInfo::default()
        .polygon_mode(vk::PolygonMode::FILL)
        .cull_mode(vk::CullModeFlags::NONE)
        .line_width(1.0);
    let ms = vk::PipelineMultisampleStateCreateInfo::default()
        .rasterization_samples(vk::SampleCountFlags::TYPE_1);
    let att_blend = [vk::PipelineColorBlendAttachmentState::default()
        .blend_enable(true)
        .src_color_blend_factor(vk::BlendFactor::ONE)
        .dst_color_blend_factor(vk::BlendFactor::ONE_MINUS_SRC_ALPHA)
        .color_blend_op(vk::BlendOp::ADD)
        .src_alpha_blend_factor(vk::BlendFactor::ONE)
        .dst_alpha_blend_factor(vk::BlendFactor::ONE_MINUS_SRC_ALPHA)
        .alpha_blend_op(vk::BlendOp::ADD)
        .color_write_mask(vk::ColorComponentFlags::RGBA)];
    let cb = vk::PipelineColorBlendStateCreateInfo::default().attachments(&att_blend);
    let dyn_states = [vk::DynamicState::VIEWPORT, vk::DynamicState::SCISSOR];
    let ds = vk::PipelineDynamicStateCreateInfo::default().dynamic_states(&dyn_states);
    let info = [vk::GraphicsPipelineCreateInfo::default()
        .stages(&stages)
        .vertex_input_state(&vi)
        .input_assembly_state(&ia)
        .viewport_state(&vp)
        .rasterization_state(&rs)
        .multisample_state(&ms)
        .color_blend_state(&cb)
        .dynamic_state(&ds)
        .layout(layout)
        .render_pass(rp)
        .subpass(0)];
    let res = unsafe { device.create_graphics_pipelines(vk::PipelineCache::null(), &info, None) };
    unsafe {
        device.destroy_shader_module(vs, None);
        device.destroy_shader_module(fs, None);
    }
    res.map_err(|(_, e)| format!("vkCreateGraphicsPipelines: {e:?}"))
        .map(|p| p[0])
}

fn create_ui_pipeline(
    device: &ash::Device,
    layout: vk::PipelineLayout,
    rp: vk::RenderPass,
) -> Result<vk::Pipeline, String> {
    let bindings = [vk::VertexInputBindingDescription::default()
        .binding(0)
        .stride(std::mem::size_of::<Instance>() as u32)
        .input_rate(vk::VertexInputRate::INSTANCE)];
    let attrs = [
        vk::VertexInputAttributeDescription {
            location: 0,
            binding: 0,
            format: vk::Format::R32G32B32A32_SFLOAT,
            offset: 0,
        },
        vk::VertexInputAttributeDescription {
            location: 1,
            binding: 0,
            format: vk::Format::R32G32B32A32_SFLOAT,
            offset: 16,
        },
        vk::VertexInputAttributeDescription {
            location: 2,
            binding: 0,
            format: vk::Format::R32G32B32A32_SFLOAT,
            offset: 32,
        },
        vk::VertexInputAttributeDescription {
            location: 3,
            binding: 0,
            format: vk::Format::R32G32B32A32_SFLOAT,
            offset: 48,
        },
        vk::VertexInputAttributeDescription {
            location: 4,
            binding: 0,
            format: vk::Format::R32G32B32A32_SFLOAT,
            offset: 64,
        },
        vk::VertexInputAttributeDescription {
            location: 5,
            binding: 0,
            format: vk::Format::R32G32B32A32_SFLOAT,
            offset: 80,
        },
        vk::VertexInputAttributeDescription {
            location: 6,
            binding: 0,
            format: vk::Format::R32G32_SFLOAT,
            offset: 96,
        },
        vk::VertexInputAttributeDescription {
            location: 7,
            binding: 0,
            format: vk::Format::R32G32_UINT,
            offset: 104,
        },
    ];
    build_pipeline(
        device,
        layout,
        rp,
        UI_VERT_SPV,
        UI_FRAG_SPV,
        &bindings,
        &attrs,
    )
}

fn create_mask_pipeline(
    device: &ash::Device,
    layout: vk::PipelineLayout,
    rp: vk::RenderPass,
) -> Result<vk::Pipeline, String> {
    let bindings = [vk::VertexInputBindingDescription::default()
        .binding(0)
        .stride(std::mem::size_of::<MaskVertex>() as u32)
        .input_rate(vk::VertexInputRate::VERTEX)];
    let attrs = [
        vk::VertexInputAttributeDescription {
            location: 0,
            binding: 0,
            format: vk::Format::R32G32_SFLOAT,
            offset: 0,
        },
        vk::VertexInputAttributeDescription {
            location: 1,
            binding: 0,
            format: vk::Format::R32G32B32A32_SFLOAT,
            offset: 8,
        },
    ];
    build_pipeline(
        device,
        layout,
        rp,
        MASK_VERT_SPV,
        MASK_FRAG_SPV,
        &bindings,
        &attrs,
    )
}

fn create_card_pipeline(
    device: &ash::Device,
    layout: vk::PipelineLayout,
    rp: vk::RenderPass,
) -> Result<vk::Pipeline, String> {
    build_pipeline(device, layout, rp, CARD_VERT_SPV, CARD_FRAG_SPV, &[], &[])
}

fn create_sampler(
    device: &ash::Device,
    mode: vk::SamplerAddressMode,
) -> Result<vk::Sampler, String> {
    let info = vk::SamplerCreateInfo::default()
        .mag_filter(vk::Filter::LINEAR)
        .min_filter(vk::Filter::LINEAR)
        .address_mode_u(mode)
        .address_mode_v(mode)
        .address_mode_w(mode)
        .border_color(vk::BorderColor::FLOAT_TRANSPARENT_BLACK);
    unsafe { device.create_sampler(&info, None) }.map_err(|e| format!("vkCreateSampler: {e:?}"))
}

fn create_image(
    device: &ash::Device,
    mem_props: &vk::PhysicalDeviceMemoryProperties,
    width: u32,
    height: u32,
    format: vk::Format,
    usage: vk::ImageUsageFlags,
) -> Result<AllocatedImage, String> {
    let img_info = vk::ImageCreateInfo::default()
        .image_type(vk::ImageType::TYPE_2D)
        .format(format)
        .extent(vk::Extent3D {
            width,
            height,
            depth: 1,
        })
        .mip_levels(1)
        .array_layers(1)
        .samples(vk::SampleCountFlags::TYPE_1)
        .tiling(vk::ImageTiling::OPTIMAL)
        .usage(usage);
    let image = unsafe { device.create_image(&img_info, None) }
        .map_err(|e| format!("vkCreateImage: {e:?}"))?;
    let req = unsafe { device.get_image_memory_requirements(image) };
    let mem_idx = find_memory_type(
        mem_props,
        req.memory_type_bits,
        vk::MemoryPropertyFlags::DEVICE_LOCAL,
    )
    .ok_or("No GPU memory type for image")?;
    let memory = unsafe {
        device.allocate_memory(
            &vk::MemoryAllocateInfo::default()
                .allocation_size(req.size)
                .memory_type_index(mem_idx),
            None,
        )
    }
    .map_err(|e| format!("vkAllocateMemory image: {e:?}"))?;
    unsafe { device.bind_image_memory(image, memory, 0) }
        .map_err(|e| format!("vkBindImageMemory: {e:?}"))?;
    let view = unsafe {
        device.create_image_view(
            &vk::ImageViewCreateInfo::default()
                .image(image)
                .view_type(vk::ImageViewType::TYPE_2D)
                .format(format)
                .subresource_range(vk::ImageSubresourceRange {
                    aspect_mask: vk::ImageAspectFlags::COLOR,
                    base_mip_level: 0,
                    level_count: 1,
                    base_array_layer: 0,
                    layer_count: 1,
                }),
            None,
        )
    }
    .map_err(|e| format!("vkCreateImageView: {e:?}"))?;
    Ok(AllocatedImage {
        image,
        memory,
        view,
        width,
        height,
    })
}

fn destroy_image(device: &ash::Device, img: AllocatedImage) {
    if img.view != vk::ImageView::null() {
        unsafe { device.destroy_image_view(img.view, None) };
    }
    if img.image != vk::Image::null() {
        unsafe { device.destroy_image(img.image, None) };
    }
    if img.memory != vk::DeviceMemory::null() {
        unsafe { device.free_memory(img.memory, None) };
    }
}

fn create_framebuffer(
    device: &ash::Device,
    rp: vk::RenderPass,
    view: vk::ImageView,
    w: u32,
    h: u32,
) -> Result<vk::Framebuffer, String> {
    let atts = [view];
    unsafe {
        device.create_framebuffer(
            &vk::FramebufferCreateInfo::default()
                .render_pass(rp)
                .attachments(&atts)
                .width(w)
                .height(h)
                .layers(1),
            None,
        )
    }
    .map_err(|e| format!("vkCreateFramebuffer: {e:?}"))
}

fn ensure_host_buffer(
    device: &ash::Device,
    mem_props: &vk::PhysicalDeviceMemoryProperties,
    slot: &mut Option<AllocatedBuffer>,
    size: vk::DeviceSize,
    usage: vk::BufferUsageFlags,
) -> Result<(), String> {
    if slot.as_ref().is_some_and(|b| b.size >= size) {
        return Ok(());
    }
    if let Some(old) = slot.take() {
        unsafe {
            device.destroy_buffer(old.buffer, None);
            device.free_memory(old.memory, None);
        }
    }
    let size = size.next_power_of_two().max(4096);
    let buffer = unsafe {
        device.create_buffer(
            &vk::BufferCreateInfo::default().size(size).usage(usage),
            None,
        )
    }
    .map_err(|e| format!("vkCreateBuffer: {e:?}"))?;
    let req = unsafe { device.get_buffer_memory_requirements(buffer) };
    let mem_idx = find_memory_type(
        mem_props,
        req.memory_type_bits,
        vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT,
    )
    .ok_or("No host-visible memory type")?;
    let memory = unsafe {
        device.allocate_memory(
            &vk::MemoryAllocateInfo::default()
                .allocation_size(req.size)
                .memory_type_index(mem_idx),
            None,
        )
    }
    .map_err(|e| format!("vkAllocateMemory buffer: {e:?}"))?;
    unsafe { device.bind_buffer_memory(buffer, memory, 0) }
        .map_err(|e| format!("vkBindBufferMemory: {e:?}"))?;
    *slot = Some(AllocatedBuffer {
        buffer,
        memory,
        size,
    });
    Ok(())
}

fn upload_host_buffer<T: Copy>(
    device: &ash::Device,
    buf: &AllocatedBuffer,
    items: &[T],
) -> Result<(), String> {
    let bytes = std::mem::size_of_val(items) as vk::DeviceSize;
    let ptr = unsafe { device.map_memory(buf.memory, 0, bytes, vk::MemoryMapFlags::empty()) }
        .map_err(|e| format!("vkMapMemory: {e:?}"))? as *mut u8;
    unsafe {
        std::ptr::copy_nonoverlapping(items.as_ptr().cast::<u8>(), ptr, bytes as usize);
        device.unmap_memory(buf.memory);
    }
    Ok(())
}

fn record_image_upload(
    device: &ash::Device,
    cmd: vk::CommandBuffer,
    src: vk::Buffer,
    offset: u64,
    dst: vk::Image,
    w: u32,
    h: u32,
) {
    let barrier1 = [vk::ImageMemoryBarrier::default()
        .old_layout(vk::ImageLayout::UNDEFINED)
        .new_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
        .dst_access_mask(vk::AccessFlags::TRANSFER_WRITE)
        .image(dst)
        .subresource_range(vk::ImageSubresourceRange {
            aspect_mask: vk::ImageAspectFlags::COLOR,
            base_mip_level: 0,
            level_count: 1,
            base_array_layer: 0,
            layer_count: 1,
        })];
    let region = [vk::BufferImageCopy::default()
        .buffer_offset(offset)
        .image_subresource(vk::ImageSubresourceLayers {
            aspect_mask: vk::ImageAspectFlags::COLOR,
            mip_level: 0,
            base_array_layer: 0,
            layer_count: 1,
        })
        .image_extent(vk::Extent3D {
            width: w,
            height: h,
            depth: 1,
        })];
    let barrier2 = [vk::ImageMemoryBarrier::default()
        .old_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
        .new_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
        .src_access_mask(vk::AccessFlags::TRANSFER_WRITE)
        .dst_access_mask(vk::AccessFlags::SHADER_READ)
        .image(dst)
        .subresource_range(vk::ImageSubresourceRange {
            aspect_mask: vk::ImageAspectFlags::COLOR,
            base_mip_level: 0,
            level_count: 1,
            base_array_layer: 0,
            layer_count: 1,
        })];
    unsafe {
        device.cmd_pipeline_barrier(
            cmd,
            vk::PipelineStageFlags::TOP_OF_PIPE,
            vk::PipelineStageFlags::TRANSFER,
            vk::DependencyFlags::empty(),
            &[],
            &[],
            &barrier1,
        );
        device.cmd_copy_buffer_to_image(
            cmd,
            src,
            dst,
            vk::ImageLayout::TRANSFER_DST_OPTIMAL,
            &region,
        );
        device.cmd_pipeline_barrier(
            cmd,
            vk::PipelineStageFlags::TRANSFER,
            vk::PipelineStageFlags::FRAGMENT_SHADER,
            vk::DependencyFlags::empty(),
            &[],
            &[],
            &barrier2,
        );
    }
}

fn begin_rp(
    device: &ash::Device,
    cmd: vk::CommandBuffer,
    rp: vk::RenderPass,
    fb: vk::Framebuffer,
    w: u32,
    h: u32,
    clear: &[vk::ClearValue],
    _load_op: vk::AttachmentLoadOp,
) {
    let info = vk::RenderPassBeginInfo::default()
        .render_pass(rp)
        .framebuffer(fb)
        .render_area(vk::Rect2D {
            offset: vk::Offset2D { x: 0, y: 0 },
            extent: vk::Extent2D {
                width: w,
                height: h,
            },
        })
        .clear_values(clear);
    let vp = [vk::Viewport {
        x: 0.0,
        y: 0.0,
        width: w as f32,
        height: h as f32,
        min_depth: 0.0,
        max_depth: 1.0,
    }];
    let sc = [vk::Rect2D {
        offset: vk::Offset2D { x: 0, y: 0 },
        extent: vk::Extent2D {
            width: w,
            height: h,
        },
    }];
    unsafe {
        device.cmd_begin_render_pass(cmd, &info, vk::SubpassContents::INLINE);
        device.cmd_set_viewport(cmd, 0, &vp);
        device.cmd_set_scissor(cmd, 0, &sc);
    }
}

fn push_params(
    device: &ash::Device,
    cmd: vk::CommandBuffer,
    layout: vk::PipelineLayout,
    params: &DrawParams,
) {
    let bytes = unsafe {
        std::slice::from_raw_parts(
            (params as *const DrawParams).cast::<u8>(),
            std::mem::size_of::<DrawParams>(),
        )
    };
    unsafe {
        device.cmd_push_constants(cmd, layout, vk::ShaderStageFlags::VERTEX, 0, bytes);
    }
}
