//! The Windows shell.
//!
//! **This crate contains no image processing.** Its entire vocabulary is: read
//! the stack, mutate a parameter, ask `pe-render` for a texture, draw it. The
//! day a convenience function that touches pixels appears in here is the day
//! the Mac port silently becomes a rewrite.
//!
//! At M0 it does the minimum that proves the pipeline is real end to end: load
//! an image, push it through source → ACEScg (16-bit float) → display, and show
//! the result. No controls yet — M1 adds the throwaway egui inspector.

use std::sync::Arc;

use pe_color::space;
use pe_render::{GpuContext, ImageTexture, TransformPass};
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowId};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let source = std::env::args().nth(1);

    let image = match &source {
        Some(path) => {
            println!("loading {path}");
            pe_io::load(path)?
        }
        None => {
            println!("no image given, showing the built-in test chart");
            pe_io::test_chart(1024, 768)
        }
    };
    println!("image is {}x{}", image.width, image.height);

    let event_loop = EventLoop::new()?;
    event_loop.set_control_flow(ControlFlow::Wait);
    let mut app = App { image, state: None };
    event_loop.run_app(&mut app)?;
    Ok(())
}

struct Renderer {
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    gpu: GpuContext,
    /// Source image, sRGB-encoded, linearised by the hardware on sample.
    source: ImageTexture,
    /// The 16-bit float working-space texture. The heart of the pipeline.
    working: ImageTexture,
    /// source -> working (ACEScg)
    to_working: TransformPass,
    /// working -> surface
    to_display: TransformPass,
}

struct App {
    image: pe_io::DecodedImage,
    state: Option<Renderer>,
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.state.is_some() {
            return;
        }
        match self.init(event_loop) {
            Ok(r) => self.state = Some(r),
            Err(e) => {
                eprintln!("could not start the renderer: {e}");
                event_loop.exit();
            }
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let Some(state) = self.state.as_mut() else {
            return;
        };
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                state.resize(size.width, size.height);
                state.window.request_redraw();
            }
            WindowEvent::RedrawRequested => {
                if let Err(e) = state.render() {
                    eprintln!("render failed: {e}");
                }
            }
            _ => {}
        }
    }
}

impl App {
    fn init(&self, event_loop: &ActiveEventLoop) -> Result<Renderer, Box<dyn std::error::Error>> {
        let attrs = Window::default_attributes()
            .with_title("Photo Editor — M0")
            .with_inner_size(winit::dpi::LogicalSize::new(1280.0, 800.0));
        let window = Arc::new(event_loop.create_window(attrs)?);

        // Order matters: one instance, the surface from it, then the adapter
        // from that same instance. A surface belongs to its creating instance,
        // and a device from a different one cannot present to it.
        let instance = GpuContext::create_instance();
        let surface = instance.create_surface(window.clone())?;
        let gpu = pollster::block_on(GpuContext::from_instance(instance, Some(&surface)))?;
        println!("GPU: {}", gpu.describe());
        assert!(
            gpu.supports_working_format(),
            "this GPU cannot render to Rgba16Float, which the pipeline requires"
        );

        let size = window.inner_size();
        let caps = surface.get_capabilities(&gpu.adapter);
        // An sRGB surface format so the hardware applies the encoding OETF on
        // write. See shaders/transform.wgsl.
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| f.is_srgb())
            .unwrap_or(caps.formats[0]);
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: caps.present_modes[0],
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&gpu.device, &config);

        let source = ImageTexture::upload_rgba8(
            &gpu.device,
            &gpu.queue,
            self.image.width,
            self.image.height,
            &self.image.pixels,
            "source",
        )?;

        let to_working = TransformPass::new(&gpu.device, pe_render::WORKING_FORMAT);
        let to_display = TransformPass::new(&gpu.device, format);

        // Decode into working space once. At M1 the effect rows run between
        // this and the display pass, and the stage cache keeps this result.
        let working = to_working.to_working(&gpu, &source, &space::SRGB);

        Ok(Renderer {
            window,
            surface,
            config,
            gpu,
            source,
            working,
            to_working,
            to_display,
        })
    }
}

impl Renderer {
    fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&self.gpu.device, &self.config);
    }

    fn render(&mut self) -> Result<(), wgpu::SurfaceError> {
        let frame = self.surface.get_current_texture()?;
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let mut encoder = self
            .gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("frame"),
            });

        // Working space (ACEScg, 16-bit float) out to the display space.
        self.to_display.encode(
            &self.gpu,
            &mut encoder,
            &self.working.view,
            &view,
            &space::ACESCG,
            &space::SRGB,
        );

        self.gpu.queue.submit([encoder.finish()]);
        frame.present();
        Ok(())
    }

    /// Kept so the unused-field warnings stay honest: these are what M1 builds
    /// on, not leftovers.
    #[allow(dead_code)]
    fn pipeline_inputs(&self) -> (&ImageTexture, &TransformPass) {
        (&self.source, &self.to_working)
    }
}
