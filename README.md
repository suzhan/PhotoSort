# ArchImages v3

A cross-platform desktop application for photo archiving, organizing, renaming, and deduplication. Built with Rust + Tauri 2 + Vue 3, replacing the legacy Python 3.6 + PyQt5 ArchImages v2.

**Design principle: data safety first, performance second.** Scanning and planning never write to disk. Source files are deleted only after all four guarantees pass: copy complete, target exists, size matches, hash matches. A full transaction journal enables crash recovery.

## Features

- **Recursive scanning** with a verified extension allowlist (nom-exif + rawler coverage)
- **EXIF metadata** via a dual-engine pipeline: `nom-exif` for standard images (JPEG/HEIC/HEIF/AVIF/TIFF/CR3/RAF/IIQ), `rawler` for camera RAW (NEF/CR2/ARW/DNG...), with an optional ExifTool runtime fallback
- **Template engine** for directory and filename rules (`{yyyy}/{camera_model}/{gps_city}`, `{yyyyMMdd}_{HHmmss}_{seq:4}`, etc.) with a concurrency-safe sequence coordinator
- **Read-only preview** before any file is touched — plan and execute share the exact same pipeline
- **Duplicate detection** in two modes: Modern (SHA-256) and LegacyStrict (MD5 + SHA1, single-pass streaming)
- **Safe file operations**: atomic copy (temp + fsync + rename), safe move (rename with cross-device fallback), copy-verify-delete with four-way verification before source removal
- **Background jobs** with bounded worker pools, progress events, and cooperative cancellation
- **SQLite persistence** for hash cache, GPS cache, and job journaling with crash recovery
- **Google Maps reverse geocoding** (optional) with OS-native credential storage (Keychain / Credential Manager), graceful degradation when no API key is configured
- **Cross-platform path safety** — sanitizes reserved names, invalid characters, path traversal, and length limits
- **Internationalization** — English and Simplified Chinese

## Tech Stack

| Layer | Technology |
|---|---|
| Frontend | Vue 3, TypeScript, Vite, Pinia, vue-i18n |
| Backend | Rust, Tauri 2, Tokio, rusqlite, nom-exif, rawler |
| Hashing | sha2, sha1, md-5 (RustCrypto, single-pass multi-hasher) |
| HTTP | reqwest (rustls-tls, no OpenSSL dependency) |
| Secrets | keyring (OS-native Keychain / Credential Manager) |

End users need **zero runtime dependencies** — no Python, ExifTool, Node, JVM, or .NET required after installation.

## Project Structure

```
archimages/
├── src/                      # Vue 3 frontend
│   ├── components/           # DirectoryPicker, RuleEditor, ScanResultTable, ...
│   ├── views/                # MainView (orchestrator)
│   ├── stores/               # Pinia: settings, scan, task, log
│   ├── types/                # TypeScript DTOs mirroring Rust models
│   ├── services/             # tauri.ts — centralized IPC
│   └── i18n/                 # en, zh-CN
└── src-tauri/
    └── src/
        ├── commands/         # Tauri IPC: scan, organize, settings, jobs, geocode
        ├── core/              # scanner, metadata, template, planner, hash,
        │                     # duplicate, file_ops, organizer, task_manager,
        │                     # geocode, api_key
        ├── db/               # schema, hash_cache, gps_cache, jobs
        ├── models/            # PhotoFile, PhotoMetadata, PhotoPlan, ...
        ├── config/            # JsonSettingsStore
        ├── error/             # AppError (thiserror, i18n keys)
        └── utils/             # path safety, logging
```

## Development

### Prerequisites

- [Node.js](https://nodejs.org/) 20+
- [Rust](https://www.rust-lang.org/) stable toolchain
- Platform Tauri prerequisites:
  - **macOS**: Xcode Command Line Tools
  - **Linux**: `libwebkit2gtk-4.1-dev libgtk-3-dev librsvg2-dev patchelf`
  - **Windows**: MSVC build tools

### Run

```bash
cd archimages
npm install
npm run tauri dev
```

### Quality Gates

Run before every commit:

```bash
# Frontend
npm run typecheck
npm run test
npm run build

# Backend
cd src-tauri
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

## Build & Distribute

```bash
cd archimages
npm run tauri build
```

Produces platform-native bundles:
- **Windows**: NSIS installer (`.exe`) and MSI (`.msi`)
- **macOS**: `.app` bundle and `.dmg` (Apple Silicon + Intel)

Current releases are **unsigned**. Code signing (Authenticode / Apple Developer ID) and notarization will be configured in a later release.

## CI

GitHub Actions (`.github/workflows/ci.yml`) runs on every push and pull request:
1. **Quality** (Ubuntu) — fmt, clippy, test, typecheck, frontend build
2. **Build Windows** (x64) — NSIS + MSI artifacts
3. **Build macOS** (Apple Silicon + Intel) — `.app` + `.dmg` artifacts

## Security Notes

- The Google Maps API key is stored in the OS-native credential store, never in `settings.json` or source code.
- All paths from the frontend are re-validated on the backend; generated targets are confirmed to stay within the destination root (no `../` escape).
- Hash failures never result in source deletion — when in doubt, the application keeps both files.

## License

All rights reserved.
