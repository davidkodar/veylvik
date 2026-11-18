//! Shared document and undo-retention limits, measured in canonical JSON bytes.
use crate::{Error, Result};
use std::io::{self, Write};
pub const MAX_NODES: usize = 4096;
pub const MAX_DOCUMENT_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_HISTORY_BYTES: usize = 128 * 1024 * 1024;
pub const MAX_HISTORY_STEPS: usize = 256;
/// Undo steps kept unless the host asks for another number.
pub const DEFAULT_HISTORY_STEPS: usize = 64;
pub(crate) fn encoded_size(value: &impl serde::Serialize) -> Result<usize> {
    struct Counter(usize);
    impl Write for Counter {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            let next = self.0.saturating_add(bytes.len());
            if next > MAX_DOCUMENT_BYTES {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "composition exceeds 16 MiB of canonical JSON",
                ));
            }
            self.0 = next;
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    let mut counter = Counter(0);
    serde_json::to_writer_pretty(&mut counter, value).map_err(|e| Error(e.to_string()))?;
    Ok(counter.0)
}
