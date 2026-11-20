//! Played pictures as the screen shows them, packed without loss: the newest in memory, older ones on disk, the oldest let go, each within its own limit. A picture is known by what it is made of and the look it was packed with.
use crate::{Result, error, screen::Look};
use std::{collections::VecDeque, path::PathBuf};
use veylvik_document::Time;

/// How hard to pack: the fastest setting already halves a grainy 4K picture in about
/// 15 ms, and harder ones gain little for much more time.
const LEVEL: i32 = 1;
/// Moments remembered for one picture: a still shows at every moment of a long film.
const MOMENTS: usize = 16384;

struct Picture {
    name: Option<u64>,
    times: Vec<Time>,
    look: Look,
    size: [u32; 2],
    memory: Option<Vec<u8>>,
    disk: Option<(PathBuf, u64)>,
}
#[derive(Clone, Copy, Default)]
pub(crate) struct Limits {
    pub memory: u64,
    pub disk: u64,
}
#[derive(Default)]
pub(crate) struct Pictures {
    entries: VecDeque<Picture>,
    serial: u64,
}
/// A picture packed without loss.
pub(crate) fn pack(picture: &[u8]) -> Result<Vec<u8>> {
    zstd::bulk::compress(picture, LEVEL).map_err(error)
}
fn same(a: Time, b: Time) -> bool {
    i128::from(a.ticks) * i128::from(b.rate) == i128::from(b.ticks) * i128::from(a.rate)
}
impl Pictures {
    fn position(&self, time: Time, name: Option<u64>, look: Look) -> Option<usize> {
        self.entries.iter().position(|e| {
            e.look == look
                && match name {
                    Some(_) => e.name == name,
                    None => e.name.is_none() && e.times.iter().any(|t| same(*t, time)),
                }
        })
    }
    /// The picture for this moment, unpacked: its bytes as the screen packs them and its
    /// size. One that cannot be read any more is forgotten.
    pub fn get(
        &mut self,
        time: Time,
        name: Option<u64>,
        look: Look,
    ) -> Option<(Vec<u8>, [u32; 2])> {
        let index = self.position(time, name, look)?;
        let mut entry = self.entries.remove(index)?;
        let expected = entry.size[0] as usize * entry.size[1] as usize * 4;
        let packed = match (&entry.memory, &entry.disk) {
            (Some(bytes), _) => Some(std::borrow::Cow::Borrowed(bytes.as_slice())),
            (None, Some((path, _))) => std::fs::read(path).ok().map(std::borrow::Cow::Owned),
            _ => None,
        };
        let picture = packed
            .and_then(|packed| zstd::bulk::decompress(&packed, expected).ok())
            .filter(|bytes| bytes.len() == expected);
        let Some(picture) = picture else {
            if let Some((path, _)) = entry.disk {
                let _ = std::fs::remove_file(path);
            }
            return None;
        };
        if !entry.times.iter().any(|t| same(*t, time)) && entry.times.len() < MOMENTS {
            entry.times.push(time);
        }
        let size = entry.size;
        self.entries.push_back(entry);
        Some((picture, size))
    }
    /// Whether this moment's picture is already kept (and note the moment if it is).
    pub fn has(&mut self, time: Time, name: Option<u64>, look: Look) -> bool {
        let Some(index) = self.position(time, name, look) else {
            return false;
        };
        let entry = &mut self.entries[index];
        if !entry.times.iter().any(|t| same(*t, time)) && entry.times.len() < MOMENTS {
            entry.times.push(time);
        }
        true
    }
    /// Keep `picture` (as the screen packs it) for this moment, then make everything fit
    /// `limits`: the oldest in memory go to `folder`, the oldest there are deleted.
    pub fn put(
        &mut self,
        (time, name, look): (Time, Option<u64>, Look),
        size: [u32; 2],
        picture: &[u8],
        limits: Limits,
        folder: Option<&std::path::Path>,
    ) -> Result<()> {
        self.put_packed((time, name, look), size, pack(picture)?, limits, folder)
    }
    /// As [`Pictures::put`], with the picture already packed by [`pack`].
    pub fn put_packed(
        &mut self,
        (time, name, look): (Time, Option<u64>, Look),
        size: [u32; 2],
        packed: Vec<u8>,
        limits: Limits,
        folder: Option<&std::path::Path>,
    ) -> Result<()> {
        if let Some(index) = self.position(time, name, look) {
            self.entries.remove(index);
        }
        self.entries.push_back(Picture {
            name,
            times: vec![time],
            look,
            size,
            memory: Some(packed),
            disk: None,
        });
        self.fit(limits, folder)
    }
    pub fn fit(&mut self, limits: Limits, folder: Option<&std::path::Path>) -> Result<()> {
        let mut failure = None;
        // Memory: the oldest move to disk when there is a disk to move to.
        while self.memory_bytes() > limits.memory {
            let Some(entry) = self.entries.iter_mut().find(|e| e.memory.is_some()) else {
                break;
            };
            let bytes = entry.memory.take().unwrap_or_default();
            if entry.disk.is_none()
                && let Some(folder) = folder
                && bytes.len() as u64 <= limits.disk
            {
                self.serial = self.serial.wrapping_add(1);
                let path = folder.join(format!("{}.picture.zst", self.serial));
                match std::fs::write(&path, &bytes) {
                    Ok(()) => entry.disk = Some((path, bytes.len() as u64)),
                    Err(e) => {
                        let _ = std::fs::remove_file(path);
                        failure = Some(error(format!("Disk cache unavailable: {e}")));
                    }
                }
            }
        }
        while self.disk_bytes() > limits.disk {
            let Some(entry) = self.entries.iter_mut().find(|e| e.disk.is_some()) else {
                break;
            };
            if let Some((path, _)) = entry.disk.take() {
                let _ = std::fs::remove_file(path);
            }
        }
        self.entries
            .retain(|e| e.memory.is_some() || e.disk.is_some());
        failure.map_or(Ok(()), Err)
    }
    /// Another plan over the same sources: a picture known by what it is made of stays,
    /// and which moments show it is found again as they are asked for.
    pub fn edited(&mut self) {
        self.entries.retain(|e| {
            let keep = e.name.is_some();
            if !keep && let Some((path, _)) = &e.disk {
                let _ = std::fs::remove_file(path);
            }
            keep
        });
        for entry in &mut self.entries {
            entry.times.clear();
        }
    }
    pub fn clear(&mut self) {
        for entry in self.entries.drain(..) {
            if let Some((path, _)) = entry.disk {
                let _ = std::fs::remove_file(path);
            }
        }
    }
    /// Every moment whose picture is kept for `look`.
    pub fn times(&self, look: Look) -> Vec<Time> {
        self.entries
            .iter()
            .filter(|e| e.look == look)
            .flat_map(|e| e.times.iter().copied())
            .collect()
    }
    pub fn memory_bytes(&self) -> u64 {
        self.entries
            .iter()
            .filter_map(|e| e.memory.as_ref())
            .map(|bytes| bytes.len() as u64)
            .sum()
    }
    pub fn disk_bytes(&self) -> u64 {
        self.entries
            .iter()
            .filter_map(|e| e.disk.as_ref())
            .map(|d| d.1)
            .sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn picture(seed: u8) -> Vec<u8> {
        // Noise does not pack, so sizes below are close to the picture's own.
        let mut state = u32::from(seed).wrapping_mul(2654435761).wrapping_add(1);
        (0..64 * 32 * 4)
            .map(|_| {
                state = state.wrapping_mul(1664525).wrapping_add(1013904223);
                (state >> 24) as u8
            })
            .collect()
    }
    #[test]
    fn pictures_move_from_memory_to_disk_and_the_oldest_are_let_go() {
        let folder = std::env::temp_dir().join(format!(
            "veylvik-pictures-test-{}",
            veylvik_document::NodeId::new_v4()
        ));
        std::fs::create_dir_all(&folder).unwrap();
        let look = Look {
            exposure: 0.0,
            filmic: true,
        };
        let at = |frame: i64| Time::new(frame, 24).unwrap();
        let one = picture(0).len() as u64;
        // Room for two in memory and three on disk.
        let limits = Limits {
            memory: one * 2 + 600,
            disk: one * 3 + 900,
        };
        let mut pictures = Pictures::default();
        for frame in 0..8u8 {
            pictures
                .put(
                    (at(frame.into()), None, look),
                    [64, 32],
                    &picture(frame),
                    limits,
                    Some(&folder),
                )
                .unwrap();
        }
        assert!(pictures.memory_bytes() <= limits.memory && pictures.disk_bytes() <= limits.disk);
        assert_eq!(std::fs::read_dir(&folder).unwrap().count(), 3);
        // The three oldest are gone; the rest come back exactly, from disk and from memory.
        for frame in 0..8u8 {
            let got = pictures.get(at(frame.into()), None, look);
            assert_eq!(got.is_some(), frame >= 3, "frame {frame}");
            if let Some((bytes, size)) = got {
                assert!(bytes == picture(frame) && size == [64, 32]);
            }
        }
        // Another look is another picture.
        assert!(pictures.get(at(7), None, Look::NEUTRAL).is_none());
        // A picture known by what it is made of is one picture at every moment, and stays
        // through an edit; one known only by its moment does not.
        pictures
            .put(
                (at(20), Some(99), look),
                [64, 32],
                &picture(20),
                limits,
                Some(&folder),
            )
            .unwrap();
        assert!(pictures.get(at(21), Some(99), look).is_some());
        assert_eq!(
            pictures
                .times(look)
                .iter()
                .filter(|t| t.ticks >= 20)
                .count(),
            2
        );
        pictures.edited();
        assert!(pictures.get(at(7), None, look).is_none());
        assert!(
            pictures
                .get(at(40), Some(99), look)
                .is_some_and(|(bytes, _)| bytes == picture(20))
        );
        assert_eq!(pictures.times(look).len(), 1);
        // A file damaged on disk is a miss, not a wrong picture.
        let limits = Limits {
            memory: 0,
            disk: one * 3 + 900,
        };
        pictures.fit(limits, Some(&folder)).unwrap();
        let file = std::fs::read_dir(&folder)
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        std::fs::write(file, b"broken").unwrap();
        assert!(pictures.get(at(40), Some(99), look).is_none());
        pictures.clear();
        assert_eq!(std::fs::read_dir(&folder).unwrap().count(), 0);
        std::fs::remove_dir(folder).unwrap();
    }
}
