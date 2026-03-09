//! LoxCC decompression — extract Loxone config XML from backup archives
//!
//! Backup ZIPs contain `sps0.LoxCC`, a custom LZ-compressed binary.
//! This module decompresses it to the original XML.
//!
//! Reference: <https://gist.github.com/sarnau/e14ff9fe081611782a3f3cb2e2c2bacd>

use anyhow::{bail, Context, Result};
use std::io::{Cursor, Read};

const LOXCC_MAGIC: u32 = 0xaabbccee;

/// Decompress a `sps0.LoxCC` binary blob into XML bytes.
///
/// Format:
///   [0..4]  u32_le magic (0xaabbccee)
///   [4..8]  u32_le compressed payload size
///   [8..12] u32_le uncompressed size hint (for pre-allocation)
///   [12..16] u32_le reserved/checksum
///   [16..]  compressed data (LZ4-style)
pub fn decompress_loxcc(data: &[u8]) -> Result<Vec<u8>> {
    if data.len() < 16 {
        bail!("LoxCC data too short ({} bytes)", data.len());
    }
    let magic = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
    if magic != LOXCC_MAGIC {
        bail!(
            "Not a valid LoxCC file (magic: 0x{:08x}, expected 0x{:08x})",
            magic,
            LOXCC_MAGIC
        );
    }
    let _comp_size = u32::from_le_bytes([data[4], data[5], data[6], data[7]]) as usize;
    let uncomp_hint = u32::from_le_bytes([data[8], data[9], data[10], data[11]]) as usize;

    let compressed = &data[16..];
    let mut output = Vec::with_capacity(uncomp_hint.max(1024));
    let mut pos = 0;

    while pos < compressed.len() {
        let token = compressed[pos];
        pos += 1;

        // High nibble: literal count
        let mut lit_len = (token >> 4) as usize;
        if lit_len == 15 {
            loop {
                if pos >= compressed.len() {
                    break;
                }
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
            let remaining = compressed.len() - pos;
            output.extend_from_slice(&compressed[pos..pos + remaining]);
            break;
        }
        output.extend_from_slice(&compressed[pos..pos + lit_len]);
        pos += lit_len;

        // End of compressed data — no match follows the last literal
        if pos >= compressed.len() {
            break;
        }

        // Back-reference offset (u16_le)
        if pos + 2 > compressed.len() {
            break;
        }
        let offset = u16::from_le_bytes([compressed[pos], compressed[pos + 1]]) as usize;
        pos += 2;

        if offset == 0 {
            bail!("LoxCC: zero back-reference offset at byte {}", pos - 2);
        }

        // Low nibble: match length (min 4)
        let mut match_len = (token & 0x0F) as usize + 4;
        if (token & 0x0F) == 15 {
            loop {
                if pos >= compressed.len() {
                    break;
                }
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
            bail!(
                "LoxCC: back-reference offset {} exceeds output size {}",
                offset,
                output.len()
            );
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
    let cursor = Cursor::new(zip_data);
    let mut archive = zip::ZipArchive::new(cursor).context("Invalid ZIP archive")?;

    // Look for sps0.LoxCC (case-insensitive)
    let mut loxcc_data = None;
    for i in 0..archive.len() {
        let mut file = archive.by_index(i)?;
        if file
            .name()
            .to_lowercase()
            .contains("sps0.loxcc")
        {
            let mut buf = Vec::new();
            file.read_to_end(&mut buf)?;
            loxcc_data = Some(buf);
            break;
        }
    }

    let data = loxcc_data.context("Backup ZIP does not contain sps0.LoxCC")?;
    decompress_loxcc(&data)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

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
        let options =
            zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
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
        let options =
            zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        zip_writer.start_file("sps0.LoxCC", options).unwrap();
        zip_writer.write_all(&loxcc_blob).unwrap();
        let cursor = zip_writer.finish().unwrap();
        let zip_data = cursor.into_inner();

        let result = extract_and_decompress(&zip_data).unwrap();
        assert_eq!(result, xml);
    }
}
