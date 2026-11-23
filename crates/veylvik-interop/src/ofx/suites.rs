//! The host's OpenFX function tables other than properties: the effect with its clips and images, parameters (whose C functions are variadic: see `get_value`), memory, threads, messages, progress and the timeline.
use super::props::PropSet;
use super::{
    ERR_BAD_HANDLE, ERR_EXISTS, ERR_MEMORY, ERR_UNKNOWN, ERR_VALUE, FAILED, OK, REPLY_DEFAULT,
    Status,
};
use std::collections::BTreeMap;
use std::ffi::{CStr, CString, c_char, c_double, c_int, c_uint, c_void};
use std::sync::{Condvar, Mutex};

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}
/// A C string from a plugin.
///
/// # Safety
/// `s` is null or NUL-terminated.
unsafe fn text(s: *const c_char) -> Option<String> {
    // SAFETY: the caller's promise.
    (!s.is_null()).then(|| unsafe { CStr::from_ptr(s) }.to_string_lossy().into_owned())
}
/// Writes `value` to the plugin's `out`, if it gave one.
///
/// # Safety
/// `out` is null or writable storage for a T.
unsafe fn out<T>(out: *mut T, value: T) -> Status {
    if out.is_null() {
        return ERR_VALUE;
    }
    // SAFETY: the caller's promise.
    unsafe { out.write(value) };
    OK
}
/// A rectangle in canonical coordinates, as OfxRectD.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct RectD {
    pub x1: c_double,
    pub y1: c_double,
    pub x2: c_double,
    pub y2: c_double,
}
/// Pixels a clip shows during a render: RGBA float, bottom row first.
#[derive(Debug, Clone, Copy)]
pub struct Pixels {
    pub data: usize,
    pub width: i32,
    pub height: i32,
}
pub struct Clip {
    pub props: PropSet,
    pub pixels: Mutex<Option<Pixels>>,
}
/// A parameter's value: numbers (ints too), or text.
#[derive(Debug, Clone)]
pub enum Value {
    Numbers(Vec<f64>),
    Text(CString),
}
pub struct Param {
    pub kind: String,
    pub props: PropSet,
    pub value: Mutex<Value>,
}
/// How many numbers a parameter type holds and whether they are ints; None for text
/// and for types with no value.
pub fn shape(kind: &str) -> Option<(usize, bool)> {
    Some(match kind {
        "OfxParamTypeDouble" => (1, false),
        "OfxParamTypeDouble2D" => (2, false),
        "OfxParamTypeDouble3D" | "OfxParamTypeRGB" => (3, false),
        "OfxParamTypeRGBA" => (4, false),
        "OfxParamTypeInteger" | "OfxParamTypeBoolean" | "OfxParamTypeChoice" => (1, true),
        "OfxParamTypeInteger2D" => (2, true),
        "OfxParamTypeInteger3D" => (3, true),
        _ => return None,
    })
}
pub fn is_text(kind: &str) -> bool {
    matches!(
        kind,
        "OfxParamTypeString" | "OfxParamTypeCustom" | "OfxParamTypeStrChoice"
    )
}
#[derive(Default)]
pub struct ParamSet {
    pub props: PropSet,
    pub params: Mutex<BTreeMap<String, Box<Param>>>,
    pub order: Mutex<Vec<String>>,
}
/// An effect: a descriptor or an instance. Its address is the effect handle.
#[derive(Default)]
pub struct Effect {
    pub props: PropSet,
    pub params: ParamSet,
    pub clips: Mutex<BTreeMap<String, Box<Clip>>>,
    pub clip_order: Mutex<Vec<String>>,
}
impl Effect {
    pub fn handle(&self) -> *mut c_void {
        std::ptr::from_ref(self).cast_mut().cast()
    }
    /// A copy whose clips and parameters are new objects (a descriptor's contents for
    /// another context, or for an instance).
    pub fn copied(&self) -> Self {
        let copy = Self {
            props: self.props.clone(),
            ..Default::default()
        };
        copy.params_from(self);
        for name in lock(&self.clip_order).iter() {
            if let Some(clip) = lock(&self.clips).get(name) {
                copy.add_clip(name, clip.props.clone());
            }
        }
        copy
    }
    fn params_from(&self, other: &Effect) {
        let order = lock(&other.params.order).clone();
        let theirs = lock(&other.params.params);
        let mut ours = lock(&self.params.params);
        for name in &order {
            if let Some(p) = theirs.get(name) {
                ours.insert(
                    name.clone(),
                    Box::new(Param {
                        kind: p.kind.clone(),
                        props: p.props.clone(),
                        value: Mutex::new(lock(&p.value).clone()),
                    }),
                );
            }
        }
        *lock(&self.params.order) = order;
    }
    pub fn add_clip(&self, name: &str, props: PropSet) -> *mut c_void {
        let mut clips = lock(&self.clips);
        let clip = clips.entry(name.to_string()).or_insert_with(|| {
            lock(&self.clip_order).push(name.to_string());
            Box::new(Clip {
                props,
                pixels: Mutex::new(None),
            })
        });
        clip.props.handle()
    }
}
/// # Safety
/// `h` is null or an effect handle we handed out that is still alive.
unsafe fn effect<'a>(h: *mut c_void) -> Option<&'a Effect> {
    // SAFETY: the caller's promise.
    unsafe { h.cast::<Effect>().as_ref() }
}
/// # Safety
/// `h` is null or a clip handle we handed out that is still alive.
unsafe fn clip<'a>(h: *mut c_void) -> Option<&'a Clip> {
    // SAFETY: the caller's promise.
    unsafe { h.cast::<Clip>().as_ref() }
}
/// # Safety
/// `h` is null or a parameter handle we handed out that is still alive.
unsafe fn param<'a>(h: *mut c_void) -> Option<&'a Param> {
    // SAFETY: the caller's promise.
    unsafe { h.cast::<Param>().as_ref() }
}
/// # Safety
/// `h` is null or a parameter set handle we handed out that is still alive.
unsafe fn param_set<'a>(h: *mut c_void) -> Option<&'a ParamSet> {
    // SAFETY: the caller's promise.
    unsafe { h.cast::<ParamSet>().as_ref() }
}
// ---- Effect suite ----
extern "C" fn get_property_set(e: *mut c_void, props: *mut *mut c_void) -> Status {
    // SAFETY: handles and outputs come from the plugin per the standard.
    unsafe { effect(e).map_or(ERR_BAD_HANDLE, |e| out(props, e.props.handle())) }
}
extern "C" fn get_param_set(e: *mut c_void, set: *mut *mut c_void) -> Status {
    // SAFETY: as for get_property_set.
    unsafe {
        effect(e).map_or(ERR_BAD_HANDLE, |e| {
            out(set, std::ptr::from_ref(&e.params).cast_mut().cast())
        })
    }
}
extern "C" fn clip_define(e: *mut c_void, name: *const c_char, props: *mut *mut c_void) -> Status {
    // SAFETY: as for get_property_set.
    let (Some(e), Some(name)) = (unsafe { effect(e) }, unsafe { text(name) }) else {
        return ERR_BAD_HANDLE;
    };
    let p = PropSet::default();
    p.string("OfxPropType", "OfxTypeClip");
    p.string("OfxPropName", &name);
    p.string("OfxPropLabel", &name);
    p.strings("OfxImageEffectPropSupportedComponents", &[]);
    p.int("OfxImageEffectPropTemporalClipAccess", &[0]);
    p.int("OfxImageClipPropOptional", &[0]);
    p.int("OfxImageClipPropIsMask", &[0]);
    p.string("OfxImageClipPropFieldExtraction", "OfxImageFieldDoubled");
    p.int("OfxImageEffectPropSupportsTiles", &[1]);
    let handle = e.add_clip(&name, p);
    if props.is_null() {
        return OK;
    }
    // SAFETY: `props` is the plugin's out pointer.
    unsafe { out(props, handle) }
}
extern "C" fn clip_get_handle(
    e: *mut c_void,
    name: *const c_char,
    c: *mut *mut c_void,
    props: *mut *mut c_void,
) -> Status {
    // SAFETY: as for get_property_set.
    let (Some(e), Some(name)) = (unsafe { effect(e) }, unsafe { text(name) }) else {
        return ERR_BAD_HANDLE;
    };
    let clips = lock(&e.clips);
    let Some(found) = clips.get(&name) else {
        return ERR_UNKNOWN;
    };
    let handle = std::ptr::from_ref(found.as_ref()).cast_mut().cast();
    if !props.is_null() {
        // SAFETY: the plugin's out pointer.
        unsafe { out(props, found.props.handle()) };
    }
    // SAFETY: the plugin's out pointer.
    unsafe { out(c, handle) }
}
extern "C" fn clip_get_property_set(c: *mut c_void, props: *mut *mut c_void) -> Status {
    // SAFETY: as for get_property_set.
    unsafe { clip(c).map_or(ERR_BAD_HANDLE, |c| out(props, c.props.handle())) }
}
extern "C" fn clip_get_image(
    c: *mut c_void,
    _time: c_double,
    _region: *const RectD,
    image: *mut *mut c_void,
) -> Status {
    // SAFETY: as for get_property_set.
    let Some(c) = (unsafe { clip(c) }) else {
        return ERR_BAD_HANDLE;
    };
    let Some(pixels) = *lock(&c.pixels) else {
        return FAILED;
    };
    let p = Box::new(PropSet::default());
    p.string("OfxPropType", "OfxTypeImage");
    p.pointer("OfxImagePropData", pixels.data as *mut c_void);
    p.int("OfxImagePropBounds", &[0, 0, pixels.width, pixels.height]);
    p.int(
        "OfxImagePropRegionOfDefinition",
        &[0, 0, pixels.width, pixels.height],
    );
    p.int("OfxImagePropRowBytes", &[pixels.width * 16]);
    p.string("OfxImageEffectPropPixelDepth", "OfxBitDepthFloat");
    p.string("OfxImageEffectPropComponents", "OfxImageComponentRGBA");
    p.string(
        "OfxImageEffectPropPreMultiplication",
        "OfxImagePreMultiplied",
    );
    p.double("OfxImagePropPixelAspectRatio", &[1.0]);
    p.double("OfxImageEffectPropRenderScale", &[1.0, 1.0]);
    p.string("OfxImagePropField", "OfxImageFieldNone");
    p.string(
        "OfxImagePropUniqueIdentifier",
        &format!("{:x}", pixels.data),
    );
    // Freed by clip_release_image.
    let handle = Box::into_raw(p).cast();
    // SAFETY: the plugin's out pointer.
    unsafe { out(image, handle) }
}
extern "C" fn clip_release_image(image: *mut c_void) -> Status {
    if image.is_null() {
        return ERR_BAD_HANDLE;
    }
    // SAFETY: every image handle is a Box made by clip_get_image, released once.
    drop(unsafe { Box::from_raw(image.cast::<PropSet>()) });
    OK
}
extern "C" fn clip_get_region_of_definition(
    c: *mut c_void,
    _time: c_double,
    bounds: *mut RectD,
) -> Status {
    // SAFETY: as for get_property_set.
    let Some(c) = (unsafe { clip(c) }) else {
        return ERR_BAD_HANDLE;
    };
    let (w, h) = lock(&c.pixels).map_or((0, 0), |p| (p.width, p.height));
    let rect = RectD {
        x1: 0.0,
        y1: 0.0,
        x2: f64::from(w),
        y2: f64::from(h),
    };
    // SAFETY: the plugin's out pointer.
    unsafe { out(bounds, rect) }
}
extern "C" fn abort(_e: *mut c_void) -> c_int {
    0
}
/// Memory handed out by the memory suites: 16-byte aligned, its size kept in front.
fn allocate(bytes: usize) -> *mut c_void {
    let Ok(layout) = std::alloc::Layout::from_size_align(bytes.saturating_add(16), 16) else {
        return std::ptr::null_mut();
    };
    // SAFETY: the layout has a non-zero size (at least 16).
    let base = unsafe { std::alloc::alloc(layout) };
    if base.is_null() {
        return base.cast();
    }
    // SAFETY: the block is at least 16 bytes; its first 8 hold the size.
    unsafe {
        base.cast::<usize>().write(bytes);
        base.add(16).cast()
    }
}
/// # Safety
/// `p` is null or came from [`allocate`] and was not freed yet.
unsafe fn release(p: *mut c_void) {
    if p.is_null() {
        return;
    }
    // SAFETY: allocate put the size 16 bytes before the pointer it returned.
    unsafe {
        let base = p.cast::<u8>().sub(16);
        let bytes = base.cast::<usize>().read();
        std::alloc::dealloc(
            base,
            std::alloc::Layout::from_size_align_unchecked(bytes + 16, 16),
        );
    }
}
extern "C" fn image_memory_alloc(
    _e: *mut c_void,
    bytes: usize,
    handle: *mut *mut c_void,
) -> Status {
    let p = allocate(bytes);
    if p.is_null() {
        return ERR_MEMORY;
    }
    // SAFETY: the plugin's out pointer.
    unsafe { out(handle, p) }
}
extern "C" fn image_memory_free(handle: *mut c_void) -> Status {
    // SAFETY: image memory handles are blocks from allocate.
    unsafe { release(handle) };
    OK
}
extern "C" fn image_memory_lock(handle: *mut c_void, p: *mut *mut c_void) -> Status {
    // SAFETY: the plugin's out pointer; the handle is the block itself.
    unsafe { out(p, handle) }
}
extern "C" fn image_memory_unlock(_handle: *mut c_void) -> Status {
    OK
}
/// OfxImageEffectSuiteV1.
#[repr(C)]
pub struct EffectSuite {
    get_property_set: extern "C" fn(*mut c_void, *mut *mut c_void) -> Status,
    get_param_set: extern "C" fn(*mut c_void, *mut *mut c_void) -> Status,
    clip_define: extern "C" fn(*mut c_void, *const c_char, *mut *mut c_void) -> Status,
    clip_get_handle:
        extern "C" fn(*mut c_void, *const c_char, *mut *mut c_void, *mut *mut c_void) -> Status,
    clip_get_property_set: extern "C" fn(*mut c_void, *mut *mut c_void) -> Status,
    clip_get_image: extern "C" fn(*mut c_void, c_double, *const RectD, *mut *mut c_void) -> Status,
    clip_release_image: extern "C" fn(*mut c_void) -> Status,
    clip_get_region_of_definition: extern "C" fn(*mut c_void, c_double, *mut RectD) -> Status,
    abort: extern "C" fn(*mut c_void) -> c_int,
    image_memory_alloc: extern "C" fn(*mut c_void, usize, *mut *mut c_void) -> Status,
    image_memory_free: extern "C" fn(*mut c_void) -> Status,
    image_memory_lock: extern "C" fn(*mut c_void, *mut *mut c_void) -> Status,
    image_memory_unlock: extern "C" fn(*mut c_void) -> Status,
}
pub static EFFECT_SUITE: EffectSuite = EffectSuite {
    get_property_set,
    get_param_set,
    clip_define,
    clip_get_handle,
    clip_get_property_set,
    clip_get_image,
    clip_release_image,
    clip_get_region_of_definition,
    abort,
    image_memory_alloc,
    image_memory_free,
    image_memory_lock,
    image_memory_unlock,
};
// ---- Parameter suite ----
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
    }
    if is_text(kind) {
        p.string("OfxParamPropDefault", "");
        p.string("OfxParamPropStringMode", "OfxParamStringIsSingleLine");
        p.int("OfxParamPropStringFilePathExists", &[1]);
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
// ---- Memory suite ----
extern "C" fn memory_alloc(_h: *mut c_void, bytes: usize, p: *mut *mut c_void) -> Status {
    let block = allocate(bytes);
    if block.is_null() {
        return ERR_MEMORY;
    }
    // SAFETY: the plugin's out pointer.
    unsafe { out(p, block) }
}
extern "C" fn memory_free(p: *mut c_void) -> Status {
    // SAFETY: blocks a plugin frees came from memory_alloc.
    unsafe { release(p) };
    OK
}
#[repr(C)]
pub struct MemorySuite {
    alloc: extern "C" fn(*mut c_void, usize, *mut *mut c_void) -> Status,
    free: extern "C" fn(*mut c_void) -> Status,
}
pub static MEMORY_SUITE: MemorySuite = MemorySuite {
    alloc: memory_alloc,
    free: memory_free,
};
// ---- Multi-thread suite ----
thread_local! {
    static INDEX: std::cell::Cell<Option<u32>> = const { std::cell::Cell::new(None) };
}
fn cpus() -> u32 {
    std::thread::available_parallelism().map_or(1, |n| n.get() as u32)
}
type ThreadFunction = extern "C" fn(c_uint, c_uint, *mut c_void);
extern "C" fn multi_thread(f: Option<ThreadFunction>, n: c_uint, arg: *mut c_void) -> Status {
    let Some(f) = f else { return ERR_VALUE };
    let n = if n == 0 { cpus() } else { n.min(256) };
    let arg = arg as usize;
    std::thread::scope(|s| {
        for i in 0..n {
            s.spawn(move || {
                INDEX.with(|c| c.set(Some(i)));
                f(i, n, arg as *mut c_void);
            });
        }
    });
    OK
}
extern "C" fn num_cpus(n: *mut c_uint) -> Status {
    // SAFETY: the plugin's out pointer.
    unsafe { out(n, cpus()) }
}
extern "C" fn thread_index(i: *mut c_uint) -> Status {
    // SAFETY: the plugin's out pointer.
    unsafe { out(i, INDEX.with(|c| c.get()).unwrap_or(0)) }
}
extern "C" fn is_spawned_thread() -> c_int {
    c_int::from(INDEX.with(|c| c.get()).is_some())
}
/// A recursive mutex a plugin locks and unlocks by handle.
#[derive(Default)]
struct PluginMutex {
    state: Mutex<(Option<std::thread::ThreadId>, u32)>,
    free: Condvar,
}
/// # Safety
/// `m` is null or a handle from mutex_create not yet destroyed.
unsafe fn plugin_mutex<'a>(m: *mut c_void) -> Option<&'a PluginMutex> {
    // SAFETY: the caller's promise.
    unsafe { m.cast::<PluginMutex>().as_ref() }
}
extern "C" fn mutex_create(m: *mut *mut c_void, count: c_int) -> Status {
    let mutex = Box::new(PluginMutex::default());
    if count > 0 {
        *lock(&mutex.state) = (Some(std::thread::current().id()), count as u32);
    }
    // SAFETY: the plugin's out pointer; the box is freed by mutex_destroy.
    unsafe { out(m, Box::into_raw(mutex).cast()) }
}
extern "C" fn mutex_destroy(m: *mut c_void) -> Status {
    if m.is_null() {
        return ERR_BAD_HANDLE;
    }
    // SAFETY: made by mutex_create, destroyed once.
    drop(unsafe { Box::from_raw(m.cast::<PluginMutex>()) });
    OK
}
fn try_take(m: &PluginMutex, wait: bool) -> bool {
    let me = std::thread::current().id();
    let mut state = lock(&m.state);
    loop {
        match state.0 {
            None => {
                *state = (Some(me), 1);
                return true;
            }
            Some(owner) if owner == me => {
                state.1 += 1;
                return true;
            }
            Some(_) if !wait => return false,
            Some(_) => state = m.free.wait(state).unwrap_or_else(|e| e.into_inner()),
        }
    }
}
extern "C" fn mutex_lock(m: *mut c_void) -> Status {
    // SAFETY: a handle from mutex_create.
    unsafe { plugin_mutex(m) }.map_or(ERR_BAD_HANDLE, |m| {
        try_take(m, true);
        OK
    })
}
extern "C" fn mutex_unlock(m: *mut c_void) -> Status {
    // SAFETY: a handle from mutex_create.
    let Some(m) = (unsafe { plugin_mutex(m) }) else {
        return ERR_BAD_HANDLE;
    };
    let mut state = lock(&m.state);
    if state.0 != Some(std::thread::current().id()) {
        return FAILED;
    }
    state.1 -= 1;
    if state.1 == 0 {
        state.0 = None;
        m.free.notify_one();
    }
    OK
}
extern "C" fn mutex_try_lock(m: *mut c_void) -> Status {
    // SAFETY: a handle from mutex_create.
    unsafe { plugin_mutex(m) }.map_or(
        ERR_BAD_HANDLE,
        |m| if try_take(m, false) { OK } else { FAILED },
    )
}
#[repr(C)]
pub struct ThreadSuite {
    multi_thread: extern "C" fn(Option<ThreadFunction>, c_uint, *mut c_void) -> Status,
    num_cpus: extern "C" fn(*mut c_uint) -> Status,
    index: extern "C" fn(*mut c_uint) -> Status,
    is_spawned: extern "C" fn() -> c_int,
    mutex_create: extern "C" fn(*mut *mut c_void, c_int) -> Status,
    mutex_destroy: extern "C" fn(*mut c_void) -> Status,
    mutex_lock: extern "C" fn(*mut c_void) -> Status,
    mutex_unlock: extern "C" fn(*mut c_void) -> Status,
    mutex_try_lock: extern "C" fn(*mut c_void) -> Status,
}
pub static THREAD_SUITE: ThreadSuite = ThreadSuite {
    multi_thread,
    num_cpus,
    index: thread_index,
    is_spawned: is_spawned_thread,
    mutex_create,
    mutex_destroy,
    mutex_lock,
    mutex_unlock,
    mutex_try_lock,
};
// ---- Messages, progress, timeline ----
/// What plugins said, oldest first (at most 64 kept).
pub static MESSAGES: Mutex<Vec<String>> = Mutex::new(Vec::new());
// message(handle, type, id, format, ...): the format's own arguments are variadic and
// are not read (the format text is kept as it is); a callee may ignore trailing
// arguments on these ABIs.
extern "C" fn message(
    _h: *mut c_void,
    kind: *const c_char,
    _id: *const c_char,
    format: *const c_char,
) -> Status {
    // SAFETY: C strings from the plugin.
    let (kind, format) = unsafe {
        (
            text(kind).unwrap_or_default(),
            text(format).unwrap_or_default(),
        )
    };
    let mut messages = lock(&MESSAGES);
    if messages.len() >= 64 {
        messages.remove(0);
    }
    let short = kind.trim_start_matches("OfxMessage").to_lowercase();
    messages.push(format!("{short}: {format}"));
    if kind == "OfxMessageQuestion" {
        REPLY_DEFAULT
    } else {
        OK
    }
}
extern "C" fn clear_message(_h: *mut c_void) -> Status {
    OK
}
#[repr(C)]
pub struct MessageSuite {
    message: extern "C" fn(*mut c_void, *const c_char, *const c_char, *const c_char) -> Status,
    persistent: extern "C" fn(*mut c_void, *const c_char, *const c_char, *const c_char) -> Status,
    clear: extern "C" fn(*mut c_void) -> Status,
}
/// V1 is the first entry of V2; one table serves both.
pub static MESSAGE_SUITE: MessageSuite = MessageSuite {
    message,
    persistent: message,
    clear: clear_message,
};
extern "C" fn progress_start(_e: *mut c_void, _label: *const c_char) -> Status {
    OK
}
extern "C" fn progress_update(_e: *mut c_void, _p: c_double) -> Status {
    OK
}
extern "C" fn progress_end(_e: *mut c_void) -> Status {
    OK
}
#[repr(C)]
pub struct ProgressSuite {
    start: extern "C" fn(*mut c_void, *const c_char) -> Status,
    update: extern "C" fn(*mut c_void, c_double) -> Status,
    end: extern "C" fn(*mut c_void) -> Status,
}
pub static PROGRESS_SUITE: ProgressSuite = ProgressSuite {
    start: progress_start,
    update: progress_update,
    end: progress_end,
};
extern "C" fn get_time(e: *mut c_void, t: *mut c_double) -> Status {
    // SAFETY: an effect handle and the plugin's out pointer.
    let now = unsafe { effect(e) }
        .and_then(|e| e.props.get_numbers("VeylvikTime").first().copied())
        .unwrap_or(0.0);
    // SAFETY: the plugin's out pointer.
    unsafe { out(t, now) }
}
extern "C" fn goto_time(_e: *mut c_void, _t: c_double) -> Status {
    OK
}
extern "C" fn get_time_bounds(e: *mut c_void, first: *mut c_double, last: *mut c_double) -> Status {
    // SAFETY: an effect handle.
    let range = unsafe { effect(e) }
        .map(|e| e.props.get_numbers("OfxImageEffectPropFrameRange"))
        .unwrap_or_default();
    // SAFETY: the plugin's out pointers.
    unsafe {
        out(first, range.first().copied().unwrap_or(0.0));
        out(last, range.get(1).copied().unwrap_or(0.0))
    }
}
#[repr(C)]
pub struct TimeLineSuite {
    get_time: extern "C" fn(*mut c_void, *mut c_double) -> Status,
    goto_time: extern "C" fn(*mut c_void, c_double) -> Status,
    get_time_bounds: extern "C" fn(*mut c_void, *mut c_double, *mut c_double) -> Status,
}
pub static TIMELINE_SUITE: TimeLineSuite = TimeLineSuite {
    get_time,
    goto_time,
    get_time_bounds,
};
