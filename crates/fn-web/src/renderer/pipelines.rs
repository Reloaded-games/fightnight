//! Bind group layouts and render pipelines.

use super::types::*;
use fn_shaders as sh;
use wgpu::*;

const MESH_ATTRS: [VertexAttribute; 4] = vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Unorm8x4, 3 => Unorm8x4];
const INSTANCE_ATTRS: [VertexAttribute; 5] = vertex_attr_array![4 => Float32x4, 5 => Float32x4, 6 => Float32x4, 7 => Float32x4, 8 => Float32x4];
const PARTICLE_ATTRS: [VertexAttribute; 4] = vertex_attr_array![0 => Float32x4, 1 => Float32x4, 2 => Float32x4, 3 => Float32x4];
const WATER_ATTRS: [VertexAttribute; 1] = vertex_attr_array![0 => Float32x4];
const POS_ONLY_ATTRS: [VertexAttribute; 1] = vertex_attr_array![0 => Float32x3];

pub fn mesh_vertex_layout() -> VertexBufferLayout<'static> {
    VertexBufferLayout { array_stride: 32, step_mode: VertexStepMode::Vertex, attributes: &MESH_ATTRS }
}
pub fn pos_only_layout() -> VertexBufferLayout<'static> {
    VertexBufferLayout { array_stride: 32, step_mode: VertexStepMode::Vertex, attributes: &POS_ONLY_ATTRS }
}
pub fn instance_layout() -> VertexBufferLayout<'static> {
    VertexBufferLayout { array_stride: 80, step_mode: VertexStepMode::Instance, attributes: &INSTANCE_ATTRS }
}
fn particle_layout() -> VertexBufferLayout<'static> {
    VertexBufferLayout { array_stride: 64, step_mode: VertexStepMode::Instance, attributes: &PARTICLE_ATTRS }
}
fn water_layout() -> VertexBufferLayout<'static> {
    VertexBufferLayout { array_stride: 16, step_mode: VertexStepMode::Instance, attributes: &WATER_ATTRS }
}

pub struct Layouts {
    pub scene: BindGroupLayout,
    pub shadow: BindGroupLayout,
    pub post: BindGroupLayout,
}

pub struct Pipelines {
    pub terrain: RenderPipeline,
    pub mesh: RenderPipeline,
    pub mesh_double: RenderPipeline,
    pub mesh_ghost: RenderPipeline,
    pub sky: RenderPipeline,
    pub water: RenderPipeline,
    pub particles: RenderPipeline,
    pub particles_add: RenderPipeline,
    pub storm: RenderPipeline,
    pub shadow_terrain: RenderPipeline,
    pub shadow_mesh: RenderPipeline,
    pub bloom_first: RenderPipeline,
    pub bloom_down: RenderPipeline,
    pub bloom_up: RenderPipeline,
    pub composite: RenderPipeline,
    pub composite_capture: RenderPipeline,
}

pub fn make_layouts(device: &Device) -> Layouts {
    let tex = |binding: u32, vis: ShaderStages, filterable: bool| BindGroupLayoutEntry {
        binding,
        visibility: vis,
        ty: BindingType::Texture { sample_type: TextureSampleType::Float { filterable }, view_dimension: TextureViewDimension::D2, multisampled: false },
        count: None,
    };
    let scene = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
        label: Some("scene-bgl"),
        entries: &[
            BindGroupLayoutEntry {
                binding: 0,
                visibility: ShaderStages::VERTEX | ShaderStages::FRAGMENT,
                ty: BindingType::Buffer { ty: BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
                count: None,
            },
            BindGroupLayoutEntry {
                binding: 1,
                visibility: ShaderStages::FRAGMENT,
                ty: BindingType::Texture { sample_type: TextureSampleType::Depth, view_dimension: TextureViewDimension::D2Array, multisampled: false },
                count: None,
            },
            BindGroupLayoutEntry { binding: 2, visibility: ShaderStages::FRAGMENT, ty: BindingType::Sampler(SamplerBindingType::Comparison), count: None },
            tex(3, ShaderStages::FRAGMENT, false),
            tex(4, ShaderStages::FRAGMENT, true),
            BindGroupLayoutEntry { binding: 5, visibility: ShaderStages::FRAGMENT, ty: BindingType::Sampler(SamplerBindingType::Filtering), count: None },
        ],
    });
    let shadow = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
        label: Some("shadow-bgl"),
        entries: &[BindGroupLayoutEntry {
            binding: 0,
            visibility: ShaderStages::VERTEX,
            ty: BindingType::Buffer { ty: BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
            count: None,
        }],
    });
    let post = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
        label: Some("post-bgl"),
        entries: &[
            BindGroupLayoutEntry {
                binding: 0,
                visibility: ShaderStages::FRAGMENT,
                ty: BindingType::Buffer { ty: BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
                count: None,
            },
            tex(1, ShaderStages::FRAGMENT, true),
            tex(2, ShaderStages::FRAGMENT, true),
            BindGroupLayoutEntry { binding: 3, visibility: ShaderStages::FRAGMENT, ty: BindingType::Sampler(SamplerBindingType::Filtering), count: None },
        ],
    });
    Layouts { scene, shadow, post }
}

struct Cfg<'a> {
    label: &'a str,
    module: &'a ShaderModule,
    vs: &'a str,
    fs: Option<&'a str>,
    layout: &'a PipelineLayout,
    buffers: &'a [Option<VertexBufferLayout<'a>>],
    color: Option<(TextureFormat, Option<BlendState>)>,
    depth: Option<(bool, CompareFunction, DepthBiasState)>,
    depth_format: TextureFormat,
    cull: Option<Face>,
    samples: u32,
}

fn build(device: &Device, c: Cfg) -> RenderPipeline {
    let targets = [c.color.map(|(format, blend)| ColorTargetState { format, blend, write_mask: ColorWrites::ALL })];
    device.create_render_pipeline(&RenderPipelineDescriptor {
        label: Some(c.label),
        layout: Some(c.layout),
        vertex: VertexState { module: c.module, entry_point: Some(c.vs), compilation_options: Default::default(), buffers: c.buffers },
        primitive: PrimitiveState { topology: PrimitiveTopology::TriangleList, strip_index_format: None, front_face: FrontFace::Ccw, cull_mode: c.cull, unclipped_depth: false, polygon_mode: PolygonMode::Fill, conservative: false },
        depth_stencil: c.depth.map(|(write, compare, bias)| DepthStencilState { format: c.depth_format, depth_write_enabled: Some(write), depth_compare: Some(compare), stencil: StencilState::default(), bias }),
        multisample: MultisampleState { count: c.samples, mask: !0, alpha_to_coverage_enabled: false },
        fragment: c.fs.map(|fs| FragmentState { module: c.module, entry_point: Some(fs), compilation_options: Default::default(), targets: if c.color.is_some() { &targets } else { &[] } }),
        multiview_mask: None,
        cache: None,
    })
}

pub fn premultiplied_alpha() -> BlendState {
    BlendState {
        color: BlendComponent { src_factor: BlendFactor::One, dst_factor: BlendFactor::OneMinusSrcAlpha, operation: BlendOperation::Add },
        alpha: BlendComponent { src_factor: BlendFactor::One, dst_factor: BlendFactor::OneMinusSrcAlpha, operation: BlendOperation::Add },
    }
}
fn additive() -> BlendState {
    BlendState {
        color: BlendComponent { src_factor: BlendFactor::One, dst_factor: BlendFactor::One, operation: BlendOperation::Add },
        alpha: BlendComponent { src_factor: BlendFactor::Zero, dst_factor: BlendFactor::One, operation: BlendOperation::Add },
    }
}

pub fn make_pipelines(device: &Device, l: &Layouts, samples: u32, surface_format: TextureFormat) -> Pipelines {
    let module = |label: &str, src: String| device.create_shader_module(ShaderModuleDescriptor { label: Some(label), source: ShaderSource::Wgsl(src.into()) });
    let m_terrain = module("terrain", sh::scene(sh::TERRAIN));
    let m_mesh = module("mesh", sh::scene(sh::MESH));
    let m_sky = module("sky", sh::scene(sh::SKY));
    let m_water = module("water", sh::scene(sh::WATER));
    let m_part = module("particles", sh::scene(sh::PARTICLES));
    let m_storm = module("storm", sh::scene(sh::STORM));
    let m_shadow = module("shadow", sh::SHADOW.to_string());
    let m_post = module("post", sh::POST.to_string());

    let scene_pl = device.create_pipeline_layout(&PipelineLayoutDescriptor { label: Some("scene-pl"), bind_group_layouts: &[Some(&l.scene)], immediate_size: 0 });
    let shadow_pl = device.create_pipeline_layout(&PipelineLayoutDescriptor { label: Some("shadow-pl"), bind_group_layouts: &[Some(&l.shadow)], immediate_size: 0 });
    let post_pl = device.create_pipeline_layout(&PipelineLayoutDescriptor { label: Some("post-pl"), bind_group_layouts: &[Some(&l.post)], immediate_size: 0 });

    // Reverse-Z: nearer fragments have larger depth.
    let main_depth = Some((true, CompareFunction::Greater, DepthBiasState::default()));
    let test_only = Some((false, CompareFunction::Greater, DepthBiasState::default()));
    let hdr = Some((HDR_FORMAT, None));

    let terrain = build(device, Cfg { label: "terrain", module: &m_terrain, vs: "vs_main", fs: Some("fs_main"), layout: &scene_pl, buffers: &[Some(mesh_vertex_layout())], color: hdr, depth: main_depth, depth_format: DEPTH_FORMAT, cull: Some(Face::Back), samples });
    let mesh = build(device, Cfg { label: "mesh", module: &m_mesh, vs: "vs_main", fs: Some("fs_main"), layout: &scene_pl, buffers: &[Some(mesh_vertex_layout()), Some(instance_layout())], color: hdr, depth: main_depth, depth_format: DEPTH_FORMAT, cull: Some(Face::Back), samples });
    let mesh_double = build(device, Cfg { label: "mesh-double", module: &m_mesh, vs: "vs_main", fs: Some("fs_main"), layout: &scene_pl, buffers: &[Some(mesh_vertex_layout()), Some(instance_layout())], color: hdr, depth: main_depth, depth_format: DEPTH_FORMAT, cull: None, samples });
    let mesh_ghost = build(device, Cfg { label: "mesh-ghost", module: &m_mesh, vs: "vs_main", fs: Some("fs_ghost"), layout: &scene_pl, buffers: &[Some(mesh_vertex_layout()), Some(instance_layout())], color: Some((HDR_FORMAT, Some(premultiplied_alpha()))), depth: test_only, depth_format: DEPTH_FORMAT, cull: Some(Face::Back), samples });
    let sky = build(device, Cfg { label: "sky", module: &m_sky, vs: "vs_main", fs: Some("fs_main"), layout: &scene_pl, buffers: &[], color: hdr, depth: Some((false, CompareFunction::GreaterEqual, DepthBiasState::default())), depth_format: DEPTH_FORMAT, cull: None, samples });
    let water = build(device, Cfg { label: "water", module: &m_water, vs: "vs_main", fs: Some("fs_main"), layout: &scene_pl, buffers: &[Some(water_layout())], color: Some((HDR_FORMAT, Some(BlendState::ALPHA_BLENDING))), depth: test_only, depth_format: DEPTH_FORMAT, cull: None, samples });
    let particles = build(device, Cfg { label: "particles", module: &m_part, vs: "vs_main", fs: Some("fs_main"), layout: &scene_pl, buffers: &[Some(particle_layout())], color: Some((HDR_FORMAT, Some(premultiplied_alpha()))), depth: test_only, depth_format: DEPTH_FORMAT, cull: None, samples });
    let particles_add = build(device, Cfg { label: "particles-add", module: &m_part, vs: "vs_main", fs: Some("fs_main"), layout: &scene_pl, buffers: &[Some(particle_layout())], color: Some((HDR_FORMAT, Some(additive()))), depth: test_only, depth_format: DEPTH_FORMAT, cull: None, samples });
    let storm = build(device, Cfg { label: "storm", module: &m_storm, vs: "vs_main", fs: Some("fs_main"), layout: &scene_pl, buffers: &[], color: Some((HDR_FORMAT, Some(premultiplied_alpha()))), depth: test_only, depth_format: DEPTH_FORMAT, cull: None, samples });

    let shadow_bias = DepthBiasState { constant: 2, slope_scale: 2.0, clamp: 0.0 };
    let shadow_depth = Some((true, CompareFunction::Less, shadow_bias));
    let shadow_terrain = build(device, Cfg { label: "shadow-terrain", module: &m_shadow, vs: "vs_terrain", fs: None, layout: &shadow_pl, buffers: &[Some(pos_only_layout())], color: None, depth: shadow_depth, depth_format: DEPTH_FORMAT, cull: Some(Face::Back), samples: 1 });
    let shadow_mesh = build(device, Cfg { label: "shadow-mesh", module: &m_shadow, vs: "vs_mesh", fs: None, layout: &shadow_pl, buffers: &[Some(mesh_vertex_layout()), Some(instance_layout())], color: None, depth: shadow_depth, depth_format: DEPTH_FORMAT, cull: Some(Face::Back), samples: 1 });

    let post_target = |format| Some((format, None::<BlendState>));
    let add_blend = Some((HDR_FORMAT, Some(BlendState { color: BlendComponent { src_factor: BlendFactor::One, dst_factor: BlendFactor::One, operation: BlendOperation::Add }, alpha: BlendComponent::REPLACE })));
    let bloom_first = build(device, Cfg { label: "bloom-first", module: &m_post, vs: "vs_main", fs: Some("fs_bloom_first"), layout: &post_pl, buffers: &[], color: post_target(HDR_FORMAT), depth: None, depth_format: DEPTH_FORMAT, cull: None, samples: 1 });
    let bloom_down = build(device, Cfg { label: "bloom-down", module: &m_post, vs: "vs_main", fs: Some("fs_bloom_down"), layout: &post_pl, buffers: &[], color: post_target(HDR_FORMAT), depth: None, depth_format: DEPTH_FORMAT, cull: None, samples: 1 });
    let bloom_up = build(device, Cfg { label: "bloom-up", module: &m_post, vs: "vs_main", fs: Some("fs_bloom_up"), layout: &post_pl, buffers: &[], color: add_blend, depth: None, depth_format: DEPTH_FORMAT, cull: None, samples: 1 });
    let composite = build(device, Cfg { label: "composite", module: &m_post, vs: "vs_main", fs: Some("fs_composite"), layout: &post_pl, buffers: &[], color: post_target(surface_format), depth: None, depth_format: DEPTH_FORMAT, cull: None, samples: 1 });
    let composite_capture = build(device, Cfg { label: "composite-capture", module: &m_post, vs: "vs_main", fs: Some("fs_composite"), layout: &post_pl, buffers: &[], color: post_target(TextureFormat::Rgba8Unorm), depth: None, depth_format: DEPTH_FORMAT, cull: None, samples: 1 });

    Pipelines { terrain, mesh, mesh_double, mesh_ghost, sky, water, particles, particles_add, storm, shadow_terrain, shadow_mesh, bloom_first, bloom_down, bloom_up, composite, composite_capture }
}
