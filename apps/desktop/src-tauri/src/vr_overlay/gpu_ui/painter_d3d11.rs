//! Direct3D 11 GPU painter for VR overlay surfaces (Windows).

use std::ffi::c_void;
use std::rc::Rc;

use windows::Win32::Foundation::{RECT, TRUE};
use windows::Win32::Graphics::Direct3D::Fxc::D3DCompile;
use windows::Win32::Graphics::Direct3D::{D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST, ID3DBlob};
use windows::Win32::Graphics::Direct3D11::{
    D3D11_BIND_CONSTANT_BUFFER, D3D11_BIND_RENDER_TARGET, D3D11_BIND_SHADER_RESOURCE,
    D3D11_BIND_VERTEX_BUFFER, D3D11_BLEND_DESC, D3D11_BLEND_INV_SRC_ALPHA, D3D11_BLEND_ONE,
    D3D11_BLEND_OP_ADD, D3D11_BOX, D3D11_BUFFER_DESC, D3D11_COLOR_WRITE_ENABLE_ALL,
    D3D11_COMPARISON_NEVER, D3D11_CPU_ACCESS_WRITE, D3D11_CULL_NONE, D3D11_FILL_SOLID,
    D3D11_FILTER_MIN_MAG_MIP_LINEAR, D3D11_INPUT_ELEMENT_DESC, D3D11_INPUT_PER_INSTANCE_DATA,
    D3D11_INPUT_PER_VERTEX_DATA, D3D11_MAP_WRITE_DISCARD, D3D11_MAPPED_SUBRESOURCE,
    D3D11_RASTERIZER_DESC, D3D11_RENDER_TARGET_BLEND_DESC, D3D11_RESOURCE_MISC_SHARED,
    D3D11_SAMPLER_DESC, D3D11_TEXTURE_ADDRESS_BORDER, D3D11_TEXTURE_ADDRESS_CLAMP,
    D3D11_TEXTURE2D_DESC, D3D11_USAGE_DEFAULT, D3D11_USAGE_DYNAMIC, D3D11_VIEWPORT,
    ID3D11BlendState, ID3D11Buffer, ID3D11Device, ID3D11DeviceContext, ID3D11InputLayout,
    ID3D11PixelShader, ID3D11RasterizerState, ID3D11RenderTargetView, ID3D11SamplerState,
    ID3D11ShaderResourceView, ID3D11Texture2D, ID3D11VertexShader,
};
#[cfg(any(test, feature = "vr-preview"))]
use windows::Win32::Graphics::Direct3D11::{
    D3D11_CPU_ACCESS_READ, D3D11_MAP_READ, D3D11_USAGE_STAGING,
};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_FORMAT_R8_UNORM, DXGI_FORMAT_R8G8B8A8_UNORM,
    DXGI_FORMAT_R32G32_FLOAT, DXGI_FORMAT_R32G32_UINT, DXGI_FORMAT_R32G32B32A32_FLOAT,
    DXGI_SAMPLE_DESC,
};
use windows::Win32::Graphics::Dxgi::IDXGIResource;
use windows::core::{Interface, PCSTR, s};

use super::draw::{DrawList, Instance, MaskVertex};
use super::ocr_scene::quad_homography;
use super::text::{ATLAS_R8_SIZE, ATLAS_RGBA_SIZE, GlyphAtlas};
use crate::vr_overlay::d3d11_texture::{SharedDevice, acquire_shared_device};

const SHADER_SOURCE: &str = include_str!("shaders/ui.hlsl");
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

pub struct SurfaceSlot {
    #[cfg_attr(not(any(test, feature = "vr-preview")), allow(dead_code))]
    pub texture: ID3D11Texture2D,
    pub rtv: ID3D11RenderTargetView,
    pub shared_handle: usize,
    pub width: u32,
    pub height: u32,
}

/// Double-buffered shared BGRA8 render target compatible with OpenVR DXGISharedHandle.
pub struct D3d11SurfaceTarget {
    slots: [Option<SurfaceSlot>; 2],
    next_slot: usize,
    width: u32,
    height: u32,
}

impl D3d11SurfaceTarget {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            slots: [None, None],
            next_slot: 0,
            width: width.max(1),
            height: height.max(1),
        }
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        let width = width.max(1);
        let height = height.max(1);
        if self.width != width || self.height != height {
            self.slots = [None, None];
            self.next_slot = 0;
            self.width = width;
            self.height = height;
        }
    }

    fn ensure_slot(&mut self, device: &ID3D11Device, slot: usize) -> Result<&SurfaceSlot, String> {
        let needs_alloc = self.slots[slot]
            .as_ref()
            .is_none_or(|s| s.width != self.width || s.height != self.height);
        if needs_alloc {
            let created = create_shared_slot(device, self.width, self.height)?;
            self.slots[slot] = Some(created);
        }
        Ok(self.slots[slot].as_ref().expect("slot initialized"))
    }

    /// Protect the just-submitted slot so the next frame renders into the other slot.
    pub fn submitted_handle(&mut self, shared_handle: usize) -> Result<(), String> {
        let slot = self
            .slots
            .iter()
            .position(|s| s.as_ref().is_some_and(|s| s.shared_handle == shared_handle))
            .ok_or_else(|| "Submitted texture does not belong to this target".to_string())?;
        self.next_slot = 1 - slot;
        Ok(())
    }

    #[cfg(any(test, feature = "vr-preview"))]
    pub fn slot_texture(&self, shared_handle: usize) -> Option<&ID3D11Texture2D> {
        self.slots
            .iter()
            .flatten()
            .find(|s| s.shared_handle == shared_handle)
            .map(|s| &s.texture)
    }
}

pub struct D3d11Painter {
    shared: Rc<SharedDevice>,
    ui_vs: ID3D11VertexShader,
    ui_ps: ID3D11PixelShader,
    ui_layout: ID3D11InputLayout,
    mask_vs: ID3D11VertexShader,
    mask_ps: ID3D11PixelShader,
    mask_layout: ID3D11InputLayout,
    card_vs: ID3D11VertexShader,
    card_ps: ID3D11PixelShader,
    blend_premul: ID3D11BlendState,
    rasterizer: ID3D11RasterizerState,
    linear_clamp: ID3D11SamplerState,
    border_zero: ID3D11SamplerState,
    cb_params: ID3D11Buffer,
    instance_vb: Option<(ID3D11Buffer, usize)>,
    mask_vb: Option<(ID3D11Buffer, usize)>,
    r8_tex: ID3D11Texture2D,
    r8_srv: ID3D11ShaderResourceView,
    r8_reset_gen: u64,
    r8_synced_dirty: usize,
    rgba_tex: ID3D11Texture2D,
    rgba_srv: ID3D11ShaderResourceView,
    rgba_reset_gen: u64,
    rgba_synced_dirty: usize,
    scratch_rtv: ID3D11RenderTargetView,
    scratch_srv: ID3D11ShaderResourceView,
}

impl D3d11Painter {
    pub fn new(adapter_index: u32) -> Result<Self, String> {
        let shared = acquire_shared_device(adapter_index)
            .map_err(|(op, err)| format!("{op}: 0x{:08x}", err.code().0))?;
        Self::from_shared(shared)
    }

    #[cfg(test)]
    pub fn new_for_test() -> Result<Self, String> {
        Self::new(0)
    }

    fn from_shared(shared: Rc<SharedDevice>) -> Result<Self, String> {
        let device = &shared.device;
        let ui_vs_bc = compile_shader(s!("UiVS"), s!("vs_5_0"))?;
        let ui_ps_bc = compile_shader(s!("UiPS"), s!("ps_5_0"))?;
        let mask_vs_bc = compile_shader(s!("MaskVS"), s!("vs_5_0"))?;
        let mask_ps_bc = compile_shader(s!("MaskPS"), s!("ps_5_0"))?;
        let card_vs_bc = compile_shader(s!("CardQuadVS"), s!("vs_5_0"))?;
        let card_ps_bc = compile_shader(s!("CardQuadPS"), s!("ps_5_0"))?;

        let ui_vs = create_vs(device, &ui_vs_bc)?;
        let ui_ps = create_ps(device, &ui_ps_bc)?;
        let mask_vs = create_vs(device, &mask_vs_bc)?;
        let mask_ps = create_ps(device, &mask_ps_bc)?;
        let card_vs = create_vs(device, &card_vs_bc)?;
        let card_ps = create_ps(device, &card_ps_bc)?;

        let ui_elems = [
            inst_elem(s!("RECT"), 0, DXGI_FORMAT_R32G32B32A32_FLOAT, 0),
            inst_elem(s!("UV"), 0, DXGI_FORMAT_R32G32B32A32_FLOAT, 16),
            inst_elem(s!("COLOR"), 0, DXGI_FORMAT_R32G32B32A32_FLOAT, 32),
            inst_elem(s!("BORDER_COLOR"), 0, DXGI_FORMAT_R32G32B32A32_FLOAT, 48),
            inst_elem(s!("RADII"), 0, DXGI_FORMAT_R32G32B32A32_FLOAT, 64),
            inst_elem(s!("CLIP_RECT"), 0, DXGI_FORMAT_R32G32B32A32_FLOAT, 80),
            inst_elem(s!("CLIP_BW"), 0, DXGI_FORMAT_R32G32_FLOAT, 96),
            inst_elem(s!("KIND_FLAGS"), 0, DXGI_FORMAT_R32G32_UINT, 104),
        ];
        let ui_layout = create_input_layout(device, &ui_elems, &ui_vs_bc)?;

        let mask_elems = [
            D3D11_INPUT_ELEMENT_DESC {
                SemanticName: s!("POSITION"),
                SemanticIndex: 0,
                Format: DXGI_FORMAT_R32G32_FLOAT,
                InputSlot: 0,
                AlignedByteOffset: 0,
                InputSlotClass: D3D11_INPUT_PER_VERTEX_DATA,
                InstanceDataStepRate: 0,
            },
            D3D11_INPUT_ELEMENT_DESC {
                SemanticName: s!("COLOR"),
                SemanticIndex: 0,
                Format: DXGI_FORMAT_R32G32B32A32_FLOAT,
                InputSlot: 0,
                AlignedByteOffset: 8,
                InputSlotClass: D3D11_INPUT_PER_VERTEX_DATA,
                InstanceDataStepRate: 0,
            },
        ];
        let mask_layout = create_input_layout(device, &mask_elems, &mask_vs_bc)?;

        let blend_desc = D3D11_BLEND_DESC {
            AlphaToCoverageEnable: false.into(),
            IndependentBlendEnable: false.into(),
            RenderTarget: [
                D3D11_RENDER_TARGET_BLEND_DESC {
                    BlendEnable: TRUE,
                    SrcBlend: D3D11_BLEND_ONE,
                    DestBlend: D3D11_BLEND_INV_SRC_ALPHA,
                    BlendOp: D3D11_BLEND_OP_ADD,
                    SrcBlendAlpha: D3D11_BLEND_ONE,
                    DestBlendAlpha: D3D11_BLEND_INV_SRC_ALPHA,
                    BlendOpAlpha: D3D11_BLEND_OP_ADD,
                    RenderTargetWriteMask: D3D11_COLOR_WRITE_ENABLE_ALL.0 as u8,
                },
                Default::default(),
                Default::default(),
                Default::default(),
                Default::default(),
                Default::default(),
                Default::default(),
                Default::default(),
            ],
        };
        let mut blend_premul = None;
        unsafe { device.CreateBlendState(&blend_desc, Some(&mut blend_premul)) }
            .map_err(|e| format!("CreateBlendState: 0x{:08x}", e.code().0))?;
        let blend_premul = blend_premul.ok_or("CreateBlendState returned None")?;

        let rast_desc = D3D11_RASTERIZER_DESC {
            FillMode: D3D11_FILL_SOLID,
            CullMode: D3D11_CULL_NONE,
            FrontCounterClockwise: false.into(),
            DepthBias: 0,
            DepthBiasClamp: 0.0,
            SlopeScaledDepthBias: 0.0,
            DepthClipEnable: false.into(),
            ScissorEnable: false.into(),
            MultisampleEnable: false.into(),
            AntialiasedLineEnable: false.into(),
        };
        let mut rasterizer = None;
        unsafe { device.CreateRasterizerState(&rast_desc, Some(&mut rasterizer)) }
            .map_err(|e| format!("CreateRasterizerState: 0x{:08x}", e.code().0))?;
        let rasterizer = rasterizer.ok_or("CreateRasterizerState returned None")?;

        let linear_clamp = create_sampler(device, D3D11_TEXTURE_ADDRESS_CLAMP)?;
        let border_zero = create_sampler(device, D3D11_TEXTURE_ADDRESS_BORDER)?;

        let mut cb_params = None;
        let cb_desc = D3D11_BUFFER_DESC {
            ByteWidth: std::mem::size_of::<DrawParams>() as u32,
            Usage: D3D11_USAGE_DYNAMIC,
            BindFlags: D3D11_BIND_CONSTANT_BUFFER.0 as u32,
            CPUAccessFlags: D3D11_CPU_ACCESS_WRITE.0 as u32,
            MiscFlags: 0,
            StructureByteStride: 0,
        };
        unsafe { device.CreateBuffer(&cb_desc, None, Some(&mut cb_params)) }
            .map_err(|e| format!("CreateBuffer CB: 0x{:08x}", e.code().0))?;
        let cb_params = cb_params.ok_or("CreateBuffer CB returned None")?;

        let (r8_tex, r8_srv) =
            create_atlas_tex(device, ATLAS_R8_SIZE, ATLAS_R8_SIZE, DXGI_FORMAT_R8_UNORM)?;
        let (rgba_tex, rgba_srv) = create_atlas_tex(
            device,
            ATLAS_RGBA_SIZE,
            ATLAS_RGBA_SIZE,
            DXGI_FORMAT_R8G8B8A8_UNORM,
        )?;

        let scratch_desc = D3D11_TEXTURE2D_DESC {
            Width: SCRATCH_SIZE,
            Height: SCRATCH_SIZE,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_B8G8R8A8_UNORM,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: (D3D11_BIND_RENDER_TARGET.0 | D3D11_BIND_SHADER_RESOURCE.0) as u32,
            ..Default::default()
        };
        let mut scratch_tex = None;
        unsafe { device.CreateTexture2D(&scratch_desc, None, Some(&mut scratch_tex)) }
            .map_err(|e| format!("CreateTexture2D scratch: 0x{:08x}", e.code().0))?;
        let scratch_tex = scratch_tex.ok_or("CreateTexture2D scratch returned None")?;
        let mut scratch_rtv = None;
        unsafe { device.CreateRenderTargetView(&scratch_tex, None, Some(&mut scratch_rtv)) }
            .map_err(|e| format!("CreateRenderTargetView scratch: 0x{:08x}", e.code().0))?;
        let scratch_rtv = scratch_rtv.ok_or("scratch RTV None")?;
        let mut scratch_srv = None;
        unsafe { device.CreateShaderResourceView(&scratch_tex, None, Some(&mut scratch_srv)) }
            .map_err(|e| format!("CreateShaderResourceView scratch: 0x{:08x}", e.code().0))?;
        let scratch_srv = scratch_srv.ok_or("scratch SRV None")?;

        Ok(Self {
            shared,
            ui_vs,
            ui_ps,
            ui_layout,
            mask_vs,
            mask_ps,
            mask_layout,
            card_vs,
            card_ps,
            blend_premul,
            rasterizer,
            linear_clamp,
            border_zero,
            cb_params,
            instance_vb: None,
            mask_vb: None,
            r8_tex,
            r8_srv,
            r8_reset_gen: 0,
            r8_synced_dirty: 0,
            rgba_tex,
            rgba_srv,
            rgba_reset_gen: 0,
            rgba_synced_dirty: 0,
            scratch_rtv,
            scratch_srv,
        })
    }

    pub fn sync_atlases(&mut self, atlas: &GlyphAtlas) {
        let ctx = &self.shared.context;
        if self.r8_reset_gen != atlas.r8_reset_gen {
            unsafe {
                ctx.UpdateSubresource(
                    &self.r8_tex,
                    0,
                    None,
                    atlas.r8_data.as_ptr().cast(),
                    ATLAS_R8_SIZE,
                    0,
                );
            }
            self.r8_reset_gen = atlas.r8_reset_gen;
            self.r8_synced_dirty = atlas.r8_dirty.len();
        } else if self.r8_synced_dirty < atlas.r8_dirty.len() {
            for &[l, t, r, b] in &atlas.r8_dirty[self.r8_synced_dirty..] {
                if l < r && t < b {
                    let region = D3D11_BOX {
                        left: l,
                        top: t,
                        front: 0,
                        right: r,
                        bottom: b,
                        back: 1,
                    };
                    let offset = t as usize * ATLAS_R8_SIZE as usize + l as usize;
                    unsafe {
                        ctx.UpdateSubresource(
                            &self.r8_tex,
                            0,
                            Some(&region),
                            atlas.r8_data.as_ptr().add(offset).cast(),
                            ATLAS_R8_SIZE,
                            0,
                        );
                    }
                }
            }
            self.r8_synced_dirty = atlas.r8_dirty.len();
        }
        if self.rgba_reset_gen != atlas.rgba_reset_gen {
            unsafe {
                ctx.UpdateSubresource(
                    &self.rgba_tex,
                    0,
                    None,
                    atlas.rgba_data.as_ptr().cast(),
                    ATLAS_RGBA_SIZE * 4,
                    0,
                );
            }
            self.rgba_reset_gen = atlas.rgba_reset_gen;
            self.rgba_synced_dirty = atlas.rgba_dirty.len();
        } else if self.rgba_synced_dirty < atlas.rgba_dirty.len() {
            for &[l, t, r, b] in &atlas.rgba_dirty[self.rgba_synced_dirty..] {
                if l < r && t < b {
                    let region = D3D11_BOX {
                        left: l,
                        top: t,
                        front: 0,
                        right: r,
                        bottom: b,
                        back: 1,
                    };
                    let offset = (t as usize * ATLAS_RGBA_SIZE as usize + l as usize) * 4;
                    unsafe {
                        ctx.UpdateSubresource(
                            &self.rgba_tex,
                            0,
                            Some(&region),
                            atlas.rgba_data.as_ptr().add(offset).cast(),
                            ATLAS_RGBA_SIZE * 4,
                            0,
                        );
                    }
                }
            }
            self.rgba_synced_dirty = atlas.rgba_dirty.len();
        }
    }

    pub fn render(
        &mut self,
        target: &mut D3d11SurfaceTarget,
        atlas: &GlyphAtlas,
        draw: &DrawList,
    ) -> Result<(usize, [u32; 4]), String> {
        self.sync_atlases(atlas);
        target.resize(draw.width, draw.height);
        let slot_idx = target.next_slot;
        let device = self.shared.device.clone();
        let ctx = self.shared.context.clone();
        let slot = target.ensure_slot(&device, slot_idx)?;
        let target_rtv = slot.rtv.clone();
        let shared_handle = slot.shared_handle;
        let (w, h) = (draw.width.max(1) as f32, draw.height.max(1) as f32);

        let clear = [0.0f32, 0.0, 0.0, 0.0];
        unsafe {
            ctx.ClearRenderTargetView(&target_rtv, &clear);
            ctx.OMSetBlendState(&self.blend_premul, None, 0xffff_ffff);
            ctx.RSSetState(&self.rasterizer);
            ctx.IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
            let samplers = [Some(self.linear_clamp.clone()), Some(self.border_zero.clone())];
            ctx.PSSetSamplers(0, Some(&samplers));
            let cbs = [Some(self.cb_params.clone())];
            ctx.VSSetConstantBuffers(0, Some(&cbs));
            ctx.PSSetConstantBuffers(0, Some(&cbs));
        }

        // Bind target RT & viewport
        self.bind_target(&ctx, &target_rtv, w, h, None)?;

        // 1. Mask quads (OCR background masks)
        if !draw.mask_vertices.is_empty() {
            Self::ensure_vb(
                &device,
                &mut self.mask_vb,
                draw.mask_vertices.len(),
                std::mem::size_of::<MaskVertex>(),
            )?;
            if let Some((vb, _)) = &self.mask_vb {
                upload_dynamic(&ctx, vb, &draw.mask_vertices)?;
                unsafe {
                    ctx.IASetInputLayout(&self.mask_layout);
                    let stride = std::mem::size_of::<MaskVertex>() as u32;
                    let offset = 0u32;
                    ctx.IASetVertexBuffers(0, 1, Some(&Some(vb.clone())), Some(&stride), Some(&offset));
                    ctx.VSSetShader(&self.mask_vs, None);
                    ctx.PSSetShader(&self.mask_ps, None);
                    ctx.Draw(draw.mask_vertices.len() as u32, 0);
                }
            }
        }

        // 2. Main UI instances
        self.draw_instances(&device, &ctx, &draw.instances)?;

        // 3. Perspective-warped OCR cards via scratch RT
        for card in &draw.ocr_cards {
            let Some(h_mat) = quad_homography(&card.quad, card.width, card.height) else {
                continue;
            };
            let cw = card.width.ceil().clamp(1.0, SCRATCH_SIZE as f32);
            let ch = card.height.ceil().clamp(1.0, SCRATCH_SIZE as f32);

            // Clear & render local card instances into scratch RT
            let null_srvs = [None, None, None];
            unsafe {
                ctx.PSSetShaderResources(0, Some(&null_srvs));
                ctx.ClearRenderTargetView(&self.scratch_rtv, &clear);
            }
            self.bind_target(
                &ctx,
                &self.scratch_rtv,
                SCRATCH_SIZE as f32,
                SCRATCH_SIZE as f32,
                Some((cw, ch)),
            )?;
            self.draw_instances(&device, &ctx, &card.instances)?;

            // Unbind scratch RT, bind target RT, and draw warped card quad
            self.bind_target(&ctx, &target_rtv, w, h, None)?;
            self.upload_params(
                &ctx,
                &DrawParams {
                    rt_size: [w, h],
                    inv_rt_size: [1.0 / w, 1.0 / h],
                    h_row0: [h_mat[0], h_mat[1], h_mat[2], 0.0],
                    h_row1: [h_mat[3], h_mat[4], h_mat[5], 0.0],
                    h_row2: [h_mat[6], h_mat[7], h_mat[8], 0.0],
                    card_info: [
                        card.width,
                        card.height,
                        card.width / SCRATCH_SIZE as f32,
                        card.height / SCRATCH_SIZE as f32,
                    ],
                },
            )?;
            unsafe {
                let srvs = [
                    Some(self.r8_srv.clone()),
                    Some(self.rgba_srv.clone()),
                    Some(self.scratch_srv.clone()),
                ];
                ctx.PSSetShaderResources(0, Some(&srvs));
                ctx.IASetInputLayout(None);
                ctx.VSSetShader(&self.card_vs, None);
                ctx.PSSetShader(&self.card_ps, None);
                ctx.Draw(6, 0);
            }
        }

        unsafe {
            let null_srvs = [None, None, None];
            ctx.PSSetShaderResources(0, Some(&null_srvs));
            ctx.OMSetRenderTargets(None, None);
            ctx.Flush();
        }
        Ok((shared_handle, draw.capture_bounds))
    }

    fn bind_target(
        &self,
        ctx: &ID3D11DeviceContext,
        rtv: &ID3D11RenderTargetView,
        w: f32,
        h: f32,
        scissor_wh: Option<(f32, f32)>,
    ) -> Result<(), String> {
        self.upload_params(
            ctx,
            &DrawParams {
                rt_size: [w, h],
                inv_rt_size: [1.0 / w, 1.0 / h],
                h_row0: [0.0; 4],
                h_row1: [0.0; 4],
                h_row2: [0.0; 4],
                card_info: [0.0; 4],
            },
        )?;
        let (vw, vh) = scissor_wh.unwrap_or((w, h));
        let vp = D3D11_VIEWPORT {
            TopLeftX: 0.0,
            TopLeftY: 0.0,
            Width: w,
            Height: h,
            MinDepth: 0.0,
            MaxDepth: 1.0,
        };
        let rect = RECT {
            left: 0,
            top: 0,
            right: vw.ceil() as i32,
            bottom: vh.ceil() as i32,
        };
        unsafe {
            ctx.OMSetRenderTargets(Some(&[Some(rtv.clone())]), None);
            ctx.RSSetViewports(Some(&[vp]));
            ctx.RSSetScissorRects(Some(&[rect]));
        }
        Ok(())
    }

    fn draw_instances(
        &mut self,
        device: &ID3D11Device,
        ctx: &ID3D11DeviceContext,
        instances: &[Instance],
    ) -> Result<(), String> {
        if instances.is_empty() {
            return Ok(());
        }
        Self::ensure_vb(
            device,
            &mut self.instance_vb,
            instances.len(),
            std::mem::size_of::<Instance>(),
        )?;
        let Some((vb, _)) = &self.instance_vb else {
            return Ok(());
        };
        upload_dynamic(ctx, vb, instances)?;
        unsafe {
            let srvs = [Some(self.r8_srv.clone()), Some(self.rgba_srv.clone())];
            ctx.PSSetShaderResources(0, Some(&srvs));
            ctx.IASetInputLayout(&self.ui_layout);
            let stride = std::mem::size_of::<Instance>() as u32;
            let offset = 0u32;
            ctx.IASetVertexBuffers(0, 1, Some(&Some(vb.clone())), Some(&stride), Some(&offset));
            ctx.VSSetShader(&self.ui_vs, None);
            ctx.PSSetShader(&self.ui_ps, None);
            ctx.DrawInstanced(6, instances.len() as u32, 0, 0);
        }
        Ok(())
    }

    fn upload_params(&self, ctx: &ID3D11DeviceContext, params: &DrawParams) -> Result<(), String> {
        upload_dynamic(ctx, &self.cb_params, std::slice::from_ref(params))
    }

    fn ensure_vb(
        device: &ID3D11Device,
        slot: &mut Option<(ID3D11Buffer, usize)>,
        count: usize,
        elem_size: usize,
    ) -> Result<(), String> {
        if slot.as_ref().is_some_and(|(_, cap)| *cap >= count) {
            return Ok(());
        }
        let cap = count.next_power_of_two().max(64);
        let desc = D3D11_BUFFER_DESC {
            ByteWidth: (cap * elem_size) as u32,
            Usage: D3D11_USAGE_DYNAMIC,
            BindFlags: D3D11_BIND_VERTEX_BUFFER.0 as u32,
            CPUAccessFlags: D3D11_CPU_ACCESS_WRITE.0 as u32,
            MiscFlags: 0,
            StructureByteStride: 0,
        };
        let mut buf = None;
        unsafe { device.CreateBuffer(&desc, None, Some(&mut buf)) }
            .map_err(|e| format!("CreateBuffer VB: 0x{:08x}", e.code().0))?;
        let buf = buf.ok_or("CreateBuffer VB returned None")?;
        *slot = Some((buf, cap));
        Ok(())
    }

    #[cfg(any(test, feature = "vr-preview"))]
    pub fn readback_rgba(
        &self,
        target: &D3d11SurfaceTarget,
        shared_handle: usize,
        bounds: [u32; 4],
    ) -> Result<Vec<u8>, String> {
        let src_tex = target
            .slot_texture(shared_handle)
            .ok_or("Unknown target handle")?;
        let [l, t, r, b] = bounds;
        if r <= l || b <= t || r > target.width || b > target.height {
            return Err("Invalid readback bounds".into());
        }
        let width = r - l;
        let height = b - t;
        let desc = D3D11_TEXTURE2D_DESC {
            Width: width,
            Height: height,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_B8G8R8A8_UNORM,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Usage: D3D11_USAGE_STAGING,
            CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
            ..Default::default()
        };
        let mut staging = None;
        unsafe { self.shared.device.CreateTexture2D(&desc, None, Some(&mut staging)) }
            .map_err(|e| format!("CreateTexture2D staging: 0x{:08x}", e.code().0))?;
        let staging = staging.ok_or("staging None")?;
        let region = windows::Win32::Graphics::Direct3D11::D3D11_BOX {
            left: l,
            top: t,
            front: 0,
            right: r,
            bottom: b,
            back: 1,
        };
        unsafe {
            self.shared
                .context
                .CopySubresourceRegion(&staging, 0, 0, 0, 0, src_tex, 0, Some(&region));
            self.shared.context.Flush();
        }
        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        unsafe {
            self.shared
                .context
                .Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))
        }
        .map_err(|e| format!("Map staging: 0x{:08x}", e.code().0))?;
        let mut rgba = Vec::with_capacity((width * height * 4) as usize);
        for y in 0..height as usize {
            let row = unsafe {
                std::slice::from_raw_parts(
                    mapped.pData.cast::<u8>().add(y * mapped.RowPitch as usize),
                    width as usize * 4,
                )
            };
            for px in row.chunks_exact(4) {
                rgba.extend_from_slice(&[px[2], px[1], px[0], px[3]]);
            }
        }
        unsafe { self.shared.context.Unmap(&staging, 0) };
        Ok(rgba)
    }
}

fn compile_shader(entry: PCSTR, target: PCSTR) -> Result<ID3DBlob, String> {
    let mut code = None;
    let mut errors: Option<ID3DBlob> = None;
    let res = unsafe {
        D3DCompile(
            SHADER_SOURCE.as_ptr().cast::<c_void>(),
            SHADER_SOURCE.len(),
            None,
            None,
            None,
            entry,
            target,
            0,
            0,
            &mut code,
            Some(&mut errors),
        )
    };
    if let Err(err) = res {
        let msg = errors
            .map(|b| {
                let bytes = unsafe {
                    std::slice::from_raw_parts(b.GetBufferPointer().cast::<u8>(), b.GetBufferSize())
                };
                String::from_utf8_lossy(bytes).into_owned()
            })
            .unwrap_or_default();
        return Err(format!("D3DCompile failed (0x{:08x}): {msg}", err.code().0));
    }
    code.ok_or_else(|| "D3DCompile returned no bytecode".to_string())
}

fn blob_bytes(blob: &ID3DBlob) -> &[u8] {
    unsafe { std::slice::from_raw_parts(blob.GetBufferPointer().cast::<u8>(), blob.GetBufferSize()) }
}

fn create_vs(device: &ID3D11Device, blob: &ID3DBlob) -> Result<ID3D11VertexShader, String> {
    let mut vs = None;
    unsafe { device.CreateVertexShader(blob_bytes(blob), None, Some(&mut vs)) }
        .map_err(|e| format!("CreateVertexShader: 0x{:08x}", e.code().0))?;
    vs.ok_or_else(|| "CreateVertexShader returned None".to_string())
}

fn create_ps(device: &ID3D11Device, blob: &ID3DBlob) -> Result<ID3D11PixelShader, String> {
    let mut ps = None;
    unsafe { device.CreatePixelShader(blob_bytes(blob), None, Some(&mut ps)) }
        .map_err(|e| format!("CreatePixelShader: 0x{:08x}", e.code().0))?;
    ps.ok_or_else(|| "CreatePixelShader returned None".to_string())
}

fn create_input_layout(
    device: &ID3D11Device,
    elems: &[D3D11_INPUT_ELEMENT_DESC],
    vs_blob: &ID3DBlob,
) -> Result<ID3D11InputLayout, String> {
    let mut layout = None;
    unsafe { device.CreateInputLayout(elems, blob_bytes(vs_blob), Some(&mut layout)) }
        .map_err(|e| format!("CreateInputLayout: 0x{:08x}", e.code().0))?;
    layout.ok_or_else(|| "CreateInputLayout returned None".to_string())
}

fn inst_elem(
    name: PCSTR,
    slot: u32,
    format: windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT,
    offset: u32,
) -> D3D11_INPUT_ELEMENT_DESC {
    D3D11_INPUT_ELEMENT_DESC {
        SemanticName: name,
        SemanticIndex: 0,
        Format: format,
        InputSlot: slot,
        AlignedByteOffset: offset,
        InputSlotClass: D3D11_INPUT_PER_INSTANCE_DATA,
        InstanceDataStepRate: 1,
    }
}

fn create_sampler(
    device: &ID3D11Device,
    addr: windows::Win32::Graphics::Direct3D11::D3D11_TEXTURE_ADDRESS_MODE,
) -> Result<ID3D11SamplerState, String> {
    let desc = D3D11_SAMPLER_DESC {
        Filter: D3D11_FILTER_MIN_MAG_MIP_LINEAR,
        AddressU: addr,
        AddressV: addr,
        AddressW: addr,
        MipLODBias: 0.0,
        MaxAnisotropy: 1,
        ComparisonFunc: D3D11_COMPARISON_NEVER,
        BorderColor: [0.0, 0.0, 0.0, 0.0],
        MinLOD: 0.0,
        MaxLOD: f32::MAX,
    };
    let mut samp = None;
    unsafe { device.CreateSamplerState(&desc, Some(&mut samp)) }
        .map_err(|e| format!("CreateSamplerState: 0x{:08x}", e.code().0))?;
    samp.ok_or_else(|| "CreateSamplerState returned None".to_string())
}

fn create_atlas_tex(
    device: &ID3D11Device,
    width: u32,
    height: u32,
    format: windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT,
) -> Result<(ID3D11Texture2D, ID3D11ShaderResourceView), String> {
    let desc = D3D11_TEXTURE2D_DESC {
        Width: width,
        Height: height,
        MipLevels: 1,
        ArraySize: 1,
        Format: format,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        Usage: D3D11_USAGE_DEFAULT,
        BindFlags: D3D11_BIND_SHADER_RESOURCE.0 as u32,
        ..Default::default()
    };
    let mut tex = None;
    unsafe { device.CreateTexture2D(&desc, None, Some(&mut tex)) }
        .map_err(|e| format!("CreateTexture2D atlas: 0x{:08x}", e.code().0))?;
    let tex = tex.ok_or("atlas tex None")?;
    let mut srv = None;
    unsafe { device.CreateShaderResourceView(&tex, None, Some(&mut srv)) }
        .map_err(|e| format!("CreateShaderResourceView atlas: 0x{:08x}", e.code().0))?;
    let srv = srv.ok_or("atlas srv None")?;
    Ok((tex, srv))
}

fn create_shared_slot(
    device: &ID3D11Device,
    width: u32,
    height: u32,
) -> Result<SurfaceSlot, String> {
    let desc = D3D11_TEXTURE2D_DESC {
        Width: width,
        Height: height,
        MipLevels: 1,
        ArraySize: 1,
        Format: DXGI_FORMAT_B8G8R8A8_UNORM,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        Usage: D3D11_USAGE_DEFAULT,
        BindFlags: (D3D11_BIND_SHADER_RESOURCE.0 | D3D11_BIND_RENDER_TARGET.0) as u32,
        MiscFlags: D3D11_RESOURCE_MISC_SHARED.0 as u32,
        ..Default::default()
    };
    let mut texture = None;
    unsafe { device.CreateTexture2D(&desc, None, Some(&mut texture)) }
        .map_err(|e| format!("CreateTexture2D shared slot: 0x{:08x}", e.code().0))?;
    let texture = texture.ok_or("shared slot texture None")?;
    let mut rtv = None;
    unsafe { device.CreateRenderTargetView(&texture, None, Some(&mut rtv)) }
        .map_err(|e| format!("CreateRenderTargetView shared slot: 0x{:08x}", e.code().0))?;
    let rtv = rtv.ok_or("shared slot rtv None")?;
    let resource: IDXGIResource = texture
        .cast()
        .map_err(|e| format!("IDXGIResource cast: 0x{:08x}", e.code().0))?;
    let handle = unsafe { resource.GetSharedHandle() }
        .map_err(|e| format!("GetSharedHandle: 0x{:08x}", e.code().0))?;
    if handle.is_invalid() {
        return Err("Invalid D3D11 shared handle".into());
    }
    Ok(SurfaceSlot {
        texture,
        rtv,
        shared_handle: handle.0 as usize,
        width,
        height,
    })
}

fn upload_dynamic<T: Copy>(
    ctx: &ID3D11DeviceContext,
    buf: &ID3D11Buffer,
    items: &[T],
) -> Result<(), String> {
    let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
    unsafe { ctx.Map(buf, 0, D3D11_MAP_WRITE_DISCARD, 0, Some(&mut mapped)) }
        .map_err(|e| format!("Map dynamic buffer: 0x{:08x}", e.code().0))?;
    let bytes = std::mem::size_of_val(items);
    unsafe {
        std::ptr::copy_nonoverlapping(items.as_ptr().cast::<u8>(), mapped.pData.cast::<u8>(), bytes);
        ctx.Unmap(buf, 0);
    }
    Ok(())
}
