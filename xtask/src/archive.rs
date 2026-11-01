// SPDX-License-Identifier: MIT OR Apache-2.0

//! Deterministic `.tar.gz` and `.zip` writers (D14 §5.4): entries sorted by
//! path, one fixed time stamp, no owners, no extra fields. The same inputs
//! give the same bytes on every platform, so the archives can be compared
//! and checksummed like the binaries inside.

use flate2::Compression;
use flate2::write::{DeflateEncoder, GzEncoder};
use std::io::Write;

/// A file in an archive. `path` uses `/` and is relative.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub path: String,
    pub data: Vec<u8>,
    pub executable: bool,
}

impl Entry {
    pub fn new(path: impl Into<String>, data: Vec<u8>, executable: bool) -> Entry {
        Entry {
            path: path.into(),
            data,
            executable,
        }
    }

    fn mode(&self) -> u32 {
        if self.executable { 0o755 } else { 0o644 }
    }
}

fn sorted(entries: &[Entry]) -> Vec<&Entry> {
    let mut v: Vec<&Entry> = entries.iter().collect();
    v.sort_by(|a, b| a.path.cmp(&b.path));
    v
}

fn octal(out: &mut [u8], v: u64) {
    // Zero padded, NUL terminated.
    let s = format!("{:0width$o}\0", v, width = out.len() - 1);
    out.copy_from_slice(s.as_bytes());
}

fn tar_header(e: &Entry, mtime: u64) -> Result<[u8; 512], String> {
    if e.path.len() > 100 {
        return Err(format!("{}: path longer than 100 bytes", e.path));
    }
    let mut h = [0u8; 512];
    h[..e.path.len()].copy_from_slice(e.path.as_bytes());
    octal(&mut h[100..108], u64::from(e.mode()));
    octal(&mut h[108..116], 0); // uid
    octal(&mut h[116..124], 0); // gid
    octal(&mut h[124..136], e.data.len() as u64);
    octal(&mut h[136..148], mtime);
    h[148..156].fill(b' '); // checksum counts these as spaces
    h[156] = b'0';
    h[257..263].copy_from_slice(b"ustar\0");
    h[263..265].copy_from_slice(b"00");
    let sum: u32 = h.iter().map(|&b| u32::from(b)).sum();
    let s = format!("{sum:06o}\0 ");
    h[148..156].copy_from_slice(s.as_bytes());
    Ok(h)
}

/// An uncompressed ustar archive.
pub fn tar(entries: &[Entry], mtime: u64) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    for e in sorted(entries) {
        out.extend_from_slice(&tar_header(e, mtime)?);
        out.extend_from_slice(&e.data);
        out.resize(out.len().next_multiple_of(512), 0);
    }
    out.resize(out.len() + 1024, 0); // end of archive
    Ok(out)
}

/// A gzip-compressed tar archive.
pub fn tar_gz(entries: &[Entry], mtime: u64) -> Result<Vec<u8>, String> {
    let raw = tar(entries, mtime)?;
    let gz = flate2::GzBuilder::new()
        .mtime(0)
        .operating_system(3) // Unix, whatever the host
        .write(Vec::new(), Compression::best());
    let mut gz: GzEncoder<Vec<u8>> = gz;
    gz.write_all(&raw).map_err(|e| e.to_string())?;
    gz.finish().map_err(|e| e.to_string())
}

/// MS-DOS time and date of a Unix time; 1980-01-01 is the earliest.
fn dos_time(epoch: u64) -> (u16, u16) {
    let (y, m, d) = crate::util::civil_from_days((epoch / 86_400) as i64);
    if y < 1980 {
        return (0, (1 << 5) | 1);
    }
    let secs = epoch % 86_400;
    let time = ((secs / 3600) << 11) | ((secs % 3600 / 60) << 5) | (secs % 60 / 2);
    let date = (((y - 1980) as u64) << 9) | (u64::from(m) << 5) | u64::from(d);
    (time as u16, date as u16)
}

/// A zip archive with deflated entries.
pub fn zip(entries: &[Entry], mtime: u64) -> Result<Vec<u8>, String> {
    let (time, date) = dos_time(mtime);
    let mut out = Vec::new();
    let mut central = Vec::new();
    let sorted = sorted(entries);
    for e in &sorted {
        let mut enc = DeflateEncoder::new(Vec::new(), Compression::best());
        enc.write_all(&e.data).map_err(|e| e.to_string())?;
        let deflated = enc.finish().map_err(|e| e.to_string())?;
        let crc = crc32fast::hash(&e.data);
        let offset = out.len() as u32;
        let name = e.path.as_bytes();
        let (csize, usize_) = (deflated.len() as u32, e.data.len() as u32);

        out.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
        out.extend_from_slice(&20u16.to_le_bytes()); // version needed
        out.extend_from_slice(&0u16.to_le_bytes()); // flags
        out.extend_from_slice(&8u16.to_le_bytes()); // deflate
        out.extend_from_slice(&time.to_le_bytes());
        out.extend_from_slice(&date.to_le_bytes());
        out.extend_from_slice(&crc.to_le_bytes());
        out.extend_from_slice(&csize.to_le_bytes());
        out.extend_from_slice(&usize_.to_le_bytes());
        out.extend_from_slice(&(name.len() as u16).to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes()); // extra
        out.extend_from_slice(name);
        out.extend_from_slice(&deflated);

        central.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
        central.extend_from_slice(&((3u16 << 8) | 20).to_le_bytes()); // made by Unix
        central.extend_from_slice(&20u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&8u16.to_le_bytes());
        central.extend_from_slice(&time.to_le_bytes());
        central.extend_from_slice(&date.to_le_bytes());
        central.extend_from_slice(&crc.to_le_bytes());
        central.extend_from_slice(&csize.to_le_bytes());
        central.extend_from_slice(&usize_.to_le_bytes());
        central.extend_from_slice(&(name.len() as u16).to_le_bytes());
        central.extend_from_slice(&[0; 8]); // extra, comment, disk, internal
        let attrs = (0o100_000 | e.mode()) << 16;
        central.extend_from_slice(&attrs.to_le_bytes());
        central.extend_from_slice(&offset.to_le_bytes());
        central.extend_from_slice(name);
    }
    let cd_offset = out.len() as u32;
    out.extend_from_slice(&central);
    out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
    out.extend_from_slice(&[0; 4]); // disk numbers
    out.extend_from_slice(&(sorted.len() as u16).to_le_bytes());
    out.extend_from_slice(&(sorted.len() as u16).to_le_bytes());
    out.extend_from_slice(&(central.len() as u32).to_le_bytes());
    out.extend_from_slice(&cd_offset.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    fn files() -> Vec<Entry> {
        vec![
            Entry::new("d/b.txt", b"second file, second file".to_vec(), false),
            Entry::new("a.bin", vec![7; 1000], true),
        ]
    }

    fn field(b: &[u8]) -> u64 {
        let s: String = b
            .iter()
            .take_while(|&&c| c != 0)
            .map(|&c| c as char)
            .collect();
        u64::from_str_radix(s.trim(), 8).unwrap()
    }

    /// Reads back `(path, mode, mtime, data)` from an uncompressed tar.
    fn read_tar(t: &[u8]) -> Vec<(String, u64, u64, Vec<u8>)> {
        let mut out = Vec::new();
        let mut pos = 0;
        while t[pos..pos + 512].iter().any(|&b| b != 0) {
            let h = &t[pos..pos + 512];
            let name =
                String::from_utf8(h[..100].iter().copied().take_while(|&c| c != 0).collect())
                    .unwrap();
            let size = field(&h[124..136]) as usize;
            let mut copy = h.to_vec();
            copy[148..156].fill(b' ');
            let sum: u64 = copy.iter().map(|&b| u64::from(b)).sum();
            assert_eq!(field(&h[148..156]), sum, "checksum of {name}");
            let data = t[pos + 512..pos + 512 + size].to_vec();
            out.push((name, field(&h[100..108]), field(&h[136..148]), data));
            pos += 512 + size.next_multiple_of(512);
        }
        assert_eq!(t.len(), pos + 1024);
        out
    }

    #[test]
    fn tar_entries_are_sorted_with_fixed_metadata() {
        let got = read_tar(&tar(&files(), 1000).unwrap());
        assert_eq!(got[0], ("a.bin".into(), 0o755, 1000, vec![7; 1000]));
        assert_eq!(got[1].0, "d/b.txt");
        assert_eq!(got[1].1, 0o644);
        assert_eq!(got[1].3, b"second file, second file");
    }

    #[test]
    fn tar_gz_is_deterministic_and_inflates_to_the_tar() {
        let a = tar_gz(&files(), 5).unwrap();
        assert_eq!(
            a,
            tar_gz(&files().into_iter().rev().collect::<Vec<_>>(), 5).unwrap()
        );
        assert_eq!(&a[..4], &[0x1f, 0x8b, 8, 0]);
        assert_eq!(&a[4..8], &[0; 4], "no time stamp in the gzip header");
        let mut raw = Vec::new();
        flate2::read::GzDecoder::new(&a[..])
            .read_to_end(&mut raw)
            .unwrap();
        assert_eq!(raw, tar(&files(), 5).unwrap());
    }

    #[test]
    fn tar_rejects_long_paths() {
        assert!(tar(&[Entry::new("x".repeat(101), vec![], false)], 0).is_err());
    }

    /// Reads `(path, data)` of every entry through the central directory.
    fn read_zip(z: &[u8]) -> Vec<(String, Vec<u8>, u32)> {
        let u16at = |i: usize| u16::from_le_bytes(z[i..i + 2].try_into().unwrap()) as usize;
        let u32at = |i: usize| u32::from_le_bytes(z[i..i + 4].try_into().unwrap()) as usize;
        let eocd = z.len() - 22;
        assert_eq!(u32at(eocd), 0x0605_4b50);
        let (n, mut cd) = (u16at(eocd + 10), u32at(eocd + 16));
        let mut out = Vec::new();
        for _ in 0..n {
            assert_eq!(u32at(cd), 0x0201_4b50);
            let (csize, name_len) = (u32at(cd + 20), u16at(cd + 28));
            let attrs = u32at(cd + 38) as u32;
            let local = u32at(cd + 42);
            let name = String::from_utf8(z[cd + 46..cd + 46 + name_len].to_vec()).unwrap();
            assert_eq!(u32at(local), 0x0403_4b50);
            let start = local + 30 + u16at(local + 26) + u16at(local + 28);
            let mut data = Vec::new();
            flate2::read::DeflateDecoder::new(&z[start..start + csize])
                .read_to_end(&mut data)
                .unwrap();
            assert_eq!(
                crc32fast::hash(&data) as usize,
                u32at(cd + 16),
                "crc of {name}"
            );
            out.push((name, data, attrs >> 16));
            cd += 46 + name_len;
        }
        out
    }

    #[test]
    fn zip_round_trips_in_sorted_order() {
        let z = zip(&files(), 1_793_491_200).unwrap();
        let got = read_zip(&z);
        assert_eq!(got.len(), 2);
        assert_eq!(got[0], ("a.bin".into(), vec![7; 1000], 0o100_755));
        assert_eq!(got[1].0, "d/b.txt");
        assert_eq!(got[1].1, b"second file, second file");
        assert_eq!(z, zip(&files(), 1_793_491_200).unwrap());
    }

    #[test]
    fn dos_time_clamps_to_1980() {
        assert_eq!(dos_time(0).1, (1 << 5) | 1);
        // 2026-11-01 00:00:02 UTC.
        let (t, d) = dos_time(1_793_491_202);
        assert_eq!(t, 1);
        assert_eq!(d, (46 << 9) | (11 << 5) | 1);
    }
}
