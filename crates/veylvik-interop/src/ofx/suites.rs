//! The host's OpenFX effect function table (clips and images) and memory suite, and the objects behind effect, clip and parameter handles shared with `params` and `services`.
use super::props::PropSet;
use super::{ERR_BAD_HANDLE, ERR_MEMORY, ERR_UNKNOWN, ERR_VALUE, FAILED, OK, Status};
use std::collections::BTreeMap;
use std::ffi::{CStr, CString, c_char, c_double, c_int, c_void};
use std::sync::Mutex;

pub(super) fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}
/// A C string from a plugin.
///
/// # Safety
/// `s` is null or NUL-terminated.
pub(super) unsafe fn text(s: *const c_char) -> Option<String> {
    // SAFETY: the caller's promise.
    (!s.is_null()).then(|| unsafe { CStr::from_ptr(s) }.to_string_lossy().into_owned())
}
/// Writes `value` to the plugin's `out`, if it gave one.
///
/// # Safety
/// `out` is null or writable storage for a T.
pub(super) unsafe fn out<T>(out: *mut T, value: T) -> Status {
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
    /// A curve setting's control points ([key, value], in key order) per curve.
    Curves(Vec<Vec<[f64; 2]>>),
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
pub(super) unsafe fn effect<'a>(h: *mut c_void) -> Option<&'a Effect> {
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
pub(super) unsafe fn param<'a>(h: *mut c_void) -> Option<&'a Param> {
    // SAFETY: the caller's promise.
    unsafe { h.cast::<Param>().as_ref() }
}
/// # Safety
/// `h` is null or a parameter set handle we handed out that is still alive.
pub(super) unsafe fn param_set<'a>(h: *mut c_void) -> Option<&'a ParamSet> {
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
    p.string("OfxImageClipPropFieldExtraction", "OfxFieldDoubled");
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
        "OfxImageAlphaPremultiplied",
    );
    p.double("OfxImagePropPixelAspectRatio", &[1.0]);
    p.double("OfxImageEffectPropRenderScale", &[1.0, 1.0]);
    p.string("OfxImagePropField", "OfxFieldNone");
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
