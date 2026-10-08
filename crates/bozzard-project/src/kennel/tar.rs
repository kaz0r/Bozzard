//! Read-only extraction of one named file from a gzip-compressed tar stream. Archive paths
//! are only compared, never used to create files, and the caller pins the result's SHA-256.
use super::MAX_UNPACKED;
use crate::content::copy_hash;
use anyhow::{Context, Result, bail, ensure};
use bozzard_assets::job::Progress;
use std::io::{Read, Write};

const BLOCK: u64 = 512;
/// GNU long names and pax records are small; anything larger is not a package archive.
const MAX_EXTENDED: u64 = 1024 * 1024;

/// Streams the regular file `member` into `output`, returning its size and SHA-256. Fails when
/// the member is missing, appears twice, is not a regular file, or exceeds `limit`.
pub(super) fn extract_member(
    input: impl Read,
    member: &str,
    output: &mut impl Write,
    limit: u64,
    progress: &Progress,
) -> Result<(u64, String)> {
    let mut reader = flate2::read::GzDecoder::new(input).take(MAX_UNPACKED);
    let mut found = None;
    let mut long_name = None;
    let mut pax_path = None;
    loop {
        progress.check()?;
        let mut header = [0; BLOCK as usize];
        reader
            .read_exact(&mut header)
            .context("tar archive is truncated or exceeds 1 GiB unpacked")?;
        if header.iter().all(|&b| b == 0) {
            break;
        }
        let stored = octal(&header[148..156]).context("tar header checksum")?;
        let computed: u64 = header
            .iter()
            .enumerate()
            .map(|(i, &b)| {
                if (148..156).contains(&i) {
                    32
                } else {
                    u64::from(b)
                }
            })
            .sum();
        ensure!(stored == computed, "tar header checksum mismatch");
        let size = octal(&header[124..136]).context("tar entry size")?;
        match header[156] {
            b'L' => long_name = Some(extended(&mut reader, size, progress)?),
            b'x' => {
                let records = extended(&mut reader, size, progress)?;
                pax_path = pax(&records, "path")?.or(pax_path);
            }
            b'g' => skip(&mut reader, padded(size), progress)?,
            kind => {
                let name = match (pax_path.take(), long_name.take()) {
                    (Some(path), _) | (None, Some(path)) => path,
                    (None, None) => ustar_name(&header)?,
                };
                if name == member && matches!(kind, b'0' | b'\0') {
                    ensure!(found.is_none(), "tar archive holds '{member}' twice");
                    ensure!(
                        size <= limit,
                        "tar member '{member}' exceeds its declared size"
                    );
                    let (bytes, sha256) =
                        copy_hash(&mut (&mut reader).take(size), output, limit, progress)?;
                    ensure!(bytes == size, "tar member '{member}' is truncated");
                    found = Some((bytes, sha256));
                    skip(&mut reader, padded(size) - size, progress)?;
                } else {
                    ensure!(
                        name != member,
                        "tar member '{member}' is not a regular file"
                    );
                    skip(&mut reader, padded(size), progress)?;
                }
            }
        }
    }
    found.with_context(|| format!("tar archive has no member '{member}'"))
}

fn padded(size: u64) -> u64 {
    size.div_ceil(BLOCK) * BLOCK
}

fn skip(reader: &mut impl Read, count: u64, progress: &Progress) -> Result<()> {
    let (skipped, _) = copy_hash(
        &mut reader.take(count),
        &mut std::io::sink(),
        count,
        progress,
    )?;
    ensure!(skipped == count, "tar archive is truncated");
    Ok(())
}

/// Reads a GNU long-name or pax payload, consuming its block padding.
fn extended(reader: &mut impl Read, size: u64, progress: &Progress) -> Result<String> {
    ensure!(size <= MAX_EXTENDED, "tar extended header exceeds 1 MiB");
    let mut bytes = Vec::with_capacity(size as usize);
    copy_hash(&mut reader.take(size), &mut bytes, size, progress)?;
    ensure!(bytes.len() as u64 == size, "tar archive is truncated");
    skip(reader, padded(size) - size, progress)?;
    let text = String::from_utf8(bytes).context("tar extended header is not UTF-8")?;
    Ok(text.trim_end_matches('\0').to_owned())
}

/// Pax records are `<length> <key>=<value>\n`, with the length counting the whole record.
fn pax(records: &str, key: &str) -> Result<Option<String>> {
    let mut rest = records;
    let mut value = None;
    while !rest.is_empty() {
        let (length, _) = rest.split_once(' ').context("malformed pax record")?;
        let length: usize = length.parse().context("malformed pax record length")?;
        ensure!(
            length > 0 && length <= rest.len() && rest.is_char_boundary(length),
            "malformed pax record length"
        );
        let record = &rest[..length];
        rest = &rest[length..];
        let body = record
            .split_once(' ')
            .map(|(_, body)| body)
            .and_then(|body| body.strip_suffix('\n'))
            .context("malformed pax record")?;
        if let Some((name, found)) = body.split_once('=')
            && name == key
        {
            value = Some(found.to_owned());
        }
    }
    Ok(value)
}

fn ustar_name(header: &[u8; BLOCK as usize]) -> Result<String> {
    let field = |bytes: &[u8]| {
        let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
        std::str::from_utf8(&bytes[..end])
            .map(str::to_owned)
            .context("tar entry name is not UTF-8")
    };
    let name = field(&header[..100])?;
    // POSIX ustar splits long paths into prefix/name; GNU tar reuses those bytes for times.
    if &header[257..263] == b"ustar\0" {
        let prefix = field(&header[345..500])?;
        if !prefix.is_empty() {
            return Ok(format!("{prefix}/{name}"));
        }
    }
    Ok(name)
}

/// Octal numbers padded with spaces/NULs, or GNU base-256 for large values.
fn octal(field: &[u8]) -> Result<u64> {
    if field[0] & 0x80 != 0 {
        ensure!(
            field.len() == 12 && field[0] == 0x80 && field[1..4].iter().all(|&b| b == 0),
            "tar number is out of range"
        );
        return Ok(u64::from_be_bytes(field[4..].try_into()?));
    }
    let text = std::str::from_utf8(field)?.trim_matches(|c| c == ' ' || c == '\0');
    if text.is_empty() {
        return Ok(0);
    }
    match u64::from_str_radix(text, 8) {
        Ok(value) => Ok(value),
        Err(_) => bail!("invalid tar number '{text}'"),
    }
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use sha2::{Digest, Sha256};

    /// A minimal ustar/GNU writer for fixtures.
    pub fn entry(archive: &mut Vec<u8>, name: &str, kind: u8, data: &[u8]) {
        let mut header = [0u8; 512];
        let (prefix, short) = if name.len() > 100 {
            let start = name.len() - 101;
            let split = start + name[start..].find('/').unwrap();
            (&name[..split], &name[split + 1..])
        } else {
            ("", name)
        };
        header[..short.len().min(100)].copy_from_slice(&short.as_bytes()[..short.len().min(100)]);
        header[100..108].copy_from_slice(b"0000644\0");
        header[124..136].copy_from_slice(format!("{:011o}\0", data.len()).as_bytes());
        header[136..148].copy_from_slice(b"00000000000\0");
        header[156] = kind;
        header[257..263].copy_from_slice(b"ustar\0");
        header[263..265].copy_from_slice(b"00");
        header[345..345 + prefix.len()].copy_from_slice(prefix.as_bytes());
        header[148..156].copy_from_slice(b"        ");
        let sum: u32 = header.iter().map(|&b| u32::from(b)).sum();
        header[148..156].copy_from_slice(format!("{sum:06o}\0 ").as_bytes());
        archive.extend_from_slice(&header);
        archive.extend_from_slice(data);
        archive.resize(archive.len().div_ceil(512) * 512, 0);
    }
    pub fn gzip(mut archive: Vec<u8>) -> Vec<u8> {
        archive.extend_from_slice(&[0; 1024]);
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        encoder.write_all(&archive).unwrap();
        encoder.finish().unwrap()
    }
    fn extract(archive: &[u8], member: &str, limit: u64) -> Result<Vec<u8>> {
        let mut output = Vec::new();
        let (bytes, sha256) =
            extract_member(archive, member, &mut output, limit, &Progress::default())?;
        assert_eq!(bytes, output.len() as u64);
        assert_eq!(sha256, crate::content::hex(Sha256::digest(&output)));
        Ok(output)
    }

    #[test]
    fn extracts_regular_members_by_ustar_gnu_and_pax_names() {
        let long = format!("pkg/{}/lib.so", "deep".repeat(30));
        let pax_name = "pkg/pax/named.so";
        let mut tar = Vec::new();
        entry(&mut tar, "pkg/", b'5', b"");
        entry(&mut tar, "pkg/a.txt", b'0', b"alpha");
        entry(
            &mut tar,
            "././@LongLink",
            b'L',
            format!("{long}\0").as_bytes(),
        );
        entry(&mut tar, "truncated-name", b'0', b"long body");
        let record = format!(" path={pax_name}\n");
        let record = format!("{}{record}", record.len() + 2);
        entry(&mut tar, "PaxHeaders/x", b'x', record.as_bytes());
        entry(&mut tar, "ignored", b'0', b"pax body");
        let split = format!("{}/split.so", "prefix".repeat(20));
        entry(&mut tar, &split, b'0', b"split body");
        let archive = gzip(tar);
        assert_eq!(extract(&archive, "pkg/a.txt", 64).unwrap(), b"alpha");
        assert_eq!(extract(&archive, &long, 64).unwrap(), b"long body");
        assert_eq!(extract(&archive, pax_name, 64).unwrap(), b"pax body");
        assert_eq!(extract(&archive, &split, 64).unwrap(), b"split body");
        // Overridden header names no longer match.
        assert!(extract(&archive, "truncated-name", 64).is_err());
        assert!(extract(&archive, "ignored", 64).is_err());
    }

    #[test]
    fn rejects_missing_duplicate_oversized_linked_and_corrupt_members() {
        let mut tar = Vec::new();
        entry(&mut tar, "a.so", b'0', b"first");
        entry(&mut tar, "a.so", b'0', b"second");
        entry(&mut tar, "link.so", b'2', b"");
        let archive = gzip(tar.clone());
        let message =
            |member: &str, limit| extract(&archive, member, limit).unwrap_err().to_string();
        assert!(message("a.so", 64).contains("twice"));
        assert!(message("absent.so", 64).contains("no member"));
        assert!(message("link.so", 64).contains("not a regular file"));

        let mut small = Vec::new();
        entry(&mut small, "big.so", b'0', &[7; 100]);
        assert!(
            extract(&gzip(small.clone()), "big.so", 99)
                .unwrap_err()
                .to_string()
                .contains("declared size")
        );

        small[0] ^= 1;
        assert!(
            extract(&gzip(small), "big.so", 128)
                .unwrap_err()
                .to_string()
                .contains("checksum")
        );

        let mut truncated = Vec::new();
        entry(&mut truncated, "cut.so", b'0', &[1; 2048]);
        truncated.truncate(1024);
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        encoder.write_all(&truncated).unwrap();
        assert!(extract(&encoder.finish().unwrap(), "cut.so", 4096).is_err());
    }
}
