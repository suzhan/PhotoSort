//! nom-exif 引擎：标准图像格式（JPEG/HEIC/TIFF/PNG/AVIF/CR3/RAF/IIQ）的主路径。

use std::path::Path;

use chrono::NaiveDateTime;
use nom_exif::{EntryValue, Exif, ExifDateTime, ExifTag, MediaParser, MediaSource};

use super::clean::clean_exif_string;
use super::datetime::parse_exif_datetime;
use crate::error::{AppError, Result};
use crate::models::metadata::{GpsCoordinate, PhotoMetadata, TakenAtSource};

fn text(exif: &Exif, tag: ExifTag) -> Option<String> {
    match exif.get(tag) {
        Some(EntryValue::Text(s)) => clean_exif_string(s),
        _ => None,
    }
}

fn datetime_at(exif: &Exif, tag: ExifTag) -> Option<NaiveDateTime> {
    let value = exif.get(tag)?;
    match value.as_datetime() {
        Some(ExifDateTime::Naive(dt)) => Some(dt),
        // 带 OffsetTimeOriginal 的相机：目录命名用相机本地墙钟时间。
        Some(ExifDateTime::Aware(dt)) => Some(dt.naive_local()),
        None => match value {
            EntryValue::Text(s) => parse_exif_datetime(s),
            _ => None,
        },
    }
}

fn gps(exif: &Exif) -> Option<GpsCoordinate> {
    let info = exif.gps_info()?;
    match (info.latitude_decimal(), info.longitude_decimal()) {
        (Some(latitude), Some(longitude)) => Some(GpsCoordinate {
            latitude,
            longitude,
        }),
        _ => None,
    }
}

pub fn read(parser: &mut MediaParser, path: &Path) -> Result<PhotoMetadata> {
    let source =
        MediaSource::open(path).map_err(|e| AppError::Exif(format!("nom-exif open: {e}")))?;
    let iter = parser
        .parse_exif(source)
        .map_err(|e| AppError::Exif(format!("nom-exif parse: {e}")))?;
    let exif: Exif = iter.into();

    let (taken_at, taken_at_source) =
        if let Some(dt) = datetime_at(&exif, ExifTag::DateTimeOriginal) {
            (Some(dt), Some(TakenAtSource::ExifDateTimeOriginal))
        } else if let Some(dt) = datetime_at(&exif, ExifTag::CreateDate) {
            (Some(dt), Some(TakenAtSource::ExifCreateDate))
        } else {
            (None, None)
        };

    Ok(PhotoMetadata {
        taken_at,
        taken_at_source,
        camera_make: text(&exif, ExifTag::Make),
        camera_model: text(&exif, ExifTag::Model),
        lens_make: text(&exif, ExifTag::LensMake),
        lens_model: text(&exif, ExifTag::LensModel),
        gps: gps(&exif),
    })
}

#[cfg(test)]
mod tests {
    //! 用手工构造的最小 EXIF JPEG 做真引擎回归（不依赖真实照片样本）。
    use super::*;
    use std::io::Write;

    const TIFF_ASCII: u16 = 2;
    const TIFF_LONG: u16 = 4;
    const TIFF_RATIONAL: u16 = 5;

    struct Entry {
        tag: u16,
        typ: u16,
        count: u32,
        inline: [u8; 4],
        data: Option<Vec<u8>>,
    }

    fn ascii(tag: u16, s: &str) -> Entry {
        let mut bytes = s.as_bytes().to_vec();
        bytes.push(0);
        let count = bytes.len() as u32;
        if bytes.len() <= 4 {
            let mut inline = [0u8; 4];
            inline[..bytes.len()].copy_from_slice(&bytes);
            Entry {
                tag,
                typ: TIFF_ASCII,
                count,
                inline,
                data: None,
            }
        } else {
            Entry {
                tag,
                typ: TIFF_ASCII,
                count,
                inline: [0; 4],
                data: Some(bytes),
            }
        }
    }

    fn long(tag: u16, value: u32) -> Entry {
        Entry {
            tag,
            typ: TIFF_LONG,
            count: 1,
            inline: value.to_le_bytes(),
            data: None,
        }
    }

    fn rational3(tag: u16, values: [(u32, u32); 3]) -> Entry {
        let mut data = Vec::with_capacity(24);
        for (n, d) in values {
            data.extend_from_slice(&n.to_le_bytes());
            data.extend_from_slice(&d.to_le_bytes());
        }
        Entry {
            tag,
            typ: TIFF_RATIONAL,
            count: 3,
            inline: [0; 4],
            data: Some(data),
        }
    }

    fn write_ifd(
        out: &mut Vec<u8>,
        entries: &mut [Entry],
        data_area: &mut Vec<u8>,
        data_base: u32,
    ) {
        entries.sort_by_key(|e| e.tag);
        out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        for entry in entries.iter_mut() {
            out.extend_from_slice(&entry.tag.to_le_bytes());
            out.extend_from_slice(&entry.typ.to_le_bytes());
            out.extend_from_slice(&entry.count.to_le_bytes());
            match entry.data.take() {
                Some(bytes) => {
                    let offset = data_base + data_area.len() as u32;
                    out.extend_from_slice(&offset.to_le_bytes());
                    data_area.extend_from_slice(&bytes);
                }
                None => out.extend_from_slice(&entry.inline),
            }
        }
    }

    fn ifd_size(count: usize) -> u32 {
        2 + count as u32 * 12 + 4
    }

    /// 构造含 Make/Model/DateTimeOriginal/CreateDate/Lens/GPS 的最小 JPEG。
    fn build_exif_jpeg(path: &Path) {
        // 22°19.158′N = 22.3193；114°10.164′E = 114.1694
        let mut ifd0 = vec![
            ascii(0x010F, "NIKON CORPORATION"),
            ascii(0x0110, "NIKON D80"),
            long(0x8769, 0), // ExifIFD 指针，稍后回填
            long(0x8825, 0), // GPSIFD 指针
        ];
        let mut exif_ifd = vec![
            ascii(0x9003, "2017:11:30 15:22:31"),
            ascii(0x9004, "2017:11:30 15:22:32"),
            ascii(0xA433, "NIKON"),
            ascii(0xA434, "18-135mm F3.5-5.6"),
        ];
        let mut gps_ifd = vec![
            ascii(0x0001, "N"),
            rational3(0x0002, [(22, 1), (19158, 1000), (0, 1)]),
            ascii(0x0003, "E"),
            rational3(0x0004, [(114, 1), (10164, 1000), (0, 1)]),
        ];

        let ifd0_offset = 8u32;
        let exif_offset = ifd0_offset + ifd_size(ifd0.len());
        let gps_offset = exif_offset + ifd_size(exif_ifd.len());
        let data_base = gps_offset + ifd_size(gps_ifd.len());

        ifd0[2].inline = exif_offset.to_le_bytes();
        ifd0[3].inline = gps_offset.to_le_bytes();

        let mut tiff = Vec::new();
        tiff.extend_from_slice(b"II");
        tiff.extend_from_slice(&42u16.to_le_bytes());
        tiff.extend_from_slice(&ifd0_offset.to_le_bytes());

        let mut data_area: Vec<u8> = Vec::new();
        write_ifd(&mut tiff, &mut ifd0, &mut data_area, data_base);
        tiff.extend_from_slice(&0u32.to_le_bytes()); // next IFD
        write_ifd(&mut tiff, &mut exif_ifd, &mut data_area, data_base);
        tiff.extend_from_slice(&0u32.to_le_bytes());
        write_ifd(&mut tiff, &mut gps_ifd, &mut data_area, data_base);
        tiff.extend_from_slice(&0u32.to_le_bytes());
        tiff.extend_from_slice(&data_area);

        let mut jpeg = vec![0xFF, 0xD8, 0xFF, 0xE1];
        let seg_len = (2 + 6 + tiff.len()) as u16;
        jpeg.extend_from_slice(&seg_len.to_be_bytes());
        jpeg.extend_from_slice(b"Exif\0\0");
        jpeg.extend_from_slice(&tiff);
        jpeg.extend_from_slice(&[0xFF, 0xD9]);

        let mut f = std::fs::File::create(path).expect("create fixture");
        f.write_all(&jpeg).expect("write fixture");
    }

    #[test]
    fn reads_full_metadata_from_exif_jpeg() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("photo.jpg");
        build_exif_jpeg(&path);

        let mut parser = MediaParser::new();
        let md = read(&mut parser, &path).expect("parse");

        assert_eq!(md.camera_make.as_deref(), Some("NIKON CORPORATION"));
        assert_eq!(md.camera_model.as_deref(), Some("NIKON D80"));
        assert_eq!(md.lens_make.as_deref(), Some("NIKON"));
        assert_eq!(md.lens_model.as_deref(), Some("18-135mm F3.5-5.6"));
        let taken = md.taken_at.expect("taken_at");
        assert_eq!(taken.date().to_string(), "2017-11-30");
        assert_eq!(taken.time().to_string(), "15:22:31");
        assert_eq!(
            md.taken_at_source,
            Some(TakenAtSource::ExifDateTimeOriginal)
        );
        let gps = md.gps.expect("gps");
        assert!((gps.latitude - 22.3193).abs() < 1e-6);
        assert!((gps.longitude - 114.1694).abs() < 1e-6);
    }

    #[test]
    fn garbage_file_is_error_not_panic() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("bad.jpg");
        std::fs::write(&path, b"not a jpeg at all").expect("write");
        let mut parser = MediaParser::new();
        assert!(read(&mut parser, &path).is_err());
    }
}
