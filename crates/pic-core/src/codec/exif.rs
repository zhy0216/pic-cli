//! Read primary-image orientation and color declarations from classic TIFF EXIF.
//! Other EXIF data is not retained on pixel export. Malformed/ambiguous orientation
//! is rejected instead of silently treating it as orientation 1.

use crate::{ErrorCode, PicError, Result};

fn invalid() -> PicError {
    PicError::new(
        ErrorCode::UnsupportedMetadata,
        "malformed or ambiguous EXIF; supply valid classic TIFF metadata",
    )
}

struct Tiff<'a> {
    bytes: &'a [u8],
    little: bool,
}

impl Tiff<'_> {
    fn slice(&self, offset: usize, length: usize) -> Result<&[u8]> {
        self.bytes
            .get(offset..offset.checked_add(length).ok_or_else(invalid)?)
            .ok_or_else(invalid)
    }

    fn short(&self, offset: usize) -> Result<u16> {
        let bytes = self.slice(offset, 2)?.try_into().map_err(|_| invalid())?;
        Ok(if self.little {
            u16::from_le_bytes(bytes)
        } else {
            u16::from_be_bytes(bytes)
        })
    }

    fn long(&self, offset: usize) -> Result<u32> {
        let bytes = self.slice(offset, 4)?.try_into().map_err(|_| invalid())?;
        Ok(if self.little {
            u32::from_le_bytes(bytes)
        } else {
            u32::from_be_bytes(bytes)
        })
    }

    fn ifd(&self, offset: usize) -> Result<impl Iterator<Item = usize>> {
        if offset < 8 {
            return Err(invalid());
        }
        let count = usize::from(self.short(offset)?);
        self.slice(offset, 2 + count * 12 + 4)?;
        Ok((0..count).map(move |index| offset + 2 + index * 12))
    }

    fn validate_entry(&self, entry: usize) -> Result<()> {
        let size: usize = match self.short(entry + 2)? {
            1 | 2 | 6 | 7 => 1,
            3 | 8 => 2,
            4 | 9 | 11 | 13 => 4,
            5 | 10 | 12 => 8,
            _ => return Err(invalid()),
        };
        let count = usize::try_from(self.long(entry + 4)?).map_err(|_| invalid())?;
        let length = count.checked_mul(size).ok_or_else(invalid)?;
        if length > 4 {
            let offset = usize::try_from(self.long(entry + 8)?).map_err(|_| invalid())?;
            self.slice(offset, length)?;
        }
        Ok(())
    }

    fn scalar(&self, entry: usize, kind: u16) -> Result<u32> {
        if self.short(entry + 2)? != kind || self.long(entry + 4)? != 1 {
            return Err(invalid());
        }
        if kind == 3 {
            Ok(u32::from(self.short(entry + 8)?))
        } else {
            self.long(entry + 8)
        }
    }
}

pub(super) fn read(bytes: &[u8]) -> Result<(u8, bool)> {
    let little = match bytes.get(..4) {
        Some(b"II\x2a\0") => true,
        Some(b"MM\0\x2a") => false,
        _ => return Err(invalid()),
    };
    let tiff = Tiff { bytes, little };
    let first = usize::try_from(tiff.long(4)?).map_err(|_| invalid())?;
    let mut orientation = None;
    let mut color_space = None;
    let mut exif_ifd = None;
    for entry in tiff.ifd(first)? {
        tiff.validate_entry(entry)?;
        match tiff.short(entry)? {
            0x0112 => {
                let value = tiff.scalar(entry, 3)?;
                if orientation.is_some() || !(1..=8).contains(&value) {
                    return Err(invalid());
                }
                orientation = Some(value as u8);
            }
            0x8769 => {
                if exif_ifd.is_some() {
                    return Err(invalid());
                }
                exif_ifd = Some(tiff.scalar(entry, 4)? as usize);
            }
            tag => check_color(&tiff, entry, tag, &mut color_space)?,
        }
    }
    if let Some(offset) = exif_ifd {
        if offset == first {
            return Err(invalid());
        }
        for entry in tiff.ifd(offset)? {
            tiff.validate_entry(entry)?;
            check_color(&tiff, entry, tiff.short(entry)?, &mut color_space)?;
        }
    }
    Ok((orientation.unwrap_or(1), color_space == Some(1)))
}

fn check_color(
    tiff: &Tiff<'_>,
    entry: usize,
    tag: u16,
    color_space: &mut Option<u32>,
) -> Result<()> {
    match tag {
        0xa001 => {
            if color_space.is_some() {
                return Err(invalid());
            }
            let value = tiff.scalar(entry, 3)?;
            if value != 1 {
                return Err(PicError::new(
                    ErrorCode::UnsupportedColor,
                    "EXIF declares unsupported or uncalibrated color space; only sRGB (1) is supported",
                ));
            }
            *color_space = Some(value);
        }
        0x012d | 0x013e | 0x013f | 0x8773 | 0xa500 => {
            return Err(PicError::new(
                ErrorCode::UnsupportedColor,
                "EXIF transfer function, chromaticity, ICC or gamma conversion is not implemented",
            ));
        }
        _ => (),
    }
    Ok(())
}
