//! Building the 3D renderer: its bind group layouts, render, shadow and conversion pipelines.
use crate::*;

impl Engine {
    pub fn new(device: &wgpu::Device) -> Self {
        Self::with_cache(device, None)
    }
    /// As [`Engine::new`], compiling its programs into `cache` (the renderer's, from
    /// `Renderer::pipeline_cache`) so they are kept between runs.
    pub fn with_cache(device: &wgpu::Device, cache: Option<&wgpu::PipelineCache>) -> Self {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("3D scene"),
            source: wgpu::ShaderSource::Wgsl(
                format!(
                    "{}{}",
                    include_str!("scene.wgsl"),
                    veylvik_render::ENVIRONMENT_WGSL
                )
                .into(),
            ),
        });
        let storage = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only: true },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let shadow_texture = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Depth,
                view_dimension: wgpu::TextureViewDimension::D2Array,
                multisampled: false,
            },
            count: None,
        };
        let ambient_uniform = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let shadow_compare = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Comparison),
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("3D object"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                storage(1),
                storage(2),
                storage(3),
                shadow_texture(4),
                shadow_compare(5),
                shadow_texture(6),
                ambient_uniform(7),
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("3D object"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let attributes = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x2];
        let pipeline = |entry: &str, blend: Option<wgpu::BlendState>, write: bool| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(entry),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &module,
                    entry_point: Some("vertex"),
                    compilation_options: Default::default(),
                    buffers: &[wgpu::VertexBufferLayout {
                        array_stride: 32,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &attributes,
                    }],
                },
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH,
                    depth_write_enabled: Some(write),
                    depth_compare: Some(wgpu::CompareFunction::Greater),
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample: wgpu::MultisampleState {
                    count: SAMPLES,
                    ..Default::default()
                },
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some(entry),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: COLOUR,
                        blend,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache,
            })
        };
        let lit = pipeline("lit", None, true);
        let catcher = pipeline(
            "catcher",
            Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
            false,
        );
        let shadow_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("3D shadow depth"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shadow.wgsl").into()),
        });
        let shadow_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("3D shadow depth"),
            layout: None,
            vertex: wgpu::VertexState {
                module: &shadow_module,
                entry_point: Some("shadow_vertex"),
                compilation_options: Default::default(),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: 32,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &attributes,
                }],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: Some(wgpu::DepthStencilState {
                format: shadow::FORMAT,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::Less),
                stencil: Default::default(),
                bias: wgpu::DepthBiasState {
                    constant: 2,
                    slope_scale: 2.0,
                    clamp: 0.0,
                },
            }),
            multisample: Default::default(),
            fragment: None,
            multiview_mask: None,
            cache,
        });
        let card = pipeline(
            "card",
            Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
            false,
        );
        // In a depth picture a card's solid parts stand in front of what is behind.
        let card_solid = pipeline("card_solid", None, true);
        // Library looks: the material kernels' bindings in group 0, the object in 1.
        let entry = |binding, ty, writable: bool| wgpu::BindGroupLayoutEntry {
            binding,
            // Only the object uniform (group 1) feeds the vertex stage.
            visibility: if ty == 0 && binding == 0 && !writable {
                wgpu::ShaderStages::VERTEX_FRAGMENT
            } else {
                wgpu::ShaderStages::FRAGMENT
            },
            ty: match ty {
                0 => wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                _ => wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage {
                        read_only: !writable,
                    },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
            },
            count: None,
        };
        let material_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("3D look material"),
            entries: &[
                entry(0, 0, false),
                entry(1, 1, false),
                entry(2, 1, false),
                entry(3, 1, true),
                entry(4, 1, false),
                entry(5, 1, false),
                entry(6, 0, false),
                entry(7, 1, false),
                entry(8, 1, false),
            ],
        });
        let look_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("3D look object"),
            entries: &[
                entry(0, 0, false),
                entry(1, 1, false),
                shadow_texture(2),
                shadow_compare(3),
                shadow_texture(4),
                ambient_uniform(5),
            ],
        });
        let look_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("3D looks"),
            source: wgpu::ShaderSource::Wgsl(
                format!(
                    "{}{}",
                    veylvik_render::MATERIAL_WGSL,
                    include_str!("look.wgsl")
                )
                .into(),
            ),
        });
        let look_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("3D looks"),
            bind_group_layouts: &[Some(&material_layout), Some(&look_layout)],
            immediate_size: 0,
        });
        let look = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("3D looks"),
            layout: Some(&look_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &look_module,
                entry_point: Some("look_vertex"),
                compilation_options: Default::default(),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: 32,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &attributes,
                }],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::Greater),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: wgpu::MultisampleState {
                count: SAMPLES,
                ..Default::default()
            },
            fragment: Some(wgpu::FragmentState {
                module: &look_module,
                entry_point: Some("look_surface"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: COLOUR,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache,
        });
        let copy_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("3D resolve copy"),
            source: wgpu::ShaderSource::Wgsl(include_str!("copy.wgsl").into()),
        });
        let copy = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("3D resolve copy"),
            layout: None,
            module: &copy_module,
            entry_point: Some("copy"),
            compilation_options: Default::default(),
            cache,
        });
        let env_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("3D environment"),
            source: wgpu::ShaderSource::Wgsl(include_str!("env.wgsl").into()),
        });
        let compute = |entry: &str| {
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some(entry),
                layout: None,
                module: &env_module,
                entry_point: Some(entry),
                compilation_options: Default::default(),
                cache,
            })
        };
        let (resample, reduce, irradiance) = (
            compute("resample"),
            compute("reduce"),
            compute("irradiance"),
        );
        let sky_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("3D sky"),
            source: wgpu::ShaderSource::Wgsl(
                format!(
                    "{}{}",
                    include_str!("sky.wgsl"),
                    veylvik_render::ENVIRONMENT_WGSL
                )
                .into(),
            ),
        });
        let sky = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("3D sky"),
            layout: None,
            vertex: wgpu::VertexState {
                module: &sky_module,
                entry_point: Some("sky_vertex"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH,
                depth_write_enabled: Some(false),
                depth_compare: Some(wgpu::CompareFunction::Always),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: wgpu::MultisampleState {
                count: SAMPLES,
                ..Default::default()
            },
            fragment: Some(wgpu::FragmentState {
                module: &sky_module,
                entry_point: Some("sky_fragment"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: COLOUR,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache,
        });
        Self {
            resample,
            reduce,
            irradiance,
            panorama: Mutex::new(None),
            sky,
            catcher,
            shadow_pipeline,
            shadow_maps: Mutex::new(None),
            sky_maps: Mutex::new(None),
            models: Mutex::new(Default::default()),
            textures: Mutex::new(Default::default()),
            layout,
            material_layout,
            look_layout,
            look,
            lit,
            card,
            card_solid,
            copy,
            meshes: Mutex::new((HashMap::new(), 0)),
        }
    }
}
