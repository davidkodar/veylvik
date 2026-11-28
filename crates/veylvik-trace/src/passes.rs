//! Render passes of the packed scene for compositing (normals, position, motion, Cryptomatte objects): 16 rays through each pixel on a 4 x 4 grid meet the nearest solid surface, with no lighting, lens or shutter, so a pixel's objects come with their exact shares of it.
use crate::gpu::{Frame, Texture, Tracer, View, frame_data, pictures};
use crate::scene::{Mat4, Packed};
use wgpu::util::DeviceExt;

/// Which render pass.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pass {
    /// Which way each surface faces (-1 to 1), opaque where an object is.
    Normals,
    /// Where each surface is, in scene units.
    Position,
    /// Pixels to the next frame (x right, y up) and from the last.
    Motion,
    /// The two objects covering most of each pixel: id, share, id, share (Cryptomatte).
    Objects,
    /// The third and fourth.
    ObjectsMore,
}
/// Where each placing was a frame before and is a frame after (placing to world, as
/// `Placed::world`), and the camera then (camera to world).
pub struct Moved<'a> {
    pub before: &'a [Mat4],
    pub after: &'a [Mat4],
    pub camera_before: Mat4,
    pub camera_after: Mat4,
}
pub struct PassJob<'a> {
    pub device: &'a wgpu::Device,
    pub encoder: &'a mut wgpu::CommandEncoder,
    pub scene: &'a Packed,
    /// The pictures surfaces name by number (cards are met where theirs is opaque).
    pub textures: &'a [Texture<'a>],
    pub view: View,
    pub width: u32,
    pub height: u32,
    pub pass: Pass,
    /// Each placing's Cryptomatte id (the objects passes).
    pub ids: &'a [u32],
    /// The motion pass's other frames.
    pub moved: Option<Moved<'a>>,
    /// The pass: width x height RGBA f32.
    pub target: &'a wgpu::Buffer,
}
fn rows(m: &Mat4) -> [[f32; 4]; 3] {
    [m[0], m[1], m[2]]
}
impl Tracer {
    pub fn passes(&self, job: PassJob<'_>) -> Result<(), String> {
        let PassJob {
            device,
            encoder,
            scene,
            textures,
            view,
            width,
            height,
            pass,
            ids,
            moved,
            target,
        } = job;
        let pixels = u64::from(width) * u64::from(height);
        if pixels == 0 {
            return Ok(());
        }
        if target.size() < pixels * 16 {
            return Err("the render pass's picture is smaller than the frame".into());
        }
        let placings = scene.instances.len();
        if matches!(pass, Pass::Objects | Pass::ObjectsMore) && ids.len() != placings {
            return Err("an objects pass needs an id for every placing".into());
        }
        let geometry = self.geometry_for(device, scene)?;
        let g = geometry.as_ref().expect("geometry was just made");
        let (pictures, places) = pictures(device, encoder, textures)?;
        let (mut data, offsets, [lights_at, textures_at, looks_at]) =
            frame_data(scene, &[], &places, &[]);
        let ids_at = data.len() as u32;
        for chunk in ids.chunks(4) {
            let mut v = [0.0f32; 4];
            for (slot, id) in v.iter_mut().zip(chunk) {
                *slot = f32::from_bits(*id);
            }
            data.push(v);
        }
        // Each placing's place a frame before and after (its own where none is given).
        let moved_at = data.len() as u32;
        let camera = view.camera;
        let (camera_before, camera_after) = moved
            .as_ref()
            .map_or((camera, camera), |m| (m.camera_before, m.camera_after));
        for (k, instance) in scene.instances.iter().enumerate() {
            let now = instance.to_world;
            let [before, after] = [
                moved.as_ref().and_then(|m| m.before.get(k)),
                moved.as_ref().and_then(|m| m.after.get(k)),
            ]
            .map(|m| m.map_or(now, rows));
            data.extend(before);
            data.extend(after);
        }
        let cameras_at = data.len() as u32;
        for c in [camera_before, camera, camera_after] {
            let to_camera = crate::scene::affine_inverse(&c)
                .ok_or("the camera's placing squashes space flat")?;
            data.extend(rows(&to_camera));
        }
        let data = data_buffer(device, &data);
        let frame = Frame {
            camera: rows(&camera),
            lens: [view.focal, width as f32, height as f32, 0.0],
            look: [1.0, 0.0, 0.0, 0.0],
            counts: [0, 0, 1, 0],
            more: [0.0, placings as f32, 0.0, 1.0],
            offsets,
            lights_at: [lights_at, 0, textures_at, looks_at],
            volumes: [0; 4],
            pass_info: [
                match pass {
                    Pass::Normals => 1,
                    Pass::Position => 2,
                    Pass::Motion => 3,
                    Pass::Objects => 4,
                    Pass::ObjectsMore => 5,
                },
                ids_at,
                moved_at,
                cameras_at,
            ],
        };
        let uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("render pass frame"),
            contents: bytemuck::bytes_of(&frame),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let none = data_buffer(device, &[[0.0f32; 4]]);
        let bind = self.bind(
            device,
            &uniform,
            [&none, &none],
            g,
            &data,
            &none,
            wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                buffer: target,
                offset: 0,
                size: wgpu::BufferSize::new(pixels * 16),
            }),
            &pictures,
        );
        let pipeline = self.pass_pipeline(device);
        let mut compute = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("render pass"),
            timestamp_writes: None,
        });
        compute.set_pipeline(pipeline);
        compute.set_bind_group(0, &bind, &[0]);
        compute.dispatch_workgroups(width.div_ceil(8), height.div_ceil(8), 1);
        Ok(())
    }
}
fn data_buffer(device: &wgpu::Device, data: &[[f32; 4]]) -> wgpu::Buffer {
    device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("render pass data"),
        contents: bytemuck::cast_slice(data),
        usage: wgpu::BufferUsages::STORAGE,
    })
}
