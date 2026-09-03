use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use anyhow::Result;

const CHUNK: usize = 64 * 1024;

/// Machine-independent file identity: size plus the head and tail bytes.
///
/// Deliberately not a full-content hash - scanning 10k files would be IO-bound
/// on the hash rather than the decode. Head+tail+size is enough to distinguish
/// real files while staying stable across renames, moves, and machines, which
/// is what makes `.sbpack` metadata portable between users.
pub fn content_key(path: &Path) -> Result<String> {
    let mut f = File::open(path)?;
    let size = f.metadata()?.len();

    let mut h = blake3::Hasher::new();
    h.update(&size.to_le_bytes());

    let mut buf = vec![0u8; CHUNK];
    let head = f.read(&mut buf)?;
    h.update(&buf[..head]);

    // Only read a distinct tail when the file is bigger than two chunks;
    // otherwise the head already covers it.
    if size > (2 * CHUNK) as u64 {
        f.seek(SeekFrom::End(-(CHUNK as i64)))?;
        let tail = f.read(&mut buf)?;
        h.update(&buf[..tail]);
    }

    Ok(format!("b3:{}", h.finalize().to_hex()))
}
