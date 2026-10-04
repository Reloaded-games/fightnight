//! The WebGPU renderer: cascaded shadows, HDR MSAA scene pass, bloom and composite.

pub mod pipelines;
pub mod types;
pub mod world_gpu;

use crate::gpu::Gpu;
use bytemuck::cast_slice;
use fn_core::camera::Frustum;
use fn_core::mesh::{Instance, MeshData, Particle};
use fn_core::shadow;
use fn_core::world::terrain_mesh::ChunkInfo;
use fn_core::world::{World, GRID_N, WORLD_HALF};
use glam::{Mat4, Vec3};
use pipelines::*;
use std::cell::RefCell;
use std::rc::Rc;
use types::*;
use wgpu::util::DeviceExt;
use wgpu::*;

struct DynBuf {
    buf: Buffer,
    cap: u64,
    usage: BufferUsages,
    label: &'static str,
}

impl DynBuf {
    fn new(device: &Device, label: &'static str, usage: BufferUsages, cap: u64) -> Self {
        let cap = cap.max(256);
        Self { buf: device.create_buffer(&BufferDescriptor { label: Some(label), size: cap, usage: usage | BufferUsages::COPY_DST, mapped_at_creation: false }), cap, usage, label }
    }
    fn upload(&mut self, device: &Device, queue: &Queue, data: &[u8]) {
        let len = data.len() as u64;
        if len > self.cap {
            let cap = len.next_power_of_two();
            self.buf = device.create_buffer(&BufferDescriptor { label: Some(self.label), size: cap, usage: self.usage | BufferUsages::COPY_DST, mapped_at_creation: false });
            self.cap = cap;
        }
        if len > 0 {
            queue.write_buffer(&self.buf, 0, data);
        }
    }
}

struct TerrainGpu {
    vb: Buffer,
    ib: Buffer,
    chunks: Vec<ChunkInfo>,
}

struct BloomLevel {
    view: TextureView,
    w: u32,
    h: u32,
}

struct Targets {
    w: u32,
    h: u32,
    color_msaa: Option<TextureView>,
    hdr_view: TextureView,
    depth_view: TextureView,
    bloom: Vec<BloomLevel>,
}

struct PostPass {
    ubuf: Buffer,
    bg: BindGroup,
}

struct PostSet {
    passes: Vec<PostPass>,
    composite: PostPass,
    capture_composite: PostPass,
}

struct Capture {
    tex: Texture,
    view: TextureView,
    buf: Buffer,
    w: u32,
    h: u32,
    bytes_per_row: u32,
    result: Rc<RefCell<Option<(u32, u32, Vec<u8>)>>>,
}

pub struct Renderer {
    pub gpu: Gpu,
    pub quality: Quality,
    layouts: Layouts,
    pipes: Pipelines,

    globals_buf: Buffer,
    scene_bg: BindGroup,
    shadow_tex_array_view: TextureView,
    shadow_layer_views: Vec<TextureView>,
    shadow_ubufs: Vec<Buffer>,
    shadow_bgs: Vec<BindGroup>,
    shadow_samp: Sampler,
    lin_samp: Sampler,
    height_view: TextureView,
    splat_view: TextureView,

    targets: Targets,
    post: PostSet,

    mesh_vb: Option<Buffer>,
    mesh_ib: Option<Buffer>,
    mesh_table: Vec<MeshRange>,
    identity_inst: Buffer,

    terrain: Option<TerrainGpu>,
    wg: world_gpu::WorldGpu,
    scratch_instances: Vec<Instance>,
    water_buf: Buffer,
    water_count: u32,

    dyn_inst: DynBuf,
    ghost_inst: DynBuf,
    part_buf: DynBuf,
    part_add_count: u32,
    part_count: u32,

    capture_requested: bool,
    capture: Option<Capture>,
    pub stats: RenderStats,
    pub frame_index: u64,
}

fn create_targets(device: &Device, w: u32, h: u32, msaa: u32, bloom_on: bool) -> Targets {
    let (w, h) = (w.max(2), h.max(2));
    let size = Extent3d { width: w, height: h, depth_or_array_layers: 1 };
    let hdr = device.create_texture(&TextureDescriptor {
        label: Some("hdr"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: TextureDimension::D2,
        format: HDR_FORMAT,
        usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let hdr_view = hdr.create_view(&TextureViewDescriptor::default());
    let color_msaa = if msaa > 1 {
        let t = device.create_texture(&TextureDescriptor {
            label: Some("hdr-msaa"),
            size,
            mip_level_count: 1,
            sample_count: msaa,
            dimension: TextureDimension::D2,
            format: HDR_FORMAT,
            usage: TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        Some(t.create_view(&TextureViewDescriptor::default()))
    } else {
        None
    };
    let depth = device.create_texture(&TextureDescriptor {
        label: Some("depth"),
        size,
        mip_level_count: 1,
        sample_count: msaa,
        dimension: TextureDimension::D2,
        format: DEPTH_FORMAT,
        usage: TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let depth_view = depth.create_view(&TextureViewDescriptor::default());
    let mut bloom = vec![];
    if bloom_on {
        let (mut bw, mut bh) = (w / 2, h / 2);
        for _ in 0..5 {
            if bw < 4 || bh < 4 {
                break;
            }
            let t = device.create_texture(&TextureDescriptor {
                label: Some("bloom"),
                size: Extent3d { width: bw, height: bh, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: TextureDimension::D2,
                format: HDR_FORMAT,
                usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            });
            bloom.push(BloomLevel { view: t.create_view(&TextureViewDescriptor::default()), w: bw, h: bh });
            bw /= 2;
            bh /= 2;
        }
    }
    Targets { w, h, color_msaa, hdr_view, depth_view, bloom }
}

impl Renderer {
    pub fn new(gpu: Gpu, quality: Quality, meshes: &[MeshData]) -> Renderer {
        let device = &gpu.device;
        let layouts = make_layouts(device);
        let pipes = make_pipelines(device, &layouts, quality.msaa, gpu.config.format);

        let globals_buf = device.create_buffer(&BufferDescriptor { label: Some("globals"), size: std::mem::size_of::<GlobalsU>() as u64, usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST, mapped_at_creation: false });

        // Shadow map: a depth texture array, one layer per cascade.
        let ss = quality.shadow_size;
        let shadow_tex = device.create_texture(&TextureDescriptor {
            label: Some("shadow-array"),
            size: Extent3d { width: ss, height: ss, depth_or_array_layers: CASCADES as u32 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: DEPTH_FORMAT,
            usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let shadow_tex_array_view = shadow_tex.create_view(&TextureViewDescriptor { dimension: Some(TextureViewDimension::D2Array), ..Default::default() });
        let shadow_layer_views: Vec<TextureView> = (0..CASCADES as u32)
            .map(|i| shadow_tex.create_view(&TextureViewDescriptor { dimension: Some(TextureViewDimension::D2), base_array_layer: i, array_layer_count: Some(1), ..Default::default() }))
            .collect();
        let shadow_ubufs: Vec<Buffer> = (0..CASCADES)
            .map(|_| device.create_buffer(&BufferDescriptor { label: Some("shadow-u"), size: std::mem::size_of::<ShadowU>() as u64, usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST, mapped_at_creation: false }))
            .collect();
        let shadow_bgs: Vec<BindGroup> = shadow_ubufs
            .iter()
            .map(|b| device.create_bind_group(&BindGroupDescriptor { label: Some("shadow-bg"), layout: &layouts.shadow, entries: &[BindGroupEntry { binding: 0, resource: b.as_entire_binding() }] }))
            .collect();
        let shadow_samp = device.create_sampler(&SamplerDescriptor { label: Some("shadow-samp"), mag_filter: FilterMode::Linear, min_filter: FilterMode::Linear, compare: Some(CompareFunction::LessEqual), address_mode_u: AddressMode::ClampToEdge, address_mode_v: AddressMode::ClampToEdge, ..Default::default() });
        let lin_samp = device.create_sampler(&SamplerDescriptor { label: Some("lin-samp"), mag_filter: FilterMode::Linear, min_filter: FilterMode::Linear, address_mode_u: AddressMode::ClampToEdge, address_mode_v: AddressMode::ClampToEdge, ..Default::default() });

        // Placeholder world textures (replaced by set_world)
        let (height_view, splat_view) = (dummy_tex(device, TextureFormat::R32Float), dummy_tex(device, TextureFormat::Rgba8Unorm));
        let scene_bg = make_scene_bg(device, &layouts, &globals_buf, &shadow_tex_array_view, &shadow_samp, &height_view, &splat_view, &lin_samp);

        let (cw, ch) = scaled_size(gpu.config.width, gpu.config.height, quality.render_scale);
        let targets = create_targets(device, cw, ch, quality.msaa, quality.bloom);
        let identity_inst = device.create_buffer_init(&util::BufferInitDescriptor { label: Some("identity-inst"), contents: cast_slice(&[Instance::from_mat4(Mat4::IDENTITY, [1.0; 4])]), usage: BufferUsages::VERTEX });
        let water_buf = device.create_buffer(&BufferDescriptor { label: Some("water"), size: 16 * 16, usage: BufferUsages::VERTEX | BufferUsages::COPY_DST, mapped_at_creation: false });
        let dyn_inst = DynBuf::new(device, "dyn-inst", BufferUsages::VERTEX, 80 * 4096);
        let ghost_inst = DynBuf::new(device, "ghost-inst", BufferUsages::VERTEX, 80 * 64);
        let part_buf = DynBuf::new(device, "particles", BufferUsages::VERTEX, 64 * 4096);

        let post = build_post(device, &gpu.queue, &layouts, &lin_samp, &targets);
        let mut r = Renderer {
            gpu,
            quality,
            layouts,
            pipes,
            globals_buf,
            scene_bg,
            shadow_tex_array_view,
            shadow_layer_views,
            shadow_ubufs,
            shadow_bgs,
            shadow_samp,
            lin_samp,
            height_view,
            splat_view,
            post,
            targets,
            mesh_vb: None,
            mesh_ib: None,
            mesh_table: vec![],
            identity_inst,
            terrain: None,
            wg: world_gpu::WorldGpu::default(),
            scratch_instances: Vec::new(),
            water_buf,
            water_count: 0,
            dyn_inst,
            ghost_inst,
            part_buf,
            part_add_count: 0,
            part_count: 0,
            capture_requested: false,
            capture: None,
            stats: RenderStats::default(),
            frame_index: 0,
        };
        r.upload_meshes(meshes);
        r
    }

    // ---- resources ---------------------------------------------------------------
    pub fn upload_meshes(&mut self, meshes: &[MeshData]) {
        let mut verts = Vec::new();
        let mut idx: Vec<u32> = Vec::new();
        let mut table = Vec::new();
        for m in meshes {
            table.push(MeshRange { first_index: idx.len() as u32, index_count: m.idx.len() as u32, base_vertex: verts.len() as i32 });
            verts.extend_from_slice(&m.verts);
            idx.extend_from_slice(&m.idx);
        }
        if verts.is_empty() {
            return;
        }
        let d = &self.gpu.device;
        self.mesh_vb = Some(d.create_buffer_init(&util::BufferInitDescriptor { label: Some("mesh-vb"), contents: cast_slice(&verts), usage: BufferUsages::VERTEX }));
        self.mesh_ib = Some(d.create_buffer_init(&util::BufferInitDescriptor { label: Some("mesh-ib"), contents: cast_slice(&idx), usage: BufferUsages::INDEX }));
        self.mesh_table = table;
    }

    /// Upload everything static about the island.
    pub fn set_world(&mut self, world: &World) {
        let d = &self.gpu.device;
        let q = &self.gpu.queue;
        // terrain geometry
        let vb = d.create_buffer_init(&util::BufferInitDescriptor { label: Some("terrain-vb"), contents: cast_slice(&world.terrain.verts), usage: BufferUsages::VERTEX });
        let ib = d.create_buffer_init(&util::BufferInitDescriptor { label: Some("terrain-ib"), contents: cast_slice(&world.terrain.indices), usage: BufferUsages::INDEX });
        self.terrain = Some(TerrainGpu { vb, ib, chunks: world.terrain.chunks.clone() });
        self.wg = world_gpu::build_world_gpu(d, world);
        // heightmap texture (r32float, sampled with textureLoad in the shaders)
        let ht = d.create_texture(&TextureDescriptor {
            label: Some("heightmap"),
            size: Extent3d { width: GRID_N as u32, height: GRID_N as u32, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: TextureFormat::R32Float,
            usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
            view_formats: &[],
        });
        q.write_texture(
            TexelCopyTextureInfo { texture: &ht, mip_level: 0, origin: Origin3d::ZERO, aspect: TextureAspect::All },
            cast_slice(&world.hm.h),
            TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(GRID_N as u32 * 4), rows_per_image: Some(GRID_N as u32) },
            Extent3d { width: GRID_N as u32, height: GRID_N as u32, depth_or_array_layers: 1 },
        );
        self.height_view = ht.create_view(&TextureViewDescriptor::default());
        // splat texture
        let n = fn_core::world::splat::SPLAT_N as u32;
        let st = d.create_texture(&TextureDescriptor {
            label: Some("splat"),
            size: Extent3d { width: n, height: n, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: TextureFormat::Rgba8Unorm,
            usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
            view_formats: &[],
        });
        q.write_texture(
            TexelCopyTextureInfo { texture: &st, mip_level: 0, origin: Origin3d::ZERO, aspect: TextureAspect::All },
            &world.splat.data,
            TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(n * 4), rows_per_image: Some(n) },
            Extent3d { width: n, height: n, depth_or_array_layers: 1 },
        );
        self.splat_view = st.create_view(&TextureViewDescriptor::default());
        self.scene_bg = make_scene_bg(d, &self.layouts, &self.globals_buf, &self.shadow_tex_array_view, &self.shadow_samp, &self.height_view, &self.splat_view, &self.lin_samp);
        // water bodies: the sea plus every lake
        let mut bodies: Vec<[f32; 4]> = vec![[0.0, fn_core::world::SEA_LEVEL, 0.0, 6000.0]];
        for l in &world.layout.lakes {
            bodies.push([l.center.x, l.level, l.center.y, l.radius * 1.35]);
        }
        self.water_buf = d.create_buffer_init(&util::BufferInitDescriptor { label: Some("water"), contents: cast_slice(&bodies), usage: BufferUsages::VERTEX });
        self.water_count = bodies.len() as u32;
    }

    /// Lazily build grass / flowers around the camera (call once per frame before `render`).
    pub fn update_ground_cover(&mut self, world: &World, cam: Vec3) {
        let radius = 75.0 * self.quality.detail;
        world_gpu::update_ground_cover(&self.gpu.device, &mut self.wg, world, cam, radius);
    }

    /// Hide a felled tree / broken rock instance.
    pub fn remove_prop(&mut self, chunk: usize, slot: usize) {
        if let Some(p) = self.wg.props.get_mut(chunk).and_then(|c| c.get_mut(slot)) {
            p.alive = false;
        }
    }

    pub fn resize_surface(&mut self, w: u32, h: u32) {
        self.gpu.resize(w, h);
        let (cw, ch) = scaled_size(self.gpu.config.width, self.gpu.config.height, self.quality.render_scale);
        if cw != self.targets.w || ch != self.targets.h {
            self.targets = create_targets(&self.gpu.device, cw, ch, self.quality.msaa, self.quality.bloom);
            self.rebuild_post();
        }
        self.capture = None;
    }

    pub fn surface_size(&self) -> (u32, u32) {
        (self.gpu.config.width, self.gpu.config.height)
    }

    /// Change quality at runtime (rebuilds pipelines and targets).
    pub fn set_quality(&mut self, q: Quality) {
        if q == self.quality {
            return;
        }
        self.quality = q;
        let device = &self.gpu.device;
        self.pipes = make_pipelines(device, &self.layouts, q.msaa, self.gpu.config.format);
        let ss = q.shadow_size;
        let shadow_tex = device.create_texture(&TextureDescriptor {
            label: Some("shadow-array"),
            size: Extent3d { width: ss, height: ss, depth_or_array_layers: CASCADES as u32 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: DEPTH_FORMAT,
            usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        self.shadow_tex_array_view = shadow_tex.create_view(&TextureViewDescriptor { dimension: Some(TextureViewDimension::D2Array), ..Default::default() });
        self.shadow_layer_views = (0..CASCADES as u32)
            .map(|i| shadow_tex.create_view(&TextureViewDescriptor { dimension: Some(TextureViewDimension::D2), base_array_layer: i, array_layer_count: Some(1), ..Default::default() }))
            .collect();
        self.scene_bg = make_scene_bg(device, &self.layouts, &self.globals_buf, &self.shadow_tex_array_view, &self.shadow_samp, &self.height_view, &self.splat_view, &self.lin_samp);
        let (cw, ch) = scaled_size(self.gpu.config.width, self.gpu.config.height, q.render_scale);
        self.targets = create_targets(device, cw, ch, q.msaa, q.bloom);
        self.rebuild_post();
    }

    fn rebuild_post(&mut self) {
        self.post = build_post(&self.gpu.device, &self.gpu.queue, &self.layouts, &self.lin_samp, &self.targets);
    }

    // ---- capture (debug / test hook) -------------------------------------------------
    /// Ask for the next frame to also be rendered into a readable RGBA8 buffer.
    pub fn request_capture(&mut self) {
        self.capture_requested = true;
        self.ensure_capture();
        if let Some(c) = &self.capture {
            *c.result.borrow_mut() = None;
        }
    }

    fn ensure_capture(&mut self) {
        let (w, h) = (self.gpu.config.width, self.gpu.config.height);
        let need_new = self.capture.as_ref().is_none_or(|c| c.w != w || c.h != h);
        if need_new {
            let d = &self.gpu.device;
            let tex = d.create_texture(&TextureDescriptor {
                label: Some("capture"),
                size: Extent3d { width: w, height: h, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: TextureDimension::D2,
                format: TextureFormat::Rgba8Unorm,
                usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::COPY_SRC,
                view_formats: &[],
            });
            let bytes_per_row = (w * 4).div_ceil(256) * 256;
            let buf = d.create_buffer(&BufferDescriptor { label: Some("capture-buf"), size: (bytes_per_row * h) as u64, usage: BufferUsages::COPY_DST | BufferUsages::MAP_READ, mapped_at_creation: false });
            self.capture = Some(Capture { view: tex.create_view(&TextureViewDescriptor::default()), tex, buf, w, h, bytes_per_row, result: Rc::new(RefCell::new(None)) });
        }
    }

    /// Returns (width, height, RGBA bytes) once a requested capture finished.
    pub fn poll_capture(&mut self) -> Option<(u32, u32, Vec<u8>)> {
        let c = self.capture.as_ref()?;
        let r = c.result.borrow_mut().take();
        r
    }

    // ---- frame -----------------------------------------------------------------------
    fn build_globals(&self, f: &FrameInput, cascades: &[shadow::Cascade; 3]) -> GlobalsU {
        let cam = &f.camera;
        let vp = cam.view_proj;
        let inv = vp.inverse();
        let t = cam.tan_half_y();
        let sun = f.sun_dir.normalize();
        let sun_up = sun.y.max(0.0);
        let _ = sun_up;
        let mut cvp = [[[0.0f32; 4]; 4]; 3];
        for i in 0..3 {
            cvp[i] = cascades[i].vp.to_cols_array_2d();
        }
        GlobalsU {
            view_proj: vp.to_cols_array_2d(),
            inv_view_proj: inv.to_cols_array_2d(),
            cascade_vp: cvp,
            cam_pos: [cam.pos.x, cam.pos.y, cam.pos.z, f.time],
            cam_right: [cam.right.x, cam.right.y, cam.right.z, t * cam.aspect],
            cam_up: [cam.up.x, cam.up.y, cam.up.z, t],
            cam_fwd: [cam.fwd.x, cam.fwd.y, cam.fwd.z, 0.0],
            sun_dir: [sun.x, sun.y, sun.z, 1.0],
            sun_color: [1.30, 1.12, 0.86, 1.0],
            sky_color: [0.28, 0.40, 0.72, 1.0],
            ground_color: [0.27, 0.31, 0.15, 1.0],
            fog_params: [0.00055, 0.010, 0.45, 0.0],
            storm: [f.storm.x, f.storm.y, f.storm.z, f.storm.w],
            cascade_splits: [shadow::SPLITS[0], shadow::SPLITS[1], shadow::SPLITS[2], shadow::SPLITS[2]],
            shadow_info: [1.0 / self.quality.shadow_size as f32, if self.quality.shadows { 1.0 } else { 0.0 }, 0.05, 0.0],
            screen: [self.targets.w as f32, self.targets.h as f32, 1.0 / self.targets.w as f32, 1.0 / self.targets.h as f32],
            post: [0.85, 1.2, 0.0, f.damage],
            world: [WORLD_HALF, GRID_N as f32, fn_core::world::CELL, fn_core::world::SEA_LEVEL],
            misc: [f.wind, f.storm_time, f.in_storm, 0.0],
        }
    }

    pub fn render(&mut self, f: &FrameInput) {
        if self.capture_requested {
            self.ensure_capture();
        }
        self.frame_index += 1;
        self.stats = RenderStats::default();
        let cam = &f.camera;
        let sun = f.sun_dir.normalize();
        let cascades = shadow::compute_cascades(cam, sun, self.quality.shadow_size);
        let globals = self.build_globals(f, &cascades);
        let (device, queue) = (&self.gpu.device, &self.gpu.queue);
        queue.write_buffer(&self.globals_buf, 0, bytemuck::bytes_of(&globals));
        for i in 0..CASCADES {
            let su = ShadowU { vp: cascades[i].vp.to_cols_array_2d(), params: [f.time, f.wind, 0.0, 0.0] };
            queue.write_buffer(&self.shadow_ubufs[i], 0, bytemuck::bytes_of(&su));
        }
        // dynamic + culled prop instances share one buffer
        let frustum = cam.frustum();
        let mut scratch = std::mem::take(&mut self.scratch_instances);
        scratch.clear();
        scratch.extend_from_slice(f.instances);
        let detail = self.quality.detail;
        let main_props = world_gpu::gather_props(&self.wg, &frustum, cam.pos, detail, false, 0.0, &mut scratch, 0);
        let mut shadow_props: Vec<Vec<Batch>> = vec![];
        if self.quality.shadows {
            for c in cascades.iter() {
                let fr = Frustum::from_view_proj(&c.vp);
                shadow_props.push(world_gpu::gather_props(&self.wg, &fr, c.center, 1.0, true, c.radius + 30.0, &mut scratch, 0));
            }
        }
        self.dyn_inst.upload(device, queue, cast_slice(&scratch));
        self.scratch_instances = scratch;
        self.ghost_inst.upload(device, queue, cast_slice(f.ghost_instances));
        let mut parts: Vec<Particle> = Vec::with_capacity(f.particles.len() + f.particles_add.len());
        parts.extend_from_slice(f.particles);
        parts.extend_from_slice(f.particles_add);
        self.part_count = f.particles.len() as u32;
        self.part_add_count = f.particles_add.len() as u32;
        self.part_buf.upload(device, queue, cast_slice(&parts));
        // post uniforms
        let t = &self.targets;
        let cq = &self.quality;
        let post_u = PostU {
            a: [1.0 / t.w as f32, 1.0 / t.h as f32, 0.0, 0.0],
            b: [0.85, 1.22, if cq.bloom { 0.5 } else { 0.0 }, f.damage],
            c: [f.vignette, f.in_storm, f.time, if cq.msaa == 1 { 1.0 } else { 0.0 }],
            d: [t.w as f32, t.h as f32, 1.0 / self.gpu.config.width as f32, 1.0 / self.gpu.config.height as f32],
        };
        queue.write_buffer(&self.post.composite.ubuf, 0, bytemuck::bytes_of(&post_u));
        queue.write_buffer(&self.post.capture_composite.ubuf, 0, bytemuck::bytes_of(&post_u));

        let frame = match self.gpu.surface.get_current_texture() {
            CurrentSurfaceTexture::Success(f) | CurrentSurfaceTexture::Suboptimal(f) => f,
            CurrentSurfaceTexture::Outdated | CurrentSurfaceTexture::Lost => {
                self.gpu.reconfigure();
                return;
            }
            CurrentSurfaceTexture::Timeout | CurrentSurfaceTexture::Occluded => return,
            CurrentSurfaceTexture::Validation => {
                web_sys::console::error_1(&"surface texture validation error".into());
                return;
            }
        };
        let surface_view = frame.texture.create_view(&TextureViewDescriptor::default());
        let debug_scope = if self.capture_requested { Some(device.push_error_scope(ErrorFilter::Validation)) } else { None };
        let mut enc = device.create_command_encoder(&CommandEncoderDescriptor { label: Some("frame") });

        let mut stats = RenderStats::default();

        // ---- shadow cascades ----
        if self.quality.shadows {
            for ci in 0..CASCADES {
                self.shadow_pass(&mut enc, ci, &cascades[ci], &mut stats, f, &shadow_props[ci]);
            }
        }
        // ---- main scene ----
        self.scene_pass(&mut enc, f, &frustum, &mut stats, &main_props);
        // ---- bloom ----
        if !self.targets.bloom.is_empty() {
            let n = self.targets.bloom.len();
            let mut pass_idx = 0;
            // first (threshold) + downs
            for i in 0..n {
                let pipe = if i == 0 { &self.pipes.bloom_first } else { &self.pipes.bloom_down };
                let mut rp = enc.begin_render_pass(&RenderPassDescriptor {
                    label: Some("bloom-down"),
                    color_attachments: &[Some(RenderPassColorAttachment { view: &self.targets.bloom[i].view, depth_slice: None, resolve_target: None, ops: Operations { load: LoadOp::Clear(Color::BLACK), store: StoreOp::Store } })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                rp.set_pipeline(pipe);
                rp.set_bind_group(0, &self.post.passes[pass_idx].bg, &[]);
                rp.draw(0..3, 0..1);
                pass_idx += 1;
            }
            for i in (1..n).rev() {
                let mut rp = enc.begin_render_pass(&RenderPassDescriptor {
                    label: Some("bloom-up"),
                    color_attachments: &[Some(RenderPassColorAttachment { view: &self.targets.bloom[i - 1].view, depth_slice: None, resolve_target: None, ops: Operations { load: LoadOp::Load, store: StoreOp::Store } })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                rp.set_pipeline(&self.pipes.bloom_up);
                rp.set_bind_group(0, &self.post.passes[pass_idx].bg, &[]);
                rp.draw(0..3, 0..1);
                pass_idx += 1;
            }
        }
        // ---- composite to the swapchain ----
        {
            let mut rp = enc.begin_render_pass(&RenderPassDescriptor {
                label: Some("composite"),
                color_attachments: &[Some(RenderPassColorAttachment { view: &surface_view, depth_slice: None, resolve_target: None, ops: Operations { load: LoadOp::Clear(Color::BLACK), store: StoreOp::Store } })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            rp.set_pipeline(&self.pipes.composite);
            rp.set_bind_group(0, &self.post.composite.bg, &[]);
            rp.draw(0..3, 0..1);
        }
        // ---- optional capture (headless testing) ----
        let mut capture_copy = false;
        if self.capture_requested {
            self.capture_requested = false;
            if let (Some(cap), cp) = (&self.capture, &self.post.capture_composite) {
                let mut rp = enc.begin_render_pass(&RenderPassDescriptor {
                    label: Some("composite-capture"),
                    color_attachments: &[Some(RenderPassColorAttachment { view: &cap.view, depth_slice: None, resolve_target: None, ops: Operations { load: LoadOp::Clear(Color::BLACK), store: StoreOp::Store } })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                rp.set_pipeline(&self.pipes.composite_capture);
                rp.set_bind_group(0, &cp.bg, &[]);
                rp.draw(0..3, 0..1);
                drop(rp);
                enc.copy_texture_to_buffer(
                    TexelCopyTextureInfo { texture: &cap.tex, mip_level: 0, origin: Origin3d::ZERO, aspect: TextureAspect::All },
                    TexelCopyBufferInfo { buffer: &cap.buf, layout: TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(cap.bytes_per_row), rows_per_image: Some(cap.h) } },
                    Extent3d { width: cap.w, height: cap.h, depth_or_array_layers: 1 },
                );
                capture_copy = true;
            }
        }
        queue.submit([enc.finish()]);
        queue.present(frame);
        if let Some(scope) = debug_scope {
            let fut = scope.pop();
            wasm_bindgen_futures::spawn_local(async move {
                if let Some(e) = fut.await {
                    web_sys::console::error_1(&format!("[wgpu capture frame] {e}").into());
                }
            });
        }
        if capture_copy {
            if let Some(cap) = &self.capture {
                let result = cap.result.clone();
                let (w, h, bpr) = (cap.w, cap.h, cap.bytes_per_row);
                let buf = cap.buf.clone();
                let buf2 = buf.clone();
                buf.slice(..).map_async(MapMode::Read, move |res| {
                    if res.is_ok() {
                        if let Ok(data) = buf2.slice(..).get_mapped_range() {
                            let mut out = Vec::with_capacity((w * h * 4) as usize);
                            for y in 0..h as usize {
                                let row = y * bpr as usize;
                                out.extend_from_slice(&data[row..row + (w * 4) as usize]);
                            }
                            drop(data);
                            buf2.unmap();
                            *result.borrow_mut() = Some((w, h, out));
                        }
                    }
                });
            }
        }
        self.stats = stats;
    }

    fn shadow_pass(&self, enc: &mut CommandEncoder, ci: usize, cascade: &shadow::Cascade, stats: &mut RenderStats, f: &FrameInput, props: &[Batch]) {
        let mut rp = enc.begin_render_pass(&RenderPassDescriptor {
            label: Some("shadow"),
            color_attachments: &[],
            depth_stencil_attachment: Some(RenderPassDepthStencilAttachment { view: &self.shadow_layer_views[ci], depth_ops: Some(Operations { load: LoadOp::Clear(1.0), store: StoreOp::Store }), stencil_ops: None }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        let fr = Frustum::from_view_proj(&cascade.vp);
        if let Some(t) = &self.terrain {
            rp.set_pipeline(&self.pipes.shadow_terrain);
            rp.set_bind_group(0, &self.shadow_bgs[ci], &[]);
            rp.set_vertex_buffer(0, t.vb.slice(..));
            rp.set_index_buffer(t.ib.slice(..), IndexFormat::Uint16);
            let lod = if ci == 0 { 1 } else { 2 };
            for c in &t.chunks {
                if !fr.intersects_aabb(&c.aabb) {
                    continue;
                }
                rp.draw_indexed(c.lod_first[lod]..c.lod_first[lod] + c.lod_count[lod], c.base_vertex as i32, 0..1);
                stats.draw_calls += 1;
            }
        }
        // static building meshes
        if !self.wg.static_meshes.is_empty() {
            rp.set_pipeline(&self.pipes.shadow_mesh);
            rp.set_bind_group(0, &self.shadow_bgs[ci], &[]);
            rp.set_vertex_buffer(1, self.identity_inst.slice(..));
            for sm in &self.wg.static_meshes {
                if !fr.intersects_aabb(&sm.aabb) {
                    continue;
                }
                rp.set_vertex_buffer(0, sm.vb.slice(..));
                rp.set_index_buffer(sm.ib.slice(..), IndexFormat::Uint32);
                rp.draw_indexed(0..sm.index_count, 0, 0..1);
                stats.draw_calls += 1;
            }
        }
        if let (Some(vb), Some(ib)) = (&self.mesh_vb, &self.mesh_ib) {
            if props.iter().any(|_| true) || f.batches.iter().any(|b| b.shadow) {
                rp.set_pipeline(&self.pipes.shadow_mesh);
                rp.set_bind_group(0, &self.shadow_bgs[ci], &[]);
                rp.set_vertex_buffer(0, vb.slice(..));
                rp.set_vertex_buffer(1, self.dyn_inst.buf.slice(..));
                rp.set_index_buffer(ib.slice(..), IndexFormat::Uint32);
                for b in f.batches.iter().filter(|b| b.shadow).chain(props.iter()) {
                    if let Some(r) = self.mesh_table.get(b.mesh as usize) {
                        rp.draw_indexed(r.first_index..r.first_index + r.index_count, r.base_vertex, b.first..b.first + b.count);
                        stats.draw_calls += 1;
                    }
                }
            }
        }
    }

    fn scene_pass(&self, enc: &mut CommandEncoder, f: &FrameInput, frustum: &Frustum, stats: &mut RenderStats, props: &[Batch]) {
        let t = &self.targets;
        let (view, resolve) = match &t.color_msaa {
            Some(m) => (m, Some(&t.hdr_view)),
            None => (&t.hdr_view, None),
        };
        let mut rp = enc.begin_render_pass(&RenderPassDescriptor {
            label: Some("scene"),
            color_attachments: &[Some(RenderPassColorAttachment {
                view,
                depth_slice: None,
                resolve_target: resolve,
                ops: Operations { load: LoadOp::Clear(Color { r: 0.4, g: 0.6, b: 0.9, a: 1.0 }), store: if resolve.is_some() { StoreOp::Discard } else { StoreOp::Store } },
            })],
            depth_stencil_attachment: Some(RenderPassDepthStencilAttachment { view: &t.depth_view, depth_ops: Some(Operations { load: LoadOp::Clear(0.0), store: StoreOp::Discard }), stencil_ops: None }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        rp.set_bind_group(0, &self.scene_bg, &[]);

        // terrain
        if let Some(tg) = &self.terrain {
            rp.set_pipeline(&self.pipes.terrain);
            rp.set_vertex_buffer(0, tg.vb.slice(..));
            rp.set_index_buffer(tg.ib.slice(..), IndexFormat::Uint16);
            let cam_xz = glam::Vec2::new(f.camera.pos.x, f.camera.pos.z);
            for c in &tg.chunks {
                if !frustum.intersects_aabb(&c.aabb) {
                    continue;
                }
                let center = fn_core::world::terrain_mesh::chunk_center(c.cx, c.cz);
                let d = center.distance(cam_xz);
                let lod = if d < 200.0 { 0 } else if d < 460.0 { 1 } else { 2 };
                rp.draw_indexed(c.lod_first[lod]..c.lod_first[lod] + c.lod_count[lod], c.base_vertex as i32, 0..1);
                stats.draw_calls += 1;
                stats.chunks_drawn += 1;
                stats.triangles += (c.lod_count[lod] / 3) as u64;
            }
        }
        // static building meshes (one draw each)
        if !self.wg.static_meshes.is_empty() {
            rp.set_pipeline(&self.pipes.mesh);
            rp.set_vertex_buffer(1, self.identity_inst.slice(..));
            for sm in &self.wg.static_meshes {
                if !frustum.intersects_aabb(&sm.aabb) {
                    continue;
                }
                rp.set_vertex_buffer(0, sm.vb.slice(..));
                rp.set_index_buffer(sm.ib.slice(..), IndexFormat::Uint32);
                rp.draw_indexed(0..sm.index_count, 0, 0..1);
                stats.draw_calls += 1;
                stats.triangles += (sm.index_count / 3) as u64;
            }
        }
        // props + dynamic opaque meshes
        if let (Some(vb), Some(ib)) = (&self.mesh_vb, &self.mesh_ib) {
            if !f.batches.is_empty() || !props.is_empty() {
                rp.set_pipeline(&self.pipes.mesh);
                rp.set_vertex_buffer(0, vb.slice(..));
                rp.set_vertex_buffer(1, self.dyn_inst.buf.slice(..));
                rp.set_index_buffer(ib.slice(..), IndexFormat::Uint32);
                for b in f.batches.iter().chain(props.iter()) {
                    if let Some(r) = self.mesh_table.get(b.mesh as usize) {
                        rp.draw_indexed(r.first_index..r.first_index + r.index_count, r.base_vertex, b.first..b.first + b.count);
                        stats.draw_calls += 1;
                        stats.triangles += (r.index_count / 3 * b.count) as u64;
                        stats.instances += b.count;
                    }
                }
            }
            // grass tufts and flowers around the camera (double sided, wind animated)
            if !self.wg.grass.is_empty() {
                let (grass_mesh, flower_mesh) = world_gpu::mesh_ids_for_grass();
                rp.set_pipeline(&self.pipes.mesh_double);
                rp.set_vertex_buffer(0, vb.slice(..));
                rp.set_index_buffer(ib.slice(..), IndexFormat::Uint32);
                let cam_xz = glam::Vec2::new(f.camera.pos.x, f.camera.pos.z);
                for (ci, gc) in &self.wg.grass {
                    if self.wg.chunk_centers[*ci].distance(cam_xz) > 75.0 * self.quality.detail + 45.0 || !frustum.intersects_aabb(&gc.aabb) {
                        continue;
                    }
                    for (mesh, data) in [(grass_mesh, &gc.grass), (flower_mesh, &gc.flowers)] {
                        if let (Some((buf, count)), Some(r)) = (data, self.mesh_table.get(mesh as usize)) {
                            rp.set_vertex_buffer(1, buf.slice(..));
                            rp.draw_indexed(r.first_index..r.first_index + r.index_count, r.base_vertex, 0..*count);
                            stats.draw_calls += 1;
                            stats.triangles += (r.index_count / 3 * *count) as u64;
                        }
                    }
                }
            }
        }
        // sky (far plane)
        rp.set_pipeline(&self.pipes.sky);
        rp.draw(0..3, 0..1);
        // water
        if self.water_count > 0 {
            rp.set_pipeline(&self.pipes.water);
            rp.set_vertex_buffer(0, self.water_buf.slice(..));
            rp.draw(0..6, 0..self.water_count);
            stats.draw_calls += 1;
        }
        // ghosts (transparent building previews)
        if let (Some(vb), Some(ib)) = (&self.mesh_vb, &self.mesh_ib) {
            if !f.ghost_batches.is_empty() {
                rp.set_pipeline(&self.pipes.mesh_ghost);
                rp.set_vertex_buffer(0, vb.slice(..));
                rp.set_vertex_buffer(1, self.ghost_inst.buf.slice(..));
                rp.set_index_buffer(ib.slice(..), IndexFormat::Uint32);
                for b in f.ghost_batches {
                    if let Some(r) = self.mesh_table.get(b.mesh as usize) {
                        rp.draw_indexed(r.first_index..r.first_index + r.index_count, r.base_vertex, b.first..b.first + b.count);
                        stats.draw_calls += 1;
                    }
                }
            }
        }
        // storm wall
        if f.storm.z > 0.0 {
            rp.set_pipeline(&self.pipes.storm);
            rp.draw(0..160 * 6, 0..1);
            stats.draw_calls += 1;
        }
        // particles
        if self.part_count > 0 {
            rp.set_pipeline(&self.pipes.particles);
            rp.set_vertex_buffer(0, self.part_buf.buf.slice(..));
            rp.draw(0..6, 0..self.part_count);
            stats.draw_calls += 1;
        }
        if self.part_add_count > 0 {
            rp.set_pipeline(&self.pipes.particles_add);
            rp.set_vertex_buffer(0, self.part_buf.buf.slice(..));
            rp.draw(0..6, self.part_count..self.part_count + self.part_add_count);
            stats.draw_calls += 1;
        }
    }
}

fn scaled_size(w: u32, h: u32, scale: f32) -> (u32, u32) {
    (((w as f32 * scale).round() as u32).max(2), ((h as f32 * scale).round() as u32).max(2))
}

fn dummy_tex(device: &Device, format: TextureFormat) -> TextureView {
    let t = device.create_texture(&TextureDescriptor {
        label: Some("dummy"),
        size: Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: TextureDimension::D2,
        format,
        usage: TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    t.create_view(&TextureViewDescriptor::default())
}

fn build_post(d: &Device, q: &Queue, layouts: &Layouts, lin: &Sampler, t: &Targets) -> PostSet {
    let mk = |src: &TextureView, other: &TextureView, label: &str| -> PostPass {
        let ubuf = d.create_buffer(&BufferDescriptor { label: Some(label), size: std::mem::size_of::<PostU>() as u64, usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST, mapped_at_creation: false });
        let bg = d.create_bind_group(&BindGroupDescriptor {
            label: Some(label),
            layout: &layouts.post,
            entries: &[
                BindGroupEntry { binding: 0, resource: ubuf.as_entire_binding() },
                BindGroupEntry { binding: 1, resource: BindingResource::TextureView(src) },
                BindGroupEntry { binding: 2, resource: BindingResource::TextureView(other) },
                BindGroupEntry { binding: 3, resource: BindingResource::Sampler(lin) },
            ],
        });
        PostPass { ubuf, bg }
    };
    let mut passes = vec![];
    if !t.bloom.is_empty() {
        // threshold + first downsample: hdr -> bloom[0]
        let p = mk(&t.hdr_view, &t.hdr_view, "bloom-first");
        q.write_buffer(&p.ubuf, 0, bytemuck::bytes_of(&PostU { a: [1.0 / t.w as f32, 1.0 / t.h as f32, 1.15, 0.6], ..Default::default() }));
        passes.push(p);
        for i in 1..t.bloom.len() {
            let p = mk(&t.bloom[i - 1].view, &t.hdr_view, "bloom-down");
            q.write_buffer(&p.ubuf, 0, bytemuck::bytes_of(&PostU { a: [1.0 / t.bloom[i - 1].w as f32, 1.0 / t.bloom[i - 1].h as f32, 0.0, 0.0], ..Default::default() }));
            passes.push(p);
        }
        for i in (1..t.bloom.len()).rev() {
            let p = mk(&t.bloom[i].view, &t.hdr_view, "bloom-up");
            q.write_buffer(&p.ubuf, 0, bytemuck::bytes_of(&PostU { a: [1.0 / t.bloom[i].w as f32, 1.0 / t.bloom[i].h as f32, 1.0, 0.0], ..Default::default() }));
            passes.push(p);
        }
    }
    let bloom_view = t.bloom.first().map(|b| &b.view).unwrap_or(&t.hdr_view);
    PostSet { composite: mk(&t.hdr_view, bloom_view, "composite"), capture_composite: mk(&t.hdr_view, bloom_view, "composite-capture"), passes }
}

#[allow(clippy::too_many_arguments)]
fn make_scene_bg(device: &Device, l: &Layouts, globals: &Buffer, shadow: &TextureView, shadow_samp: &Sampler, height: &TextureView, splat: &TextureView, lin: &Sampler) -> BindGroup {
    device.create_bind_group(&BindGroupDescriptor {
        label: Some("scene-bg"),
        layout: &l.scene,
        entries: &[
            BindGroupEntry { binding: 0, resource: globals.as_entire_binding() },
            BindGroupEntry { binding: 1, resource: BindingResource::TextureView(shadow) },
            BindGroupEntry { binding: 2, resource: BindingResource::Sampler(shadow_samp) },
            BindGroupEntry { binding: 3, resource: BindingResource::TextureView(height) },
            BindGroupEntry { binding: 4, resource: BindingResource::TextureView(splat) },
            BindGroupEntry { binding: 5, resource: BindingResource::Sampler(lin) },
        ],
    })
}

