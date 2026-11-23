//! The OpenFX parametric parameter suite: curve settings (a colour lookup's curves, a hue correction) as control points per curve, evaluated by the host with a monotone cubic, so a curve passes through its points without overshooting between them and holds its end values beyond them.
use super::suites::{Value, lock, out, param};
use super::{ERR_BAD_HANDLE, ERR_BAD_INDEX, ERR_VALUE, OK, Status};
use std::ffi::{c_double, c_int, c_void};

/// Most curves a setting has, and points a curve has.
const MAX_CURVES: usize = 16;
const MAX_POINTS: usize = 256;

/// A curve's value at `x`: monotone cubic through `points` (sorted by key), flat beyond
/// the ends; 0 for a curve without points.
pub fn evaluate(points: &[[f64; 2]], x: f64) -> f64 {
    let n = points.len();
    match n {
        0 => return 0.0,
        1 => return points[0][1],
        _ => {}
    }
    if x <= points[0][0] {
        return points[0][1];
    }
    if x >= points[n - 1][0] {
        return points[n - 1][1];
    }
    // Secant slopes between neighbouring points.
    let slope = |i: usize| {
        let dx = points[i + 1][0] - points[i][0];
        if dx.abs() < 1e-12 {
            0.0
        } else {
            (points[i + 1][1] - points[i][1]) / dx
        }
    };
    let i = points
        .windows(2)
        .position(|w| x >= w[0][0] && x <= w[1][0])
        .unwrap_or(n - 2);
    let (x0, y0) = (points[i][0], points[i][1]);
    let (x1, y1) = (points[i + 1][0], points[i + 1][1]);
    let h = x1 - x0;
    if h.abs() < 1e-12 {
        return y1;
    }
    // Tangents by the harmonic mean of neighbouring slopes (Fritsch-Butland): monotone.
    let m = |k: usize| -> f64 {
        if k == 0 || k == n - 1 {
            return slope(k.min(n - 2));
        }
        let (a, b) = (slope(k - 1), slope(k));
        if a * b <= 0.0 {
            0.0
        } else {
            3.0 * a * b / (a.max(b) + 2.0 * a.min(b))
        }
    };
    let (m0, m1) = (m(i), m(i + 1));
    let t = (x - x0) / h;
    let (t2, t3) = (t * t, t * t * t);
    (2.0 * t3 - 3.0 * t2 + 1.0) * y0
        + (t3 - 2.0 * t2 + t) * h * m0
        + (-2.0 * t3 + 3.0 * t2) * y1
        + (t3 - t2) * h * m1
}
/// Runs `f` on curve `index` of a parametric setting, growing the list of curves to it.
///
/// # Safety
/// `p` is null or a parameter handle we gave.
unsafe fn with_curve<T>(
    p: *mut c_void,
    index: c_int,
    f: impl FnOnce(&mut Vec<[f64; 2]>) -> Result<T, Status>,
) -> Result<T, Status> {
    // SAFETY: the caller's promise.
    let param = unsafe { param(p) }.ok_or(ERR_BAD_HANDLE)?;
    let index = usize::try_from(index).map_err(|_| ERR_BAD_INDEX)?;
    if index >= MAX_CURVES {
        return Err(ERR_BAD_INDEX);
    }
    let mut value = lock(&param.value);
    if !matches!(*value, Value::Curves(_)) {
        *value = Value::Curves(Vec::new());
    }
    let Value::Curves(curves) = &mut *value else {
        return Err(ERR_VALUE);
    };
    if curves.len() <= index {
        curves.resize(index + 1, Vec::new());
    }
    f(&mut curves[index])
}
fn status(result: Result<(), Status>) -> Status {
    result.err().unwrap_or(OK)
}
extern "C" fn get_value(
    p: *mut c_void,
    curve: c_int,
    _time: c_double,
    position: c_double,
    value: *mut c_double,
) -> Status {
    // SAFETY: a parameter handle we gave.
    let found = unsafe { with_curve(p, curve, |points| Ok(evaluate(points, position))) };
    match found {
        // SAFETY: `value` is the plugin's double.
        Ok(v) => unsafe { out(value, v) },
        Err(s) => s,
    }
}
extern "C" fn get_count(
    p: *mut c_void,
    curve: c_int,
    _time: c_double,
    count: *mut c_int,
) -> Status {
    // SAFETY: a parameter handle we gave.
    let found = unsafe { with_curve(p, curve, |points| Ok(points.len() as c_int)) };
    match found {
        // SAFETY: `count` is the plugin's int.
        Ok(n) => unsafe { out(count, n) },
        Err(s) => s,
    }
}
extern "C" fn get_nth(
    p: *mut c_void,
    curve: c_int,
    _time: c_double,
    nth: c_int,
    key: *mut c_double,
    value: *mut c_double,
) -> Status {
    // SAFETY: as for get_value.
    let point = unsafe {
        with_curve(p, curve, |points| {
            usize::try_from(nth)
                .ok()
                .and_then(|i| points.get(i).copied())
                .ok_or(ERR_BAD_INDEX)
        })
    };
    status(point.and_then(|[k, v]| {
        // SAFETY: the plugin's doubles.
        match unsafe { (out(key, k), out(value, v)) } {
            (OK, OK) => Ok(()),
            (OK, s) | (s, _) => Err(s),
        }
    }))
}
/// Keeps a curve's points in key order.
fn sort(points: &mut [[f64; 2]]) {
    points.sort_by(|a, b| a[0].total_cmp(&b[0]));
}
extern "C" fn set_nth(
    p: *mut c_void,
    curve: c_int,
    _time: c_double,
    nth: c_int,
    key: c_double,
    value: c_double,
    _animate: bool,
) -> Status {
    if !key.is_finite() || !value.is_finite() {
        return ERR_VALUE;
    }
    // SAFETY: a parameter handle we gave.
    status(unsafe {
        with_curve(p, curve, |points| {
            let point = usize::try_from(nth)
                .ok()
                .and_then(|i| points.get_mut(i))
                .ok_or(ERR_BAD_INDEX)?;
            *point = [key, value];
            sort(points);
            Ok(())
        })
    })
}
extern "C" fn add(
    p: *mut c_void,
    curve: c_int,
    _time: c_double,
    key: c_double,
    value: c_double,
    _animate: bool,
) -> Status {
    if !key.is_finite() || !value.is_finite() {
        return ERR_VALUE;
    }
    // SAFETY: a parameter handle we gave.
    status(unsafe {
        with_curve(p, curve, |points| {
            // A point close enough to an existing key replaces it, as the standard says.
            if let Some(point) = points.iter_mut().find(|q| (q[0] - key).abs() < 1e-9) {
                point[1] = value;
                return Ok(());
            }
            if points.len() >= MAX_POINTS {
                return Err(ERR_VALUE);
            }
            points.push([key, value]);
            sort(points);
            Ok(())
        })
    })
}
extern "C" fn delete(p: *mut c_void, curve: c_int, nth: c_int) -> Status {
    // SAFETY: a parameter handle we gave.
    status(unsafe {
        with_curve(p, curve, |points| {
            let i = usize::try_from(nth)
                .ok()
                .filter(|i| *i < points.len())
                .ok_or(ERR_BAD_INDEX)?;
            points.remove(i);
            Ok(())
        })
    })
}
extern "C" fn delete_all(p: *mut c_void, curve: c_int) -> Status {
    // SAFETY: a parameter handle we gave.
    status(unsafe {
        with_curve(p, curve, |points| {
            points.clear();
            Ok(())
        })
    })
}
/// OfxParametricParameterSuiteV1.
#[repr(C)]
pub struct Suite {
    get_value: extern "C" fn(*mut c_void, c_int, c_double, c_double, *mut c_double) -> Status,
    get_count: extern "C" fn(*mut c_void, c_int, c_double, *mut c_int) -> Status,
    get_nth:
        extern "C" fn(*mut c_void, c_int, c_double, c_int, *mut c_double, *mut c_double) -> Status,
    set_nth: extern "C" fn(*mut c_void, c_int, c_double, c_int, c_double, c_double, bool) -> Status,
    add: extern "C" fn(*mut c_void, c_int, c_double, c_double, c_double, bool) -> Status,
    delete: extern "C" fn(*mut c_void, c_int, c_int) -> Status,
    delete_all: extern "C" fn(*mut c_void, c_int) -> Status,
}
pub static SUITE: Suite = Suite {
    get_value,
    get_count,
    get_nth,
    set_nth,
    add,
    delete,
    delete_all,
};
#[cfg(test)]
mod tests {
    use super::evaluate;
    #[test]
    fn a_curve_passes_through_its_points_without_overshooting() {
        let points = [[0.0, 0.0], [0.3, 0.8], [0.6, 0.85], [1.0, 1.0]];
        for [x, y] in points {
            assert!((evaluate(&points, x) - y).abs() < 1e-12);
        }
        // Monotone data stays monotone and within its neighbours between points.
        let mut last = 0.0;
        for i in 0..=1000 {
            let v = evaluate(&points, i as f64 / 1000.0);
            assert!(v >= last - 1e-12, "falls at {i}");
            last = v;
        }
        let between = evaluate(&points, 0.45);
        assert!((0.8..=0.85).contains(&between), "{between}");
        // Flat beyond the ends; a straight line stays straight.
        assert_eq!(evaluate(&points, -1.0), 0.0);
        assert_eq!(evaluate(&points, 2.0), 1.0);
        let line = [[0.0, 0.0], [0.5, 0.5], [1.0, 1.0]];
        assert!((evaluate(&line, 0.25) - 0.25).abs() < 1e-12);
        assert_eq!(evaluate(&[], 0.5), 0.0);
        assert_eq!(evaluate(&[[0.2, 0.7]], 0.9), 0.7);
    }
}
