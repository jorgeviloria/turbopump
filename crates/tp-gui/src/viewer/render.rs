//! Off-screen rendering of the 3D view with wgpu.
//!
//! The scene is drawn into a multisampled texture, resolved into a plain Rgba8Unorm
//! texture and handed to egui as an image, so the view needs nothing from egui's own
//! render pass (no depth buffer, no MSAA).
//!
//! Where the cut runs through a closed volume it is capped, with the stencil buffer:
//! every face of the volume left by the cut flips the stencil of the pixels it covers,
//! whatever its depth, so that the pixels where the plane lies inside the volume end
//! odd; the plane is drawn there, and clears the stencil for the next volume.

use eframe::egui;
use eframe::egui_wgpu::RenderState;
use eframe::wgpu::{self, util::DeviceExt};

use super::mesh::{Cap, MAX_CAPS, MeshData, Vertex};

const COLOR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth24PlusStencil8;
const SAMPLES: u32 = 4;

#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct Uniforms {
    pub view_projection: [[f32; 4]; 4],
    /// The eye, or with w = 0 the direction toward it in a parallel view.
    pub eye: [f32; 4],
    /// Directions toward the key and fill lights, world space.
    pub key_light: [f32; 4],
    pub fill_light: [f32; 4],
    /// Screen up direction, world space.
    pub up: [f32; 4],
    /// Gamma-encoded background at the top and at the bottom of the view.
    pub sky: [f32; 4],
    pub ground: [f32; 4],
    /// The cutting plane, normal · p = w; a zero normal when the model is whole.
    pub clip: [f32; 4],
    /// Gamma-encoded tint of highlighted parts.
    pub accent: [f32; 4],
    /// Centre and radius of the model, which the caps cover; filled in by the GPU side.
    pub cap: [f32; 4],
    /// Linear color of each cap.
    pub cap_colors: [[f32; 4]; MAX_CAPS],
    /// x: zebra stripes instead of shading when 1; y: stripes over a half turn.
    pub style: [f32; 4],
}

/// A part to tint, by its vertices.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Highlight {
    pub range: std::ops::Range<u32>,
    /// Selected, rather than only under the pointer.
    pub selected: bool,
}

struct GpuMesh {
    vertices: Option<wgpu::Buffer>,
    opaque: u32,
    total: u32,
    lines: Option<wgpu::Buffer>,
    line_vertices: u32,
    closures: Option<wgpu::Buffer>,
    caps: Vec<Cap>,
    /// Centre and radius of the model.
    bounds: [f32; 4],
}

struct Target {
    size: [u32; 2],
    color: wgpu::TextureView,
    depth: wgpu::TextureView,
    resolved: wgpu::TextureView,
}

/// GPU resources of the 3D view.
pub(crate) struct Gpu {
    background: wgpu::RenderPipeline,
    opaque: wgpu::RenderPipeline,
    transparent: wgpu::RenderPipeline,
    lines: wgpu::RenderPipeline,
    highlight: wgpu::RenderPipeline,
    mark: wgpu::RenderPipeline,
    cap: wgpu::RenderPipeline,
    uniforms: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    mesh: Option<GpuMesh>,
    target: Option<Target>,
    texture: Option<egui::TextureId>,
}

impl std::fmt::Debug for Gpu {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Gpu")
            .field("size", &self.target.as_ref().map(|t| t.size))
            .finish_non_exhaustive()
    }
}

/// How a pipeline draws.
#[derive(Clone, Copy, PartialEq)]
enum Kind {
    /// The gradient behind the model: no vertices, no depth.
    Background,
    Opaque,
    Transparent,
    Lines,
    /// A part again, tinted.
    Highlight,
    /// A closed volume, counted into the stencil and not drawn.
    Mark,
    /// The cutting plane where the stencil is odd.
    Cap,
}

fn pipeline(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    shader: &wgpu::ShaderModule,
    label: &str,
    kind: Kind,
) -> wgpu::RenderPipeline {
    let attributes = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x4];
    let vertex_layout = [Some(wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<Vertex>() as wgpu::BufferAddress,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &attributes,
    })];
    let (vertex, fragment, topology) = match kind {
        Kind::Background => (
            "vs_background",
            "fs_background",
            wgpu::PrimitiveTopology::TriangleList,
        ),
        Kind::Opaque | Kind::Transparent => {
            ("vs_main", "fs_main", wgpu::PrimitiveTopology::TriangleList)
        }
        Kind::Lines => ("vs_line", "fs_line", wgpu::PrimitiveTopology::LineList),
        Kind::Highlight => (
            "vs_highlight",
            "fs_highlight",
            wgpu::PrimitiveTopology::TriangleList,
        ),
        Kind::Mark => ("vs_main", "fs_mark", wgpu::PrimitiveTopology::TriangleList),
        Kind::Cap => ("vs_cap", "fs_cap", wgpu::PrimitiveTopology::TriangleList),
    };
    let stencil = |compare, pass_op| {
        let face = wgpu::StencilFaceState {
            compare,
            fail_op: wgpu::StencilOperation::Keep,
            depth_fail_op: pass_op,
            pass_op,
        };
        wgpu::StencilState {
            front: face,
            back: face,
            read_mask: 0xff,
            write_mask: 0xff,
        }
    };
    let transparent = matches!(kind, Kind::Transparent | Kind::Highlight);
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(label),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some(vertex),
            compilation_options: Default::default(),
            buffers: if matches!(kind, Kind::Background | Kind::Cap) {
                &[]
            } else {
                &vertex_layout
            },
        },
        primitive: wgpu::PrimitiveState {
            topology,
            cull_mode: None,
            ..Default::default()
        },
        depth_stencil: Some(wgpu::DepthStencilState {
            format: DEPTH_FORMAT,
            depth_write_enabled: Some(matches!(kind, Kind::Opaque | Kind::Lines | Kind::Cap)),
            depth_compare: Some(if matches!(kind, Kind::Background | Kind::Mark) {
                wgpu::CompareFunction::Always
            } else {
                wgpu::CompareFunction::LessEqual
            }),
            stencil: match kind {
                Kind::Mark => stencil(
                    wgpu::CompareFunction::Always,
                    wgpu::StencilOperation::Invert,
                ),
                // Odd pixels only, cleared whether the plane shows there or not.
                Kind::Cap => stencil(
                    wgpu::CompareFunction::NotEqual,
                    wgpu::StencilOperation::Zero,
                ),
                _ => Default::default(),
            },
            bias: Default::default(),
        }),
        multisample: wgpu::MultisampleState {
            count: SAMPLES,
            ..Default::default()
        },
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some(fragment),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: COLOR_FORMAT,
                blend: transparent.then_some(wgpu::BlendState::ALPHA_BLENDING),
                write_mask: if kind == Kind::Mark {
                    wgpu::ColorWrites::empty()
                } else {
                    wgpu::ColorWrites::ALL
                },
            })],
        }),
        multiview_mask: None,
        cache: None,
    })
}

impl Gpu {
    pub fn new(device: &wgpu::Device) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("impeller shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()),
        });
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("impeller uniforms"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("impeller pipeline layout"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("impeller uniforms"),
            size: std::mem::size_of::<Uniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("impeller uniforms"),
            layout: &bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniforms.as_entire_binding(),
            }],
        });
        Self {
            background: pipeline(device, &layout, &shader, "background", Kind::Background),
            opaque: pipeline(device, &layout, &shader, "opaque", Kind::Opaque),
            transparent: pipeline(device, &layout, &shader, "transparent", Kind::Transparent),
            lines: pipeline(device, &layout, &shader, "lines", Kind::Lines),
            highlight: pipeline(device, &layout, &shader, "highlight", Kind::Highlight),
            mark: pipeline(device, &layout, &shader, "cap mark", Kind::Mark),
            cap: pipeline(device, &layout, &shader, "cap", Kind::Cap),
            uniforms,
            bind_group,
            mesh: None,
            target: None,
            texture: None,
        }
    }

    pub fn upload(&mut self, device: &wgpu::Device, mesh: &MeshData) {
        let buffer = |label, data: &[Vertex]| {
            (!data.is_empty()).then(|| {
                device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some(label),
                    contents: bytemuck::cast_slice(data),
                    usage: wgpu::BufferUsages::VERTEX,
                })
            })
        };
        self.mesh = Some(GpuMesh {
            vertices: buffer("impeller triangles", &mesh.vertices),
            opaque: mesh.opaque,
            total: mesh.vertices.len() as u32,
            lines: buffer("impeller lines", &mesh.lines),
            line_vertices: mesh.lines.len() as u32,
            closures: buffer("impeller closed volumes", &mesh.closures),
            caps: mesh.caps.clone(),
            bounds: mesh.center.extend(mesh.radius).to_array(),
        });
    }

    fn ensure_target(&mut self, state: &RenderState, size: [u32; 2]) -> egui::TextureId {
        let device = &state.device;
        if self.target.as_ref().is_none_or(|t| t.size != size) {
            let target = Target::new(device, size, wgpu::TextureUsages::TEXTURE_BINDING).1;
            let mut renderer = state.renderer.write();
            match self.texture {
                Some(id) => renderer.update_egui_texture_from_wgpu_texture(
                    device,
                    &target.resolved,
                    wgpu::FilterMode::Linear,
                    id,
                ),
                None => {
                    self.texture = Some(renderer.register_native_texture(
                        device,
                        &target.resolved,
                        wgpu::FilterMode::Linear,
                    ));
                }
            }
            self.target = Some(target);
        }
        self.texture.expect("registered above")
    }

    /// Draws the uploaded mesh at `size` pixels and returns the egui texture with the
    /// image.
    pub fn render(
        &mut self,
        state: &RenderState,
        size: [u32; 2],
        uniforms: &Uniforms,
        highlights: &[Highlight],
    ) -> egui::TextureId {
        let texture = self.ensure_target(state, size);
        let target = self.target.as_ref().expect("created above");
        let encoder = self.draw(state, target, uniforms, highlights);
        state.queue.submit([encoder.finish()]);
        texture
    }

    /// Draws the uploaded mesh at `size` pixels into a texture of its own and reads it
    /// back: RGBA, row by row. `None` when the GPU fails to hand it over.
    pub fn capture(
        &mut self,
        state: &RenderState,
        size: [u32; 2],
        uniforms: &Uniforms,
    ) -> Option<Vec<u8>> {
        let device = &state.device;
        let (image, target) = Target::new(device, size, wgpu::TextureUsages::COPY_SRC);
        let mut encoder = self.draw(state, &target, uniforms, &[]);
        let row = 4 * size[0] as usize;
        let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT as usize;
        let padded = row.div_ceil(align) * align;
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("3d capture"),
            size: (padded * size[1] as usize) as u64,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        encoder.copy_texture_to_buffer(
            image.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded as u32),
                    rows_per_image: None,
                },
            },
            image.size(),
        );
        let submitted = state.queue.submit([encoder.finish()]);
        let (sender, receiver) = std::sync::mpsc::channel();
        buffer
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |r| drop(sender.send(r)));
        device
            .poll(wgpu::PollType::Wait {
                submission_index: Some(submitted),
                timeout: Some(std::time::Duration::from_secs(10)),
            })
            .ok()?;
        receiver.recv().ok()?.ok()?;
        let data = buffer.slice(..).get_mapped_range().ok()?;
        Some(
            data.chunks_exact(padded)
                .flat_map(|r| &r[..row])
                .copied()
                .collect(),
        )
    }

    /// Records the background, the mesh and the tints of `highlights` into `target`.
    fn draw(
        &self,
        state: &RenderState,
        target: &Target,
        uniforms: &Uniforms,
        highlights: &[Highlight],
    ) -> wgpu::CommandEncoder {
        let mut uniforms = *uniforms;
        if let Some(mesh) = &self.mesh {
            uniforms.cap = mesh.bounds;
            for (color, cap) in uniforms.cap_colors.iter_mut().zip(&mesh.caps) {
                *color = cap.color;
            }
        }
        let [nx, ny, nz, _] = uniforms.clip;
        let cut = nx * nx + ny * ny + nz * nz > 0.25;
        state
            .queue
            .write_buffer(&self.uniforms, 0, bytemuck::bytes_of(&uniforms));
        let mut encoder = state
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("3d view"),
            });
        {
            let [r, g, b, _] = uniforms.ground;
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("3d view"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &target.color,
                    depth_slice: None,
                    resolve_target: Some(&target.resolved),
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: f64::from(r),
                            g: f64::from(g),
                            b: f64::from(b),
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Discard,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &target.depth,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Discard,
                    }),
                    stencil_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(0),
                        store: wgpu::StoreOp::Discard,
                    }),
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_bind_group(0, &self.bind_group, &[]);
            pass.set_pipeline(&self.background);
            pass.draw(0..3, 0..1);
            if let Some(mesh) = &self.mesh {
                if let Some(vertices) = &mesh.vertices {
                    pass.set_vertex_buffer(0, vertices.slice(..));
                    if mesh.opaque > 0 {
                        pass.set_pipeline(&self.opaque);
                        pass.draw(0..mesh.opaque, 0..1);
                    }
                }
                // Caps over the opaque parts, one closed volume at a time.
                if cut && let Some(closures) = &mesh.closures {
                    pass.set_vertex_buffer(0, closures.slice(..));
                    for (k, cap) in (0u32..).zip(&mesh.caps) {
                        pass.set_pipeline(&self.mark);
                        pass.draw(cap.range.clone(), 0..1);
                        pass.set_pipeline(&self.cap);
                        pass.draw(0..6, k..k + 1);
                    }
                }
                // Lines before glass, so see-through parts tint the lines behind them.
                if let Some(lines) = &mesh.lines {
                    pass.set_pipeline(&self.lines);
                    pass.set_vertex_buffer(0, lines.slice(..));
                    pass.draw(0..mesh.line_vertices, 0..1);
                }
                if let Some(vertices) = &mesh.vertices
                    && mesh.total > mesh.opaque
                {
                    pass.set_pipeline(&self.transparent);
                    pass.set_vertex_buffer(0, vertices.slice(..));
                    pass.draw(mesh.opaque..mesh.total, 0..1);
                }
                if let Some(vertices) = &mesh.vertices
                    && !highlights.is_empty()
                {
                    pass.set_pipeline(&self.highlight);
                    pass.set_vertex_buffer(0, vertices.slice(..));
                    for h in highlights {
                        let instance = u32::from(h.selected);
                        pass.draw(h.range.clone(), instance..instance + 1);
                    }
                }
            }
        }
        encoder
    }
}

impl Target {
    /// Multisampled color and depth of `size` pixels, resolved into an image that can
    /// also be used as `usage`. Returns that image's texture too.
    fn new(
        device: &wgpu::Device,
        size: [u32; 2],
        usage: wgpu::TextureUsages,
    ) -> (wgpu::Texture, Self) {
        let texture = |label, format, samples, usage| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d {
                    width: size[0],
                    height: size[1],
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: samples,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage,
                view_formats: &[],
            })
        };
        let attachment = wgpu::TextureUsages::RENDER_ATTACHMENT;
        let view = |t: wgpu::Texture| t.create_view(&wgpu::TextureViewDescriptor::default());
        let image = texture("3d image", COLOR_FORMAT, 1, attachment | usage);
        let target = Target {
            size,
            color: view(texture("3d color", COLOR_FORMAT, SAMPLES, attachment)),
            depth: view(texture("3d depth", DEPTH_FORMAT, SAMPLES, attachment)),
            resolved: image.create_view(&wgpu::TextureViewDescriptor::default()),
        };
        (image, target)
    }
}
