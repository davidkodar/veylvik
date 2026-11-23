//! Baked flocks: where every particle of a simulated flock was at every frame, written once and read back frame by frame, so an approved flock stays as it is whatever card or driver renders it.
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use veylvik_document::Time;

const MAGIC: &[u8; 8] = b"VEYFLOCK";
const VERSION: u32 = 1;
/// Most frames in one bake (an hour at 60 a second).
const MOST_FRAMES: u32 = 216_000;
type Result<T> = std::result::Result<T, String>;

/// What a bake holds: the flock's key, the composition's frame rate, its first frame
/// number, how many frames and particles.
#[derive(Debug, Clone, Copy)]
pub struct Header {
    pub key: u64,
    pub frame_rate: Time,
    pub first: i64,
    pub frames: u32,
    pub particles: u32,
}
/// Writes a bake to `path`, frame after frame from `next` (each frame's particles:
/// where and generation, negative when not alive), through a file beside it renamed
/// into place, so a bake is either whole or not there.
pub fn write(
    path: &Path,
    header: Header,
    mut next: impl FnMut(u32) -> Result<Vec<[f32; 4]>>,
) -> Result<()> {
    if header.frames == 0 || header.frames > MOST_FRAMES {
        return Err(format!("a bake holds 1 to {MOST_FRAMES} frames"));
    }
    let partial = path.with_extension("veyflock.partial");
    let made = (|| -> Result<()> {
        let mut file =
            std::io::BufWriter::new(std::fs::File::create(&partial).map_err(|e| e.to_string())?);
        let io = |e: std::io::Error| e.to_string();
        file.write_all(MAGIC).map_err(io)?;
        file.write_all(&VERSION.to_le_bytes()).map_err(io)?;
        file.write_all(&header.key.to_le_bytes()).map_err(io)?;
        file.write_all(&header.frame_rate.ticks.to_le_bytes())
            .map_err(io)?;
        file.write_all(&header.frame_rate.rate.to_le_bytes())
            .map_err(io)?;
        file.write_all(&header.first.to_le_bytes()).map_err(io)?;
        file.write_all(&header.frames.to_le_bytes()).map_err(io)?;
        file.write_all(&header.particles.to_le_bytes())
            .map_err(io)?;
        let mut at = (8 + 4 + 8 + 8 + 4 + 8 + 4 + 4) as u64;
        let mut table = Vec::with_capacity(header.frames as usize);
        for k in 0..header.frames {
            let states = next(k)?;
            if states.len() != header.particles as usize {
                return Err(format!(
                    "frame {k} has {} particles, not {}",
                    states.len(),
                    header.particles
                ));
            }
            let packed =
                zstd::encode_all(bytemuck::cast_slice::<[f32; 4], u8>(&states), 1).map_err(io)?;
            file.write_all(&packed).map_err(io)?;
            table.push((at, packed.len() as u64));
            at += packed.len() as u64;
        }
        for (offset, length) in &table {
            file.write_all(&offset.to_le_bytes()).map_err(io)?;
            file.write_all(&length.to_le_bytes()).map_err(io)?;
        }
        file.write_all(&at.to_le_bytes()).map_err(io)?;
        file.into_inner()
            .map_err(|e| e.to_string())?
            .sync_all()
            .map_err(io)
    })();
    if let Err(e) = made {
        let _ = std::fs::remove_file(&partial);
        return Err(e);
    }
    std::fs::rename(&partial, path).map_err(|e| e.to_string())
}
/// A frame read, with its number.
type Kept = (i64, Arc<Vec<[f32; 4]>>);
/// A bake, open for reading frames as they are asked for (the last one kept).
pub struct Bake {
    path: PathBuf,
    header: Header,
    table: Vec<(u64, u64)>,
    file: Mutex<std::fs::File>,
    last: Mutex<Option<Kept>>,
}
impl Bake {
    pub fn open(path: &Path) -> Result<Self> {
        let io = |e: std::io::Error| format!("{}: {e}", path.display());
        let mut file = std::fs::File::open(path).map_err(io)?;
        let mut head = [0u8; 48];
        file.read_exact(&mut head).map_err(io)?;
        if &head[..8] != MAGIC {
            return Err(format!("{} is not a baked flock", path.display()));
        }
        let u32_at = |k: usize| u32::from_le_bytes(head[k..k + 4].try_into().unwrap_or_default());
        let u64_at = |k: usize| u64::from_le_bytes(head[k..k + 8].try_into().unwrap_or_default());
        if u32_at(8) != VERSION {
            return Err(format!("{} was baked by another version", path.display()));
        }
        let header = Header {
            key: u64_at(12),
            frame_rate: Time::new(u64_at(20) as i64, u32_at(28)).map_err(|e| e.to_string())?,
            first: u64_at(32) as i64,
            frames: u32_at(40),
            particles: u32_at(44),
        };
        if header.frames == 0 || header.frames > MOST_FRAMES || header.particles > 4_000_000 {
            return Err(format!("{} is damaged", path.display()));
        }
        let length = file.metadata().map_err(io)?.len();
        file.seek(SeekFrom::End(-8)).map_err(io)?;
        let mut tail = [0u8; 8];
        file.read_exact(&mut tail).map_err(io)?;
        let at = u64::from_le_bytes(tail);
        if at + header.frames as u64 * 16 + 8 != length {
            return Err(format!("{} is damaged", path.display()));
        }
        file.seek(SeekFrom::Start(at)).map_err(io)?;
        let mut raw = vec![0u8; header.frames as usize * 16];
        file.read_exact(&mut raw).map_err(io)?;
        let table = raw
            .as_chunks::<16>()
            .0
            .iter()
            .map(|c| {
                (
                    u64::from_le_bytes([c[0], c[1], c[2], c[3], c[4], c[5], c[6], c[7]]),
                    u64::from_le_bytes([c[8], c[9], c[10], c[11], c[12], c[13], c[14], c[15]]),
                )
            })
            .collect::<Vec<_>>();
        if table.iter().any(|(o, l)| o + l > at) {
            return Err(format!("{} is damaged", path.display()));
        }
        Ok(Self {
            path: path.to_path_buf(),
            header,
            table,
            file: Mutex::new(file),
            last: Mutex::new(None),
        })
    }
    pub fn header(&self) -> Header {
        self.header
    }
    fn frame(&self, frame: i64) -> Result<Arc<Vec<[f32; 4]>>> {
        if let Some((k, states)) = self.last.lock().map_err(|_| "bake lock")?.as_ref()
            && *k == frame
        {
            return Ok(states.clone());
        }
        let index = frame - self.header.first;
        let &(offset, length) = usize::try_from(index)
            .ok()
            .and_then(|i| self.table.get(i))
            .ok_or_else(|| format!("frame {frame} is not in the bake"))?;
        let mut packed = vec![0u8; length as usize];
        {
            let mut file = self.file.lock().map_err(|_| "bake lock")?;
            file.seek(SeekFrom::Start(offset))
                .map_err(|e| e.to_string())?;
            file.read_exact(&mut packed)
                .map_err(|e| format!("{}: {e}", self.path.display()))?;
        }
        let bytes = zstd::decode_all(packed.as_slice()).map_err(|e| e.to_string())?;
        if bytes.len() != self.header.particles as usize * 16 {
            return Err(format!(
                "{} is damaged at frame {frame}",
                self.path.display()
            ));
        }
        let states = Arc::new(bytemuck::cast_slice::<u8, [f32; 4]>(&bytes).to_vec());
        *self.last.lock().map_err(|_| "bake lock")? = Some((frame, states.clone()));
        Ok(states)
    }
}
impl veylvik_render::BakedFlock for Bake {
    fn key(&self) -> u64 {
        self.header.key
    }
    fn frames(&self) -> std::ops::Range<i64> {
        self.header.first..self.header.first + i64::from(self.header.frames)
    }
    fn frame_rate(&self) -> Time {
        self.header.frame_rate
    }
    fn states(&self, frame: i64) -> veylvik_render::Result<Vec<[f32; 4]>> {
        self.frame(frame)
            .map(|s| s.as_ref().clone())
            .map_err(veylvik_render::Error::Message)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use veylvik_render::BakedFlock;
    #[test]
    fn a_bake_gives_back_every_frame_as_it_was_written() {
        let folder = std::env::temp_dir().join(format!("veylvik-flock-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let path = folder.join("birds.veyflock");
        let header = Header {
            key: 0x1234_5678_9abc,
            frame_rate: Time::new(24000, 1001).unwrap(),
            first: 10,
            frames: 5,
            particles: 300,
        };
        let frame = |k: u32| -> Vec<[f32; 4]> {
            (0..300)
                .map(|p| {
                    [
                        p as f32 * 1.5,
                        k as f32,
                        -(p as f32),
                        if p % 7 == 0 {
                            -1.0
                        } else {
                            1_048_576.0 + k as f32
                        },
                    ]
                })
                .collect()
        };
        write(&path, header, |k| Ok(frame(k))).unwrap();
        let bake = Bake::open(&path).unwrap();
        let read = bake.header();
        assert_eq!(
            (
                read.key,
                read.frame_rate.ticks,
                read.frame_rate.rate,
                read.first,
                read.frames,
                read.particles
            ),
            (header.key, 24000, 1001, 10, 5, 300)
        );
        assert_eq!(bake.frames(), 10..15);
        for k in 0..5u32 {
            assert_eq!(
                bake.states(10 + i64::from(k)).unwrap(),
                frame(k),
                "frame {k}"
            );
        }
        assert!(bake.states(15).is_err(), "past its end");
        // A cut-off file is refused, not read wrong.
        let whole = std::fs::read(&path).unwrap();
        std::fs::write(&path, &whole[..whole.len() - 3]).unwrap();
        assert!(Bake::open(&path).is_err());
        let _ = std::fs::remove_dir_all(&folder);
    }
}
