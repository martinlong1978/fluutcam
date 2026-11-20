// SPDX-License-Identifier: MIT OR Apache-2.0

//! A small ZIP reader for untrusted archives: stored and deflated entries
//! only, with limits on entry count, unpacked size and compression ratio,
//! and no entry name that could leave the archive.
//!
//! Every offset is checked; a malformed archive is an error, never a
//! panic. ZIP64, encryption and archives split over several disks are
//! refused: an exchange file needs none of them.

use crate::Finding;
use std::io::Read;

/// Bounds on an archive (spec 06 §6.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    pub max_entries: usize,
    /// Sum of the unpacked sizes of all entries, bytes.
    pub max_unpacked: u64,
    /// Largest unpacked / packed size of an entry larger than
    /// `ratio_from` bytes.
    pub max_ratio: u64,
    pub ratio_from: u64,
}

impl Default for Limits {
    fn default() -> Limits {
        Limits {
            max_entries: 1000,
            max_unpacked: 1 << 30,
            max_ratio: 200,
            ratio_from: 1 << 20,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Entry {
    name: String,
    method: u16,
    crc: u32,
    packed: u64,
    unpacked: u64,
    local: u64,
}

/// An opened archive: its entry table; data is unpacked on [`Archive::read`].
#[derive(Debug)]
pub struct Archive<'a> {
    bytes: &'a [u8],
    entries: Vec<Entry>,
}

fn u16_at(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(
        b.get(at..at.checked_add(2)?)?.try_into().ok()?,
    ))
}

fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(
        b.get(at..at.checked_add(4)?)?.try_into().ok()?,
    ))
}

const EOCD: u32 = 0x0605_4b50;
const CENTRAL: u32 = 0x0201_4b50;
const LOCAL: u32 = 0x0403_4b50;

fn invalid(what: &str) -> Finding {
    Finding::error(
        "import.craftx.zip.invalid",
        format!("the file is not a readable ZIP archive: {what}"),
    )
}

fn unsupported(what: &str) -> Finding {
    Finding::error(
        "import.craftx.zip.unsupported",
        format!("the archive uses a ZIP feature FluutCAM does not read: {what}"),
    )
}

/// Why an entry name is unsafe, if it is: absolute, a drive letter, a
/// `..` or empty segment, a backslash or a control character.
fn unsafe_name(n: &str) -> Option<&'static str> {
    if n.is_empty() {
        return Some("an empty name");
    }
    if n.starts_with('/') {
        return Some("an absolute path");
    }
    if n.contains('\\') {
        return Some("a backslash");
    }
    if n.chars().any(char::is_control) {
        return Some("a control character");
    }
    let b = n.as_bytes();
    if b.len() >= 2 && b[1] == b':' && b[0].is_ascii_alphabetic() {
        return Some("a drive letter");
    }
    let body = n.strip_suffix('/').unwrap_or(n);
    if body
        .split('/')
        .any(|s| s == ".." || s == "." || s.is_empty())
    {
        return Some("a `..`, `.` or empty path segment");
    }
    None
}

impl<'a> Archive<'a> {
    /// Reads the central directory and checks every entry against the
    /// limits. Directory entries are left out.
    pub fn open(bytes: &'a [u8], lim: &Limits) -> Result<Archive<'a>, Finding> {
        // End of central directory: the last signature within the comment
        // reach (22 bytes plus a comment of up to 65 535).
        if bytes.len() < 22 {
            return Err(invalid("too short"));
        }
        let lo = bytes.len().saturating_sub(22 + 0xffff);
        let eocd = (lo..=bytes.len() - 22)
            .rev()
            .find(|&i| u32_at(bytes, i) == Some(EOCD))
            .ok_or_else(|| invalid("no end of central directory"))?;
        let field = |o: usize| u16_at(bytes, eocd + o).ok_or_else(|| invalid("truncated"));
        let (disk, cd_disk) = (field(4)?, field(6)?);
        let (on_disk, total) = (field(8)?, field(10)?);
        let size = u32_at(bytes, eocd + 12).ok_or_else(|| invalid("truncated"))?;
        let offset = u32_at(bytes, eocd + 16).ok_or_else(|| invalid("truncated"))?;
        if total == 0xffff || size == 0xffff_ffff || offset == 0xffff_ffff {
            return Err(unsupported("ZIP64"));
        }
        if disk != 0 || cd_disk != 0 || on_disk != total {
            return Err(unsupported("an archive split over several disks"));
        }
        if total as usize > lim.max_entries {
            return Err(Finding::error(
                "import.craftx.zip.entries",
                format!(
                    "the archive has {total} entries; at most {} are read",
                    lim.max_entries
                ),
            ));
        }
        let mut at = offset as usize;
        let end = at
            .checked_add(size as usize)
            .filter(|&e| e <= eocd)
            .ok_or_else(|| invalid("the central directory lies outside the file"))?;
        let mut entries = Vec::new();
        let mut unpacked_sum = 0u64;
        for _ in 0..total {
            if u32_at(bytes, at) != Some(CENTRAL) || at + 46 > end {
                return Err(invalid("a damaged central directory"));
            }
            let h = |o: usize| u16_at(bytes, at + o).ok_or_else(|| invalid("truncated"));
            let w = |o: usize| u32_at(bytes, at + o).ok_or_else(|| invalid("truncated"));
            let flags = h(8)?;
            let method = h(10)?;
            let crc = w(16)?;
            let packed = w(20)?;
            let unpacked = w(24)?;
            let (nl, xl, cl) = (h(28)? as usize, h(30)? as usize, h(32)? as usize);
            let local = w(42)?;
            let name_bytes = bytes
                .get(at + 46..at + 46 + nl)
                .ok_or_else(|| invalid("truncated entry name"))?;
            at += 46 + nl + xl + cl;
            if at > end {
                return Err(invalid("a damaged central directory"));
            }
            let name = std::str::from_utf8(name_bytes)
                .map_err(|_| invalid("an entry name that is not UTF-8"))?
                .to_owned();
            if let Some(why) = unsafe_name(&name) {
                return Err(Finding::error(
                    "import.craftx.zip.name",
                    format!(
                        "archive entry `{}` has {why}; the file is refused",
                        name.escape_debug()
                    ),
                ));
            }
            if flags & 1 != 0 {
                return Err(unsupported("encrypted entries"));
            }
            if packed == 0xffff_ffff || unpacked == 0xffff_ffff || local == 0xffff_ffff {
                return Err(unsupported("ZIP64"));
            }
            if name.ends_with('/') {
                continue;
            }
            if method != 0 && method != 8 {
                return Err(unsupported(&format!(
                    "compression method {method} in `{name}` (only stored and deflated entries)"
                )));
            }
            if entries.iter().any(|e: &Entry| e.name == name) {
                return Err(Finding::error(
                    "import.craftx.zip.duplicate",
                    format!("archive entry `{name}` appears twice"),
                ));
            }
            let (packed, unpacked) = (u64::from(packed), u64::from(unpacked));
            unpacked_sum += unpacked;
            if unpacked_sum > lim.max_unpacked {
                return Err(Finding::error(
                    "import.craftx.zip.size",
                    format!(
                        "the archive unpacks to more than {} MiB",
                        lim.max_unpacked >> 20
                    ),
                ));
            }
            if unpacked > lim.ratio_from && unpacked > packed.max(1).saturating_mul(lim.max_ratio) {
                return Err(Finding::error(
                    "import.craftx.zip.ratio",
                    format!(
                        "archive entry `{name}` is compressed more than {}:1, as a ZIP bomb is",
                        lim.max_ratio
                    ),
                ));
            }
            if method == 0 && packed != unpacked {
                return Err(invalid(&format!("stored entry `{name}` has two sizes")));
            }
            entries.push(Entry {
                name,
                method,
                crc,
                packed,
                unpacked,
                local: u64::from(local),
            });
        }
        Ok(Archive { bytes, entries })
    }

    /// Entry names, in archive order.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.entries.iter().map(|e| e.name.as_str())
    }

    pub fn contains(&self, name: &str) -> bool {
        self.entries.iter().any(|e| e.name == name)
    }

    /// The unpacked data of entry `name`, checked against its size and
    /// CRC-32. `None` when there is no such entry.
    pub fn read(&self, name: &str) -> Option<Result<Vec<u8>, Finding>> {
        let e = self.entries.iter().find(|e| e.name == name)?;
        Some(self.read_entry(e))
    }

    fn read_entry(&self, e: &Entry) -> Result<Vec<u8>, Finding> {
        let damaged = || invalid(&format!("entry `{}` is damaged", e.name));
        let at = usize::try_from(e.local).map_err(|_| damaged())?;
        if u32_at(self.bytes, at) != Some(LOCAL) {
            return Err(damaged());
        }
        let nl = u16_at(self.bytes, at + 26).ok_or_else(damaged)? as usize;
        let xl = u16_at(self.bytes, at + 28).ok_or_else(damaged)? as usize;
        let start = at + 30 + nl + xl;
        let packed = usize::try_from(e.packed).map_err(|_| damaged())?;
        let data = start
            .checked_add(packed)
            .and_then(|end| self.bytes.get(start..end))
            .ok_or_else(damaged)?;
        let out = match e.method {
            0 => data.to_vec(),
            _ => {
                let mut out = Vec::with_capacity(e.unpacked.min(1 << 24) as usize);
                // One byte more than declared shows a lying size.
                flate2::read::DeflateDecoder::new(data)
                    .take(e.unpacked + 1)
                    .read_to_end(&mut out)
                    .map_err(|_| damaged())?;
                out
            }
        };
        if out.len() as u64 != e.unpacked || crc32fast::hash(&out) != e.crc {
            return Err(Finding::error(
                "import.craftx.zip.crc",
                format!(
                    "archive entry `{}` does not unpack to the data it declares",
                    e.name
                ),
            ));
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unsafe_names() {
        for n in [
            "../x",
            "a/../../x",
            "/etc/passwd",
            "C:/x",
            "c:x",
            "a\\b",
            "a//b",
            "./a",
            "a\u{0}b",
            "",
        ] {
            assert!(unsafe_name(n).is_some(), "{n:?}");
        }
        for n in ["manifest.json", "bodies/0.step", "dir/", "a..b/c"] {
            assert!(unsafe_name(n).is_none(), "{n:?}");
        }
    }

    #[test]
    fn garbage_is_refused_without_panic() {
        let lim = Limits::default();
        for b in [
            &b""[..],
            b"PK",
            &[0u8; 30][..],
            b"PK\x05\x06\0\0\0\0\x01\0\x01\0\xff\0\0\0\0\0\0\0\0\0",
        ] {
            assert!(Archive::open(b, &lim).is_err());
        }
    }
}
