//! WebGPU device / surface setup through wgpu's browser backend.

use web_sys::HtmlCanvasElement;

pub struct Gpu {
    pub _instance: wgpu::Instance,
    pub _adapter: wgpu::Adapter,
    pub surface: wgpu::Surface<'static>,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub config: wgpu::SurfaceConfiguration,
    pub adapter_name: String,
    pub backend: String,
}

impl Gpu {
    pub async fn new(canvas: &HtmlCanvasElement) -> Result<Gpu, String> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let surface = instance
            .create_surface(wgpu::SurfaceTarget::Canvas(canvas.clone()))
            .map_err(|e| format!("could not create a WebGPU surface: {e}"))?;
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
                ..Default::default()
            })
            .await
            .map_err(|e| format!("no suitable WebGPU adapter: {e}"))?;
        let info = adapter.get_info();
        let adapter_limits = adapter.limits();
        let mut limits = wgpu::Limits::default();
        limits.max_texture_dimension_2d = adapter_limits.max_texture_dimension_2d.min(8192).max(limits.max_texture_dimension_2d);
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("fightnight-device"),
                required_features: wgpu::Features::empty(),
                required_limits: limits,
                ..Default::default()
            })
            .await
            .map_err(|e| format!("could not create a WebGPU device: {e}"))?;
        device.on_uncaptured_error(std::sync::Arc::new(|e: wgpu::Error| {
            web_sys::console::error_1(&format!("[wgpu] {e}").into());
        }));
        let caps = surface.get_capabilities(&adapter);
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| !f.is_srgb())
            .or_else(|| caps.formats.first().copied())
            .ok_or_else(|| "surface reports no formats".to_string())?;
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: canvas.width().max(1),
            height: canvas.height().max(1),
            present_mode: wgpu::PresentMode::AutoVsync,
            alpha_mode: wgpu::CompositeAlphaMode::Opaque,
            view_formats: vec![],
            color_space: wgpu::SurfaceColorSpace::Auto,
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);
        Ok(Gpu {
            _instance: instance,
            _adapter: adapter,
            surface,
            device,
            queue,
            config,
            adapter_name: info.name.clone(),
            backend: format!("{:?}", info.backend),
        })
    }

    pub fn resize(&mut self, w: u32, h: u32) {
        let (w, h) = (w.max(1), h.max(1));
        if w == self.config.width && h == self.config.height {
            return;
        }
        self.config.width = w;
        self.config.height = h;
        self.surface.configure(&self.device, &self.config);
    }

    pub fn reconfigure(&self) {
        self.surface.configure(&self.device, &self.config);
    }
}
