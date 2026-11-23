//! The OpenFX services a plugin leans on besides images and parameters: threads and mutexes, messages, progress and the timeline.
use super::suites::{effect, lock, out, text};
use super::{ERR_BAD_HANDLE, ERR_VALUE, FAILED, OK, REPLY_DEFAULT, Status};
use std::ffi::{c_char, c_double, c_int, c_uint, c_void};
use std::sync::{Condvar, Mutex};

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
// message(handle, type, id, format, ...): the format's arguments are variadic. Plugins
// built on the standard's support library always send the format "%s" with the text as
// its one argument; that argument arrives in the next integer register, exactly as a
// fifth declared pointer parameter would, so it is read when, and only when, the format
// asks for exactly one string. Any other format is kept as it is.
extern "C" fn message(
    _h: *mut c_void,
    kind: *const c_char,
    _id: *const c_char,
    format: *const c_char,
    first: *const c_char,
) -> Status {
    // SAFETY: C strings from the plugin.
    let (kind, format) = unsafe {
        (
            text(kind).unwrap_or_default(),
            text(format).unwrap_or_default(),
        )
    };
    let said = if format.matches('%').count() == 1 && format.contains("%s") {
        // SAFETY: the format promises one C string argument, which is `first`.
        format.replacen("%s", &unsafe { text(first) }.unwrap_or_default(), 1)
    } else {
        format
    };
    let mut messages = lock(&MESSAGES);
    if messages.len() >= 64 {
        messages.remove(0);
    }
    let short = kind.trim_start_matches("OfxMessage").to_lowercase();
    messages.push(format!("{short}: {said}"));
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
    message: extern "C" fn(
        *mut c_void,
        *const c_char,
        *const c_char,
        *const c_char,
        *const c_char,
    ) -> Status,
    persistent: extern "C" fn(
        *mut c_void,
        *const c_char,
        *const c_char,
        *const c_char,
        *const c_char,
    ) -> Status,
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
