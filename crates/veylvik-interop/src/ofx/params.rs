//! The OpenFX parameter suite. Its value functions are variadic in C; see `get_value` and `set_value` for how they are received.
use super::props::PropSet;
use super::suites::{Param, Value, is_text, lock, out, param, param_set, shape, text};
use super::{ERR_BAD_HANDLE, ERR_EXISTS, ERR_UNKNOWN, ERR_VALUE, FAILED, OK, Status};
use std::ffi::{CString, c_char, c_double, c_int, c_uint, c_void};
use std::sync::Mutex;

/// The properties a new parameter of `kind` starts with.
fn param_props(kind: &str, name: &str) -> PropSet {
    let p = PropSet::default();
    p.string("OfxPropType", "OfxTypeParameter");
    p.string("OfxPropName", name);
    p.string("OfxPropLabel", name);
    p.string("OfxPropShortLabel", name);
    p.string("OfxPropLongLabel", name);
    p.string("OfxParamPropType", kind);
    p.string("OfxParamPropScriptName", name);
    p.string("OfxParamPropHint", "");
    p.string("OfxParamPropParent", "");
    p.int("OfxParamPropSecret", &[0]);
    p.int("OfxParamPropEnabled", &[1]);
    p.int("OfxParamPropAnimates", &[1]);
    p.int("OfxParamPropIsAnimating", &[0]);
    p.int("OfxParamPropIsAutoKeying", &[0]);
    p.int("OfxParamPropPersistant", &[1]);
    p.int("OfxParamPropEvaluateOnChange", &[1]);
    p.int("OfxParamPropPluginMayWrite", &[0]);
    p.int("OfxParamPropCanUndo", &[1]);
    p.string(
        "OfxParamPropCacheInvalidation",
        "OfxParamInvalidateValueChange",
    );
    p.pointer("OfxParamPropDataPtr", std::ptr::null_mut());
    p.pointer("OfxParamPropInteractV1", std::ptr::null_mut());
    if let Some((n, ints)) = shape(kind) {
        if ints {
            p.int("OfxParamPropDefault", &vec![0; n]);
            p.int("OfxParamPropMin", &vec![i32::MIN; n]);
            p.int("OfxParamPropMax", &vec![i32::MAX; n]);
            p.int("OfxParamPropDisplayMin", &vec![i32::MIN; n]);
            p.int("OfxParamPropDisplayMax", &vec![i32::MAX; n]);
        } else {
            p.double("OfxParamPropDefault", &vec![0.0; n]);
            p.double("OfxParamPropMin", &vec![-f64::MAX; n]);
            p.double("OfxParamPropMax", &vec![f64::MAX; n]);
            p.double("OfxParamPropDisplayMin", &vec![-f64::MAX; n]);
            p.double("OfxParamPropDisplayMax", &vec![f64::MAX; n]);
            p.double("OfxParamPropIncrement", &[1.0]);
            p.int("OfxParamPropDigits", &[2]);
            p.string("OfxParamPropDoubleType", "OfxParamDoubleTypePlain");
            p.string(
                "OfxParamPropDefaultCoordinateSystem",
                "OfxParamCoordinatesCanonical",
            );
        }
    }
    if kind == "OfxParamTypeChoice" {
        p.strings("OfxParamPropChoiceOption", &[]);
        p.strings("OfxParamPropChoiceLabelOption", &[]);
        p.strings("OfxParamPropChoiceEnum", &[]);
    }
    if is_text(kind) {
        p.string("OfxParamPropDefault", "");
        p.string("OfxParamPropStringMode", "OfxParamStringIsSingleLine");
        p.int("OfxParamPropStringFilePathExists", &[1]);
    }
    if kind == "OfxParamTypePage" {
        p.strings("OfxParamPropPageChild", &[]);
    }
    if kind == "OfxParamTypeGroup" {
        p.int("OfxParamPropGroupOpen", &[1]);
    }
    p
}
/// A parameter's value at first: its default.
pub fn default_value(param: &Param) -> Value {
    if is_text(&param.kind) {
        Value::Text(
            CString::new(
                param
                    .props
                    .get_string("OfxParamPropDefault", 0)
                    .unwrap_or_default(),
            )
            .unwrap_or_default(),
        )
    } else {
        Value::Numbers(param.props.get_numbers("OfxParamPropDefault"))
    }
}
extern "C" fn param_define(
    set: *mut c_void,
    kind: *const c_char,
    name: *const c_char,
    props: *mut *mut c_void,
) -> Status {
    // SAFETY: handles and strings from the plugin per the standard.
    let (Some(set), Some(kind), Some(name)) =
        (unsafe { param_set(set) }, unsafe { text(kind) }, unsafe {
            text(name)
        })
    else {
        return ERR_BAD_HANDLE;
    };
    let mut params = lock(&set.params);
    if params.contains_key(&name) {
        return ERR_EXISTS;
    }
    let props_set = param_props(&kind, &name);
    let param = Box::new(Param {
        kind,
        props: props_set,
        value: Mutex::new(Value::Numbers(Vec::new())),
    });
    let handle = param.props.handle();
    params.insert(name.clone(), param);
    lock(&set.order).push(name);
    if props.is_null() {
        return OK;
    }
    // SAFETY: the plugin's out pointer.
    unsafe { out(props, handle) }
}
extern "C" fn param_get_handle(
    set: *mut c_void,
    name: *const c_char,
    p: *mut *mut c_void,
    props: *mut *mut c_void,
) -> Status {
    // SAFETY: as for param_define.
    let (Some(set), Some(name)) = (unsafe { param_set(set) }, unsafe { text(name) }) else {
        return ERR_BAD_HANDLE;
    };
    let params = lock(&set.params);
    let Some(found) = params.get(&name) else {
        return ERR_UNKNOWN;
    };
    if !props.is_null() {
        // SAFETY: the plugin's out pointer.
        unsafe { out(props, found.props.handle()) };
    }
    // SAFETY: the plugin's out pointer.
    unsafe { out(p, std::ptr::from_ref(found.as_ref()).cast_mut().cast()) }
}
extern "C" fn param_set_get_property_set(set: *mut c_void, props: *mut *mut c_void) -> Status {
    // SAFETY: as for param_define.
    unsafe { param_set(set).map_or(ERR_BAD_HANDLE, |s| out(props, s.props.handle())) }
}
extern "C" fn param_get_property_set(p: *mut c_void, props: *mut *mut c_void) -> Status {
    // SAFETY: as for param_define.
    unsafe { param(p).map_or(ERR_BAD_HANDLE, |p| out(props, p.props.handle())) }
}
/// Writes a parameter's value through the plugin's out pointers.
///
/// # Safety
/// Each pointer the parameter's type needs is writable storage of that type
/// (double, int, or a `const char *`).
unsafe fn write_value(p: &Param, outs: [*mut c_void; 4]) -> Status {
    let value = lock(&p.value).clone();
    match value {
        Value::Text(_) => {
            // The string stays alive in the parameter until its value is set again.
            let stored = lock(&p.value);
            let Value::Text(kept) = &*stored else {
                return ERR_VALUE;
            };
            // SAFETY: a text parameter's out pointer is a `const char **`.
            unsafe { out(outs[0].cast::<*const c_char>(), kept.as_ptr()) }
        }
        Value::Numbers(n) => {
            let Some((count, ints)) = shape(&p.kind) else {
                return ERR_UNKNOWN;
            };
            for (i, slot) in outs.iter().enumerate().take(count) {
                let v = n.get(i).copied().unwrap_or(0.0);
                // SAFETY: the caller's promise for this type.
                let status = unsafe {
                    if ints {
                        out(slot.cast::<c_int>(), v as c_int)
                    } else {
                        out(slot.cast::<c_double>(), v)
                    }
                };
                if status != OK {
                    return status;
                }
            }
            OK
        }
    }
}
// paramGetValue(handle, ...) is variadic in C, and stable Rust cannot define a
// variadic function. Its variadic arguments are all pointers; on the System V x86-64
// and AAPCS64 Linux ABIs a variadic call passes them in the same integer registers as
// a call to a function declaring them, so declaring four pointer parameters reads
// them correctly. Unused ones hold whatever the caller left there and are not touched.
extern "C" fn get_value(
    p: *mut c_void,
    a: *mut c_void,
    b: *mut c_void,
    c: *mut c_void,
    d: *mut c_void,
) -> Status {
    // SAFETY: a parameter handle we gave; out pointers per its type (see above).
    unsafe { param(p).map_or(ERR_BAD_HANDLE, |p| write_value(p, [a, b, c, d])) }
}
// As get_value, after the time (a double, in the first vector register either way).
extern "C" fn get_value_at_time(
    p: *mut c_void,
    _t: c_double,
    a: *mut c_void,
    b: *mut c_void,
    c: *mut c_void,
    d: *mut c_void,
) -> Status {
    // SAFETY: as for get_value.
    unsafe { param(p).map_or(ERR_BAD_HANDLE, |p| write_value(p, [a, b, c, d])) }
}
extern "C" fn get_derivative(
    p: *mut c_void,
    _t: c_double,
    a: *mut c_void,
    b: *mut c_void,
    c: *mut c_void,
    d: *mut c_void,
) -> Status {
    // Values are constant over a render: their rate of change is zero.
    // SAFETY: as for get_value.
    let Some(p) = (unsafe { param(p) }) else {
        return ERR_BAD_HANDLE;
    };
    let Some((count, false)) = shape(&p.kind) else {
        return ERR_UNKNOWN;
    };
    for slot in [a, b, c, d].iter().take(count) {
        // SAFETY: double out pointers for a double parameter.
        unsafe { out(slot.cast::<c_double>(), 0.0) };
    }
    OK
}
extern "C" fn get_integral(
    p: *mut c_void,
    t1: c_double,
    t2: c_double,
    a: *mut c_void,
    b: *mut c_void,
    c: *mut c_void,
    d: *mut c_void,
) -> Status {
    // SAFETY: as for get_value.
    let Some(p) = (unsafe { param(p) }) else {
        return ERR_BAD_HANDLE;
    };
    let Some((count, false)) = shape(&p.kind) else {
        return ERR_UNKNOWN;
    };
    let Value::Numbers(n) = lock(&p.value).clone() else {
        return ERR_VALUE;
    };
    for (i, slot) in [a, b, c, d].iter().enumerate().take(count) {
        // SAFETY: double out pointers for a double parameter.
        unsafe {
            out(
                slot.cast::<c_double>(),
                n.get(i).copied().unwrap_or(0.0) * (t2 - t1),
            )
        };
    }
    OK
}
/// Stores a value a plugin sets: ints and strings arrive in the integer registers
/// `i`, doubles in the vector registers `f` (see `set_value`).
///
/// # Safety
/// For a text parameter `i[0]` is a C string.
unsafe fn store(p: &Param, i: [usize; 4], f: [f64; 4]) -> Status {
    let value = if is_text(&p.kind) {
        // SAFETY: the caller's promise.
        Value::Text(
            CString::new(unsafe { text(i[0] as *const c_char) }.unwrap_or_default())
                .unwrap_or_default(),
        )
    } else {
        let Some((count, ints)) = shape(&p.kind) else {
            return ERR_UNKNOWN;
        };
        Value::Numbers(
            (0..count)
                .map(|k| if ints { f64::from(i[k] as c_int) } else { f[k] })
                .collect(),
        )
    };
    *lock(&p.value) = value;
    OK
}
// paramSetValue(handle, ...): the variadic values are ints, doubles or a string. A
// variadic call puts ints and pointers in the integer registers and doubles in the
// vector registers, in order, exactly as for a function declaring them; declaring four
// of each receives every case, and the parameter's own type says which to read.
#[allow(clippy::too_many_arguments)]
extern "C" fn set_value(
    p: *mut c_void,
    i0: usize,
    i1: usize,
    i2: usize,
    i3: usize,
    f0: f64,
    f1: f64,
    f2: f64,
    f3: f64,
) -> Status {
    // SAFETY: a parameter handle we gave; values per its type (see above).
    unsafe {
        param(p).map_or(ERR_BAD_HANDLE, |p| {
            store(p, [i0, i1, i2, i3], [f0, f1, f2, f3])
        })
    }
}
// As set_value, after the time (the first vector register).
#[allow(clippy::too_many_arguments)]
extern "C" fn set_value_at_time(
    p: *mut c_void,
    _t: f64,
    i0: usize,
    i1: usize,
    i2: usize,
    i3: usize,
    f0: f64,
    f1: f64,
    f2: f64,
    f3: f64,
) -> Status {
    // SAFETY: as for set_value.
    unsafe {
        param(p).map_or(ERR_BAD_HANDLE, |p| {
            store(p, [i0, i1, i2, i3], [f0, f1, f2, f3])
        })
    }
}
extern "C" fn get_num_keys(_p: *mut c_void, n: *mut c_uint) -> Status {
    // SAFETY: the plugin's out pointer. Values reach a plugin already evaluated: no keys.
    unsafe { out(n, 0) }
}
extern "C" fn get_key_time(_p: *mut c_void, _nth: c_uint, _t: *mut c_double) -> Status {
    ERR_UNKNOWN
}
extern "C" fn get_key_index(_p: *mut c_void, _t: c_double, _dir: c_int, _i: *mut c_int) -> Status {
    FAILED
}
extern "C" fn delete_key(_p: *mut c_void, _t: c_double) -> Status {
    OK
}
extern "C" fn delete_all_keys(_p: *mut c_void) -> Status {
    OK
}
extern "C" fn param_copy(
    to: *mut c_void,
    from: *mut c_void,
    _offset: c_double,
    _range: *const c_void,
) -> Status {
    // SAFETY: parameter handles we gave.
    let (Some(to), Some(from)) = (unsafe { param(to) }, unsafe { param(from) }) else {
        return ERR_BAD_HANDLE;
    };
    let value = lock(&from.value).clone();
    *lock(&to.value) = value;
    OK
}
extern "C" fn edit_begin(_set: *mut c_void, _name: *const c_char) -> Status {
    OK
}
extern "C" fn edit_end(_set: *mut c_void) -> Status {
    OK
}
/// OfxParameterSuiteV1, with the variadic entries declared as described above.
#[repr(C)]
pub struct ParamSuite {
    define: extern "C" fn(*mut c_void, *const c_char, *const c_char, *mut *mut c_void) -> Status,
    get_handle:
        extern "C" fn(*mut c_void, *const c_char, *mut *mut c_void, *mut *mut c_void) -> Status,
    set_get_property_set: extern "C" fn(*mut c_void, *mut *mut c_void) -> Status,
    get_property_set: extern "C" fn(*mut c_void, *mut *mut c_void) -> Status,
    get_value:
        extern "C" fn(*mut c_void, *mut c_void, *mut c_void, *mut c_void, *mut c_void) -> Status,
    get_value_at_time: extern "C" fn(
        *mut c_void,
        c_double,
        *mut c_void,
        *mut c_void,
        *mut c_void,
        *mut c_void,
    ) -> Status,
    get_derivative: extern "C" fn(
        *mut c_void,
        c_double,
        *mut c_void,
        *mut c_void,
        *mut c_void,
        *mut c_void,
    ) -> Status,
    get_integral: extern "C" fn(
        *mut c_void,
        c_double,
        c_double,
        *mut c_void,
        *mut c_void,
        *mut c_void,
        *mut c_void,
    ) -> Status,
    set_value: extern "C" fn(*mut c_void, usize, usize, usize, usize, f64, f64, f64, f64) -> Status,
    set_value_at_time:
        extern "C" fn(*mut c_void, f64, usize, usize, usize, usize, f64, f64, f64, f64) -> Status,
    get_num_keys: extern "C" fn(*mut c_void, *mut c_uint) -> Status,
    get_key_time: extern "C" fn(*mut c_void, c_uint, *mut c_double) -> Status,
    get_key_index: extern "C" fn(*mut c_void, c_double, c_int, *mut c_int) -> Status,
    delete_key: extern "C" fn(*mut c_void, c_double) -> Status,
    delete_all_keys: extern "C" fn(*mut c_void) -> Status,
    copy: extern "C" fn(*mut c_void, *mut c_void, c_double, *const c_void) -> Status,
    edit_begin: extern "C" fn(*mut c_void, *const c_char) -> Status,
    edit_end: extern "C" fn(*mut c_void) -> Status,
}
pub static PARAM_SUITE: ParamSuite = ParamSuite {
    define: param_define,
    get_handle: param_get_handle,
    set_get_property_set: param_set_get_property_set,
    get_property_set: param_get_property_set,
    get_value,
    get_value_at_time,
    get_derivative,
    get_integral,
    set_value,
    set_value_at_time,
    get_num_keys,
    get_key_time,
    get_key_index,
    delete_key,
    delete_all_keys,
    copy: param_copy,
    edit_begin,
    edit_end,
};
