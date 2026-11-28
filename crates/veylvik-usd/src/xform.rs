//! USD transforms: a prim's place is its ancestors' local transforms and its own multiplied, each local transform the product of its xform ops in the order `xformOpOrder` lists them (the first outermost), any of them inverted or the parents' reset as the order says; and the matrix arithmetic the reader needs (row-major 4 x 4 acting on column vectors).
use crate::{Result, error};
use openusd::{usd, vt};
use veylvik_mesh::animation::{IDENTITY, Mat, affine_inverse, compose};

pub(crate) fn mul(a: &Mat, b: &Mat) -> Mat {
    let mut out = [[0.0; 4]; 4];
    for (i, row) in out.iter_mut().enumerate() {
        for (j, v) in row.iter_mut().enumerate() {
            *v = (0..4).map(|k| a[i][k] * b[k][j]).sum();
        }
    }
    out
}
pub(crate) fn point(m: &Mat, p: [f32; 3]) -> [f32; 3] {
    [0, 1, 2].map(|r| m[r][0] * p[0] + m[r][1] * p[1] + m[r][2] * p[2] + m[r][3])
}
pub(crate) fn unit(v: [f32; 3]) -> [f32; 3] {
    let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if l > 0.0 {
        v.map(|x| x / l)
    } else {
        [0.0, 1.0, 0.0]
    }
}
fn axis(k: usize, degrees: f64) -> Mat {
    let (s, c) = (degrees.to_radians() as f32).sin_cos();
    let mut m = IDENTITY;
    let (a, b) = ((k + 1) % 3, (k + 2) % 3);
    m[a][a] = c;
    m[a][b] = -s;
    m[b][a] = s;
    m[b][b] = c;
    m
}
fn triple(value: &vt::Value) -> Option<[f64; 3]> {
    Some(match value {
        vt::Value::Vec3f(v) => [v.x, v.y, v.z].map(f64::from),
        vt::Value::Vec3d(v) => [v.x, v.y, v.z],
        vt::Value::Vec3h(v) => [v.x, v.y, v.z].map(|x| f64::from(x.to_f32())),
        vt::Value::Float(v) => [f64::from(*v); 3],
        vt::Value::Double(v) => [*v; 3],
        _ => return None,
    })
}
fn single(value: &vt::Value) -> Option<f64> {
    Some(match value {
        vt::Value::Float(v) => f64::from(*v),
        vt::Value::Double(v) => *v,
        vt::Value::Half(v) => f64::from(v.to_f32()),
        _ => return None,
    })
}
/// One xform op's matrix from its kind (`translate`, `rotateXYZ`, `orient`, ...) and value.
fn op_matrix(kind: &str, value: &vt::Value) -> Option<Mat> {
    let rotation_of = |order: &str, angles: [f64; 3]| {
        // rotateABC turns about A first, then B, then C.
        let mut m = IDENTITY;
        for c in order.chars() {
            let k = match c {
                'X' => 0,
                'Y' => 1,
                _ => 2,
            };
            m = mul(&axis(k, angles[k]), &m);
        }
        m
    };
    Some(match kind {
        "translate" => {
            let [x, y, z] = triple(value)?;
            let mut m = IDENTITY;
            m[0][3] = x as f32;
            m[1][3] = y as f32;
            m[2][3] = z as f32;
            m
        }
        "scale" => {
            let [x, y, z] = triple(value)?;
            let mut m = IDENTITY;
            m[0][0] = x as f32;
            m[1][1] = y as f32;
            m[2][2] = z as f32;
            m
        }
        "rotateX" => axis(0, single(value)?),
        "rotateY" => axis(1, single(value)?),
        "rotateZ" => axis(2, single(value)?),
        "orient" => {
            let q = match value {
                vt::Value::Quatf(q) => [q.x, q.y, q.z, q.w],
                vt::Value::Quatd(q) => [q.x, q.y, q.z, q.w].map(|v| v as f32),
                vt::Value::Quath(q) => [q.x, q.y, q.z, q.w].map(|v| v.to_f32()),
                _ => return None,
            };
            compose([0.0; 3], q, [1.0; 3])
        }
        "transform" => match value {
            // USD's matrices act on row vectors: ours is its transpose.
            vt::Value::Matrix4d(m) => {
                std::array::from_fn(|r| std::array::from_fn(|c| m.0[c * 4 + r] as f32))
            }
            _ => return None,
        },
        _ if kind.starts_with("rotate") && kind.len() == 9 => {
            rotation_of(&kind[6..], triple(value)?)
        }
        _ => return None,
    })
}
/// The ops a prim lists, each (attribute name, inverted), and whether it resets its
/// parents' transforms.
fn ops(prim: &usd::Prim) -> Result<(Vec<(String, bool)>, bool)> {
    let order = match prim
        .attribute("xformOpOrder")
        .get::<vt::Value>()
        .map_err(error)?
    {
        Some(vt::Value::TokenVec(v)) => v,
        _ => return Ok((Vec::new(), false)),
    };
    let mut out = Vec::new();
    let mut reset = false;
    for token in order {
        let name = token.as_str();
        if name == "!resetXformStack!" {
            reset = true;
            out.clear();
            continue;
        }
        match name.strip_prefix("!invert!") {
            Some(rest) => out.push((rest.to_owned(), true)),
            None => out.push((name.to_owned(), false)),
        }
    }
    Ok((out, reset))
}
fn local_at(prim: &usd::Prim, at: Option<f64>) -> Result<(Mat, bool)> {
    let (list, reset) = ops(prim)?;
    let mut m = IDENTITY;
    for (name, inverted) in list {
        let kind = name.strip_prefix("xformOp:").unwrap_or(&name);
        let kind = kind.split(':').next().unwrap_or(kind);
        let Some(value) = prim
            .attribute(name.as_str())
            .get_at::<vt::Value>(at.map(usd::TimeCode::new))
            .map_err(error)?
        else {
            continue;
        };
        let Some(mut op) = op_matrix(kind, &value) else {
            return Err(format!(
                "{}: the transform op {name} is not understood",
                prim.path()
            ));
        };
        if inverted {
            op = affine_inverse(&op).unwrap_or(IDENTITY);
        }
        m = mul(&m, &op);
    }
    Ok((m, reset))
}
/// A prim's place in the scene at time code `at` (its default with None).
pub(crate) fn world_at(prim: &usd::Prim, at: Option<f64>) -> Result<Mat> {
    // The prim itself, then the prims above it.
    let chain = prim.ancestors();
    let mut m = IDENTITY;
    for p in chain {
        let (local, reset) = local_at(&p, at)?;
        m = mul(&local, &m);
        if reset {
            break;
        }
    }
    Ok(m)
}
/// The moments any transform placing `prim` (its own or its ancestors') is keyed at.
pub(crate) fn sample_times(prim: &usd::Prim) -> Result<Vec<f64>> {
    // The prim itself, then the prims above it.
    let chain = prim.ancestors();
    let mut times = Vec::new();
    for p in chain {
        let (list, reset) = ops(&p)?;
        for (name, _) in list {
            times.extend(
                p.attribute(name.as_str())
                    .time_sample_times()
                    .map_err(error)?,
            );
        }
        if reset {
            break;
        }
    }
    times.sort_by(f64::total_cmp);
    times.dedup();
    Ok(times)
}
