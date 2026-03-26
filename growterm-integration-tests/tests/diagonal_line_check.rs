use growterm_types::{CellFlags, RenderCommand, Rgb};

const FONT_SIZE: f32 = 32.0;
const WIDTH: u32 = 800;
const HEIGHT: u32 = 600;

/// 실제 GPU 렌더링 후, 동일 색 셀 그리드 내부에 검은 픽셀(갭/빗금)이 있는지 검사.
/// 전체 화면을 채운 큰 그리드로 테스트한다.
#[test]
fn no_gap_between_same_color_cells() {
    let atlas = growterm_gpu_draw::GlyphAtlas::new(FONT_SIZE);
    let (cell_w, cell_h) = atlas.cell_size();
    let cols = (WIDTH as f32 / cell_w).floor() as u16;
    let rows = (HEIGHT as f32 / cell_h).floor() as u16;

    let white = Rgb { r: 255, g: 255, b: 255 };
    let black = Rgb { r: 0, g: 0, b: 0 };

    let mut commands = Vec::new();
    for row in 0..rows {
        for col in 0..cols {
            commands.push(RenderCommand {
                row,
                col,
                character: ' ',
                fg: black,
                bg: white,
                underline_color: None,
                flags: CellFlags::empty(),
            });
        }
    }

    eprintln!("cell_size=({cell_w}, {cell_h}), grid={cols}x{rows}");

    let pixels = render_headless(WIDTH, HEIGHT, cell_w, cell_h, &commands);

    let grid_w = (cols as f32 * cell_w).floor() as u32;
    let grid_h = (rows as f32 * cell_h).floor() as u32;

    let mut black_pixels = Vec::new();
    for y in 0..grid_h.min(HEIGHT) {
        for x in 0..grid_w.min(WIDTH) {
            let idx = ((y * WIDTH + x) * 4) as usize;
            let (r, g, b) = (pixels[idx], pixels[idx + 1], pixels[idx + 2]);
            if r < 128 && g < 128 && b < 128 {
                black_pixels.push((x, y, r, g, b));
            }
        }
    }

    // 셀 경계 부근 픽셀값 출력 (세로 경계)
    let boundary_x = cell_w.round() as u32; // 첫 번째 세로 경계
    eprintln!("--- 세로 경계 x={boundary_x} 부근 픽셀 (y=0..5) ---");
    for y in 0..5u32 {
        for dx in 0..3 {
            let x = boundary_x - 1 + dx;
            let idx = ((y * WIDTH + x) * 4) as usize;
            eprintln!(
                "  ({x},{y}): rgba=({},{},{},{})",
                pixels[idx], pixels[idx + 1], pixels[idx + 2], pixels[idx + 3]
            );
        }
    }

    // 가로 경계 부근
    let boundary_y = cell_h.round() as u32;
    eprintln!("--- 가로 경계 y={boundary_y} 부근 픽셀 (x=0..5) ---");
    for dy in 0..3 {
        let y = boundary_y - 1 + dy;
        for x in 0..5u32 {
            let idx = ((y * WIDTH + x) * 4) as usize;
            eprintln!(
                "  ({x},{y}): rgba=({},{},{},{})",
                pixels[idx], pixels[idx + 1], pixels[idx + 2], pixels[idx + 3]
            );
        }
    }

    assert!(
        black_pixels.is_empty(),
        "셀 경계에 어두운 갭(빗금) 발견! {}개 픽셀.\n처음 10개: {:?}\ncell_size=({cell_w}, {cell_h})",
        black_pixels.len(),
        &black_pixels[..black_pixels.len().min(10)]
    );
}

fn render_headless(
    width: u32,
    height: u32,
    cell_w: f32,
    cell_h: f32,
    commands: &[RenderCommand],
) -> Vec<u8> {
    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
        backends: wgpu::Backends::all(),
        ..Default::default()
    });

    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::default(),
        compatible_surface: None,
        force_fallback_adapter: false,
    }))
    .expect("GPU adapter를 찾을 수 없음");

    let (device, queue) = pollster::block_on(adapter.request_device(
        &wgpu::DeviceDescriptor {
            label: Some("headless"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::default(),
            ..Default::default()
        },
        None,
    ))
    .expect("GPU device 생성 실패");

    // 실제 앱과 동일: 텍스처는 sRGB, 렌더 뷰는 non-sRGB
    let texture_storage_format = wgpu::TextureFormat::Bgra8UnormSrgb;
    let texture_format = wgpu::TextureFormat::Bgra8Unorm;

    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("offscreen"),
        size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: texture_storage_format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[texture_format],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor {
        format: Some(texture_format), // non-sRGB 뷰 (앱과 동일)
        ..Default::default()
    });

    #[repr(C)]
    #[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
    struct Uniforms { screen_size: [f32; 2], _padding: [f32; 2] }

    let uniforms = Uniforms { screen_size: [width as f32, height as f32], _padding: [0.0; 2] };
    let uniform_buffer = wgpu::util::DeviceExt::create_buffer_init(&device, &wgpu::util::BufferInitDescriptor {
        label: Some("uniforms"),
        contents: bytemuck::bytes_of(&uniforms),
        usage: wgpu::BufferUsages::UNIFORM,
    });

    let uniform_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: None,
        entries: &[wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::VERTEX,
            ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
            count: None,
        }],
    });

    let uniform_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None, layout: &uniform_bgl,
        entries: &[wgpu::BindGroupEntry { binding: 0, resource: uniform_buffer.as_entire_binding() }],
    });

    let shader_src = "
struct Uniforms { screen_size: vec2<f32> };
@group(0) @binding(0) var<uniform> uniforms: Uniforms;
struct VIn { @location(0) position: vec2<f32>, @location(1) color: vec3<f32> };
struct VOut { @builtin(position) clip_position: vec4<f32>, @location(0) color: vec3<f32> };
@vertex fn vs_main(in: VIn) -> VOut {
    var out: VOut;
    let x = in.position.x / uniforms.screen_size.x * 2.0 - 1.0;
    let y = 1.0 - in.position.y / uniforms.screen_size.y * 2.0;
    out.clip_position = vec4<f32>(x, y, 0.0, 1.0);
    out.color = in.color;
    return out;
}
@fragment fn fs_main(in: VOut) -> @location(0) vec4<f32> {
    return vec4<f32>(in.color, 1.0);
}
";
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: None, source: wgpu::ShaderSource::Wgsl(shader_src.into()),
    });

    #[repr(C)]
    #[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
    struct BgVertex { position: [f32; 2], color: [f32; 3] }

    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: None, bind_group_layouts: &[&uniform_bgl], push_constant_ranges: &[],
    });

    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: None,
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: &shader, entry_point: Some("vs_main"),
            buffers: &[wgpu::VertexBufferLayout {
                array_stride: std::mem::size_of::<BgVertex>() as u64,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x3],
            }],
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader, entry_point: Some("fs_main"),
            targets: &[Some(wgpu::ColorTargetState { format: texture_format, blend: None, write_mask: wgpu::ColorWrites::ALL })],
            compilation_options: Default::default(),
        }),
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview: None,
        cache: None,
    });

    let mut vertices: Vec<BgVertex> = Vec::new();
    for cmd in commands {
        let x = cmd.col as f32 * cell_w;
        let y = cmd.row as f32 * cell_h;
        let w = if cmd.flags.contains(CellFlags::WIDE_CHAR) { cell_w * 2.0 } else { cell_w };
        let color = [cmd.bg.r as f32 / 255.0, cmd.bg.g as f32 / 255.0, cmd.bg.b as f32 / 255.0];
        vertices.push(BgVertex { position: [x, y], color });
        vertices.push(BgVertex { position: [x + w, y], color });
        vertices.push(BgVertex { position: [x, y + cell_h], color });
        vertices.push(BgVertex { position: [x + w, y], color });
        vertices.push(BgVertex { position: [x + w, y + cell_h], color });
        vertices.push(BgVertex { position: [x, y + cell_h], color });
    }

    let vb = wgpu::util::DeviceExt::create_buffer_init(&device, &wgpu::util::BufferInitDescriptor {
        label: None, contents: bytemuck::cast_slice(&vertices), usage: wgpu::BufferUsages::VERTEX,
    });

    let mut encoder = device.create_command_encoder(&Default::default());
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: None,
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view, resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
            })],
            depth_stencil_attachment: None,
            ..Default::default()
        });
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &uniform_bg, &[]);
        pass.set_vertex_buffer(0, vb.slice(..));
        pass.draw(0..vertices.len() as u32, 0..1);
    }

    let bytes_per_row = (width * 4 + 255) & !255;
    let output_buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: (bytes_per_row * height) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });

    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo { texture: &texture, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
        wgpu::TexelCopyBufferInfo { buffer: &output_buffer, layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(bytes_per_row), rows_per_image: None } },
        wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
    );

    queue.submit(std::iter::once(encoder.finish()));

    let buffer_slice = output_buffer.slice(..);
    let (tx, rx) = std::sync::mpsc::channel();
    buffer_slice.map_async(wgpu::MapMode::Read, move |result| { tx.send(result).unwrap(); });
    device.poll(wgpu::Maintain::Wait);
    rx.recv().unwrap().unwrap();

    let data = buffer_slice.get_mapped_range();
    let mut pixels = vec![0u8; (width * height * 4) as usize];
    for y in 0..height {
        let src_offset = (y * bytes_per_row) as usize;
        let dst_offset = (y * width * 4) as usize;
        let row_bytes = (width * 4) as usize;
        pixels[dst_offset..dst_offset + row_bytes].copy_from_slice(&data[src_offset..src_offset + row_bytes]);
    }
    pixels
}
