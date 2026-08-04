//! Minimal reader for the SIF (Singularity Image Format) descriptor table.
//!
//! Reads only what the unpack progress bar needs: the `unpacked_bytes`
//! value from the GenericJSON object embedded by lib/build-sif.nix.
//! Layout constants follow the frozen v1 on-disk format
//! (github.com/apptainer/sif, pkg/sif/sif.go): packed structs,
//! little-endian integers. The committed fixture test guards the offsets.

use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

const HEADER_LEN: usize = 128;
const MAGIC_OFFSET: usize = 32;
const MAGIC: &[u8] = b"SIF_MAGIC";
/// int64 fields trailing the identity block: created(64) modified(72)
/// descr_free(80) descr_total(88) descr_offset(96) descr_size(104) ...
const DESCR_TOTAL_OFFSET: usize = 88;
const DESCR_OFFSET_OFFSET: usize = 96;

/// rawDescriptor: datatype i32(0), used u8(4), id u32(5), groupid u32(9),
/// linkedid u32(13), offset i64(17), size i64(25), ... name/extra padding.
const DESCRIPTOR_LEN: usize = 585;
const D_DATATYPE: usize = 0;
const D_USED: usize = 4;
const D_OFFSET: usize = 17;
const D_SIZE: usize = 25;

const DT_GENERIC_JSON: i32 = 0x4006;
const MAX_DESCRIPTORS: i64 = 4096;
const MAX_META_BYTES: i64 = 65536;

fn le_i32(buf: &[u8], off: usize) -> Option<i32> {
    buf.get(off..off + 4)
        .map(|b| i32::from_le_bytes(b.try_into().expect("4-byte slice")))
}

fn le_i64(buf: &[u8], off: usize) -> Option<i64> {
    buf.get(off..off + 8)
        .map(|b| i64::from_le_bytes(b.try_into().expect("8-byte slice")))
}

/// Read the `unpacked_bytes` metadata from a SIF. Returns None on any
/// anomaly (older/custom image, truncated file, unexpected layout) — the
/// caller falls back to indeterminate progress.
pub fn read_unpacked_bytes(sif: &Path) -> Option<u64> {
    let mut f = std::fs::File::open(sif).ok()?;
    let mut header = [0u8; HEADER_LEN];
    f.read_exact(&mut header).ok()?;
    if &header[MAGIC_OFFSET..MAGIC_OFFSET + MAGIC.len()] != MAGIC {
        return None;
    }
    let total = le_i64(&header, DESCR_TOTAL_OFFSET)?;
    let offset = le_i64(&header, DESCR_OFFSET_OFFSET)?;
    if !(1..=MAX_DESCRIPTORS).contains(&total) || offset <= 0 {
        return None;
    }

    f.seek(SeekFrom::Start(offset as u64)).ok()?;
    let mut json_loc: Option<(i64, i64)> = None;
    let mut d = [0u8; DESCRIPTOR_LEN];
    for _ in 0..total {
        f.read_exact(&mut d).ok()?;
        if d[D_USED] == 0 || le_i32(&d, D_DATATYPE)? != DT_GENERIC_JSON {
            continue;
        }
        json_loc = Some((le_i64(&d, D_OFFSET)?, le_i64(&d, D_SIZE)?));
        break;
    }

    let (obj_offset, obj_size) = json_loc?;
    if obj_offset <= 0 || !(1..=MAX_META_BYTES).contains(&obj_size) {
        return None;
    }
    f.seek(SeekFrom::Start(obj_offset as u64)).ok()?;
    let mut buf = vec![0u8; obj_size as usize];
    f.read_exact(&mut buf).ok()?;
    serde_json::from_slice::<serde_json::Value>(&buf)
        .ok()?
        .get("unpacked_bytes")?
        .as_u64()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::path::Path;

    #[test]
    fn test_read_unpacked_bytes_fixture() {
        let p = Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/meta.sif"
        ));
        assert_eq!(read_unpacked_bytes(p), Some(12345));
    }

    #[test]
    fn test_read_unpacked_bytes_not_a_sif() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(b"definitely not a SIF file").unwrap();
        assert_eq!(read_unpacked_bytes(f.path()), None);
    }

    #[test]
    fn test_read_unpacked_bytes_missing_file() {
        assert_eq!(read_unpacked_bytes(Path::new("/nonexistent.sif")), None);
    }
}
