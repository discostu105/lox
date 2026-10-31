//! Miniserver config archives: the backup ZIP a Miniserver serves (and
//! takes back as `sps_new.zip`) and the `sps0.LoxCC` inside it, the
//! compressed program file (`.Loxone` XML).
//!
//! - [`decompress_loxcc`] / [`compress_loxcc`]: the LoxCC codec
//! - [`extract_and_decompress`]: a backup ZIP → its `.Loxone` XML
//! - [`read_entry`]: any other entry (`LoxAPP3.json`, `permissions.bin`, …)
//! - [`stage_upload_zip`]: an upload ZIP around a new XML, every other entry
//!   copied raw from a Loxone-written download
//!
//! This is the `archive` feature (on by default). It reads and writes bytes
//! only; getting the ZIP to and from a Miniserver (FTP, HTTP) is the
//! caller's job.
//!
//! Reference: <https://gist.github.com/sarnau/e14ff9fe081611782a3f3cb2e2c2bacd>

use crate::{Error, Result};
use std::io::{Cursor, Read, Write};

pub const LOXCC_MAGIC: u32 = 0xaabbccee;
const LOXCC_HEADER_LEN: usize = 16;
/// The entry name of the program file inside a backup ZIP.
pub const SPS0_ENTRY: &str = "sps0.LoxCC";
/// The access table next to the program file (see [`crate::permissions`]).
pub const PERMISSIONS_ENTRY: &str = "permissions.bin";

fn err(msg: impl Into<String>) -> Error {
    Error::Archive(msg.into())
}

fn zip_err(what: &str) -> impl Fn(zip::result::ZipError) -> Error + '_ {
    move |e| err(format!("{what}: {e}"))
}

/// Whether `data` is a ZIP archive (by its magic bytes), e.g. a backup
/// rather than a plain `.Loxone` file.
pub fn is_zip(data: &[u8]) -> bool {
    data.starts_with(b"PK\x03\x04")
}

/// Decompress a `sps0.LoxCC` binary blob into XML bytes.
///
/// Format:
///   [0..4]  u32_le magic (0xaabbccee)
///   [4..8]  u32_le compressed payload size (the bytes after the header)
///   [8..12] u32_le uncompressed size (the XML length)
///   [12..16] u32_le CRC-32 (IEEE) of the uncompressed XML
///   [16..]  compressed data (an LZ4 block)
///
/// The size and checksum fields were checked against Loxone-written
/// downloads; this reader does not verify them.
pub fn decompress_loxcc(data: &[u8]) -> Result<Vec<u8>> {
    if data.len() < LOXCC_HEADER_LEN {
        return Err(err(format!("LoxCC data too short ({} bytes)", data.len())));
    }
    let magic = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
    if magic != LOXCC_MAGIC {
        return Err(err(format!(
            "Not a valid LoxCC file (magic: 0x{magic:08x}, expected 0x{LOXCC_MAGIC:08x})"
        )));
    }
    let uncomp_hint = u32::from_le_bytes([data[8], data[9], data[10], data[11]]) as usize;

    let compressed = &data[LOXCC_HEADER_LEN..];
    // The hint comes from the file; cap the pre-allocation so a corrupt
    // header cannot ask for gigabytes.
    let mut output = Vec::with_capacity(uncomp_hint.clamp(1024, 64 << 20));
    let mut pos = 0;

    while pos < compressed.len() {
        let token = compressed[pos];
        pos += 1;

        // High nibble: literal count
        let mut lit_len = (token >> 4) as usize;
        if lit_len == 15 {
            while pos < compressed.len() {
                let extra = compressed[pos] as usize;
                pos += 1;
                lit_len += extra;
                if extra != 255 {
                    break;
                }
            }
        }

        // Copy literals
        if pos + lit_len > compressed.len() {
            // Allow partial final literal block (end of stream)
            output.extend_from_slice(&compressed[pos..]);
            break;
        }
        output.extend_from_slice(&compressed[pos..pos + lit_len]);
        pos += lit_len;

        // End of compressed data — no match follows the last literal
        if pos + 2 > compressed.len() {
            break;
        }

        // Back-reference offset (u16_le)
        let offset = u16::from_le_bytes([compressed[pos], compressed[pos + 1]]) as usize;
        pos += 2;

        if offset == 0 {
            return Err(err(format!(
                "LoxCC: zero back-reference offset at byte {}",
                pos - 2
            )));
        }

        // Low nibble: match length (min 4)
        let mut match_len = (token & 0x0F) as usize + 4;
        if (token & 0x0F) == 15 {
            while pos < compressed.len() {
                let extra = compressed[pos] as usize;
                pos += 1;
                match_len += extra;
                if extra != 255 {
                    break;
                }
            }
        }

        // Copy from output buffer (byte-by-byte for overlapping runs)
        if offset > output.len() {
            return Err(err(format!(
                "LoxCC: back-reference offset {offset} exceeds output size {}",
                output.len()
            )));
        }
        let start = output.len() - offset;
        for i in 0..match_len {
            let byte = output[start + (i % offset)];
            output.push(byte);
        }
    }

    Ok(output)
}

/// Extract `sps0.LoxCC` from a backup ZIP, then decompress to XML.
pub fn extract_and_decompress(zip_data: &[u8]) -> Result<Vec<u8>> {
    let mut archive =
        zip::ZipArchive::new(Cursor::new(zip_data)).map_err(zip_err("Invalid ZIP archive"))?;

    for i in 0..archive.len() {
        let mut file = archive
            .by_index(i)
            .map_err(zip_err("Invalid ZIP archive"))?;
        if file.name().to_lowercase().contains("sps0.loxcc") {
            let mut buf = Vec::new();
            file.read_to_end(&mut buf)?;
            return decompress_loxcc(&buf);
        }
    }
    Err(err("Backup ZIP does not contain sps0.LoxCC"))
}

/// The program file of `data`: a backup ZIP is extracted and decompressed,
/// anything else is taken as `.Loxone` XML as it is.
pub fn program_xml(data: &[u8]) -> Result<std::borrow::Cow<'_, [u8]>> {
    if is_zip(data) {
        Ok(std::borrow::Cow::Owned(extract_and_decompress(data)?))
    } else {
        Ok(std::borrow::Cow::Borrowed(data))
    }
}

/// Compress config XML into a `sps0.LoxCC` blob — the inverse of
/// [`decompress_loxcc`].
///
/// The payload is a standard LZ4 block (which the Miniserver accepts); the
/// header carries the payload size, the XML length and the CRC-32 of the
/// XML, as in Loxone-written files. The bytes differ from Loxone's own
/// encoder (which compresses tighter), the decompressed XML does not.
pub fn compress_loxcc(xml: &[u8]) -> Result<Vec<u8>> {
    let payload = lz4_flex::block::compress(xml);
    let xml_len = u32::try_from(xml.len())
        .map_err(|_| err(format!("Config too large for LoxCC ({} bytes)", xml.len())))?;
    let payload_len = u32::try_from(payload.len()).map_err(|_| err("LoxCC payload too large"))?;

    let mut blob = Vec::with_capacity(LOXCC_HEADER_LEN + payload.len());
    blob.extend_from_slice(&LOXCC_MAGIC.to_le_bytes());
    blob.extend_from_slice(&payload_len.to_le_bytes());
    blob.extend_from_slice(&xml_len.to_le_bytes());
    blob.extend_from_slice(&crc32fast::hash(xml).to_le_bytes());
    blob.extend_from_slice(&payload);
    Ok(blob)
}

/// A ZIP entry timestamp (local time, as ZIP stores it), for the entries
/// [`stage_upload_zip`] writes new.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ZipTime {
    pub year: u16,
    pub month: u8,
    pub day: u8,
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
}

impl ZipTime {
    fn to_zip(self) -> Result<zip::DateTime> {
        zip::DateTime::from_date_and_time(
            self.year,
            self.month,
            self.day,
            self.hour,
            self.minute,
            self.second,
        )
        .map_err(|e| err(format!("{self:?} is not a valid ZIP timestamp: {e}")))
    }
}

/// Build an upload ZIP that carries `xml` as its config.
///
/// `template_zip` is a Loxone-written download: every entry except
/// `sps0.LoxCC` is copied raw (name, order, compressed bytes, timestamp),
/// so the side files stay exactly what the Miniserver wrote. `sps0.LoxCC`
/// is replaced by [`compress_loxcc`]`(xml)`, keeping the template entry's
/// name and compression method; its timestamp becomes `now` (pass the
/// current local time), because the content is new. Before returning, the
/// result is extracted again and must yield exactly `xml`.
pub fn stage_upload_zip(template_zip: &[u8], xml: &[u8], now: ZipTime) -> Result<Vec<u8>> {
    stage_upload_zip_replacing(template_zip, xml, &[], now)
}

/// [`stage_upload_zip`], with more entries replaced the way `sps0.LoxCC`
/// is: each `(name, content)` names an entry the template has exactly once
/// (in any case), which keeps its name, position and compression method and
/// gets `content` and `now`. The staged ZIP must extract to exactly these
/// contents.
pub fn stage_upload_zip_replacing(
    template_zip: &[u8],
    xml: &[u8],
    replace: &[(&str, &[u8])],
    now: ZipTime,
) -> Result<Vec<u8>> {
    let mut template = zip::ZipArchive::new(Cursor::new(template_zip))
        .map_err(zip_err("Invalid template ZIP archive"))?;
    let mtime = now.to_zip()?;
    let loxcc = compress_loxcc(xml)?;
    let mut entries: Vec<(&str, &[u8])> = vec![(SPS0_ENTRY, &loxcc)];
    entries.extend_from_slice(replace);
    for (name, _) in &entries {
        let hits = template
            .file_names()
            .filter(|n| n.eq_ignore_ascii_case(name))
            .count();
        if hits != 1 {
            return Err(err(format!(
                "Template ZIP must contain exactly one {name}, found {hits}"
            )));
        }
    }

    let write_err = zip_err("Cannot write the staged ZIP");
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for i in 0..template.len() {
        let entry = template
            .by_index_raw(i)
            .map_err(zip_err("Invalid template ZIP archive"))?;
        let new = entries
            .iter()
            .find(|(name, _)| entry.name().eq_ignore_ascii_case(name));
        if let Some((_, content)) = new {
            let options = zip::write::SimpleFileOptions::default()
                .compression_method(entry.compression())
                .last_modified_time(mtime);
            let name = entry.name().to_string();
            drop(entry);
            writer.start_file(name, options).map_err(&write_err)?;
            writer.write_all(content)?;
        } else {
            writer.raw_copy_file(entry).map_err(&write_err)?;
        }
    }
    let staged = writer.finish().map_err(&write_err)?.into_inner();

    let roundtrip = extract_and_decompress(&staged)
        .map_err(|e| err(format!("Staged ZIP does not extract: {e}")))?;
    if roundtrip != xml {
        return Err(err(format!(
            "Staged ZIP does not extract byte-identically ({} bytes in, {} bytes out)",
            xml.len(),
            roundtrip.len()
        )));
    }
    for (name, content) in replace {
        if read_entry(&staged, name)?.as_deref() != Some(*content) {
            return Err(err(format!(
                "Staged ZIP does not extract {name} byte-identically"
            )));
        }
    }
    Ok(staged)
}

/// The content of the entry `name` (in any case) of a ZIP; `None` when the
/// ZIP has no such entry.
pub fn read_entry(zip_data: &[u8], name: &str) -> Result<Option<Vec<u8>>> {
    let mut archive =
        zip::ZipArchive::new(Cursor::new(zip_data)).map_err(zip_err("Invalid ZIP archive"))?;
    for i in 0..archive.len() {
        let mut file = archive
            .by_index(i)
            .map_err(zip_err("Invalid ZIP archive"))?;
        if file.name().eq_ignore_ascii_case(name) {
            let mut buf = Vec::new();
            file.read_to_end(&mut buf)?;
            return Ok(Some(buf));
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// A fixed timestamp for the entries staging writes new.
    const T: ZipTime = ZipTime {
        year: 2026,
        month: 10,
        day: 31,
        hour: 12,
        minute: 0,
        second: 0,
    };

    #[test]
    fn test_bad_magic() {
        let data = vec![0x00; 100];
        let err = decompress_loxcc(&data).unwrap_err();
        assert!(err.to_string().contains("Not a valid LoxCC"));
    }

    #[test]
    fn test_too_short() {
        let data = vec![0xee, 0xcc, 0xbb, 0xaa]; // just magic, no header
        let err = decompress_loxcc(&data).unwrap_err();
        assert!(err.to_string().contains("too short"));
    }

    #[test]
    fn test_decompress_literals_only() {
        // Build a minimal LoxCC with only literal data (no back-references)
        let xml = b"<Config>Hello</Config>";
        let mut compressed = Vec::new();

        // Encode as a single literal block: high nibble = len (or 15+extra)
        let lit_len = xml.len();
        if lit_len < 15 {
            compressed.push((lit_len as u8) << 4); // high nibble = lit_len, low = 0
        } else {
            compressed.push(0xF0); // high nibble = 15
            let mut remaining = lit_len - 15;
            while remaining >= 255 {
                compressed.push(255);
                remaining -= 255;
            }
            compressed.push(remaining as u8);
        }
        compressed.extend_from_slice(xml);

        // Build full LoxCC blob
        let mut blob = Vec::new();
        blob.extend_from_slice(&LOXCC_MAGIC.to_le_bytes());
        blob.extend_from_slice(&(compressed.len() as u32).to_le_bytes());
        blob.extend_from_slice(&(xml.len() as u32).to_le_bytes());
        blob.extend_from_slice(&0u32.to_le_bytes()); // checksum placeholder
        blob.extend_from_slice(&compressed);

        let result = decompress_loxcc(&blob).unwrap();
        assert_eq!(result, xml);
    }

    #[test]
    fn test_decompress_with_backreference() {
        // Encode "ABCABCABC" using a literal "ABC" + back-reference
        let mut compressed = Vec::new();

        // Token: 3 literals, match_len - 4 = 2 (i.e., match_len = 6)
        compressed.push(0x32); // high=3 (literals), low=2 (match_extra)
        compressed.extend_from_slice(b"ABC"); // 3 literal bytes
        compressed.extend_from_slice(&3u16.to_le_bytes()); // offset = 3 (back to start of "ABC")

        let mut blob = Vec::new();
        blob.extend_from_slice(&LOXCC_MAGIC.to_le_bytes());
        blob.extend_from_slice(&(compressed.len() as u32).to_le_bytes());
        blob.extend_from_slice(&9u32.to_le_bytes()); // uncompressed hint
        blob.extend_from_slice(&0u32.to_le_bytes());
        blob.extend_from_slice(&compressed);

        let result = decompress_loxcc(&blob).unwrap();
        assert_eq!(result, b"ABCABCABC"); // 3 literal + 6 from back-ref
    }

    #[test]
    fn test_extract_missing_loxcc() {
        // Create a ZIP without sps0.LoxCC
        let buf = Vec::new();
        let cursor = std::io::Cursor::new(buf);
        let mut zip_writer = zip::ZipWriter::new(cursor);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        zip_writer.start_file("other.txt", options).unwrap();
        zip_writer.write_all(b"hello").unwrap();
        let cursor = zip_writer.finish().unwrap();
        let zip_data = cursor.into_inner();

        let err = extract_and_decompress(&zip_data).unwrap_err();
        assert!(err.to_string().contains("sps0.LoxCC"));
    }

    #[test]
    fn test_extract_and_decompress_roundtrip() {
        // Build a LoxCC blob with literal XML
        let xml = b"<?xml version=\"1.0\"?><LoxoneProject/>";
        let mut compressed = Vec::new();
        let lit_len = xml.len();
        if lit_len < 15 {
            compressed.push((lit_len as u8) << 4);
        } else {
            compressed.push(0xF0);
            let mut remaining = lit_len - 15;
            while remaining >= 255 {
                compressed.push(255);
                remaining -= 255;
            }
            compressed.push(remaining as u8);
        }
        compressed.extend_from_slice(xml);

        let mut loxcc_blob = Vec::new();
        loxcc_blob.extend_from_slice(&LOXCC_MAGIC.to_le_bytes());
        loxcc_blob.extend_from_slice(&(compressed.len() as u32).to_le_bytes());
        loxcc_blob.extend_from_slice(&(xml.len() as u32).to_le_bytes());
        loxcc_blob.extend_from_slice(&0u32.to_le_bytes());
        loxcc_blob.extend_from_slice(&compressed);

        // Pack into a ZIP as sps0.LoxCC
        let buf = Vec::new();
        let cursor = std::io::Cursor::new(buf);
        let mut zip_writer = zip::ZipWriter::new(cursor);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        zip_writer.start_file("sps0.LoxCC", options).unwrap();
        zip_writer.write_all(&loxcc_blob).unwrap();
        let cursor = zip_writer.finish().unwrap();
        let zip_data = cursor.into_inner();

        let result = extract_and_decompress(&zip_data).unwrap();
        assert_eq!(result, xml);
    }

    /// Deterministic pseudo-random bytes (incompressible beyond the 64 KiB window).
    fn noise(len: usize) -> Vec<u8> {
        let mut state = 0x2545_f491_u32;
        (0..len)
            .map(|_| {
                state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                (state >> 24) as u8
            })
            .collect()
    }

    fn header(blob: &[u8]) -> [u32; 4] {
        let field = |i: usize| u32::from_le_bytes(blob[i * 4..i * 4 + 4].try_into().unwrap());
        [field(0), field(1), field(2), field(3)]
    }

    fn sample_xmls() -> Vec<Vec<u8>> {
        let repetitive = "<C Type=\"Memory\" V=\"175\"/>\n".repeat(40_000);
        let mut large = br#"<?xml version="1.0" encoding="utf-8"?><C>"#.to_vec();
        large.extend(noise(200_000));
        large.extend(repetitive.as_bytes()[..100_000].iter());
        vec![
            Vec::new(),
            b"<a/>".to_vec(),
            repetitive.into_bytes(),
            large,
            "<C Title=\"Küche ° Licht €\" Desc=\"日本語\"/>"
                .as_bytes()
                .to_vec(),
        ]
    }

    #[test]
    fn program_xml_takes_a_zip_or_plain_xml() {
        let xml = b"<ControlList/>";
        assert_eq!(&*program_xml(xml).unwrap(), xml);
        let zip = zip_with(&[(
            SPS0_ENTRY,
            &compress_loxcc(xml).unwrap(),
            zip::CompressionMethod::Deflated,
        )]);
        assert!(is_zip(&zip));
        assert_eq!(&*program_xml(&zip).unwrap(), xml);
    }

    #[test]
    fn test_compress_roundtrip() {
        for xml in sample_xmls() {
            let blob = compress_loxcc(&xml).unwrap();
            assert_eq!(decompress_loxcc(&blob).unwrap(), xml, "len {}", xml.len());
        }
    }

    #[test]
    fn test_compress_header_fields() {
        for xml in sample_xmls() {
            let blob = compress_loxcc(&xml).unwrap();
            let [magic, payload_len, xml_len, crc] = header(&blob);
            assert_eq!(magic, LOXCC_MAGIC);
            assert_eq!(payload_len as usize, blob.len() - LOXCC_HEADER_LEN);
            assert_eq!(xml_len as usize, xml.len());
            assert_eq!(crc, crc32fast::hash(&xml));
        }
    }

    #[test]
    fn test_compress_payload_is_standard_lz4() {
        for xml in sample_xmls() {
            let blob = compress_loxcc(&xml).unwrap();
            let plain = lz4_flex::block::decompress(&blob[LOXCC_HEADER_LEN..], xml.len()).unwrap();
            assert_eq!(plain, xml);
        }
    }

    #[test]
    fn test_compress_shrinks_repetitive_xml() {
        let xml = "<C Type=\"And\"/>".repeat(10_000);
        let blob = compress_loxcc(xml.as_bytes()).unwrap();
        assert!(blob.len() < xml.len() / 10, "{} bytes", blob.len());
    }

    fn zip_with(entries: &[(&str, &[u8], zip::CompressionMethod)]) -> Vec<u8> {
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let modified = zip::DateTime::from_date_and_time(2026, 10, 18, 20, 30, 56).unwrap();
        for (name, data, method) in entries {
            let options = zip::write::SimpleFileOptions::default()
                .compression_method(*method)
                .last_modified_time(modified);
            writer.start_file(*name, options).unwrap();
            writer.write_all(data).unwrap();
        }
        writer.finish().unwrap().into_inner()
    }

    /// Name, compression method, timestamp and raw (still compressed) bytes per entry.
    type RawEntry = (
        String,
        zip::CompressionMethod,
        Option<zip::DateTime>,
        Vec<u8>,
    );

    fn raw_entries(zip_data: &[u8]) -> Vec<RawEntry> {
        let mut archive = zip::ZipArchive::new(Cursor::new(zip_data)).unwrap();
        (0..archive.len())
            .map(|i| {
                let mut entry = archive.by_index_raw(i).unwrap();
                let mut raw = Vec::new();
                entry.read_to_end(&mut raw).unwrap();
                let name = entry.name().to_string();
                (name, entry.compression(), entry.last_modified(), raw)
            })
            .collect()
    }

    fn template_zip() -> Vec<u8> {
        let old = compress_loxcc(b"<old config/>").unwrap();
        let deflated = zip::CompressionMethod::Deflated;
        zip_with(&[
            ("sps0.LoxCC", &old, deflated),
            ("LoxAPP3.json", br#"{"msInfo":{}}"#, deflated),
            (
                "permissions.bin",
                &noise(3000),
                zip::CompressionMethod::Stored,
            ),
            ("Emergency.LoxCC", &old, deflated),
            ("Music.json", b"{}", deflated),
        ])
    }

    #[test]
    fn test_stage_upload_zip_replaces_only_sps0() {
        let template = template_zip();
        let xml = "<C Title=\"Küche\"/>".repeat(5000);
        let staged = stage_upload_zip(&template, xml.as_bytes(), T).unwrap();

        assert_eq!(extract_and_decompress(&staged).unwrap(), xml.as_bytes());
        let before = raw_entries(&template);
        let after = raw_entries(&staged);
        assert_eq!(
            before.iter().map(|e| &e.0).collect::<Vec<_>>(),
            after.iter().map(|e| &e.0).collect::<Vec<_>>()
        );
        // sps0.LoxCC: new content and timestamp, same method
        assert_eq!(before[0].1, after[0].1);
        assert_ne!(before[0].2, after[0].2);
        assert_ne!(before[0].3, after[0].3);
        // every other entry is copied raw, byte for byte
        assert_eq!(before[1..], after[1..]);
    }

    #[test]
    fn test_stage_upload_zip_replaces_permissions_too() {
        let template = template_zip();
        let permissions = noise(48);
        let staged = stage_upload_zip_replacing(
            &template,
            b"<new/>",
            &[(PERMISSIONS_ENTRY, &permissions)],
            T,
        )
        .unwrap();
        assert_eq!(
            read_entry(&staged, PERMISSIONS_ENTRY).unwrap().unwrap(),
            permissions
        );
        let before = raw_entries(&template);
        let after = raw_entries(&staged);
        // permissions.bin: same name, place and method, new content and time
        assert_eq!((&before[2].0, before[2].1), (&after[2].0, after[2].1));
        assert_ne!(before[2].2, after[2].2);
        // the rest is still copied raw
        assert_eq!(before[1], after[1]);
        assert_eq!(before[3..], after[3..]);

        let none = zip_with(&[(
            "sps0.LoxCC",
            &compress_loxcc(b"<old/>").unwrap(),
            zip::CompressionMethod::Stored,
        )]);
        let err =
            stage_upload_zip_replacing(&none, b"<new/>", &[(PERMISSIONS_ENTRY, &permissions)], T)
                .unwrap_err();
        assert!(
            err.to_string()
                .contains("exactly one permissions.bin, found 0"),
            "{err}"
        );
    }

    #[test]
    fn test_stage_upload_zip_matches_name_case_insensitively() {
        let old = compress_loxcc(b"<old/>").unwrap();
        let template = zip_with(&[("SPS0.LOXCC", &old, zip::CompressionMethod::Stored)]);
        let staged = stage_upload_zip(&template, b"<new/>", T).unwrap();
        assert_eq!(raw_entries(&staged)[0].0, "SPS0.LOXCC");
        assert_eq!(extract_and_decompress(&staged).unwrap(), b"<new/>");
    }

    #[test]
    fn test_stage_upload_zip_needs_exactly_one_sps0() {
        let old = compress_loxcc(b"<old/>").unwrap();
        let stored = zip::CompressionMethod::Stored;

        let none = zip_with(&[("Music.json", b"{}", stored)]);
        let err = stage_upload_zip(&none, b"<new/>", T).unwrap_err();
        assert!(err.to_string().contains("found 0"), "{err}");

        let two = zip_with(&[("sps0.LoxCC", &old, stored), ("SPS0.loxcc", &old, stored)]);
        let err = stage_upload_zip(&two, b"<new/>", T).unwrap_err();
        assert!(err.to_string().contains("found 2"), "{err}");

        let err = stage_upload_zip(b"not a zip", b"<new/>", T).unwrap_err();
        assert!(err.to_string().contains("Invalid template ZIP"), "{err}");
    }

    /// Real downloads: `LOX_CORPUS=<dir> cargo test -p lxir corpus_ -- --nocapture`.
    /// Every ZIP in the directory (by content, whatever its name) must carry
    /// the header semantics documented on [`decompress_loxcc`], and its XML
    /// must survive compress → decompress and stage → extract unchanged.
    #[test]
    fn test_corpus_roundtrip() {
        let Some(dir) = std::env::var_os("LOX_CORPUS") else {
            return;
        };
        let mut seen = 0;
        let mut paths: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect();
        paths.sort();
        for path in paths {
            let data = std::fs::read(&path).unwrap();
            if !data.starts_with(b"PK\x03\x04") {
                continue;
            }
            let name = path.display();
            let mut archive = zip::ZipArchive::new(Cursor::new(&data[..])).unwrap();
            let mut original = Vec::new();
            archive
                .by_name("sps0.LoxCC")
                .unwrap()
                .read_to_end(&mut original)
                .unwrap();
            let xml = decompress_loxcc(&original).unwrap();
            let [_, payload_len, xml_len, crc] = header(&original);
            assert_eq!(
                payload_len as usize,
                original.len() - LOXCC_HEADER_LEN,
                "{name}"
            );
            assert_eq!(xml_len as usize, xml.len(), "{name}");
            assert_eq!(crc, crc32fast::hash(&xml), "{name}");

            let ours = compress_loxcc(&xml).unwrap();
            assert_eq!(decompress_loxcc(&ours).unwrap(), xml, "{name}");
            let staged = stage_upload_zip(&data, &xml, T).unwrap();
            assert_eq!(extract_and_decompress(&staged).unwrap(), xml, "{name}");
            println!(
                "{name}: xml {} B, sps0.LoxCC {} B as downloaded, {} B ours{}",
                xml.len(),
                original.len(),
                ours.len(),
                if ours == original { " (identical)" } else { "" }
            );
            seen += 1;
        }
        assert!(seen > 0, "no ZIPs in LOX_CORPUS");
    }

    /// Real downloads: `LOX_CORPUS=<dir> cargo test -p lxir corpus_ -- --nocapture`.
    /// Every ZIP with a `permissions.bin` must get exactly that file back
    /// from `crate::permissions::generate` over its own program and
    /// `LoxAPP3.json`.
    #[test]
    fn test_corpus_permissions() {
        let Some(dir) = std::env::var_os("LOX_CORPUS") else {
            return;
        };
        let mut paths: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect();
        paths.sort();
        let (mut matched, mut failed) = (0, Vec::new());
        for path in paths {
            let data = std::fs::read(&path).unwrap();
            if !data.starts_with(b"PK\x03\x04") {
                continue;
            }
            let name = path.display().to_string();
            let (Some(bin), Some(app)) = (
                read_entry(&data, PERMISSIONS_ENTRY).unwrap(),
                read_entry(&data, crate::catalogue::MEMBER).unwrap(),
            ) else {
                println!("{name}: no permissions.bin or LoxAPP3.json, skipped");
                continue;
            };
            let xml = extract_and_decompress(&data).unwrap();
            let doc = crate::LoxoneDoc::parse(&xml).unwrap();
            let app = String::from_utf8(app).unwrap();
            let ours =
                crate::permissions::encode(&crate::permissions::generate(&doc, &app).unwrap());
            if ours == bin {
                println!("{name}: permissions.bin identical ({} B)", bin.len());
                matched += 1;
            } else {
                let at = ours.iter().zip(&bin).position(|(a, b)| a != b);
                println!(
                    "{name}: permissions.bin differs ({} B ours, {} B written, first at {at:?})",
                    ours.len(),
                    bin.len()
                );
                failed.push(name);
            }
        }
        assert!(failed.is_empty(), "permissions.bin differs for {failed:?}");
        assert!(matched > 0, "no ZIP with permissions.bin in LOX_CORPUS");
    }
}
