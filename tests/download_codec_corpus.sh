#!/usr/bin/env bash
# Download a representative sample from imazen/codec-corpus for integration testing.
# Does not clone the full ~670MB repo; uses GitHub raw URLs for selected files.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
CORPUS="$SCRIPT_DIR/codec-corpus"

if [ -d "$CORPUS/heic-conformance" ] && [ "$(find "$CORPUS" -type f \( -name '*.heic' -o -name '*.heif' -o -name '*.avif' \) | wc -l)" -gt 10 ]; then
  echo "codec-corpus already downloaded at $CORPUS"
  exit 0
fi

mkdir -p "$CORPUS"/{heic-conformance/{valid/{dsoprea-exif,nokia-conformance,libheif-testdata},edge-cases},jpeg-conformance,tiff-conformance}

BASE="https://raw.githubusercontent.com/imazen/codec-corpus/master"

echo "Downloading HEIC conformance files..."
for f in image1.heic image2.heic image3.heic image4.heic; do
  curl -sL "$BASE/heic-conformance/valid/dsoprea-exif/$f" -o "$CORPUS/heic-conformance/valid/dsoprea-exif/$f" &
done
for f in C001.heic C002.heic C003.heic C004.heic C005.heic; do
  curl -sL "$BASE/heic-conformance/valid/nokia-conformance/$f" -o "$CORPUS/heic-conformance/valid/nokia-conformance/$f" &
done
for f in example.heic example.avif lightning_mini.heif; do
  curl -sL "$BASE/heic-conformance/valid/libheif-testdata/$f" -o "$CORPUS/heic-conformance/valid/libheif-testdata/$f" &
done
for f in double_ftyp.heic heix_brand.heic minimal_ftyp_only.heic avif_brand.heif many_brands.heic; do
  curl -sL "$BASE/heic-conformance/edge-cases/$f" -o "$CORPUS/heic-conformance/edge-cases/$f" &
done

echo "Downloading JPEG conformance files..."
for f in Canon_40D.jpg Nikon_D70.jpg Olympus_C8080WZ.jpg Sony_HDR-HC3.jpg blank_800x280.jpg; do
  curl -sL "$BASE/jpeg-conformance/valid/$f" -o "$CORPUS/jpeg-conformance/$f" &
done

echo "Downloading TIFF conformance files..."
TIFF_FILES=$(curl -sL "https://api.github.com/repos/imazen/codec-corpus/contents/tiff-conformance/valid" | python3 -c "import sys,json; [print(f['name']) for f in json.load(sys.stdin)]" 2>/dev/null || true)
for f in $TIFF_FILES; do
  curl -sL "$BASE/tiff-conformance/valid/$f" -o "$CORPUS/tiff-conformance/$f" &
done

wait
echo "=== Download complete ==="
find "$CORPUS" -type f \( -name '*.heic' -o -name '*.heif' -o -name '*.avif' -o -name '*.jpg' -o -name '*.tif' -o -name '*.tiff' \) | wc -l
echo "files downloaded to $CORPUS"
