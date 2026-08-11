//! Integration tests using the imazen/codec-corpus dataset.
//!
//! These tests validate PhotoSort's scanner and metadata pipeline against
//! real-world codec conformance files — HEIC/HEIF/AVIF from libheif, Nokia,
//! and dsoprea-exif; JPEG from real cameras (Canon, Nikon, Olympus, Sony);
//! and TIFF conformance files including BigTIFF and edge cases.
//!
//! Test data is downloaded separately (not committed to git).
//! If the corpus is absent, all tests are skipped gracefully.
//!
//! Download:
//!   See tests/download_codec_corpus.sh

use std::path::PathBuf;

use archimages_lib::core::metadata::{MetadataOptions, MetadataReader};
use archimages_lib::core::scanner::{self, ScanOptions};
use archimages_lib::models::photo::PhotoFile;

/// Root of the codec-corpus test data.
/// CARGO_MANIFEST_DIR = archimages/src-tauri → up two to ArchiveImages → tests/codec-corpus
fn corpus_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("tests")
        .join("codec-corpus")
}

fn corpus_available() -> bool {
    let root = corpus_root();
    root.is_dir() && root.join("heic-conformance").is_dir()
}

/// Collect all supported image files under a directory via the real scanner.
fn collect_scanned(root: &std::path::Path) -> Vec<PhotoFile> {
    let mut photos = Vec::new();
    let options = ScanOptions {
        include_subfolders: true,
    };
    scanner::scan(root, &options, &|| false, |p| {
        photos.push(p);
    })
    .expect("scan");
    photos
}

fn make_photo(path: &std::path::Path) -> PhotoFile {
    let extension = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    PhotoFile {
        path: path.to_path_buf(),
        size: std::fs::metadata(path).unwrap().len(),
        extension,
        modified_time: std::time::SystemTime::UNIX_EPOCH,
    }
}

// ─────────────────────────────────────────────────────────
// Scanner: all codec-corpus files with supported extensions
// are collected
// ─────────────────────────────────────────────────────────

#[test]
fn scanner_collects_heic_conformance_files() {
    if !corpus_available() {
        eprintln!("skipped: codec-corpus not downloaded");
        return;
    }
    let root = corpus_root().join("heic-conformance");
    let photos = collect_scanned(&root);
    assert!(
        photos.len() >= 15,
        "expected at least 15 HEIC/AVIF files, got {}",
        photos.len()
    );
}

#[test]
fn scanner_collects_jpeg_conformance_files() {
    if !corpus_available() {
        eprintln!("skipped: codec-corpus not downloaded");
        return;
    }
    let root = corpus_root().join("jpeg-conformance");
    let photos = collect_scanned(&root);
    assert!(
        photos.len() >= 5,
        "expected at least 5 JPEG files, got {}",
        photos.len()
    );
}

#[test]
fn scanner_collects_tiff_conformance_files() {
    if !corpus_available() {
        eprintln!("skipped: codec-corpus not downloaded");
        return;
    }
    let root = corpus_root().join("tiff-conformance");
    let photos = collect_scanned(&root);
    assert!(
        photos.len() >= 5,
        "expected at least 5 TIFF files, got {}",
        photos.len()
    );
}

// ─────────────────────────────────────────────────────────
// Metadata: real camera JPEGs should yield EXIF data
// ─────────────────────────────────────────────────────────

#[test]
fn real_camera_jpeg_yields_metadata() {
    if !corpus_available() {
        eprintln!("skipped: codec-corpus not downloaded");
        return;
    }
    let mut reader = MetadataReader::new(MetadataOptions::default());
    let mut any_success = false;
    for name in ["Canon_40D.jpg", "Nikon_D70.jpg", "Olympus_C8080WZ.jpg"] {
        let path = corpus_root().join("jpeg-conformance").join(name);
        if !path.exists() {
            eprintln!("skipped: {name} not found");
            continue;
        }
        let photo = make_photo(&path);
        let outcome = reader.read(&photo);
        eprintln!(
            "{name}: parse_failed={}, taken_at={:?}, camera={:?}",
            outcome.parse_failed, outcome.metadata.taken_at, outcome.metadata.camera_model
        );
        // Real camera JPEGs should produce metadata (not parse_failed).
        assert!(!outcome.parse_failed, "{name} should not fail parsing");
        if outcome.metadata.taken_at.is_some() || outcome.metadata.camera_model.is_some() {
            any_success = true;
        }
    }
    assert!(
        any_success,
        "at least one camera JPEG should yield EXIF data"
    );
}

// ─────────────────────────────────────────────────────────
// Metadata: dsoprea HEIC files (with EXIF) should yield data
// ─────────────────────────────────────────────────────────

#[test]
fn dsoprea_heic_yields_metadata_or_fails_gracefully() {
    if !corpus_available() {
        eprintln!("skipped: codec-corpus not downloaded");
        return;
    }
    let mut reader = MetadataReader::new(MetadataOptions::default());
    for n in 1..=4 {
        let name = format!("image{n}.heic");
        let path = corpus_root()
            .join("heic-conformance")
            .join("valid")
            .join("dsoprea-exif")
            .join(&name);
        if !path.exists() {
            eprintln!("skipped: {name} not found");
            continue;
        }
        let photo = make_photo(&path);
        let outcome = reader.read(&photo);
        eprintln!(
            "{name}: parse_failed={}, taken_at={:?}, camera={:?}",
            outcome.parse_failed, outcome.metadata.taken_at, outcome.metadata.camera_model
        );
        // Key invariant: no panic. parse_failed=true is acceptable for edge cases,
        // but dsoprea files should ideally parse successfully.
    }
}

// ─────────────────────────────────────────────────────────
// Metadata: Nokia conformance HEIC files don't panic
// ─────────────────────────────────────────────────────────

#[test]
fn nokia_heic_no_panic() {
    if !corpus_available() {
        eprintln!("skipped: codec-corpus not downloaded");
        return;
    }
    let mut reader = MetadataReader::new(MetadataOptions::default());
    for n in 1..=5 {
        let name = format!("C{n:03}.heic");
        let path = corpus_root()
            .join("heic-conformance")
            .join("valid")
            .join("nokia-conformance")
            .join(&name);
        if !path.exists() {
            continue;
        }
        let photo = make_photo(&path);
        let outcome = reader.read(&photo);
        eprintln!(
            "{name}: parse_failed={}, engine={:?}",
            outcome.parse_failed, outcome.engine
        );
    }
}

// ─────────────────────────────────────────────────────────
// Metadata: edge-case HEIC/AVIF files don't panic
// ─────────────────────────────────────────────────────────

#[test]
fn edge_case_heic_avif_no_panic() {
    if !corpus_available() {
        eprintln!("skipped: codec-corpus not downloaded");
        return;
    }
    let mut reader = MetadataReader::new(MetadataOptions::default());
    let edge_dir = corpus_root().join("heic-conformance").join("edge-cases");
    if !edge_dir.is_dir() {
        eprintln!("skipped: edge-cases dir not found");
        return;
    }
    let mut tested = 0;
    for entry in std::fs::read_dir(&edge_dir).unwrap().flatten() {
        let path = entry.path();
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if !matches!(ext.as_str(), "heic" | "heif" | "avif") {
            continue;
        }
        let photo = make_photo(&path);
        let outcome = reader.read(&photo);
        eprintln!(
            "{}: parse_failed={}, engine={:?}",
            path.file_name().unwrap().to_string_lossy(),
            outcome.parse_failed,
            outcome.engine
        );
        tested += 1;
    }
    assert!(tested > 0, "expected at least 1 edge-case file");
}

// ─────────────────────────────────────────────────────────
// Metadata: TIFF conformance files don't panic
// ─────────────────────────────────────────────────────────

#[test]
fn tiff_conformance_no_panic() {
    if !corpus_available() {
        eprintln!("skipped: codec-corpus not downloaded");
        return;
    }
    let mut reader = MetadataReader::new(MetadataOptions::default());
    let tiff_dir = corpus_root().join("tiff-conformance");
    if !tiff_dir.is_dir() {
        eprintln!("skipped: tiff-conformance dir not found");
        return;
    }
    let mut tested = 0;
    for entry in std::fs::read_dir(&tiff_dir).unwrap().flatten() {
        let path = entry.path();
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if !matches!(ext.as_str(), "tif" | "tiff") {
            continue;
        }
        let photo = make_photo(&path);
        let outcome = reader.read(&photo);
        eprintln!(
            "{}: parse_failed={}, engine={:?}",
            path.file_name().unwrap().to_string_lossy(),
            outcome.parse_failed,
            outcome.engine
        );
        tested += 1;
    }
    assert!(
        tested >= 5,
        "expected at least 5 TIFF files, tested {tested}"
    );
}

// ─────────────────────────────────────────────────────────
// Full pipeline: scan entire corpus → metadata on all → no panics
// ─────────────────────────────────────────────────────────

#[test]
fn full_corpus_scan_and_metadata_no_panic() {
    if !corpus_available() {
        eprintln!("skipped: codec-corpus not downloaded");
        return;
    }
    let root = corpus_root();
    let photos = collect_scanned(&root);
    assert!(
        photos.len() >= 25,
        "expected 25+ files, got {}",
        photos.len()
    );

    let mut reader = MetadataReader::new(MetadataOptions::default());
    let mut ok_count = 0;
    let mut fail_count = 0;
    for photo in &photos {
        let outcome = reader.read(photo);
        if outcome.parse_failed {
            fail_count += 1;
        } else {
            ok_count += 1;
        }
    }
    eprintln!(
        "full corpus: {} files scanned, {} parsed ok, {} parse failed (no panics)",
        photos.len(),
        ok_count,
        fail_count
    );
    // At least some files should parse (real camera JPEGs).
    assert!(
        ok_count > 0,
        "expected at least 1 successful metadata parse"
    );
}
