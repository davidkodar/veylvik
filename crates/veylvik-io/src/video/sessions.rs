//! At most `MAX_OPEN` video decoders stay open across all clips: using one beyond that
//! closes the least recently used idle clip's decoder, which reopens on demand.
use super::State;
use std::collections::VecDeque;
use std::sync::{Mutex, Weak};

/// Enough for split screens and stacked layers; each open 1080p hardware session
/// costs roughly 90 MiB of graphics memory with its frames.
pub(super) const MAX_OPEN: usize = 12;

struct Entry {
    owner: u64,
    state: Weak<Mutex<State>>,
}
/// Clips that may hold a decoder, least recently used first.
static OPEN: Mutex<VecDeque<Entry>> = Mutex::new(VecDeque::new());

/// Mark `owner` as the most recent decoder user and close the oldest idle ones beyond
/// the limit. Called with the caller's own state locked: other clips are only ever
/// `try_lock`ed, so a busy clip is skipped instead of waited for and nothing deadlocks.
pub(super) fn touch(owner: u64, state: &std::sync::Arc<Mutex<State>>) {
    let mut open = OPEN.lock().unwrap_or_else(|e| e.into_inner());
    open.retain(|e| e.owner != owner && e.state.strong_count() > 0);
    open.push_back(Entry {
        owner,
        state: std::sync::Arc::downgrade(state),
    });
    let mut at = 0;
    while open.len() > MAX_OPEN && at + 1 < open.len() {
        let closed = open[at].state.upgrade().is_none_or(|victim| {
            victim.try_lock().is_ok_and(|mut victim| {
                close(open[at].owner, &mut victim);
                true
            })
        });
        if closed {
            open.remove(at);
        } else {
            at += 1;
        }
    }
}
/// Release a clip's decoder and the frames that pin it; the next request reopens it.
fn close(owner: u64, state: &mut State) {
    state.decoder = None;
    state.gpu = None;
    state.raw.clear();
    crate::host_cache::forget(owner, Some(super::RAW));
}
/// Open decoder sessions right now (for diagnostics and tests).
pub fn open_count() -> usize {
    OPEN.lock()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .filter(|e| {
            e.state
                .upgrade()
                .is_some_and(|s| s.try_lock().map_or(true, |s| s.decoder.is_some()))
        })
        .count()
}
