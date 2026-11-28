//! A model in a compact binary form for the trip from the helper: little-endian numbers, lengths before lists, a magic word and version first; decoding checks every length against what is left.
use std::sync::Arc;
use veylvik_document::nodes::{CameraKey, ModelCamera};
use veylvik_mesh::animation::{Between, Channel, Clip, Joint, Keyed, Mat};
use veylvik_mesh::{Mesh, Model, Part, PointCache, Vertex};

const MAGIC: &[u8; 8] = b"VEYLFBX1";

struct Out(Vec<u8>);
impl Out {
    fn u32(&mut self, v: u32) {
        self.0.extend(v.to_le_bytes());
    }
    fn f32(&mut self, v: f32) {
        self.0.extend(v.to_le_bytes());
    }
    fn len(&mut self, n: usize) {
        self.u32(n as u32);
    }
    fn floats(&mut self, v: &[f32]) {
        for x in v {
            self.f32(*x);
        }
    }
    fn mat(&mut self, m: &Mat) {
        for row in m {
            self.floats(row);
        }
    }
    fn text(&mut self, s: &str) {
        self.len(s.len());
        self.0.extend(s.as_bytes());
    }
}
/// A model as bytes.
pub fn encode(model: &Model) -> Vec<u8> {
    let mut o = Out(MAGIC.to_vec());
    o.len(model.parts.len());
    for p in &model.parts {
        o.len(p.mesh.vertices.len());
        for v in &p.mesh.vertices {
            o.floats(&v.position);
            o.floats(&v.normal);
            o.floats(&v.uv);
        }
        o.len(p.mesh.indices.len());
        for i in &p.mesh.indices {
            o.u32(*i);
        }
        o.floats(&p.colour);
        o.f32(p.metallic);
        o.f32(p.roughness);
        o.u32(p.joint.map_or(u32::MAX, |j| j as u32));
        o.mat(&p.rest_inverse);
        match &p.cache {
            None => o.u32(0),
            Some(c) => {
                o.u32(1);
                o.len(c.seconds.len());
                o.floats(&c.seconds);
                for frame in &c.frames {
                    o.len(frame.len());
                    for p in frame {
                        o.floats(p);
                    }
                }
                o.len(c.point_of.len());
                for i in &c.point_of {
                    o.u32(*i);
                }
            }
        }
    }
    o.floats(&model.extent);
    o.len(model.joints.len());
    for j in &model.joints {
        o.u32(j.parent.map_or(u32::MAX, |p| p as u32));
        o.floats(&j.translation);
        o.floats(&j.rotation);
        o.floats(&j.scale);
    }
    o.len(model.clips.len());
    for c in &model.clips {
        o.text(&c.name);
        o.f32(c.length);
        o.len(c.channels.len());
        for ch in &c.channels {
            o.u32(ch.node as u32);
            o.u32(match ch.keyed {
                Keyed::Translation => 0,
                Keyed::Rotation => 1,
                Keyed::Scale => 2,
            });
            o.u32(match ch.between {
                Between::Step => 0,
                Between::Linear => 1,
                Between::CubicSpline => 2,
            });
            o.len(ch.times.len());
            o.floats(&ch.times);
            o.len(ch.values.len());
            for v in &ch.values {
                o.floats(v);
            }
        }
    }
    o.floats(&model.centre);
    o.f32(model.largest);
    o.len(model.cameras.len());
    for c in &model.cameras {
        o.text(&c.name);
        o.len(c.keys.len());
        for k in &c.keys {
            o.f32(k.seconds);
            o.floats(&k.position);
            o.floats(&k.rotation);
            o.f32(k.focal_length);
            o.f32(k.sensor_width);
        }
    }
    o.0
}
struct In<'a>(&'a [u8]);
impl In<'_> {
    fn bytes(&mut self, n: usize) -> Result<&[u8], String> {
        if self.0.len() < n {
            return Err("the FBX reader's answer is cut short".into());
        }
        let (a, b) = self.0.split_at(n);
        self.0 = b;
        Ok(a)
    }
    fn u32(&mut self) -> Result<u32, String> {
        Ok(u32::from_le_bytes(
            self.bytes(4)?.try_into().expect("four bytes"),
        ))
    }
    fn f32(&mut self) -> Result<f32, String> {
        Ok(f32::from_le_bytes(
            self.bytes(4)?.try_into().expect("four bytes"),
        ))
    }
    /// A count of things at least `each` bytes long each, checked against what is left.
    fn len(&mut self, each: usize) -> Result<usize, String> {
        let n = self.u32()? as usize;
        if n.saturating_mul(each.max(1)) > self.0.len() {
            return Err("the FBX reader's answer holds a list longer than itself".into());
        }
        Ok(n)
    }
    fn floats<const N: usize>(&mut self) -> Result<[f32; N], String> {
        let mut out = [0.0; N];
        for v in &mut out {
            *v = self.f32()?;
        }
        Ok(out)
    }
    fn mat(&mut self) -> Result<Mat, String> {
        Ok([
            self.floats()?,
            self.floats()?,
            self.floats()?,
            self.floats()?,
        ])
    }
    fn text(&mut self) -> Result<String, String> {
        let n = self.len(1)?;
        String::from_utf8(self.bytes(n)?.to_vec()).map_err(|_| "a name is not UTF-8".into())
    }
    fn index(&mut self) -> Result<Option<usize>, String> {
        let v = self.u32()?;
        Ok((v != u32::MAX).then_some(v as usize))
    }
}
/// The model the bytes hold.
pub fn decode(bytes: &[u8]) -> Result<Model, String> {
    let mut i = In(bytes);
    if i.bytes(MAGIC.len())? != MAGIC {
        return Err("the FBX reader answered in a form this Veylvik does not read".into());
    }
    let mut model = Model::default();
    for _ in 0..i.len(64)? {
        let mut mesh = Mesh::default();
        for _ in 0..i.len(32)? {
            mesh.vertices.push(Vertex {
                position: i.floats()?,
                normal: i.floats()?,
                uv: i.floats()?,
            });
        }
        for _ in 0..i.len(4)? {
            let k = i.u32()?;
            if k as usize >= mesh.vertices.len() {
                return Err("a triangle names a vertex the part does not have".into());
            }
            mesh.indices.push(k);
        }
        let colour = i.floats()?;
        let (metallic, roughness) = (i.f32()?, i.f32()?);
        let joint = i.index()?;
        let rest_inverse = i.mat()?;
        let cache = match i.u32()? {
            0 => None,
            _ => {
                let n = i.len(4)?;
                let mut seconds = Vec::with_capacity(n);
                for _ in 0..n {
                    seconds.push(i.f32()?);
                }
                let mut frames = Vec::with_capacity(n);
                for _ in 0..n {
                    let mut frame = Vec::new();
                    for _ in 0..i.len(12)? {
                        frame.push(i.floats()?);
                    }
                    frames.push(frame);
                }
                let mut point_of = Vec::new();
                for _ in 0..i.len(4)? {
                    point_of.push(i.u32()?);
                }
                Some(Arc::new(PointCache {
                    seconds,
                    frames,
                    point_of,
                }))
            }
        };
        model.parts.push(Part {
            mesh,
            colour,
            metallic,
            roughness,
            texture: None,
            joint,
            rest_inverse,
            skin: None,
            cache,
        });
    }
    model.extent = i.floats()?;
    for _ in 0..i.len(44)? {
        model.joints.push(Joint {
            parent: i.index()?,
            translation: i.floats()?,
            rotation: i.floats()?,
            scale: i.floats()?,
        });
    }
    for _ in 0..i.len(12)? {
        let name = i.text()?;
        let length = i.f32()?;
        let mut channels = Vec::new();
        for _ in 0..i.len(20)? {
            let node = i.u32()? as usize;
            let keyed = match i.u32()? {
                0 => Keyed::Translation,
                1 => Keyed::Rotation,
                _ => Keyed::Scale,
            };
            let between = match i.u32()? {
                0 => Between::Step,
                1 => Between::Linear,
                _ => Between::CubicSpline,
            };
            let mut times = Vec::new();
            for _ in 0..i.len(4)? {
                times.push(i.f32()?);
            }
            let mut values = Vec::new();
            for _ in 0..i.len(16)? {
                values.push(i.floats()?);
            }
            channels.push(Channel {
                node,
                keyed,
                between,
                times,
                values,
            });
        }
        model.clips.push(Clip {
            name,
            channels,
            length,
        });
    }
    model.centre = i.floats()?;
    model.largest = i.f32()?;
    for _ in 0..i.len(8)? {
        let name = i.text()?;
        let mut keys = Vec::new();
        for _ in 0..i.len(40)? {
            keys.push(CameraKey {
                seconds: i.f32()?,
                position: i.floats()?,
                rotation: i.floats()?,
                focal_length: i.f32()?,
                sensor_width: i.f32()?,
            });
        }
        model.cameras.push(ModelCamera { name, keys });
    }
    if !i.0.is_empty() {
        return Err("the FBX reader's answer has bytes left over".into());
    }
    veylvik_document::nodes::validate_cameras(&model.cameras).map_err(|e| e.to_string())?;
    Ok(model)
}
