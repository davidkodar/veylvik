//! Memory shared between this process and a helper it started: an anonymous memory file (closed on exec, so no other child inherits it) that the helper opens through `/proc/<parent>/fd/<n>` and both map. Each side touches it only while the other waits on the pipe between them, so access never overlaps.
use memmap2::MmapMut;
use std::fs::File;
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};

pub type Result<T> = std::result::Result<T, String>;
/// Largest region: four inputs and the output at 8K.
pub const MAX_BYTES: u64 = 5 * 8192 * 8192 * 16;

pub struct Shared {
    file: File,
    map: Option<MmapMut>,
}
impl Shared {
    /// A new, empty region owned by this process.
    pub fn create() -> Result<Self> {
        let fd = rustix::fs::memfd_create("veylvik-frames", rustix::fs::MemfdFlags::CLOEXEC)
            .map_err(|e| format!("could not make shared memory: {e}"))?;
        Ok(Self {
            file: File::from(fd),
            map: None,
        })
    }
    /// The path a child of this process opens the region by.
    pub fn path(&self) -> PathBuf {
        PathBuf::from(format!(
            "/proc/{}/fd/{}",
            std::process::id(),
            self.file.as_raw_fd()
        ))
    }
    /// The region another process made, by its [`Shared::path`].
    pub fn open(path: &Path) -> Result<Self> {
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(path)
            .map_err(|e| format!("could not open shared memory {}: {e}", path.display()))?;
        Ok(Self { file, map: None })
    }
    /// Maps at least `bytes`, growing the region first when this side made it
    /// (`grow`); the other side maps what is there.
    pub fn reserve(&mut self, bytes: u64, grow: bool) -> Result<()> {
        if bytes > MAX_BYTES {
            return Err("frames too large for shared memory".into());
        }
        let len = self.file.metadata().map_err(|e| e.to_string())?.len();
        if grow && len < bytes {
            self.file
                .set_len(bytes)
                .map_err(|e| format!("could not grow shared memory: {e}"))?;
        } else if len < bytes {
            return Err("the shared memory is smaller than the frames sent".into());
        }
        let len = self.file.metadata().map_err(|e| e.to_string())?.len();
        if self.map.as_ref().is_none_or(|m| (m.len() as u64) != len) {
            self.map = None;
            // SAFETY: the file is a memory file of `len` bytes owned by this process and
            // its helper; neither shrinks it, and each side touches the mapping only while
            // the other waits for its message (see the module note), so no access races.
            let map = unsafe { MmapMut::map_mut(&self.file) }
                .map_err(|e| format!("could not map shared memory: {e}"))?;
            self.map = Some(map);
        }
        Ok(())
    }
    /// `count` floats from float index `at`.
    pub fn floats(&self, at: usize, count: usize) -> Result<&[f32]> {
        let map = self.map.as_ref().ok_or("shared memory is not mapped")?;
        let bytes = map
            .get(at * 4..(at + count) * 4)
            .ok_or("beyond the shared memory")?;
        bytemuck::try_cast_slice(bytes).map_err(|e| e.to_string())
    }
    /// The first `inputs` floats to read and the `output` floats after them to write, at
    /// once (they do not overlap).
    pub fn parts(&mut self, inputs: usize, output: usize) -> Result<(&[f32], &mut [f32])> {
        let all = self.floats_mut(0, inputs + output)?;
        let (read, write) = all.split_at_mut(inputs);
        Ok((read, write))
    }
    pub fn floats_mut(&mut self, at: usize, count: usize) -> Result<&mut [f32]> {
        let map = self.map.as_mut().ok_or("shared memory is not mapped")?;
        let bytes = map
            .get_mut(at * 4..(at + count) * 4)
            .ok_or("beyond the shared memory")?;
        bytemuck::try_cast_slice_mut(bytes).map_err(|e| e.to_string())
    }
}
