use growterm_types::{CellFlags, RenderCommand, Rgb};
use wgpu::util::DeviceExt;

use unicode_width::UnicodeWidthChar;

use crate::atlas::GlyphAtlas;

use std::sync::Mutex;
pub static GLYPH_LOG: std::sync::LazyLock<Mutex<Option<std::fs::File>>> = std::sync::LazyLock::new(|| {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    let dir = std::path::PathBuf::from(home).join("Library/Logs/growterm");
    let _ = std::fs::create_dir_all(&dir);
    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("glyph.log"))
        .ok();
    Mutex::new(file)
});

#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct BgVertex {
    position: [f32; 2],
    color: [f32; 4],
}

#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct GlyphVertex {
    position: [f32; 2],
    tex_coords: [f32; 2],
    color: [f32; 3],
}

#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct Uniforms {
    screen_size: [f32; 2],
    _padding: [f32; 2],
}

pub struct GpuDrawer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    surface: wgpu::Surface<'static>,
    surface_config: wgpu::SurfaceConfiguration,
    render_format: wgpu::TextureFormat,
    bg_pipeline: wgpu::RenderPipeline,
    overlay_pipeline: wgpu::RenderPipeline,
    glyph_pipeline: wgpu::RenderPipeline,
    uniform_buffer: wgpu::Buffer,
    uniform_bind_group: wgpu::BindGroup,
    glyph_texture: wgpu::Texture,
    glyph_texture_bind_group: wgpu::BindGroup,
    glyph_texture_size: u32,
    atlas: GlyphAtlas,
    tab_atlas: GlyphAtlas,
    atlas_cursor_x: u32,
    atlas_cursor_y: u32,
    atlas_row_height: u32,
    glyph_regions: std::collections::HashMap<(char, bool), GlyphRegion>,
    tab_glyph_regions: std::collections::HashMap<char, GlyphRegion>,
    surface_dirty: bool,
}

#[derive(Clone, Copy)]
struct GlyphRegion {
    u0: f32,
    v0: f32,
    u1: f32,
    v1: f32,
    width: u32,
    height: u32,
    offset_x: f32,
    offset_y: f32,
}

const GLYPH_TEXTURE_SIZE: u32 = 1024;

fn preferred_surface_alpha_mode(
    available: &[wgpu::CompositeAlphaMode],
) -> wgpu::CompositeAlphaMode {
    [
        wgpu::CompositeAlphaMode::PostMultiplied,
        wgpu::CompositeAlphaMode::PreMultiplied,
        wgpu::CompositeAlphaMode::Inherit,
        wgpu::CompositeAlphaMode::Opaque,
    ]
    .into_iter()
    .find(|candidate| available.contains(candidate))
    .or_else(|| available.first().copied())
    .unwrap_or(wgpu::CompositeAlphaMode::Opaque)
}
/// Push a textured quad (2 triangles, 6 vertices) for a glyph.
fn push_glyph_quad(
    verts: &mut Vec<GlyphVertex>,
    region: &GlyphRegion,
    gx: f32, gy: f32,
    color: [f32; 3],
) {
    let gw = region.width as f32;
    let gh = region.height as f32;
    verts.push(GlyphVertex { position: [gx, gy], tex_coords: [region.u0, region.v0], color });
    verts.push(GlyphVertex { position: [gx + gw, gy], tex_coords: [region.u1, region.v0], color });
    verts.push(GlyphVertex { position: [gx, gy + gh], tex_coords: [region.u0, region.v1], color });
    verts.push(GlyphVertex { position: [gx + gw, gy], tex_coords: [region.u1, region.v0], color });
    verts.push(GlyphVertex { position: [gx + gw, gy + gh], tex_coords: [region.u1, region.v1], color });
    verts.push(GlyphVertex { position: [gx, gy + gh], tex_coords: [region.u0, region.v1], color });
}

const TAB_FONT_SIZE: f32 = 24.0;
const TAB_BAR_PADDING: f32 = 8.0;

/// Tab bar rendering info passed from the app layer.
pub struct TabBarInfo {
    pub titles: Vec<String>,
    pub active_index: usize,
    pub dragging_index: Option<usize>,
}

impl GpuDrawer {
    pub fn new<W>(window: std::sync::Arc<W>, width: u32, height: u32, font_size: f32, font_path: Option<&str>) -> Self
    where
        W: raw_window_handle::HasWindowHandle
            + raw_window_handle::HasDisplayHandle
            + Send
            + Sync
            + 'static,
    {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            ..Default::default()
        });

        let surface = instance.create_surface(window.clone()).unwrap();

        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::default(),
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
        }))
        .unwrap();

        let (device, queue) = pollster::block_on(adapter.request_device(
            &wgpu::DeviceDescriptor {
                label: Some("growterm device"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::default(),
                ..Default::default()
            },
            None,
        ))
        .unwrap();
        let surface_caps = surface.get_capabilities(&adapter);
        let surface_format = surface_caps.formats[0];

        // Use non-sRGB view to avoid double gamma encoding of ANSI colors
        let render_format = match surface_format {
            wgpu::TextureFormat::Bgra8UnormSrgb => wgpu::TextureFormat::Bgra8Unorm,
            wgpu::TextureFormat::Rgba8UnormSrgb => wgpu::TextureFormat::Rgba8Unorm,
            other => other,
        };
        let view_formats = if render_format != surface_format {
            vec![render_format]
        } else {
            vec![]
        };

        let surface_config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: surface_format,
            width: width.max(1),
            height: height.max(1),
            present_mode: wgpu::PresentMode::Fifo,
            alpha_mode: preferred_surface_alpha_mode(&surface_caps.alpha_modes),
            view_formats,
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &surface_config);

        // Uniform buffer
        let uniforms = Uniforms {
            screen_size: [width as f32, height as f32],
            _padding: [0.0; 2],
        };
        let uniform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("uniforms"),
            contents: bytemuck::bytes_of(&uniforms),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let uniform_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("uniform_bgl"),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                }],
            });

        let uniform_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("uniform_bg"),
            layout: &uniform_bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buffer.as_entire_binding(),
            }],
        });

        // Glyph texture + bind group
        let glyph_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("glyph_atlas"),
            size: wgpu::Extent3d {
                width: GLYPH_TEXTURE_SIZE,
                height: GLYPH_TEXTURE_SIZE,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });

        let glyph_texture_view = glyph_texture.create_view(&Default::default());
        let glyph_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });

        let glyph_texture_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("glyph_bgl"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                ],
            });

        let glyph_texture_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("glyph_bg"),
            layout: &glyph_texture_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&glyph_texture_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&glyph_sampler),
                },
            ],
        });

        // Background pipeline
        let bg_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("bg_shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/bg.wgsl").into()),
        });

        let bg_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("bg_pipeline_layout"),
            bind_group_layouts: &[&uniform_bind_group_layout],
            push_constant_ranges: &[],
        });

        let bg_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("bg_pipeline"),
            layout: Some(&bg_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &bg_shader,
                entry_point: Some("vs_main"),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<BgVertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x4],
                }],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &bg_shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: render_format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        // Overlay pipeline (same vertex layout as bg, with alpha blending)
        let overlay_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("overlay_shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/overlay.wgsl").into()),
        });

        let overlay_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("overlay_pipeline"),
            layout: Some(&bg_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &overlay_shader,
                entry_point: Some("vs_main"),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<BgVertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x4],
                }],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &overlay_shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: render_format,
                    blend: Some(wgpu::BlendState {
                        color: wgpu::BlendComponent {
                            src_factor: wgpu::BlendFactor::SrcAlpha,
                            dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                            operation: wgpu::BlendOperation::Add,
                        },
                        alpha: wgpu::BlendComponent::OVER,
                    }),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        // Glyph pipeline
        let glyph_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("glyph_shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/glyph.wgsl").into()),
        });

        let glyph_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("glyph_pipeline_layout"),
                bind_group_layouts: &[&uniform_bind_group_layout, &glyph_texture_bind_group_layout],
                push_constant_ranges: &[],
            });

        let glyph_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("glyph_pipeline"),
            layout: Some(&glyph_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &glyph_shader,
                entry_point: Some("vs_main"),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<GlyphVertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x2, 2 => Float32x3],
                }],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &glyph_shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: render_format,
                    blend: Some(wgpu::BlendState {
                        color: wgpu::BlendComponent {
                            src_factor: wgpu::BlendFactor::SrcAlpha,
                            dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                            operation: wgpu::BlendOperation::Add,
                        },
                        alpha: wgpu::BlendComponent::OVER,
                    }),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        let font = std::sync::Arc::new(GlyphAtlas::load_font(font_size, font_path));
        let fallback_font = std::sync::Arc::new(GlyphAtlas::load_fallback_font(font_size));
        let bold_font = std::sync::Arc::new(GlyphAtlas::load_builtin_bold_font(font_size));
        let bold_fallback_font = std::sync::Arc::new(GlyphAtlas::load_fallback_bold_font(font_size));
        let atlas = GlyphAtlas::with_shared_fonts(font_size, font, fallback_font.clone(), bold_font, bold_fallback_font.clone());
        let tab_bold_font = std::sync::Arc::new(GlyphAtlas::load_builtin_bold_font(TAB_FONT_SIZE));
        let tab_bold_fallback = std::sync::Arc::new(GlyphAtlas::load_fallback_bold_font(TAB_FONT_SIZE));
        let tab_atlas = GlyphAtlas::with_shared_fonts(TAB_FONT_SIZE, std::sync::Arc::new(GlyphAtlas::load_builtin_font(TAB_FONT_SIZE)), fallback_font, tab_bold_font, tab_bold_fallback);

        Self {
            device,
            queue,
            surface,
            surface_config,
            render_format,
            bg_pipeline,
            overlay_pipeline,
            glyph_pipeline,
            uniform_buffer,
            uniform_bind_group,
            glyph_texture,
            glyph_texture_bind_group,
            glyph_texture_size: GLYPH_TEXTURE_SIZE,
            atlas,
            tab_atlas,
            atlas_cursor_x: 0,
            atlas_cursor_y: 0,
            atlas_row_height: 0,
            glyph_regions: std::collections::HashMap::new(),
            tab_glyph_regions: std::collections::HashMap::new(),
            surface_dirty: false,
        }
    }

    pub fn set_font_size(&mut self, size: f32) {
        self.atlas.set_size(size);
        self.glyph_regions.clear();
        self.tab_glyph_regions.clear();
        self.atlas_cursor_x = 0;
        self.atlas_cursor_y = 0;
        self.atlas_row_height = 0;
    }

    pub fn set_font(&mut self, font_path: Option<&str>, size: f32) {
        self.atlas.set_font(font_path, size);
        self.glyph_regions.clear();
        self.tab_glyph_regions.clear();
        self.atlas_cursor_x = 0;
        self.atlas_cursor_y = 0;
        self.atlas_row_height = 0;
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        if self.surface_config.width == width && self.surface_config.height == height {
            return;
        }
        self.surface_config.width = width;
        self.surface_config.height = height;
        self.surface_dirty = true;
    }

    pub fn cell_size(&self) -> (f32, f32) {
        self.atlas.cell_size()
    }

    pub fn tab_cell_size(&self) -> (f32, f32) {
        self.tab_atlas.cell_size()
    }

    /// Fixed tab bar height in pixels (independent of body font size).
    pub fn tab_bar_height(&self) -> f32 {
        let (_, tab_ch) = self.tab_atlas.cell_size();
        tab_ch + TAB_BAR_PADDING
    }

    /// Returns true if the glyph budget was exceeded and another redraw is needed.
    pub fn draw(
        &mut self,
        commands: &[RenderCommand],
        scrollbar: Option<(f32, f32)>,
        tab_bar: Option<&TabBarInfo>,
        is_break: bool,
        break_text: Option<&[String]>,
        transparent_tab_bar: bool,
        content_y_offset: f32,
        title_bar_height: f32,
        header_opacity: f32,
    ) {
        if self.surface_dirty {
            self.surface_dirty = false;
            self.surface.configure(&self.device, &self.surface_config);
            let uniforms = Uniforms {
                screen_size: [
                    self.surface_config.width as f32,
                    self.surface_config.height as f32,
                ],
                _padding: [0.0; 2],
            };
            self.queue
                .write_buffer(&self.uniform_buffer, 0, bytemuck::bytes_of(&uniforms));
        }
        let output = match self.surface.get_current_texture() {
            Ok(t) => t,
            Err(_) => return,
        };
        let view = output.texture.create_view(&wgpu::TextureViewDescriptor {
            format: Some(self.render_format),
            ..Default::default()
        });

        let (cell_w, cell_h) = self.atlas.cell_size();
        let y_off = content_y_offset;

        // Build bg vertices
        let mut bg_vertices: Vec<BgVertex> = Vec::new();

        for cmd in commands {
            let x = cmd.col as f32 * cell_w;
            let y = y_off + cmd.row as f32 * cell_h;
            let w = if cmd.flags.contains(CellFlags::WIDE_CHAR) {
                cell_w * 2.0
            } else {
                cell_w
            };
            let color = rgb_to_f32a(cmd.bg);

            bg_vertices.push(BgVertex {
                position: [x, y],
                color,
            });
            bg_vertices.push(BgVertex {
                position: [x + w, y],
                color,
            });
            bg_vertices.push(BgVertex {
                position: [x, y + cell_h],
                color,
            });
            bg_vertices.push(BgVertex {
                position: [x + w, y],
                color,
            });
            bg_vertices.push(BgVertex {
                position: [x + w, y + cell_h],
                color,
            });
            bg_vertices.push(BgVertex {
                position: [x, y + cell_h],
                color,
            });

            // Underline: thin rect at cell bottom using fg color
            if cmd.flags.contains(CellFlags::UNDERLINE) {
                let underline_h = (cell_h * 0.07).max(1.0);
                let underline_y = y + cell_h - underline_h;
                let fg_color = rgb_to_f32a(cmd.fg);
                push_bg_rect(&mut bg_vertices, x, underline_y, w, underline_h, fg_color);
            }

            // Strikethrough: thin rect at cell vertical center using fg color
            if cmd.flags.contains(CellFlags::STRIKETHROUGH) {
                let strike_h = (cell_h * 0.07).max(1.0);
                let strike_y = y + (cell_h - strike_h) / 2.0;
                let fg_color = rgb_to_f32a(cmd.fg);
                push_bg_rect(&mut bg_vertices, x, strike_y, w, strike_h, fg_color);
            }
        }

        // Build glyph vertices
        let mut glyph_vertices: Vec<GlyphVertex> = Vec::new();

        // Preload glyphs for lower rows first so the input/status area is not
        // starved by large body updates when the per-frame glyph budget is low.
        for idx in prioritized_glyph_command_indices(commands) {
            let cmd = &commands[idx];
            let ch = cmd.character;
            if ch >= '\u{2500}' && ch <= '\u{257F}' {
                continue;
            }
            if ch >= '\u{2580}' && ch <= '\u{259F}' && !(ch >= '\u{2591}' && ch <= '\u{2593}') {
                continue;
            }
            let bold = cmd.flags.contains(growterm_types::CellFlags::BOLD);
            let _ = self.ensure_glyph_in_atlas(ch, bold);
        }

        // Helper: push a fg-colored rectangle into bg_vertices
        let push_rect =
            |bg_verts: &mut Vec<BgVertex>, x: f32, y: f32, w: f32, h: f32, color: [f32; 4]| {
                push_bg_rect(bg_verts, x, y, w, h, color);
            };

        for cmd in commands {
            if cmd.character == ' ' {
                continue;
            }
            if cmd.flags.contains(CellFlags::HIDDEN) {
                continue;
            }

            // Block elements (U+2580..U+259F, excluding shades U+2591-U+2593)
            let ch = cmd.character;
            if ch >= '\u{2580}' && ch <= '\u{259F}' && !(ch >= '\u{2591}' && ch <= '\u{2593}') {
                let cx = cmd.col as f32 * cell_w;
                let cy = y_off + cmd.row as f32 * cell_h;
                let fg = rgb_to_f32a(cmd.fg);
                if push_block_element_rects(&mut bg_vertices, ch, cx, cy, cell_w, cell_h, fg) {
                    continue;
                }
            }

            // Box drawing characters (U+2500..U+257F)
            if ch >= '\u{2500}' && ch <= '\u{257F}' {
                if let Some(segs) = box_drawing_segments(ch) {
                    let cx = cmd.col as f32 * cell_w;
                    let cy = y_off + cmd.row as f32 * cell_h;
                    let fg = rgb_to_f32a(cmd.fg);
                    let light_h = 1.0_f32;
                    let heavy_h = (cell_h / 8.0).ceil().max(2.0);
                    let light_w = 1.0_f32;
                    let heavy_w = (cell_w / 8.0).ceil().max(2.0);
                    let mid_x = (cell_w / 2.0).floor();
                    let mid_y = (cell_h / 2.0).floor();

                    // Horizontal segment
                    if segs.h_weight != LineWeight::None && segs.h_weight != LineWeight::Double {
                        let th = if segs.h_weight == LineWeight::Heavy {
                            heavy_h
                        } else {
                            light_h
                        };
                        let x0 = if segs.left {
                            0.0
                        } else {
                            mid_x - (th / 2.0).floor()
                        };
                        let x1 = if segs.right {
                            cell_w
                        } else {
                            mid_x + (th / 2.0).ceil()
                        };
                        push_rect(
                            &mut bg_vertices,
                            cx + x0,
                            cy + mid_y - (th / 2.0).floor(),
                            x1 - x0,
                            th,
                            fg,
                        );
                    }
                    // Vertical segment
                    if segs.v_weight != LineWeight::None && segs.v_weight != LineWeight::Double {
                        let tw = if segs.v_weight == LineWeight::Heavy {
                            heavy_w
                        } else {
                            light_w
                        };
                        let y0 = if segs.up {
                            0.0
                        } else {
                            mid_y - (tw / 2.0).floor()
                        };
                        let y1 = if segs.down {
                            cell_h
                        } else {
                            mid_y + (tw / 2.0).ceil()
                        };
                        push_rect(
                            &mut bg_vertices,
                            cx + mid_x - (tw / 2.0).floor(),
                            cy + y0,
                            tw,
                            y1 - y0,
                            fg,
                        );
                    }
                    // Double horizontal
                    if segs.h_weight == LineWeight::Double {
                        let gap = (cell_h / 6.0).ceil();
                        let th = light_h;
                        let x0 = if segs.left { 0.0 } else { mid_x };
                        let x1 = if segs.right { cell_w } else { mid_x + light_w };
                        push_rect(
                            &mut bg_vertices,
                            cx + x0,
                            cy + mid_y - gap - th / 2.0,
                            x1 - x0,
                            th,
                            fg,
                        );
                        push_rect(
                            &mut bg_vertices,
                            cx + x0,
                            cy + mid_y + gap - th / 2.0,
                            x1 - x0,
                            th,
                            fg,
                        );
                    }
                    // Double vertical
                    if segs.v_weight == LineWeight::Double {
                        let gap = (cell_w / 6.0).ceil();
                        let tw = light_w;
                        let y0 = if segs.up { 0.0 } else { mid_y };
                        let y1 = if segs.down { cell_h } else { mid_y + light_h };
                        push_rect(
                            &mut bg_vertices,
                            cx + mid_x - gap - tw / 2.0,
                            cy + y0,
                            tw,
                            y1 - y0,
                            fg,
                        );
                        push_rect(
                            &mut bg_vertices,
                            cx + mid_x + gap - tw / 2.0,
                            cy + y0,
                            tw,
                            y1 - y0,
                            fg,
                        );
                    }
                    continue;
                }
            }

            let bold = cmd.flags.contains(growterm_types::CellFlags::BOLD);
            let region = self.ensure_glyph_in_atlas(cmd.character, bold);
            if region.width == 0 || region.height == 0 {
                continue;
            }

            let cell_x = cmd.col as f32 * cell_w;
            let cell_y = y_off + cmd.row as f32 * cell_h;

            // Position glyph within cell
            let baseline_y = cell_y + cell_h * 0.8; // approximate baseline
            let gx = cell_x + region.offset_x;
            let gy = baseline_y - region.offset_y - region.height as f32;

            let color = rgb_to_f32(cmd.fg);

            push_glyph_quad(&mut glyph_vertices, &region, gx, gy, color);
        }

        // Scrollbar
        if let Some((thumb_top_ratio, thumb_height_ratio)) = scrollbar {
            let screen_w = self.surface_config.width as f32;
            let screen_h = self.surface_config.height as f32;
            let term_h = screen_h - y_off;
            let bar_w = 6.0_f32;
            let x0 = screen_w - bar_w;
            let y0 = y_off + thumb_top_ratio * term_h;
            let h = thumb_height_ratio * term_h;
            let color = [0.5, 0.5, 0.5, 1.0];
            push_rect(&mut bg_vertices, x0, y0, bar_w, h, color);
        }

        // Title bar + Tab bar overlay
        let mut tab_bg_verts: Vec<BgVertex> = Vec::new();
        let mut tab_glyph_verts: Vec<GlyphVertex> = Vec::new();
        // Title bar overlay when no tabs
        if tab_bar.is_none() && transparent_tab_bar && title_bar_height > 0.0 {
            let screen_w = self.surface_config.width as f32;
            push_bg_rect(&mut tab_bg_verts, 0.0, 0.0, screen_w, title_bar_height, [0.0, 0.0, 0.0, header_opacity]);
        }
        if let Some(tab_info) = tab_bar {
            let (tab_cw, tab_ch) = self.tab_atlas.cell_size();
            let tab_ascent = self.tab_atlas.ascent();
            let bar_h = self.tab_bar_height();
            let screen_w = self.surface_config.width as f32;
            let tab_y = if transparent_tab_bar { title_bar_height } else { 0.0 };
            let bar_bg: [f32; 4] = if transparent_tab_bar {
                [0.0, 0.0, 0.0, header_opacity]
            } else {
                [0.0, 0.0, 0.0, 1.0]
            };
            let dragging_bg: [f32; 4] = [0.4, 0.4, 0.2, 1.0];

            // Title bar overlay (transparent mode only)
            if transparent_tab_bar && title_bar_height > 0.0 {
                push_bg_rect(&mut tab_bg_verts, 0.0, 0.0, screen_w, title_bar_height, bar_bg);
            }
            push_bg_rect(&mut tab_bg_verts, 0.0, tab_y, screen_w, bar_h, bar_bg);

            let tab_count = tab_info.titles.len().max(1) as f32;
            let tab_w = screen_w / tab_count;
            let mut x = 0.0_f32;
            for (i, title) in tab_info.titles.iter().enumerate() {
                if tab_info.dragging_index == Some(i) {
                    push_bg_rect(&mut tab_bg_verts, x, tab_y, tab_w, bar_h, dragging_bg);
                }

                let text_w = title.chars().count() as f32 * tab_cw;
                let mut cx = x + (tab_w - text_w) / 2.0;
                for ch in title.chars() {
                    if ch == ' ' {
                        cx += tab_cw;
                        continue;
                    }
                    let region = self.ensure_tab_glyph_in_atlas(ch);
                    if region.width > 0 && region.height > 0 {
                        let baseline_y = tab_y + (bar_h - tab_ch) / 2.0 + tab_ascent;
                        let gx = cx + region.offset_x;
                        let gy = baseline_y - region.offset_y - region.height as f32;
                        let color: [f32; 3] = if i == tab_info.active_index {
                            [1.0, 1.0, 1.0]
                        } else {
                            [0.4, 0.4, 0.4]
                        };
                        push_glyph_quad(&mut tab_glyph_verts, &region, gx, gy, color);
                    }
                    cx += tab_cw;
                }

                x += tab_w;
            }
        }

        let bg_buffer = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("bg_vb"),
                contents: bytemuck::cast_slice(&bg_vertices),
                usage: wgpu::BufferUsages::VERTEX,
            });

        let glyph_buffer = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("glyph_vb"),
                contents: bytemuck::cast_slice(&glyph_vertices),
                usage: wgpu::BufferUsages::VERTEX,
            });

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("render_encoder"),
            });

        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("render_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.0,
                            g: 0.0,
                            b: 0.0,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                ..Default::default()
            });

            // Pass 1: backgrounds
            if !bg_vertices.is_empty() {
                pass.set_pipeline(&self.bg_pipeline);
                pass.set_bind_group(0, &self.uniform_bind_group, &[]);
                pass.set_vertex_buffer(0, bg_buffer.slice(..));
                pass.draw(0..bg_vertices.len() as u32, 0..1);
            }

            // Pass 2: glyphs
            if !glyph_vertices.is_empty() {
                pass.set_pipeline(&self.glyph_pipeline);
                pass.set_bind_group(0, &self.uniform_bind_group, &[]);
                pass.set_bind_group(1, &self.glyph_texture_bind_group, &[]);
                pass.set_vertex_buffer(0, glyph_buffer.slice(..));
                pass.draw(0..glyph_vertices.len() as u32, 0..1);
            }

            // Pass 2.5: tab bar (uses bg_pipeline with alpha blending)
            if !tab_bg_verts.is_empty() {
                let tab_bg_buffer = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("tab_bg_vb"),
                    contents: bytemuck::cast_slice(&tab_bg_verts),
                    usage: wgpu::BufferUsages::VERTEX,
                });
                pass.set_pipeline(&self.bg_pipeline);
                pass.set_bind_group(0, &self.uniform_bind_group, &[]);
                pass.set_vertex_buffer(0, tab_bg_buffer.slice(..));
                pass.draw(0..tab_bg_verts.len() as u32, 0..1);
            }
            if !tab_glyph_verts.is_empty() {
                let tab_glyph_buffer = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("tab_glyph_vb"),
                    contents: bytemuck::cast_slice(&tab_glyph_verts),
                    usage: wgpu::BufferUsages::VERTEX,
                });
                pass.set_pipeline(&self.glyph_pipeline);
                pass.set_bind_group(0, &self.uniform_bind_group, &[]);
                pass.set_bind_group(1, &self.glyph_texture_bind_group, &[]);
                pass.set_vertex_buffer(0, tab_glyph_buffer.slice(..));
                pass.draw(0..tab_glyph_verts.len() as u32, 0..1);
            }

            // Pass 3: break overlay (semi-transparent red tint over everything)
            if is_break {
                let screen_w = self.surface_config.width as f32;
                let screen_h = self.surface_config.height as f32;
                let mut overlay_verts: Vec<BgVertex> = Vec::new();
                push_bg_rect(&mut overlay_verts, 0.0, 0.0, screen_w, screen_h, [0.6, 0.0, 0.0, 1.0]);
                let overlay_buffer = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("overlay_vb"),
                    contents: bytemuck::cast_slice(&overlay_verts),
                    usage: wgpu::BufferUsages::VERTEX,
                });
                pass.set_pipeline(&self.overlay_pipeline);
                pass.set_bind_group(0, &self.uniform_bind_group, &[]);
                pass.set_vertex_buffer(0, overlay_buffer.slice(..));
                pass.draw(0..overlay_verts.len() as u32, 0..1);
            }

            // Pass 4: coaching text over break overlay
            if let Some(lines) = break_text {
                let screen_w = self.surface_config.width as f32;
                let screen_h = self.surface_config.height as f32;
                let (tab_cw, tab_ch) = self.tab_atlas.cell_size();
                let tab_ascent = self.tab_atlas.ascent();
                let line_spacing = tab_ch * 0.4;

                // Helper: display width of a char (wide chars = 2)
                let char_w = |c: char| -> usize {
                    UnicodeWidthChar::width(c).unwrap_or(1)
                };
                let str_w = |s: &str| -> usize {
                    s.chars().map(|c| char_w(c)).sum()
                };

                // Word-wrap lines to fit screen width (in cell units)
                let pad = tab_ch; // padding around text
                let max_cols = ((screen_w - pad * 4.0) / tab_cw).floor().max(10.0) as usize;
                let wrapped: Vec<String> = lines.iter().flat_map(|line| {
                    if line.is_empty() {
                        return vec![String::new()];
                    }
                    if str_w(line) <= max_cols {
                        return vec![line.clone()];
                    }
                    let chars: Vec<char> = line.chars().collect();
                    let mut result = Vec::new();
                    let mut start = 0;
                    while start < chars.len() {
                        let mut width = 0;
                        let mut end = start;
                        while end < chars.len() {
                            let cw = char_w(chars[end]);
                            if width + cw > max_cols { break; }
                            width += cw;
                            end += 1;
                        }
                        if end == chars.len() {
                            result.push(chars[start..end].iter().collect());
                            break;
                        }
                        // Find last space for word break
                        let chunk = &chars[start..end];
                        if let Some(sp) = chunk.iter().rposition(|&c| c == ' ') {
                            result.push(chars[start..start + sp].iter().collect());
                            start = start + sp + 1;
                        } else {
                            result.push(chars[start..end].iter().collect());
                            start = end;
                        }
                    }
                    result
                }).collect();
                let lines = &wrapped;

                let total_height = lines.len() as f32 * (tab_ch + line_spacing) - line_spacing;
                let start_y = (screen_h - total_height) / 2.0;

                // Background box behind coaching text
                let max_line_w = lines.iter().map(|l| str_w(l)).max().unwrap_or(0) as f32 * tab_cw;
                let bg_x = ((screen_w - max_line_w) / 2.0 - pad).max(0.0);
                let bg_y = (start_y - pad).max(0.0);
                let bg_w = (max_line_w + pad * 2.0).min(screen_w);
                let bg_h = (total_height + pad * 2.0).min(screen_h);
                let mut coaching_bg_verts: Vec<BgVertex> = Vec::new();
                push_bg_rect(&mut coaching_bg_verts, bg_x, bg_y, bg_w, bg_h, [0.0, 0.0, 0.0, 1.0]);
                let coaching_bg_buffer = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("coaching_bg_vb"),
                    contents: bytemuck::cast_slice(&coaching_bg_verts),
                    usage: wgpu::BufferUsages::VERTEX,
                });
                pass.set_pipeline(&self.bg_pipeline);
                pass.set_bind_group(0, &self.uniform_bind_group, &[]);
                pass.set_vertex_buffer(0, coaching_bg_buffer.slice(..));
                pass.draw(0..coaching_bg_verts.len() as u32, 0..1);

                let mut coaching_verts: Vec<GlyphVertex> = Vec::new();
                for (line_idx, line) in lines.iter().enumerate() {
                    let text_w = str_w(line) as f32 * tab_cw;
                    let mut cx = (screen_w - text_w) / 2.0;
                    let line_y = start_y + line_idx as f32 * (tab_ch + line_spacing);

                    for ch in line.chars() {
                        let advance = tab_cw * char_w(ch) as f32;
                        if ch == ' ' {
                            cx += advance;
                            continue;
                        }
                        let region = self.ensure_tab_glyph_in_atlas(ch);
                        if region.width > 0 && region.height > 0 {
                            let baseline_y = line_y + tab_ascent;
                            let gx = cx + region.offset_x;
                            let gy = baseline_y - region.offset_y - region.height as f32;
                            let gw = region.width as f32;
                            let gh = region.height as f32;
                            let color: [f32; 3] = [1.0, 1.0, 1.0];
                            coaching_verts.push(GlyphVertex {
                                position: [gx, gy],
                                tex_coords: [region.u0, region.v0],
                                color,
                            });
                            coaching_verts.push(GlyphVertex {
                                position: [gx + gw, gy],
                                tex_coords: [region.u1, region.v0],
                                color,
                            });
                            coaching_verts.push(GlyphVertex {
                                position: [gx, gy + gh],
                                tex_coords: [region.u0, region.v1],
                                color,
                            });
                            coaching_verts.push(GlyphVertex {
                                position: [gx + gw, gy],
                                tex_coords: [region.u1, region.v0],
                                color,
                            });
                            coaching_verts.push(GlyphVertex {
                                position: [gx + gw, gy + gh],
                                tex_coords: [region.u1, region.v1],
                                color,
                            });
                            coaching_verts.push(GlyphVertex {
                                position: [gx, gy + gh],
                                tex_coords: [region.u0, region.v1],
                                color,
                            });
                        }
                        cx += advance;
                    }
                }

                if !coaching_verts.is_empty() {
                    let coaching_buffer = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("coaching_vb"),
                        contents: bytemuck::cast_slice(&coaching_verts),
                        usage: wgpu::BufferUsages::VERTEX,
                    });
                    pass.set_pipeline(&self.glyph_pipeline);
                    pass.set_bind_group(0, &self.uniform_bind_group, &[]);
                    pass.set_bind_group(1, &self.glyph_texture_bind_group, &[]);
                    pass.set_vertex_buffer(0, coaching_buffer.slice(..));
                    pass.draw(0..coaching_verts.len() as u32, 0..1);
                }
            }
        }

        self.queue.submit(std::iter::once(encoder.finish()));
        output.present();
    }

    fn ensure_tab_glyph_in_atlas(&mut self, c: char) -> GlyphRegion {
        if let Some(&region) = self.tab_glyph_regions.get(&c) {
            return region;
        }

        let glyph = self.tab_atlas.get_or_insert(c);
        let w = glyph.width;
        let h = glyph.height;

        if w == 0 || h == 0 {
            let region = GlyphRegion {
                u0: 0.0, v0: 0.0, u1: 0.0, v1: 0.0,
                width: 0, height: 0, offset_x: 0.0, offset_y: 0.0,
            };
            self.tab_glyph_regions.insert(c, region);
            return region;
        }

        if self.atlas_cursor_x + w > self.glyph_texture_size {
            self.atlas_cursor_x = 0;
            self.atlas_cursor_y += self.atlas_row_height;
            self.atlas_row_height = 0;
        }

        // If the glyph would overflow the texture vertically, reset the atlas
        if self.atlas_cursor_y + h > self.glyph_texture_size {
            self.glyph_regions.clear();
            self.tab_glyph_regions.clear();
            self.atlas_cursor_x = 0;
            self.atlas_cursor_y = 0;
            self.atlas_row_height = 0;
        }

        let x = self.atlas_cursor_x;
        let y = self.atlas_cursor_y;
        self.atlas_cursor_x += w;
        self.atlas_row_height = self.atlas_row_height.max(h);

        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.glyph_texture,
                mip_level: 0,
                origin: wgpu::Origin3d { x, y, z: 0 },
                aspect: wgpu::TextureAspect::All,
            },
            &glyph.bitmap,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(w),
                rows_per_image: None,
            },
            wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        );

        let ts = self.glyph_texture_size as f32;
        let region = GlyphRegion {
            u0: x as f32 / ts, v0: y as f32 / ts,
            u1: (x + w) as f32 / ts, v1: (y + h) as f32 / ts,
            width: w, height: h,
            offset_x: glyph.offset_x, offset_y: glyph.offset_y,
        };
        self.tab_glyph_regions.insert(c, region);
        region
    }

    fn ensure_glyph_in_atlas(&mut self, c: char, bold: bool) -> GlyphRegion {
        let cache_key = (c, bold);
        if let Some(&region) = self.glyph_regions.get(&cache_key) {
            return region;
        }

        let glyph = if bold {
            self.atlas.get_or_insert_bold(c)
        } else {
            self.atlas.get_or_insert(c)
        };
        let w = glyph.width;
        let h = glyph.height;

        if w == 0 || h == 0 {
            let region = GlyphRegion {
                u0: 0.0,
                v0: 0.0,
                u1: 0.0,
                v1: 0.0,
                width: 0,
                height: 0,
                offset_x: 0.0,
                offset_y: 0.0,
            };
            self.glyph_regions.insert(cache_key, region);
            return region;
        }

        // Advance cursor, wrap to next row if needed
        if self.atlas_cursor_x + w > self.glyph_texture_size {
            self.atlas_cursor_x = 0;
            self.atlas_cursor_y += self.atlas_row_height;
            self.atlas_row_height = 0;
        }

        // If the glyph would overflow the texture vertically, reset the atlas
        if self.atlas_cursor_y + h > self.glyph_texture_size {
            self.glyph_regions.clear();
            self.tab_glyph_regions.clear();
            self.atlas_cursor_x = 0;
            self.atlas_cursor_y = 0;
            self.atlas_row_height = 0;
        }

        let x = self.atlas_cursor_x;
        let y = self.atlas_cursor_y;
        self.atlas_cursor_x += w;
        self.atlas_row_height = self.atlas_row_height.max(h);

        // Upload to GPU texture
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.glyph_texture,
                mip_level: 0,
                origin: wgpu::Origin3d { x, y, z: 0 },
                aspect: wgpu::TextureAspect::All,
            },
            &glyph.bitmap,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(w),
                rows_per_image: None,
            },
            wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
        );

        let ts = self.glyph_texture_size as f32;
        let region = GlyphRegion {
            u0: x as f32 / ts,
            v0: y as f32 / ts,
            u1: (x + w) as f32 / ts,
            v1: (y + h) as f32 / ts,
            width: w,
            height: h,
            offset_x: glyph.offset_x,
            offset_y: glyph.offset_y,
        };
        self.glyph_regions.insert(cache_key, region);
        region
    }
}

fn rgb_to_f32(rgb: Rgb) -> [f32; 3] {
    [
        rgb.r as f32 / 255.0,
        rgb.g as f32 / 255.0,
        rgb.b as f32 / 255.0,
    ]
}

fn rgb_to_f32a(rgb: Rgb) -> [f32; 4] {
    let [r, g, b] = rgb_to_f32(rgb);
    [r, g, b, 1.0]
}

fn push_bg_rect(bg_verts: &mut Vec<BgVertex>, x: f32, y: f32, w: f32, h: f32, color: [f32; 4]) {
    bg_verts.push(BgVertex {
        position: [x, y],
        color,
    });
    bg_verts.push(BgVertex {
        position: [x + w, y],
        color,
    });
    bg_verts.push(BgVertex {
        position: [x, y + h],
        color,
    });
    bg_verts.push(BgVertex {
        position: [x + w, y],
        color,
    });
    bg_verts.push(BgVertex {
        position: [x + w, y + h],
        color,
    });
    bg_verts.push(BgVertex {
        position: [x, y + h],
        color,
    });
}

fn push_block_element_rects(
    bg_verts: &mut Vec<BgVertex>,
    ch: char,
    cx: f32,
    cy: f32,
    cell_w: f32,
    cell_h: f32,
    fg: [f32; 4],
) -> bool {
    let hw = cell_w / 2.0;
    let hh = cell_h / 2.0;
    match ch {
        '\u{2588}' => push_bg_rect(bg_verts, cx, cy, cell_w, cell_h, fg), // █
        '\u{2580}' => push_bg_rect(bg_verts, cx, cy, cell_w, hh, fg),     // ▀
        '\u{2584}' => push_bg_rect(bg_verts, cx, cy + hh, cell_w, hh, fg), // ▄
        '\u{258C}' => push_bg_rect(bg_verts, cx, cy, hw, cell_h, fg),     // ▌
        '\u{2590}' => push_bg_rect(bg_verts, cx + hw, cy, hw, cell_h, fg), // ▐
        '\u{259B}' => {
            // ▛ upper half + lower-left
            push_bg_rect(bg_verts, cx, cy, cell_w, hh, fg);
            push_bg_rect(bg_verts, cx, cy + hh, hw, hh, fg);
        }
        '\u{259C}' => {
            // ▜ upper half + lower-right
            push_bg_rect(bg_verts, cx, cy, cell_w, hh, fg);
            push_bg_rect(bg_verts, cx + hw, cy + hh, hw, hh, fg);
        }
        '\u{2599}' => {
            // ▙ lower half + upper-left
            push_bg_rect(bg_verts, cx, cy, hw, hh, fg);
            push_bg_rect(bg_verts, cx, cy + hh, cell_w, hh, fg);
        }
        '\u{259F}' => {
            // ▟ lower half + upper-right
            push_bg_rect(bg_verts, cx + hw, cy, hw, hh, fg);
            push_bg_rect(bg_verts, cx, cy + hh, cell_w, hh, fg);
        }
        '\u{2598}' => push_bg_rect(bg_verts, cx, cy, hw, hh, fg), // ▘
        '\u{259D}' => push_bg_rect(bg_verts, cx + hw, cy, hw, hh, fg), // ▝
        '\u{2596}' => push_bg_rect(bg_verts, cx, cy + hh, hw, hh, fg), // ▖
        '\u{2597}' => push_bg_rect(bg_verts, cx + hw, cy + hh, hw, hh, fg), // ▗
        '\u{259A}' => {
            // ▚ upper-left + lower-right
            push_bg_rect(bg_verts, cx, cy, hw, hh, fg);
            push_bg_rect(bg_verts, cx + hw, cy + hh, hw, hh, fg);
        }
        '\u{259E}' => {
            // ▞ upper-right + lower-left
            push_bg_rect(bg_verts, cx + hw, cy, hw, hh, fg);
            push_bg_rect(bg_verts, cx, cy + hh, hw, hh, fg);
        }
        _ => return false,
    }
    true
}

#[derive(Clone, Copy, PartialEq)]
enum LineWeight {
    None,
    Light,
    Heavy,
    Double,
}

struct BoxSegments {
    left: bool,
    right: bool,
    up: bool,
    down: bool,
    h_weight: LineWeight,
    v_weight: LineWeight,
}

fn box_drawing_segments(ch: char) -> Option<BoxSegments> {
    use LineWeight::*;
    let s = |left, right, up, down, h: LineWeight, v: LineWeight| {
        Some(BoxSegments {
            left,
            right,
            up,
            down,
            h_weight: h,
            v_weight: v,
        })
    };
    match ch {
        // Light lines
        '\u{2500}' => s(true, true, false, false, Light, None), // ─
        '\u{2502}' => s(false, false, true, true, None, Light), // │
        '\u{250C}' => s(false, true, false, true, Light, Light), // ┌
        '\u{2510}' => s(true, false, false, true, Light, Light), // ┐
        '\u{2514}' => s(false, true, true, false, Light, Light), // └
        '\u{2518}' => s(true, false, true, false, Light, Light), // ┘
        '\u{251C}' => s(false, true, true, true, Light, Light), // ├
        '\u{2524}' => s(true, false, true, true, Light, Light), // ┤
        '\u{252C}' => s(true, true, false, true, Light, Light), // ┬
        '\u{2534}' => s(true, true, true, false, Light, Light), // ┴
        '\u{253C}' => s(true, true, true, true, Light, Light),  // ┼
        // Heavy lines
        '\u{2501}' => s(true, true, false, false, Heavy, None), // ━
        '\u{2503}' => s(false, false, true, true, None, Heavy), // ┃
        '\u{250F}' => s(false, true, false, true, Heavy, Heavy), // ┏
        '\u{2513}' => s(true, false, false, true, Heavy, Heavy), // ┓
        '\u{2517}' => s(false, true, true, false, Heavy, Heavy), // ┗
        '\u{251B}' => s(true, false, true, false, Heavy, Heavy), // ┛
        '\u{2523}' => s(false, true, true, true, Heavy, Heavy), // ┣
        '\u{252B}' => s(true, false, true, true, Heavy, Heavy), // ┫
        '\u{2533}' => s(true, true, false, true, Heavy, Heavy), // ┳
        '\u{253B}' => s(true, true, true, false, Heavy, Heavy), // ┻
        '\u{254B}' => s(true, true, true, true, Heavy, Heavy),  // ╋
        // Double lines
        '\u{2550}' => s(true, true, false, false, Double, None), // ═
        '\u{2551}' => s(false, false, true, true, None, Double), // ║
        '\u{2554}' => s(false, true, false, true, Double, Double), // ╔
        '\u{2557}' => s(true, false, false, true, Double, Double), // ╗
        '\u{255A}' => s(false, true, true, false, Double, Double), // ╚
        '\u{255D}' => s(true, false, true, false, Double, Double), // ╝
        '\u{2560}' => s(false, true, true, true, Double, Double), // ╠
        '\u{2563}' => s(true, false, true, true, Double, Double), // ╣
        '\u{2566}' => s(true, true, false, true, Double, Double), // ╦
        '\u{2569}' => s(true, true, true, false, Double, Double), // ╩
        '\u{256C}' => s(true, true, true, true, Double, Double), // ╬
        // Rounded corners (light)
        '\u{256D}' => s(false, true, false, true, Light, Light), // ╭
        '\u{256E}' => s(true, false, false, true, Light, Light), // ╮
        '\u{256F}' => s(true, false, true, false, Light, Light), // ╯
        '\u{2570}' => s(false, true, true, false, Light, Light), // ╰
        _ => Option::None,
    }
}

fn prioritized_glyph_command_indices(commands: &[RenderCommand]) -> Vec<usize> {
    let mut indexed: Vec<(usize, u16)> = commands
        .iter()
        .enumerate()
        .filter(|(_, cmd)| cmd.character != ' ' && !cmd.flags.contains(CellFlags::HIDDEN))
        .map(|(idx, cmd)| (idx, cmd.row))
        .collect();
    indexed.sort_by(|(lhs_idx, lhs_row), (rhs_idx, rhs_row)| {
        rhs_row
            .cmp(lhs_row)
            .then_with(|| lhs_idx.cmp(rhs_idx))
    });
    indexed.into_iter().map(|(idx, _)| idx).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use growterm_types::RenderCommand;

    fn command(row: u16, col: u16, character: char) -> RenderCommand {
        RenderCommand {
            col,
            row,
            character,
            fg: Rgb::new(255, 255, 255),
            bg: Rgb::new(0, 0, 0),
            flags: CellFlags::empty(),
        }
    }

    #[test]
    fn supported_block_element_uses_rect_path() {
        let mut vertices = Vec::new();
        let handled = push_block_element_rects(
            &mut vertices,
            '\u{2588}', // █
            0.0,
            0.0,
            10.0,
            20.0,
            [1.0, 1.0, 1.0, 1.0],
        );
        assert!(handled);
        assert_eq!(vertices.len(), 6);
    }

    #[test]
    fn unsupported_block_element_falls_back_to_glyph_path() {
        let mut vertices = Vec::new();
        let handled = push_block_element_rects(
            &mut vertices,
            '\u{2585}', // ▅ (currently not in rect mapping)
            0.0,
            0.0,
            10.0,
            20.0,
            [1.0, 1.0, 1.0, 1.0],
        );
        assert!(!handled);
        assert!(vertices.is_empty());
    }

    #[test]
    fn prioritized_glyph_commands_prefer_lower_rows() {
        let commands = vec![
            command(0, 0, 'A'),
            command(4, 0, 'B'),
            command(2, 0, 'C'),
            command(4, 1, 'D'),
        ];

        let order = prioritized_glyph_command_indices(&commands);

        assert_eq!(order, vec![1, 3, 2, 0]);
    }

    #[test]
    fn prioritized_glyph_commands_skip_blank_and_hidden_cells() {
        let mut hidden = command(5, 0, 'X');
        hidden.flags = CellFlags::HIDDEN;
        let commands = vec![command(1, 0, ' '), hidden, command(3, 0, 'P')];

        let order = prioritized_glyph_command_indices(&commands);

        assert_eq!(order, vec![2]);
    }

    #[test]
    fn preferred_surface_alpha_mode_uses_postmultiplied_when_available() {
        let available = [
            wgpu::CompositeAlphaMode::Opaque,
            wgpu::CompositeAlphaMode::PostMultiplied,
        ];

        let mode = preferred_surface_alpha_mode(&available);

        assert_eq!(mode, wgpu::CompositeAlphaMode::PostMultiplied);
    }

    #[test]
    fn preferred_surface_alpha_mode_falls_back_to_first_available_mode() {
        let available = [wgpu::CompositeAlphaMode::Opaque];

        let mode = preferred_surface_alpha_mode(&available);

        assert_eq!(mode, wgpu::CompositeAlphaMode::Opaque);
    }

}
