//! GPU passes and uniforms for particles: a splat pass over particle threads, then a resolve pass over pixels. The node's settings travel in a data image, since they outnumber the uniform's slots.
use super::{Cx, From, Passes, RenderNode, Step};
use crate::{Kernel, Params, Result};
use veylvik_document::{Time, nodes::Particles};

/// Most particle threads from a box, and the threads each emitter cell has.
const MAX_BOX: f32 = 400_000.0;
const SLOTS: f32 = 4.0;
/// Threads in one dispatch row (1024 workgroups of 64).
pub(crate) const ROW: u32 = 65536;

/// How the threads are laid out at `time`: (box threads, box chance, stride, image chance).
fn layout(node: &Particles, time: Time) -> Result<(f32, f32, f32, f32)> {
    let life = node.life.at(time)?;
    let wanted = node.rate.at(time)? * life;
    let threads = wanted.ceil().clamp(1.0, MAX_BOX);
    // A cell of stride x stride pixels holds SLOTS threads; make cells as large as the
    // density allows, so sparse systems cost little.
    let per_pixel = node.density.at(time)? * life / 10_000.0;
    let stride = if per_pixel > 0.0 {
        (SLOTS / per_pixel).sqrt().floor().clamp(2.0, 16.0)
    } else {
        16.0
    };
    Ok((
        threads,
        (wanted / threads).min(1.0),
        stride,
        (per_pixel * stride * stride / SLOTS).min(1.0),
    ))
}
static STEPS: [Step; 2] = [
    Step {
        kernel: Kernel::ParticleSplat,
        inputs: [From::Port(0), From::Data, From::Port(1), From::None],
    },
    Step {
        kernel: Kernel::ParticleResolve,
        inputs: [From::Step(0), From::Data, From::None, From::None],
    },
];
impl RenderNode for Particles {
    fn passes(&self) -> Passes {
        Passes::chain(&STEPS)
    }
    fn has_data(&self) -> bool {
        true
    }
    /// The settings at `time` as a 12 x 1 image; equal settings share one image.
    fn data(
        &self,
        time: Time,
        _width: u32,
        _height: u32,
    ) -> Result<Option<std::sync::Arc<crate::Image>>> {
        type Kept = Vec<(Vec<[u32; 4]>, std::sync::Arc<crate::Image>)>;
        static BLOCKS: std::sync::Mutex<Kept> = std::sync::Mutex::new(Vec::new());
        let at = |scalar: &veylvik_document::Scalar| scalar.at(time);
        let (_, box_chance, stride, image_chance) = layout(self, time)?;
        // Three values a texel, with alpha 1, so the block is a valid image.
        let values = [
            at(&self.life)?,
            at(&self.speed)?,
            at(&self.direction)?.to_radians(),
            at(&self.spread)?.to_radians(),
            at(&self.gravity)?,
            at(&self.wind)?,
            at(&self.turbulence)?,
            at(&self.turbulence_scale)?,
            at(&self.size)?,
            at(&self.glow)?,
            at(&self.variation)?,
            at(&self.fade)?,
            at(&self.start)?,
            at(&self.duration)?,
            box_chance,
            image_chance,
            stride,
            SLOTS,
            at(&self.x)?,
            at(&self.y)?,
            0.0,
            at(&self.width)?,
            at(&self.height)?,
            0.0,
            self.colour[0],
            self.colour[1],
            self.colour[2],
            self.end_colour[0],
            self.end_colour[1],
            self.end_colour[2],
            self.colour[3],
            self.end_colour[3],
            0.0,
            at(&self.inherit)?,
            at(&self.homing)?,
            at(&self.streak)?,
        ];
        let block: Vec<[f32; 4]> = values.chunks(3).map(|v| [v[0], v[1], v[2], 1.0]).collect();
        let key: Vec<[u32; 4]> = block.iter().map(|row| row.map(f32::to_bits)).collect();
        let mut kept = BLOCKS
            .lock()
            .map_err(|_| crate::error("particle settings lock poisoned"))?;
        if let Some((_, image)) = kept.iter().find(|(k, _)| *k == key) {
            return Ok(Some(image.clone()));
        }
        let image = std::sync::Arc::new(crate::Image::new(12, 1, block)?);
        if kept.len() >= 32 {
            kept.remove(0);
        }
        kept.push((key, image.clone()));
        Ok(Some(image))
    }
    fn params(
        &self,
        cx: &Cx<'_>,
        _sample: &dyn Fn(&str) -> Result<f32>,
        p: &mut Params,
    ) -> Result<()> {
        let (box_threads, _, stride, _) = layout(self, cx.time)?;
        let emitter = cx.node.inputs.contains_key("emitter");
        let threads = if emitter {
            let stride = stride as u32;
            cx.width.div_ceil(stride) * cx.height.div_ceil(stride) * SLOTS as u32
        } else {
            box_threads as u32
        };
        p.size_op[2] = (cx.time.seconds()? as f32).to_bits();
        p.size_op[3] = self.seed;
        p.values = [threads as f32, threads.min(ROW) as f32, 0.0, 0.0];
        p.extra = [
            f32::from(u8::from(emitter)),
            f32::from(u8::from(cx.node.inputs.contains_key("target"))),
            0.0,
            0.0,
        ];
        Ok(())
    }
}
