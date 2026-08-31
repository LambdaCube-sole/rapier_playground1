mod texture;
mod model;
mod resource;
use std::sync::Arc;
use rapier3d::prelude::*;
use wgpu::{util::{BufferInitDescriptor, DeviceExt}, wgc::instance::GetSurfaceSupportError::FailedToRetrieveSurfaceCapabilitiesForAdapter};
use winit::{
    application::ApplicationHandler, event::*, event_loop::{ActiveEventLoop, EventLoop}, keyboard::{KeyCode, PhysicalKey}, window::Window
};
use rand::rngs::StdRng;
use rand::SeedableRng;
use rand::Rng;
use rand::prelude::IndexedRandom;

#[cfg(target_arch="wasm32")]
use web_time::Instant;

#[cfg(not(target_arch="wasm32"))]
use std::time::Instant;
use model::Vertex;
const NUM_MAX_OBJECT: usize = 3000;
const NUM_INSTANCE_PER_ROW: u32 = 10;
/*const INSTANCE_DISPLACEMENT: glam::Vec3 = glam::Vec3::new(
    NUM_INSTANCE_PER_ROW as f32 * 0.5,
    0.0,
    NUM_INSTANCE_PER_ROW as f32 * 0.5,
);*/

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;
#[cfg(target_arch = "wasm32")]
use winit::platform::web::EventLoopExtWebSys;
/* 
#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct Vertex {
    position: [f32; 3],
    tex_coords: [f32; 2],
}

impl Vertex {
    fn desc() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Vertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[wgpu::VertexAttribute {
                offset: 0,
                format: wgpu::VertexFormat::Float32x3,
                shader_location: 0,
            },
            wgpu::VertexAttribute {
                offset: std::mem::size_of::<[f32; 3]>() as wgpu::BufferAddress,
                format: wgpu::VertexFormat::Float32x2,
                shader_location: 1,
            },
            ],
        }
    }
}
*/
struct Camera {
    eye: glam::Vec3,
    target: glam::Vec3,
    up: glam::Vec3,
    aspect: f32,
    fovy: f32,
    znear: f32,
    zfar: f32,
}

impl Camera {
    fn build_view_projection_matrix(&self) -> glam::Mat4 {
        let view = glam::camera::rh::view::look_at_mat4(self.eye, self.target, self.up);
        let proj = glam::camera::rh::proj::directx::perspective(self.fovy.to_radians(), self.aspect, self.znear, self.zfar);
        return proj * view;
    }
}

// lib.rs
#[repr(C)]
#[derive(Debug, Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct LightUniform {
    position: [f32; 3],
    // Due to uniforms requiring 16 byte (4 float) spacing, we need to use a padding field here
    apply_contact: u32,
    color: [f32; 3],
    // Due to uniforms requiring 16 byte (4 float) spacing, we need to use a padding field here
    ball_count: u32,
    balls: [[f32; 4]; 3000],

}


#[repr(C)]
#[derive(Debug, Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct CameraUniform {
    view_position: [f32; 4],
    view_proj: [[f32; 4]; 4],
}

impl CameraUniform {
    fn new() -> Self {
        Self {
            view_position: [0.0; 4],
            view_proj: glam::Mat4::IDENTITY.to_cols_array_2d(),
        }
    }

    fn update_view_proj(&mut self, camera: &Camera) {
        self.view_position = camera.eye.to_homogeneous().into();
        self.view_proj = camera.build_view_projection_matrix().to_cols_array_2d()
    }
}

struct CameraController {
    speed: f32,
    is_forward_pressed: bool,
    is_backward_pressed: bool,
    is_left_pressed: bool,
    is_right_pressed: bool,
}

impl CameraController {
    fn new(speed: f32) -> Self {
        Self {
            speed,
            is_forward_pressed: false,
            is_backward_pressed: false,
            is_left_pressed: false,
            is_right_pressed: false,
        }
    }

    fn handle_key(&mut self, code: KeyCode, is_pressed: bool) -> bool {
        match code {
            KeyCode::KeyW | KeyCode::ArrowUp => {
                self.is_forward_pressed = is_pressed;
                true
            }
            KeyCode::KeyA | KeyCode::ArrowLeft => {
                self.is_left_pressed = is_pressed;
                true
            }
            KeyCode::KeyS | KeyCode::ArrowDown => {
                self.is_backward_pressed = is_pressed;
                true
            }
            KeyCode::KeyD | KeyCode::ArrowRight => {
                self.is_right_pressed = is_pressed;
                true
            }
            _ => false,
        }
    }

    fn update_camera(&self, camera: &mut Camera) {
        let forward = camera.target - camera.eye;
        let forward_norm = forward.normalize();
        let forward_mag = forward.length();

        if self.is_forward_pressed && forward_mag > self.speed {
            camera.eye += forward_norm * self.speed;
        }
        if self.is_backward_pressed {
            camera.eye -= forward_norm * self.speed;
        }

        let right = forward_norm.cross(camera.up);

        let forward = camera.target - camera.eye;
        let forward_mag = forward.length();

        if self.is_right_pressed {
            camera.eye =
                camera.target - (forward + right * self.speed).normalize() * forward_mag;
        }
        if self.is_left_pressed {
            camera.eye =
                camera.target - (forward - right * self.speed).normalize() * forward_mag;
        }
    }
}

struct Instance { 
    position: glam::Vec3,
    rotation: glam::Quat,
    color1: glam::Vec4,
    color2: glam::Vec4,
    pattern: Pattern
}

#[repr(u32)]
#[derive(Copy, Clone)]
enum Pattern {
    Solid,
    Checker,
    Stripe1,
    Stripe2,
    Bubble,
}

#[repr(C)]
#[derive(Copy,Clone,bytemuck::Pod,bytemuck::Zeroable)]
struct InstanceRaw {
    model: [[f32; 4]; 4],
    normal: [[f32; 3]; 3],
    color1: [f32; 4],
    color2: [f32; 4],
    pattern: u32,
}

impl InstanceRaw {
    fn desc() -> wgpu::VertexBufferLayout<'static> {
        use std::mem;
        wgpu::VertexBufferLayout {
            array_stride: mem::size_of::<InstanceRaw>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &[
                wgpu::VertexAttribute { //Mat1
                    offset:0,
                    shader_location: 5,
                    format: wgpu::VertexFormat::Float32x4,
                },
                wgpu::VertexAttribute { //Mat2
                    offset:mem::size_of::<[f32; 4]>() as wgpu::BufferAddress,
                    shader_location: 6,
                    format: wgpu::VertexFormat::Float32x4,
                },
                wgpu::VertexAttribute { //Mat3
                    offset: mem::size_of::<[f32;8]>() as wgpu::BufferAddress,
                    shader_location: 7,
                    format: wgpu::VertexFormat::Float32x4,
                },
                wgpu::VertexAttribute { //Mat4
                    offset: mem::size_of::<[f32; 12]>() as wgpu::BufferAddress,
                    shader_location: 8,
                    format: wgpu::VertexFormat::Float32x4,
                },
                wgpu::VertexAttribute { //normal1
                    offset: mem::size_of::<[f32; 16]>() as wgpu::BufferAddress,
                    shader_location: 9,
                    format: wgpu::VertexFormat::Float32x3,
                },
                wgpu::VertexAttribute { //normal2
                    offset: mem::size_of::<[f32; 19]>() as wgpu::BufferAddress,
                    shader_location: 10,
                    format: wgpu::VertexFormat::Float32x3,
                },
                wgpu::VertexAttribute { //normal3
                    offset: mem::size_of::<[f32; 22]>() as wgpu::BufferAddress,
                    shader_location: 11,
                    format: wgpu::VertexFormat::Float32x3,
                },
                wgpu::VertexAttribute {//color1
                    offset: mem::size_of::<[f32; 25]>() as wgpu::BufferAddress,
                    shader_location: 12,
                    format: wgpu::VertexFormat::Float32x4,
                },
                wgpu::VertexAttribute {//color2
                    offset: mem::size_of::<[f32; 29]>() as wgpu::BufferAddress,
                    shader_location: 13,
                    format: wgpu::VertexFormat::Float32x4,
                },
                wgpu::VertexAttribute {//pattern
                    offset: mem::size_of::<[f32; 33]>() as wgpu::BufferAddress,
                    shader_location: 14,
                    format: wgpu::VertexFormat::Uint32, 
                }
            ],
        }
    }
}

impl Instance { 
    fn to_raw(&self) -> InstanceRaw {
        InstanceRaw {
            model: (glam::Mat4::from_translation(self.position)
                * glam::Mat4::from_quat(self.rotation))
                .to_cols_array_2d(),
                normal: glam::Mat3::from_quat(self.rotation).to_cols_array_2d(),
                color1: self.color1.to_array(),
                color2: self.color2.to_array(),
                pattern: self.pattern as u32,
        }
    }
}

pub struct World {
    pub physics_world: PhysicsWorld,
    pub objects: Vec<GameObject>,
    instance_buffer: wgpu::Buffer,
    instance_raws: Vec<InstanceRaw>,
}

pub enum GameObjectState {
    Alive,
    Dead,
}

pub struct GameObject {
    instance: Instance,
    handle: RigidBodyHandle,
    state: GameObjectState,
}

impl World {
    pub fn new(physics_world: PhysicsWorld, instance_buffer: wgpu::Buffer) -> Self {
        World {
            physics_world,
            instance_buffer,
            objects: Vec::with_capacity(NUM_MAX_OBJECT),
            instance_raws: Vec::with_capacity(NUM_MAX_OBJECT),
        }
    }

    pub fn add_object(
        &mut self, 
        position: glam::Vec3, 
        rotation: glam::Quat, 
        v: Vec3,
        color1: Vec4,
        color2: Vec4,
        pattern: Pattern,
    ) {
        let rigid_body = RigidBodyBuilder::dynamic()
            .translation(Vector::new(position.x, position.y, position.z))
            .linvel(Vector::new(v.x, 0.0, v.z))
            .angvel(Vector::new(0.0, 0.0, 0.0))
            .ccd_enabled(true);
        let collider = ColliderBuilder::ball(1.0).restitution(0.8).friction(0.6);
        let (handle, _) = self.physics_world.insert(rigid_body, collider);
        let obj = GameObject {
            instance: Instance {
                position,
                rotation,
                color1,
                color2,
                pattern,
            },
            handle,
            state: GameObjectState::Alive,
        };
        self.instance_raws.push(obj.instance.to_raw());
        self.objects.push(obj);
    }

    pub fn sync_physics_to_instance(&mut self) {
        self.physics_world.step();
        self.objects.iter_mut().enumerate().for_each(|(num,obj)| {
            let body = &self.physics_world.bodies[obj.handle];
            obj.instance.position =body.position().translation;
            obj.instance.rotation = body.position().rotation;
            self.instance_raws[num] = obj.instance.to_raw();
        });
    }

    pub fn write_instance_buffer(&self, queue: &wgpu::Queue) {
        queue.write_buffer(&self.instance_buffer, 0, bytemuck::cast_slice(&self.instance_raws));
    }

    pub fn apply_ao_uniform(&self, light: &mut LightUniform) {
        self.objects.iter().enumerate().for_each(|(i, obj)| {
            light.balls[i][0] = obj.instance.position.x;
            light.balls[i][1] = obj.instance.position.y;
            light.balls[i][2] = obj.instance.position.z;
        });
        light.ball_count = self.objects.len() as u32;
    }

    pub fn drop_deads(&mut self, light: &mut LightUniform) {
        let mut idxs = self.objects.iter().enumerate().filter(|(_, obj)| {
            match obj.state {
                GameObjectState::Dead => true,
                GameObjectState::Alive => false,
            } 
        }).map(|(i, _)| i).collect::<Vec<_>>();
        idxs.sort_by(|a, b| b.cmp(a));
        idxs.iter().for_each(|i| {
            light.balls.swap(*i, self.objects.len() - 1);
            let obj = self.objects.swap_remove(*i);
            self.instance_raws.swap_remove(*i);
            light.ball_count = self.objects.len() as u32;
            self.physics_world.remove_body(obj.handle);

        })
    }

}
/* 
// lib.rs
const VERTICES: &[Vertex] = &[
    Vertex { position: [-0.0868241, 0.49240386, 0.0], tex_coords: [0.4131759, 1.0 - 0.99240386], }, // A
    Vertex { position: [-0.49513406, 0.06958647, 0.0], tex_coords: [0.0048659444, 1.0 - 0.56958647], }, // B
    Vertex { position: [-0.21918549, -0.44939706, 0.0], tex_coords: [0.28081453, 1.0 - 0.05060294], }, // C
    Vertex { position: [0.35966998, -0.3473291, 0.0], tex_coords: [0.85967, 1.0 - 0.1526709], }, // D
    Vertex { position: [0.44147372, 0.2347359, 0.0], tex_coords: [0.9414737, 1.0 - 0.7347359], }, // E
];
*/


const INDICES: &[u16] = &[
    0, 1, 4,
    1, 2, 4,
    2, 3, 4,
];

pub struct State {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    is_surface_configured: bool,
    window: Arc<Window>,
    render_pipeline: wgpu::RenderPipeline,
    /*vertex_buffer: wgpu::Buffer,
    num_vertices: u32,*/
    index_buffer: wgpu::Buffer,
    num_indices: u32,
    diffuse_bind_group: wgpu::BindGroup,
    diffuse_texture: texture::Texture,
    camera: Camera,
    camera_uniform: CameraUniform,
    camera_buffer: wgpu::Buffer,
    camera_bind_group: wgpu::BindGroup,
    camera_controller: CameraController,
    hole_instances: Vec<Instance>,
    hole_instance_buffer: wgpu::Buffer,
    depth_texture: texture::Texture,
    ball_model: model::Model,
    hole_model: model::Model,
    last_frame: Instant,
    light_uniform: LightUniform,
    light_buffer: wgpu::Buffer,
    light_buffer1: wgpu::Buffer,
    light_bind_group: wgpu::BindGroup,
    light_bind_group1: wgpu::BindGroup,
    world: World,
    physics_acc: f32,
    object_acc: f32,
    rand: StdRng,
    patterns: Vec<Pattern>,
    bibrate: bool,
    hole_handle: RigidBodyHandle,
    vibe_amp: f32,
    vibe_time: f32,
    ui_context: egui::Context,
    ui_state: egui_winit::State,
    ui_renderer: egui_wgpu::Renderer,
    fps: f32,
    touch_map: std::collections::HashMap<u64, Touch>,
    space_down: bool,
    mouse_down: bool,
}

impl State {
    async fn new(window: Arc<Window>) -> anyhow::Result<Self> {
        let size = window.inner_size();
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            #[cfg(not(target_arch="wasm32"))]
            backends: wgpu::Backends::PRIMARY,
            #[cfg(target_arch="wasm32")]
            backends: wgpu::Backends::BROWSER_WEBGPU | wgpu::Backends::GL,
            flags: Default::default(),
            memory_budget_thresholds: Default::default(),
            backend_options: Default::default(),
            display: None,
        });

        let surface = instance.create_surface(window.clone()).unwrap();

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::default(),
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
                apply_limit_buckets: true,
            }).await?;

        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: None,
                required_features: wgpu::Features::empty(),
                experimental_features: wgpu::ExperimentalFeatures::disabled(),
                required_limits: if cfg!(target_arch="wasm32") {
                    wgpu::Limits::downlevel_webgl2_defaults()
                } else {
                    wgpu::Limits::defaults()
                },
                memory_hints: Default::default(),
                trace: wgpu::Trace::Off,
            }).await?;

        let surface_cap = surface.get_capabilities(&adapter);

        let surface_format = surface_cap.formats.iter()
            .find(|f| f.is_srgb())
            .copied()
            .unwrap_or(surface_cap.formats[0]);

        let apply_gamma = !surface_format.is_srgb();

        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: surface_format,
            width: size.width,
            height: size.height,
            present_mode: surface_cap.present_modes[0],
            alpha_mode: surface_cap.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
            color_space: wgpu::SurfaceColorSpace::Auto,
        };

        let aspect = if config.height > 0 {
            config.width as f32 / config.height as f32
        } else {
            1.0
        };
        let camera = Camera {
            eye: (0.0, 32.0, 20.0).into(),
            target: (0.0, 10.0, 0.0).into(),
            up: glam::Vec3::Y,
            aspect,
            fovy: 45.0,
            znear: 0.1,
            zfar: 100.0,
        };


        let camera_controller = CameraController::new(0.2);

        let mut camera_uniform = CameraUniform::new();
        camera_uniform.update_view_proj(&camera);

        let camera_buffer = device.create_buffer_init(&BufferInitDescriptor {
            label: Some("Camera Buffer"),
            contents: bytemuck::cast_slice(&[camera_uniform]),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let camera_bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX|wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                }
            ],
            label: Some("camera_bind_group_layout"),
        });

        let camera_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            layout: &camera_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: camera_buffer.as_entire_binding(),
                }
            ],
            label: Some("camera_bind_group"),
        });

        let light_uniform = LightUniform {
            position: [30.0, 50.0, 40.0],
            apply_contact: 0,
            color: [1.0, 1.0, 1.0],
            ball_count: 0,
            balls: [[0.0; 4]; 3000],
        };
        
         // We'll want to update our lights position, so we use COPY_DST
        let light_buffer = device.create_buffer_init(
            &wgpu::util::BufferInitDescriptor {
                label: Some("Light VB"),
                contents: bytemuck::cast_slice(&[light_uniform]),
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            }
        );

        let light_buffer1 = device.create_buffer_init(
            &wgpu::util::BufferInitDescriptor {
                label: Some("Light Hole"),
                contents: bytemuck::cast_slice(&[light_uniform]),
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            }
        );
        
/* 
        let vertex_buffer = device.create_buffer_init(&BufferInitDescriptor {
            label: Some("Vertex Buffer"),
            contents: bytemuck::cast_slice(VERTICES),
            usage: wgpu::BufferUsages::VERTEX,
        });
*/
        let index_buffer = device.create_buffer_init(&BufferInitDescriptor {
            label: Some("Index Buffer"),
            contents: bytemuck::cast_slice(INDICES),
            usage: wgpu::BufferUsages::INDEX,
        });

        let num_indices = INDICES.len() as u32;
            
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()),
        });

        let diffuse_byte = include_bytes!("happy-tree.png");
        let diffuse_texture = texture::Texture::from_bytes(&device, &queue, diffuse_byte, "happy-tree.png").unwrap();

        let texture_bind_group_layout = 
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            multisampled: false,
                            view_dimension: wgpu::TextureViewDimension::D2,
                            sample_type: wgpu::TextureSampleType::Float {filterable: true},
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    }
                ],
                label: Some("texture_bind_group_layout"),
            });
        
        let diffuse_bind_group = device.create_bind_group(
            &wgpu::BindGroupDescriptor {
                layout: &texture_bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource:  wgpu::BindingResource::TextureView(&diffuse_texture.view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&diffuse_texture.sampler),
                    }
                ],
                label: Some("diffuse_bind_group"),
            }
        );

        let light_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                }],
                label: None,
            });

        let light_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            layout: &light_bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: light_buffer.as_entire_binding(),
            }],
            label: None,
        });


        let render_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Render Pipeline Layout"),
            bind_group_layouts: &[
                Some(&texture_bind_group_layout),
                Some(&camera_bind_group_layout),
                Some(&light_bind_group_layout),
            ],
            immediate_size: 0,
        });

        let depth_texture = texture::Texture::create_depth_texture(&device, &config, "depth_texture");

        let ball_model =
            resource::load_model("ball.glb", &device, &queue, &texture_bind_group_layout)
            .await
            .unwrap();

        let hole_model = 
            resource::load_model("hole.glb", &device, &queue, &texture_bind_group_layout)
            .await
            .unwrap();

        let render_pipeline = {
            let shader = wgpu::ShaderModuleDescriptor {
                label: Some("Normal Shader"),
                source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()),
            };
            create_render_pipeline(
                &device,
                &render_pipeline_layout,
                config.format,
                Some(texture::Texture::DEPTH_FORMAT),
                &[Some(model::ModelVertex::desc()), Some(InstanceRaw::desc())],
                shader,
                apply_gamma,
            )
        };

        let light_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                }],
                label: None,
            });

        let light_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            layout: &light_bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: light_buffer.as_entire_binding(),
            }],
            label: Some("light_bind_group_ball"),
        });
        let light_bind_group1 = device.create_bind_group(&wgpu::BindGroupDescriptor {
            layout: &light_bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: light_buffer1.as_entire_binding(),
            }],
            label: Some("light_bind_group_hole"),
        });


        /*const SPACE_BETWEEN: f32 = 3.0;
        let instances = (0..NUM_INSTANCE_PER_ROW).flat_map(|z| {
            (0..NUM_INSTANCE_PER_ROW).map(move |x| {
                let x = SPACE_BETWEEN * (x as f32 - NUM_INSTANCE_PER_ROW as f32 / 2.0);
                let z = SPACE_BETWEEN * (z as f32 - NUM_INSTANCE_PER_ROW as f32 / 2.0);

                let position = glam::Vec3 { x, y: 0.0, z };

                let rotation = if position == glam::Vec3::ZERO {
                    glam::Quat::from_axis_angle(glam::Vec3::Z, 0.0)
                } else {
                    glam::Quat::from_axis_angle(position.normalize(), (45.0 as f32).to_radians())
                };

                Instance {
                    position, rotation,
                }
            })
        }).collect::<Vec<_>>();*/
        let mut rand = StdRng::seed_from_u64(1);

        let ball_instance_buffer = device.create_buffer(
            &wgpu::BufferDescriptor {
                label: Some("Ball Instance Buffer"),
                size: (NUM_MAX_OBJECT * std::mem::size_of::<InstanceRaw>()) as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }
        );
        let patterns = vec![
            Pattern::Solid,
            Pattern::Checker,
            Pattern::Stripe1,
            Pattern::Stripe2,
            Pattern::Bubble,
        ];
        let hole_instances = vec![
            Instance {
                position: glam::Vec3 {x:0.0, y:0.0, z:0.0 },
                rotation: glam::Quat::from_axis_angle(glam::Vec3::Z, 0.0),
                color1: random_color(&mut rand),
                color2: random_color(&mut rand),
                pattern: *patterns.choose(&mut rand).unwrap(),
            }
        ];
        let hole_instance_data = hole_instances.iter().map(Instance::to_raw).collect::<Vec<_>>();
        let hole_instance_buffer = device.create_buffer_init(
            &wgpu::util::BufferInitDescriptor {
                label: Some("Hole Instance Buffer"),
                contents: bytemuck::cast_slice(&hole_instance_data),
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            }
        );
        
        /*let instance_data = instances.iter().map(Instance::to_raw).collect::<Vec<_>>();
        let instance_buffer = device.create_buffer_init(
            &wgpu::util::BufferInitDescriptor {
                label: Some("Instance Buffer"),
                contents: bytemuck::cast_slice(&instance_data),
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            }
        );*/

        let mut rapier = PhysicsWorld::new();
        rapier.gravity = Vector::Y * (-100.0);
        

        let hole_body = RigidBodyBuilder::kinematic_position_based();
        let hole_collider = ColliderBuilder::trimesh(
            hole_model.meshes[0].positions.iter().map(
                |p| Vec3{x:p[0], y:p[1], z:p[2]}
            ).collect(), 
            hole_model.meshes[0].tri_indices.clone()
        ).unwrap().restitution(0.3);
        let (hole_handle, _) = rapier.insert(hole_body, hole_collider);
        let world = World::new(rapier, ball_instance_buffer);
        //let num_vertices = VERTICES.len() as u32;
        let ui_context = egui::Context::default();
        let ui_state = egui_winit::State::new(
            ui_context.clone(),
            egui::ViewportId::ROOT,
            window.as_ref(),
            Some(window.scale_factor() as f32),
            window.theme(),
            Some(device.limits().max_texture_dimension_2d as usize),
        );
        let ui_renderer = egui_wgpu::Renderer::new(
            &device,
            surface_format,
            egui_wgpu::RendererOptions::default(),
        );
        Ok(Self {
            surface,
            device,
            queue,
            config,
            is_surface_configured: false,
            window,
            render_pipeline,
            /*vertex_buffer,
            num_vertices,*/
            index_buffer,
            num_indices,
            diffuse_bind_group,
            diffuse_texture,
            camera,
            camera_uniform,
            camera_buffer,
            camera_bind_group,
            camera_controller, 
            hole_instances,
            hole_instance_buffer,
            depth_texture,
            ball_model,
            hole_model,
            last_frame: Instant::now(),
            light_uniform,
            light_buffer,
            light_buffer1,
            light_bind_group,
            light_bind_group1,
            world,
            physics_acc: 0.0,
            object_acc: 0.0,
            rand,
            patterns,
            bibrate:false,
            hole_handle,
            vibe_amp: 0.0,
            vibe_time: 0.0,
            ui_context: ui_context.clone(),
            ui_state,
            ui_renderer,
            fps: 0.0,
            touch_map: std::collections::HashMap::new(),
            space_down: false,
            mouse_down: false,
        })
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        if width > 0 && height > 0 {
            let max = 2048;
            self.config.width = width.min(max);
            self.config.height = height.min(max);
            self.camera.aspect = self.config.width as f32 / self.config.height as f32;
            self.camera_uniform.update_view_proj(&self.camera);
            self.surface.configure(&self.device, &self.config);
            self.depth_texture = texture::Texture::create_depth_texture(&self.device, &self.config, "depth_texture");
            self.is_surface_configured = true;
            self.window.request_redraw();
        }
    }

    pub fn render(&mut self) -> anyhow::Result<()> {

        if !self.is_surface_configured {
            let size = self.window.inner_size();
            self.resize(size.width, size.height);
            if !self.is_surface_configured {
                self.window.request_redraw();
                return Ok(());
            }
        }

        let output = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(surface_texture) => surface_texture,
            wgpu::CurrentSurfaceTexture::Suboptimal(surface_texture) => {
                surface_texture
            }
            wgpu::CurrentSurfaceTexture::Timeout
            | wgpu::CurrentSurfaceTexture::Occluded
            | wgpu::CurrentSurfaceTexture::Validation => {
                // macOS often returns Occluded before the first layout; keep asking for frames.
                self.window.request_redraw();
                return Ok(());
            }
            wgpu::CurrentSurfaceTexture::Outdated => {
                self.surface.configure(&self.device, &self.config);
                self.window.request_redraw();
                return Ok(());
            }
            wgpu::CurrentSurfaceTexture::Lost => {
                // You could recreate the devices and all resources
                // created with it here, but we'll just bail
                anyhow::bail!("Lost device");
            }
        };
        self.queue.write_buffer(&self.camera_buffer, 0, bytemuck::cast_slice(&[self.camera_uniform]));

        self.light_uniform.apply_contact = 0;
        self.queue.write_buffer(&self.light_buffer, 0, bytemuck::cast_slice(&[self.light_uniform]));
        self.light_uniform.apply_contact = 1;
        self.queue.write_buffer(&self.light_buffer1, 0, bytemuck::cast_slice(&[self.light_uniform]));
        
        let hole_instance_data: Vec<InstanceRaw> = self.hole_instances.iter().map(Instance::to_raw).collect::<Vec<_>>();
        self.queue.write_buffer(&self.hole_instance_buffer, 0, bytemuck::cast_slice(&hole_instance_data));
        self.world.write_instance_buffer(&self.queue);

        let view = output.texture.create_view(&wgpu::TextureViewDescriptor::default());

        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Render Encoder"),
        });
        let raw = self.ui_state.take_egui_input(&self.window);
        let mut egui_out = self.ui_context.run_ui(raw, |ui| {
            egui::Area::new(egui::Id::new("hud"))
                .anchor(egui::Align2::LEFT_TOP, [12.0, 12.0])
                .show(ui.ctx(), |ui| {
                    ui.label(format!("balls  {}", self.world.objects.len()));
                    ui.label(format!("fps    {:.0}", self.fps));
                });
        });
        self.ui_state.handle_platform_output(&self.window, egui_out.platform_output);

        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Render Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.1,
                            g: 0.2,
                            b: 0.3,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.depth_texture.view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                occlusion_query_set: None,
                timestamp_writes: None,
                multiview_mask: None,
            });
            /*render_pass.set_bind_group(0, &self.diffuse_bind_group, &[]);
            render_pass.set_bind_group(1, &self.camera_bind_group, &[]);*/

            render_pass.set_pipeline(&self.render_pipeline);
            render_pass.set_index_buffer(self.index_buffer.slice(..), wgpu::IndexFormat::Uint16);
            use model::DrawModel;
            render_pass.set_vertex_buffer(1, self.world.instance_buffer.slice(..));
            render_pass.draw_model_instanced(&self.ball_model, 0..self.world.instance_raws.len() as u32, &self.camera_bind_group, &self.light_bind_group);
            render_pass.set_vertex_buffer(1, self.hole_instance_buffer.slice(..));
            render_pass.draw_model_instanced(&self.hole_model, 0..self.hole_instances.len() as u32, &self.camera_bind_group, &self.light_bind_group1);
            //let mesh = &self.obj_model.meshes[0];
            //let material = &self.obj_model.materials[mesh.material];
            //render_pass.draw_mesh_instanced(mesh, material,  0..self.instances.len() as u32, &self.camera_bind_group);
            //render_pass.draw_indexed(0..self.num_indices,0, 0..self.instances.len() as _);
        }

        let pixels_per_point = self.ui_context.pixels_per_point();
        let screen = egui_wgpu::ScreenDescriptor {
            size_in_pixels: [self.config.width, self.config.height],
            pixels_per_point,
        };
        let clipped = self.ui_context.tessellate(egui_out.shapes, pixels_per_point);
        
        for (id, deltas) in &egui_out.textures_delta.set {
            for image_delta in deltas {
                self.ui_renderer.update_texture(&self.device, &self.queue, *id, image_delta);
            }
        }
        self.ui_renderer.update_buffers(
            &self.device,
            &self.queue,
            &mut encoder,
            &clipped,
            &screen,
        );
        
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("egui"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load, // 3D 유지
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                occlusion_query_set: None,
                timestamp_writes: None,
                multiview_mask: None,
            });
            self.ui_renderer.render(&mut pass.forget_lifetime(), &clipped, &screen);
        }
        
        for id in &egui_out.textures_delta.free {
            self.ui_renderer.free_texture(id);
        }

        // submit will accept anything that implements IntoIter
        self.queue.submit(std::iter::once(encoder.finish()));
        self.queue.present(output);
        egui_out.textures_delta.clear();
        self.window.request_redraw();
        Ok(())
    }

    fn handle_key(&mut self, event_loop: &ActiveEventLoop, code: KeyCode, is_pressed: bool) {
        match (code, is_pressed) {
            (KeyCode::Escape, true) => event_loop.exit(),

            (key, _)  if matches!(
                key,
                KeyCode::KeyW | KeyCode::ArrowUp | KeyCode::KeyA | KeyCode::ArrowLeft | KeyCode::KeyS | KeyCode::ArrowDown | KeyCode::KeyD | KeyCode::ArrowRight
            )=> {
                self.camera_controller.handle_key(code, is_pressed);
            }
            (KeyCode::Space, pressed) => {
                self.space_down = pressed;
                self.sync_vibe();
            }
            _ => {}
        }
    }

    fn update(&mut self, dt: f32) {
        /*let instances = self.instances.iter().map(|i| {
            let speed = 0.1;
            Instance {
                position: i.position,
                rotation: i.rotation * glam::Quat::from_axis_angle(glam::Vec3::Y, speed * dt)
            }
        }).collect::<Vec<_>>();
        self.instances = instances;*/
        self.camera_controller.update_camera(&mut self.camera);
        self.camera_uniform.update_view_proj(&self.camera);
        self.physics_acc += dt;
        self.object_acc += dt;
        let step = 1.0 / 60.0;
        let ostep: f32 = 0.1;
        if dt > 0.0 {
            let instant = 1.0 / dt;
            self.fps = if self.fps == 0.0 {
                instant
            } else {
                self.fps * 0.9 + instant * 0.1
            };
        }
        while self.physics_acc >= step {
            self.sync_vibe();
            let max_amp =  0.8;
            let charge_per_sec = 0.05;
            //let target = if self.bibrate {0.2} else {0.0};
            let tau = 0.2;
            if self.bibrate {
                self.vibe_amp = (self.vibe_amp + charge_per_sec * step).min(max_amp);
            } else {
                let tau = 0.25;
                self.vibe_amp += (0.0 - self.vibe_amp) * (1.0 - (-step / tau).exp());
            }
            let target = if self.bibrate { max_amp } else { 0.0 };
            self.vibe_amp += (target - self.vibe_amp) * (1.0 - (-step/ tau).exp());
            self.vibe_time += step;
            let omega = std::f32::consts::TAU * 3.0;
            let y = self.vibe_amp * (self.vibe_time * omega).sin();
            self.world.physics_world.bodies[self.hole_handle].set_next_kinematic_translation(Vector::new(0.0, y, 0.0));
            self.hole_instances[0].position.y = y;

            self.world.sync_physics_to_instance();
            self.world.apply_ao_uniform(&mut self.light_uniform);
            self.physics_acc -= step;
        }
        while self.object_acc >= ostep {
            self.world.objects.iter_mut().filter( |obj| {
                obj.instance.position.y < -40.0
            }).for_each(|obj| {
               obj.state = GameObjectState::Dead
            });
            self.world.drop_deads(&mut self.light_uniform);
            let rad: f32 = self.rand.random_range(0.0f32 .. 360.0).to_radians();
            let t  = Vec3 {x:-rad.sin(), y:0.0, z:rad.cos()};
            let r_hat = Vec3 { x: rad.cos(), y: 0.0, z: rad.sin() };
            let v =  t * 25.0  - r_hat * 2.0; // 접선으로 돌면서 안쪽으로;
            self.world.add_object(Vec3 {
                    x:23.0f32 * rad.cos(),
                    y: 33.0,
                    z:23.0f32 * rad.sin(),
                }, 
                glam::Quat::from_axis_angle(glam::Vec3::Z, 0.0),
                v,
                random_color(&mut self.rand),
                random_color(&mut self.rand),
                *self.patterns.choose(&mut self.rand).unwrap(),
            );

            
            self.object_acc -= ostep;
        }


        // Update the light
        /*let old_position: glam::Vec3 = self.light_uniform.position.into();
        self.light_uniform.position =
            (glam::Quat::from_axis_angle((0.0, 1.0, 0.0).into(), (1.0 as f32).to_radians())
                * old_position)
                .into();*/
 
    }

    fn sync_vibe(&mut self) {
        self.bibrate = self.space_down || self.mouse_down || !self.touch_map.is_empty();
    }
}

pub struct App {
    #[cfg(target_arch = "wasm32")]
    proxy: Option<winit::event_loop::EventLoopProxy<State>>,
    state: Option<State>,
}

impl App {
    pub fn new(#[cfg(target_arch = "wasm32")] event_loop: &EventLoop<State>) -> Self {
        #[cfg(target_arch = "wasm32")]
        let proxy = Some(event_loop.create_proxy());
        Self {
            state: None,
            #[cfg(target_arch = "wasm32")]
            proxy,
        }
    }
}

impl ApplicationHandler<State> for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        #[allow(unused_mut)]
        let mut window_attributes = Window::default_attributes();

        #[cfg(target_arch = "wasm32")]
        {
            use wasm_bindgen::JsCast;
            use winit::platform::web::WindowAttributesExtWebSys;

            const CANVAS_ID: &str = "canvas";

            let window = wgpu::web_sys::window().unwrap_throw();
            let document = window.document().unwrap_throw();
            let canvas = document.get_element_by_id(CANVAS_ID).unwrap_throw();
            let html_canvas_element = canvas.unchecked_into();
            window_attributes = window_attributes.with_canvas(Some(html_canvas_element));
        }

        let window = Arc::new(event_loop.create_window(window_attributes).unwrap());

        #[cfg(not(target_arch = "wasm32"))]
        {
            let mut state = pollster::block_on(State::new(window.clone())).unwrap();
            let size = window.inner_size();
            state.resize(size.width, size.height);
            window.request_redraw();
            self.state = Some(state);
        }

        #[cfg(target_arch = "wasm32")]
        {
            if let Some(proxy) = self.proxy.take() {
                wasm_bindgen_futures::spawn_local(async move {
                    assert!(proxy
                        .send_event(
                            State::new(window)
                                .await
                                .expect("Unable to create canvas!!!")
                        )
                        .is_ok())
                });
            }
        }
    }

    #[allow(unused_mut)]
    fn user_event(&mut self, _event_loop: &ActiveEventLoop, mut event: State) {
        #[cfg(target_arch = "wasm32")]
        {
            event.window.request_redraw();
            event.resize(
                event.window.inner_size().width,
                event.window.inner_size().height,
            );
        }
        self.state = Some(event);
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: winit::window::WindowId,
        event: WindowEvent,
    ) {
        let state = match &mut self.state {
            Some(canvas) => canvas,
            None => return,
        };
        let event_response = state.ui_state.on_window_event(&state.window, &event);
        if event_response.repaint {
            state.window.request_redraw();
        }
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => state.resize(size.width, size.height),
            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        physical_key: PhysicalKey::Code(code),
                        state: key_state,
                        ..
                    },
                ..
            } => state.handle_key(event_loop, code, key_state.is_pressed()),
            WindowEvent::RedrawRequested => {
                let now = Instant::now();
                let dt = (now - state.last_frame).as_secs_f32().min(0.1);
                state.last_frame = now;
                state.update(dt);
                match state.render() {
                    Ok(_) => {}
                    Err(e) => {
                        // Log the error and exit gracefully
                        log::error!("{e}");
                        event_loop.exit();
                    }
                }
            }
            WindowEvent::Focused(true) | WindowEvent::Occluded(false) => {
                state.window.request_redraw();
            },
            WindowEvent::Touch(touch @ Touch {phase: p, ..})  => {
                match p {
                    TouchPhase::Started => {
                        state.touch_map.insert(touch.id, touch);
                    } 
                    TouchPhase::Moved => {}
                    TouchPhase::Ended | TouchPhase::Cancelled => {
                        state.touch_map.remove(&touch.id);
                    }
                }
                state.sync_vibe();
            },
            WindowEvent::MouseInput{
                state: mouse_state,
                ..
            } => {
                match mouse_state {
                    ElementState::Pressed => {
                        state.mouse_down = true;
                    }
                    ElementState::Released => {
                        state.mouse_down = false;
                    }
                }
                state.sync_vibe();
            }
            _ => {}
        }
    }
}

fn create_render_pipeline(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    color_format: wgpu::TextureFormat,
    depth_format: Option<wgpu::TextureFormat>,
    vertex_layouts: &[Option<wgpu::VertexBufferLayout>],
    shader: wgpu::ShaderModuleDescriptor,
    apply_gamma: bool,
) -> wgpu::RenderPipeline {
    let shader = device.create_shader_module(shader);

    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("Render Pipeline"),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            buffers: vertex_layouts,
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_main"),
            targets: &[Some(wgpu::ColorTargetState {
                format: color_format,
                blend: Some(wgpu::BlendState {
                    alpha: wgpu::BlendComponent::REPLACE,
                    color: wgpu::BlendComponent::REPLACE,
                }),
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: wgpu::PipelineCompilationOptions {
                constants: &[("apply_gamma", if apply_gamma { 1.0 } else { 0.0 })],
                ..Default::default()
            },
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            strip_index_format: None,
            front_face: wgpu::FrontFace::Ccw,
            cull_mode: Some(wgpu::Face::Back),
            // Setting this to anything other than Fill requires Features::NON_FILL_POLYGON_MODE
            polygon_mode: wgpu::PolygonMode::Fill,
            // Requires Features::DEPTH_CLIP_CONTROL
            unclipped_depth: false,
            // Requires Features::CONSERVATIVE_RASTERIZATION
            conservative: false,
        },
        depth_stencil: depth_format.map(|format| wgpu::DepthStencilState {
            format,
            depth_write_enabled: Some(true),
            depth_compare: Some(wgpu::CompareFunction::Less),
            stencil: wgpu::StencilState::default(),
            bias: wgpu::DepthBiasState::default(),
        }),
        multisample: wgpu::MultisampleState {
            count: 1,
            mask: !0,
            alpha_to_coverage_enabled: false,
        },
        multiview_mask: None,
        cache: None,

    })
}

pub fn random_color(rand : &mut StdRng) -> Vec4 {
    Vec4::new(
        rand.random_range(0.0..1.0), 
        rand.random_range(0.0..1.0), 
        rand.random_range(0.0..1.0), 
        1.0,
    )
}

pub fn run() -> anyhow::Result<()> {
    #[cfg(not(target_arch="wasm32"))]
    {
        env_logger::init();
    }

    #[cfg(target_arch="wasm32")]
    {
        console_log::init_with_level(log::Level::Info).unwrap_throw();
    }

    let event_loop = EventLoop::with_user_event().build()?;
    #[cfg(not(target_arch="wasm32"))]
    {
        let mut app = App::new();
        event_loop.run_app(&mut app)?;
    }

    #[cfg(target_arch = "wasm32")]
    {
        let app = App::new(&event_loop);
        event_loop.spawn_app(app);
    }

    Ok(())
}

#[cfg(target_arch="wasm32")]
#[wasm_bindgen(start)]
pub fn run_web() -> Result<(), wasm_bindgen::JsValue> {
    console_error_panic_hook::set_once();
    run().unwrap_throw();

    Ok(())
}
