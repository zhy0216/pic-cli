//! Conservative metadata gates. Decoding and encoding remain entirely in `image`.
//! Reject unsupported representations before the decoder silently converts them.

use crate::{ErrorCode, PicError, Result};

fn malformed(message: &str) -> PicError {
    PicError::new(ErrorCode::DecodeFailed, message)
}

pub(super) fn png(bytes: &[u8]) -> Result<bool> {
    let mut offset = 8;
    let mut declared_srgb = false;
    let mut saw_header = false;
    while offset < bytes.len() {
        let header = bytes
            .get(offset..offset + 8)
            .ok_or_else(|| malformed("truncated PNG chunk header"))?;
        let length = u32::from_be_bytes(header[..4].try_into().expect("four-byte slice")) as usize;
        let end = offset
            .checked_add(12)
            .and_then(|n| n.checked_add(length))
            .filter(|end| *end <= bytes.len())
            .ok_or_else(|| malformed("truncated PNG chunk"))?;
        let kind = &header[4..8];
        let data = &bytes[offset + 8..end - 4];
        if !saw_header && kind != b"IHDR" {
            return Err(malformed("PNG must start with IHDR"));
        }
        match kind {
            b"IHDR" => {
                if saw_header || data.len() != 13 {
                    return Err(malformed("invalid PNG IHDR"));
                }
                saw_header = true;
                if data[8] != 8 || ![0, 2, 4, 6].contains(&data[9]) {
                    return Err(PicError::new(
                        ErrorCode::UnsupportedColor,
                        "PNG requires 8-bit grayscale/RGB/RGBA; palette and other bit depths are not implemented",
                    ));
                }
            }
            b"sRGB" => {
                if data.len() != 1 || data[0] > 3 {
                    return Err(malformed("invalid PNG sRGB chunk"));
                }
                declared_srgb = true;
            }
            b"iCCP" | b"gAMA" | b"cHRM" | b"cICP" | b"mDCV" | b"cLLI" => {
                return Err(PicError::new(
                    ErrorCode::UnsupportedColor,
                    "PNG ICC/gamma/chromaticity/HDR metadata is not implemented; supply 8-bit sRGB with only an sRGB tag or no color tags",
                ));
            }
            b"eXIf" => {
                return Err(PicError::new(
                    ErrorCode::UnsupportedMetadata,
                    "EXIF handling is not implemented; supply pixels with orientation already applied and EXIF removed",
                ));
            }
            b"acTL" => {
                return Err(PicError::new(
                    ErrorCode::UnsupportedFormat,
                    "animated PNG is not implemented",
                ));
            }
            b"IEND" => {
                if !data.is_empty() || end != bytes.len() {
                    return Err(malformed("invalid PNG ending"));
                }
                return Ok(declared_srgb);
            }
            _ => (),
        }
        offset = end;
    }
    Err(malformed("PNG has no IEND"))
}

pub(super) fn jpeg(bytes: &[u8]) -> Result<()> {
    let mut offset = 2;
    let mut saw_frame = false;
    let mut saw_scan = false;
    while offset < bytes.len() {
        if bytes[offset] != 0xff {
            return Err(malformed("invalid JPEG marker"));
        }
        while bytes.get(offset) == Some(&0xff) {
            offset += 1;
        }
        let marker = *bytes
            .get(offset)
            .ok_or_else(|| malformed("truncated JPEG marker"))?;
        offset += 1;
        if marker == 0xd9 {
            return if saw_frame && saw_scan && offset == bytes.len() {
                Ok(())
            } else {
                Err(malformed("invalid JPEG ending"))
            };
        }
        let length = bytes
            .get(offset..offset + 2)
            .ok_or_else(|| malformed("truncated JPEG segment"))?;
        let length = u16::from_be_bytes([length[0], length[1]]) as usize;
        if length < 2 {
            return Err(malformed("invalid JPEG segment length"));
        }
        let data = bytes
            .get(offset + 2..offset + length)
            .ok_or_else(|| malformed("truncated JPEG segment data"))?;
        if marker == 0xe1 && data.starts_with(b"Exif\0\0") {
            return Err(PicError::new(
                ErrorCode::UnsupportedMetadata,
                "EXIF handling is not implemented; supply pixels with orientation already applied and EXIF removed",
            ));
        }
        if marker == 0xe2 && data.starts_with(b"ICC_PROFILE\0") {
            return Err(PicError::new(
                ErrorCode::UnsupportedColor,
                "ICC profiles are not implemented; supply untagged 8-bit sRGB JPEG",
            ));
        }
        if [
            0xc0, 0xc1, 0xc2, 0xc3, 0xc5, 0xc6, 0xc7, 0xc9, 0xca, 0xcb, 0xcd, 0xce, 0xcf,
        ]
        .contains(&marker)
        {
            if data.len() < 6 {
                return Err(malformed("truncated JPEG frame header"));
            }
            if ![0xc0, 0xc1, 0xc2].contains(&marker) || data[0] != 8 || ![1, 3].contains(&data[5]) {
                return Err(PicError::new(
                    ErrorCode::UnsupportedColor,
                    "JPEG requires 8-bit grayscale/RGB/YCbCr; CMYK and other coding modes are not implemented",
                ));
            }
            saw_frame = true;
        }
        if marker == 0xda {
            if !saw_frame {
                return Err(malformed("JPEG scan has no frame header"));
            }
            saw_scan = true;
        }
        offset += length;
        if marker == 0xda {
            // Skip entropy bytes, stuffed FF bytes and restart markers. Continue
            // inspecting segments between progressive scans and before EOI too.
            while offset < bytes.len() {
                if bytes[offset] != 0xff {
                    offset += 1;
                    continue;
                }
                let marker_start = offset;
                while bytes.get(offset) == Some(&0xff) {
                    offset += 1;
                }
                match bytes.get(offset) {
                    Some(0x00 | 0xd0..=0xd7) => offset += 1,
                    Some(_) => {
                        offset = marker_start;
                        break;
                    }
                    None => return Err(malformed("truncated JPEG scan")),
                }
            }
        }
    }
    Err(malformed("JPEG has no end-of-image marker"))
}
