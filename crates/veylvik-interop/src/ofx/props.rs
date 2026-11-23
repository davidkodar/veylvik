//! OpenFX property sets: named arrays of pointers, strings, doubles or ints behind one lock, handed to plugins as raw handles; strings stay valid until their property is set again, as the standard requires.
use super::{ERR_BAD_HANDLE, ERR_BAD_INDEX, ERR_UNKNOWN, ERR_VALUE, OK, Status};
use std::collections::BTreeMap;
use std::ffi::{CStr, CString, c_char, c_double, c_int, c_void};
use std::sync::Mutex;

#[derive(Debug, Clone)]
pub enum Prop {
    Pointer(Vec<usize>),
    String(Vec<CString>),
    Double(Vec<f64>),
    Int(Vec<i32>),
}
impl Prop {
    fn len(&self) -> usize {
        match self {
            Prop::Pointer(v) => v.len(),
            Prop::String(v) => v.len(),
            Prop::Double(v) => v.len(),
            Prop::Int(v) => v.len(),
        }
    }
}
/// A property set. Its address is the handle a plugin holds.
#[derive(Debug, Default)]
pub struct PropSet {
    values: Mutex<BTreeMap<String, Prop>>,
}
impl Clone for PropSet {
    fn clone(&self) -> Self {
        Self {
            values: Mutex::new(self.lock().clone()),
        }
    }
}
impl PropSet {
    fn lock(&self) -> std::sync::MutexGuard<'_, BTreeMap<String, Prop>> {
        self.values.lock().unwrap_or_else(|e| e.into_inner())
    }
    pub fn handle(&self) -> *mut c_void {
        std::ptr::from_ref(self).cast_mut().cast()
    }
    pub fn set(&self, name: &str, prop: Prop) {
        self.lock().insert(name.to_string(), prop);
    }
    pub fn string(&self, name: &str, value: &str) {
        self.strings(name, &[value]);
    }
    pub fn strings(&self, name: &str, values: &[&str]) {
        let values = values
            .iter()
            .map(|v| CString::new(*v).unwrap_or_default())
            .collect();
        self.set(name, Prop::String(values));
    }
    pub fn int(&self, name: &str, values: &[i32]) {
        self.set(name, Prop::Int(values.to_vec()));
    }
    pub fn double(&self, name: &str, values: &[f64]) {
        self.set(name, Prop::Double(values.to_vec()));
    }
    pub fn pointer(&self, name: &str, value: *mut c_void) {
        self.set(name, Prop::Pointer(vec![value as usize]));
    }
    pub fn get(&self, name: &str) -> Option<Prop> {
        self.lock().get(name).cloned()
    }
    pub fn get_string(&self, name: &str, index: usize) -> Option<String> {
        match self.get(name)? {
            Prop::String(v) => v.get(index).map(|s| s.to_string_lossy().into_owned()),
            _ => None,
        }
    }
    pub fn get_strings(&self, name: &str) -> Vec<String> {
        match self.get(name) {
            Some(Prop::String(v)) => v.iter().map(|s| s.to_string_lossy().into_owned()).collect(),
            _ => Vec::new(),
        }
    }
    /// Numbers of a property, ints read as doubles.
    pub fn get_numbers(&self, name: &str) -> Vec<f64> {
        match self.get(name) {
            Some(Prop::Double(v)) => v,
            Some(Prop::Int(v)) => v.into_iter().map(f64::from).collect(),
            _ => Vec::new(),
        }
    }
    pub fn get_pointer(&self, name: &str) -> Option<usize> {
        match self.get(name)? {
            Prop::Pointer(v) => v.first().copied(),
            _ => None,
        }
    }
    pub fn names(&self) -> Vec<String> {
        self.lock().keys().cloned().collect()
    }
}
/// The property set behind a handle.
///
/// # Safety
/// `handle` is null or came from [`PropSet::handle`] of a set that is still alive.
unsafe fn set<'a>(handle: *mut c_void) -> Option<&'a PropSet> {
    // SAFETY: the caller promises the handle is null or a live PropSet.
    unsafe { handle.cast::<PropSet>().as_ref() }
}
/// The name a plugin passed.
///
/// # Safety
/// `name` is null or a NUL-terminated string.
unsafe fn name(name: *const c_char) -> Option<String> {
    if name.is_null() {
        return None;
    }
    // SAFETY: a non-null name from a plugin is a C string, as the standard requires.
    Some(
        unsafe { CStr::from_ptr(name) }
            .to_string_lossy()
            .into_owned(),
    )
}
/// Writes one value of `kind` at `index`, growing the property as needed.
///
/// # Safety
/// `handle` and `property` as for [`set`] and [`name`].
unsafe fn put(handle: *mut c_void, property: *const c_char, index: c_int, value: Prop) -> Status {
    // SAFETY: forwarded from the suite function's own contract.
    let (Some(set), Some(property)) = (unsafe { set(handle) }, unsafe { name(property) }) else {
        return ERR_BAD_HANDLE;
    };
    let Ok(index) = usize::try_from(index) else {
        return ERR_BAD_INDEX;
    };
    let mut values = set.lock();
    let entry = values.entry(property).or_insert_with(|| match &value {
        Prop::Pointer(_) => Prop::Pointer(Vec::new()),
        Prop::String(_) => Prop::String(Vec::new()),
        Prop::Double(_) => Prop::Double(Vec::new()),
        Prop::Int(_) => Prop::Int(Vec::new()),
    });
    // A plugin may set an int property with a double or the other way; keep the first kind.
    match (entry, value) {
        (Prop::Pointer(v), Prop::Pointer(n)) => place(v, index, n[0]),
        (Prop::String(v), Prop::String(n)) => place(v, index, n[0].clone()),
        (Prop::Double(v), Prop::Double(n)) => place(v, index, n[0]),
        (Prop::Double(v), Prop::Int(n)) => place(v, index, f64::from(n[0])),
        (Prop::Int(v), Prop::Int(n)) => place(v, index, n[0]),
        (Prop::Int(v), Prop::Double(n)) => place(v, index, n[0] as i32),
        (entry, value) => *entry = value,
    }
    OK
}
fn place<T: Clone + Default>(values: &mut Vec<T>, index: usize, value: T) {
    if values.len() <= index {
        values.resize(index + 1, T::default());
    }
    values[index] = value;
}
/// Reads one value, or why not.
///
/// # Safety
/// As for [`put`].
unsafe fn take(handle: *mut c_void, property: *const c_char, index: c_int) -> Result<Prop, Status> {
    // SAFETY: forwarded from the suite function's own contract.
    let (Some(set), Some(property)) = (unsafe { set(handle) }, unsafe { name(property) }) else {
        return Err(ERR_BAD_HANDLE);
    };
    let values = set.lock();
    let prop = values.get(&property).ok_or(ERR_UNKNOWN)?;
    let index = usize::try_from(index).map_err(|_| ERR_BAD_INDEX)?;
    if index >= prop.len() {
        return Err(ERR_BAD_INDEX);
    }
    Ok(match prop {
        Prop::Pointer(v) => Prop::Pointer(vec![v[index]]),
        // The string itself stays in the set; hand back its address only.
        Prop::String(v) => Prop::Pointer(vec![v[index].as_ptr() as usize]),
        Prop::Double(v) => Prop::Double(vec![v[index]]),
        Prop::Int(v) => Prop::Int(vec![v[index]]),
    })
}
extern "C" fn set_pointer(h: *mut c_void, p: *const c_char, i: c_int, v: *mut c_void) -> Status {
    // SAFETY: the plugin passes handles we gave it and C strings.
    unsafe { put(h, p, i, Prop::Pointer(vec![v as usize])) }
}
extern "C" fn set_string(h: *mut c_void, p: *const c_char, i: c_int, v: *const c_char) -> Status {
    // SAFETY: a value string from a plugin is a C string (or null, read as empty).
    let value = unsafe { name(v) }.unwrap_or_default();
    // SAFETY: as for set_pointer.
    unsafe {
        put(
            h,
            p,
            i,
            Prop::String(vec![CString::new(value).unwrap_or_default()]),
        )
    }
}
extern "C" fn set_double(h: *mut c_void, p: *const c_char, i: c_int, v: c_double) -> Status {
    // SAFETY: as for set_pointer.
    unsafe { put(h, p, i, Prop::Double(vec![v])) }
}
extern "C" fn set_int(h: *mut c_void, p: *const c_char, i: c_int, v: c_int) -> Status {
    // SAFETY: as for set_pointer.
    unsafe { put(h, p, i, Prop::Int(vec![v])) }
}
/// Sets `count` values read from `values` with `one`.
fn set_n(count: c_int, mut one: impl FnMut(c_int) -> Status) -> Status {
    for i in 0..count.max(0) {
        let status = one(i);
        if status != OK {
            return status;
        }
    }
    OK
}
extern "C" fn set_pointer_n(
    h: *mut c_void,
    p: *const c_char,
    n: c_int,
    v: *const *mut c_void,
) -> Status {
    if v.is_null() {
        return ERR_VALUE;
    }
    // SAFETY: the plugin passes `n` pointers at `v`.
    set_n(n, |i| set_pointer(h, p, i, unsafe { *v.add(i as usize) }))
}
extern "C" fn set_string_n(
    h: *mut c_void,
    p: *const c_char,
    n: c_int,
    v: *const *const c_char,
) -> Status {
    if v.is_null() {
        return ERR_VALUE;
    }
    // SAFETY: the plugin passes `n` strings at `v`.
    set_n(n, |i| set_string(h, p, i, unsafe { *v.add(i as usize) }))
}
extern "C" fn set_double_n(
    h: *mut c_void,
    p: *const c_char,
    n: c_int,
    v: *const c_double,
) -> Status {
    if v.is_null() {
        return ERR_VALUE;
    }
    // SAFETY: the plugin passes `n` doubles at `v`.
    set_n(n, |i| set_double(h, p, i, unsafe { *v.add(i as usize) }))
}
extern "C" fn set_int_n(h: *mut c_void, p: *const c_char, n: c_int, v: *const c_int) -> Status {
    if v.is_null() {
        return ERR_VALUE;
    }
    // SAFETY: the plugin passes `n` ints at `v`.
    set_n(n, |i| set_int(h, p, i, unsafe { *v.add(i as usize) }))
}
/// Writes what [`take`] read into the plugin's `out`, if the kinds agree.
///
/// # Safety
/// `out` is null or points to writable storage of the kind asked for.
unsafe fn give<T>(
    read: Result<Prop, Status>,
    out: *mut T,
    pick: impl Fn(Prop) -> Option<T>,
) -> Status {
    if out.is_null() {
        return ERR_VALUE;
    }
    match read.map(pick) {
        // SAFETY: the caller promises `out` is writable storage for a T.
        Ok(Some(value)) => unsafe {
            out.write(value);
            OK
        },
        Ok(None) => ERR_VALUE,
        Err(status) => status,
    }
}
extern "C" fn get_pointer(
    h: *mut c_void,
    p: *const c_char,
    i: c_int,
    out: *mut *mut c_void,
) -> Status {
    // SAFETY: handles, names and `out` come from the plugin as the standard requires.
    unsafe {
        give(take(h, p, i), out, |v| match v {
            Prop::Pointer(v) => Some(v[0] as *mut c_void),
            _ => None,
        })
    }
}
extern "C" fn get_string(
    h: *mut c_void,
    p: *const c_char,
    i: c_int,
    out: *mut *const c_char,
) -> Status {
    // SAFETY: as for get_pointer. A string property is read back as its address.
    unsafe {
        let read = take(h, p, i);
        let is_string = matches!(
            set(h).and_then(|s| name(p).and_then(|n| s.get(&n))),
            Some(Prop::String(_))
        );
        give(read, out, |v| match v {
            Prop::Pointer(v) if is_string => Some(v[0] as *const c_char),
            _ => None,
        })
    }
}
extern "C" fn get_double(h: *mut c_void, p: *const c_char, i: c_int, out: *mut c_double) -> Status {
    // SAFETY: as for get_pointer.
    unsafe {
        give(take(h, p, i), out, |v| match v {
            Prop::Double(v) => Some(v[0]),
            Prop::Int(v) => Some(f64::from(v[0])),
            _ => None,
        })
    }
}
extern "C" fn get_int(h: *mut c_void, p: *const c_char, i: c_int, out: *mut c_int) -> Status {
    // SAFETY: as for get_pointer.
    unsafe {
        give(take(h, p, i), out, |v| match v {
            Prop::Int(v) => Some(v[0]),
            Prop::Double(v) => Some(v[0] as i32),
            _ => None,
        })
    }
}
/// Reads `count` values into `out` with `one`.
fn get_n(count: c_int, mut one: impl FnMut(c_int) -> Status) -> Status {
    set_n(count, &mut one)
}
extern "C" fn get_pointer_n(
    h: *mut c_void,
    p: *const c_char,
    n: c_int,
    out: *mut *mut c_void,
) -> Status {
    if out.is_null() {
        return ERR_VALUE;
    }
    // SAFETY: the plugin gives room for `n` values at `out`.
    get_n(n, |i| get_pointer(h, p, i, unsafe { out.add(i as usize) }))
}
extern "C" fn get_string_n(
    h: *mut c_void,
    p: *const c_char,
    n: c_int,
    out: *mut *const c_char,
) -> Status {
    if out.is_null() {
        return ERR_VALUE;
    }
    // SAFETY: as for get_pointer_n.
    get_n(n, |i| get_string(h, p, i, unsafe { out.add(i as usize) }))
}
extern "C" fn get_double_n(
    h: *mut c_void,
    p: *const c_char,
    n: c_int,
    out: *mut c_double,
) -> Status {
    if out.is_null() {
        return ERR_VALUE;
    }
    // SAFETY: as for get_pointer_n.
    get_n(n, |i| get_double(h, p, i, unsafe { out.add(i as usize) }))
}
extern "C" fn get_int_n(h: *mut c_void, p: *const c_char, n: c_int, out: *mut c_int) -> Status {
    if out.is_null() {
        return ERR_VALUE;
    }
    // SAFETY: as for get_pointer_n.
    get_n(n, |i| get_int(h, p, i, unsafe { out.add(i as usize) }))
}
extern "C" fn reset(h: *mut c_void, p: *const c_char) -> Status {
    // SAFETY: as for get_pointer.
    match unsafe { (set(h), name(p)) } {
        (Some(_), Some(_)) => OK,
        _ => ERR_BAD_HANDLE,
    }
}
extern "C" fn dimension(h: *mut c_void, p: *const c_char, out: *mut c_int) -> Status {
    // SAFETY: as for get_pointer.
    let (Some(set), Some(property)) = (unsafe { set(h) }, unsafe { name(p) }) else {
        return ERR_BAD_HANDLE;
    };
    let Some(prop) = set.get(&property) else {
        return ERR_UNKNOWN;
    };
    if out.is_null() {
        return ERR_VALUE;
    }
    // SAFETY: `out` is the plugin's int.
    unsafe { out.write(prop.len() as c_int) };
    OK
}
/// The OfxPropertySuiteV1 table, field order as in the standard.
#[repr(C)]
pub struct Suite {
    set_pointer: extern "C" fn(*mut c_void, *const c_char, c_int, *mut c_void) -> Status,
    set_string: extern "C" fn(*mut c_void, *const c_char, c_int, *const c_char) -> Status,
    set_double: extern "C" fn(*mut c_void, *const c_char, c_int, c_double) -> Status,
    set_int: extern "C" fn(*mut c_void, *const c_char, c_int, c_int) -> Status,
    set_pointer_n: extern "C" fn(*mut c_void, *const c_char, c_int, *const *mut c_void) -> Status,
    set_string_n: extern "C" fn(*mut c_void, *const c_char, c_int, *const *const c_char) -> Status,
    set_double_n: extern "C" fn(*mut c_void, *const c_char, c_int, *const c_double) -> Status,
    set_int_n: extern "C" fn(*mut c_void, *const c_char, c_int, *const c_int) -> Status,
    get_pointer: extern "C" fn(*mut c_void, *const c_char, c_int, *mut *mut c_void) -> Status,
    get_string: extern "C" fn(*mut c_void, *const c_char, c_int, *mut *const c_char) -> Status,
    get_double: extern "C" fn(*mut c_void, *const c_char, c_int, *mut c_double) -> Status,
    get_int: extern "C" fn(*mut c_void, *const c_char, c_int, *mut c_int) -> Status,
    get_pointer_n: extern "C" fn(*mut c_void, *const c_char, c_int, *mut *mut c_void) -> Status,
    get_string_n: extern "C" fn(*mut c_void, *const c_char, c_int, *mut *const c_char) -> Status,
    get_double_n: extern "C" fn(*mut c_void, *const c_char, c_int, *mut c_double) -> Status,
    get_int_n: extern "C" fn(*mut c_void, *const c_char, c_int, *mut c_int) -> Status,
    reset: extern "C" fn(*mut c_void, *const c_char) -> Status,
    dimension: extern "C" fn(*mut c_void, *const c_char, *mut c_int) -> Status,
}
pub static SUITE: Suite = Suite {
    set_pointer,
    set_string,
    set_double,
    set_int,
    set_pointer_n,
    set_string_n,
    set_double_n,
    set_int_n,
    get_pointer,
    get_string,
    get_double,
    get_int,
    get_pointer_n,
    get_string_n,
    get_double_n,
    get_int_n,
    reset,
    dimension,
};
