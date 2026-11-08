#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"

echo "=== Building Rnote AppImage ==="

BIN_PATH="${ROOT_DIR}/target/release/rnote"
if [ ! -f "${BIN_PATH}" ]; then
    echo "Release binary not found at ${BIN_PATH}, falling back to debug binary..."
    BIN_PATH="${ROOT_DIR}/target/debug/rnote"
fi

if [ ! -f "${BIN_PATH}" ]; then
    echo "Error: Neither target/release/rnote nor target/debug/rnote found!" >&2
    exit 1
fi

APPDIR="/tmp/rnote-appdir"
rm -rf "${APPDIR}"
mkdir -p "${APPDIR}/usr/bin"
mkdir -p "${APPDIR}/usr/share/applications"
mkdir -p "${APPDIR}/usr/share/icons/hicolor/scalable/apps"
mkdir -p "${APPDIR}/usr/share/glib-2.0/schemas"

# 1. Install binary
echo "Installing binary: ${BIN_PATH} -> ${APPDIR}/usr/bin/rnote"
cp "${BIN_PATH}" "${APPDIR}/usr/bin/rnote"
strip "${APPDIR}/usr/bin/rnote" 2>/dev/null || true
chmod +x "${APPDIR}/usr/bin/rnote"

# 2. Desktop file
cat > "${APPDIR}/com.github.flxzt.rnote.Devel.desktop" << 'EOF'
[Desktop Entry]
Name=Rnote
Comment=Sketch and take handwritten notes
Icon=com.github.flxzt.rnote.Devel
Exec=rnote %f
Terminal=false
MimeType=application/rnote;image/svg+xml;image/png;image/jpeg;application/pdf;application/x-xopp;
Type=Application
Categories=GNOME;GTK;Graphics;
Keywords=Gnome;GTK;Office;Graphics;Notes;
StartupNotify=true
Actions=new-window;

[Desktop Action new-window]
Name=New Window
Exec=rnote %f
EOF

cp "${APPDIR}/com.github.flxzt.rnote.Devel.desktop" "${APPDIR}/rnote.desktop"
cp "${APPDIR}/com.github.flxzt.rnote.Devel.desktop" "${APPDIR}/usr/share/applications/com.github.flxzt.rnote.Devel.desktop"

# 3. Icons
ICON_SRC="${ROOT_DIR}/crates/rnote-ui/data/icons/scalable/apps/rnote-devel.svg"
if [ ! -f "${ICON_SRC}" ]; then
    ICON_SRC="${ROOT_DIR}/crates/rnote-ui/data/icons/scalable/apps/rnote.svg"
fi

cp "${ICON_SRC}" "${APPDIR}/com.github.flxzt.rnote.Devel.svg"
cp "${ICON_SRC}" "${APPDIR}/rnote.svg"
cp "${ICON_SRC}" "${APPDIR}/usr/share/icons/hicolor/scalable/apps/com.github.flxzt.rnote.Devel.svg"
cp "${ICON_SRC}" "${APPDIR}/usr/share/icons/hicolor/scalable/apps/com.github.flxzt.rnote.svg"

# Generate PNG .DirIcon and app icon
python3 -c "
import gi
gi.require_version('Rsvg', '2.0')
from gi.repository import Rsvg
import cairo

handle = Rsvg.Handle.new_from_file('${ICON_SRC}')
surface = cairo.ImageSurface(cairo.FORMAT_ARGB32, 256, 256)
ctx = cairo.Context(surface)
rect = Rsvg.Rectangle()
rect.x = 0; rect.y = 0; rect.width = 256; rect.height = 256
handle.render_document(ctx, rect)
surface.write_to_png('${APPDIR}/.DirIcon')
surface.write_to_png('${APPDIR}/com.github.flxzt.rnote.Devel.png')
surface.write_to_png('${APPDIR}/rnote.png')
"

# 4. GSettings schemas
sed -e 's|@APP_ID@|com.github.flxzt.rnote.Devel|g' \
    -e 's|@APP_IDPATH@|/com/github/flxzt/rnote/|g' \
    -e 's|@APP_NAME@|rnote|g' \
    "${ROOT_DIR}/crates/rnote-ui/data/app.gschema.xml.in" > "${APPDIR}/usr/share/glib-2.0/schemas/com.github.flxzt.rnote.Devel.gschema.xml"

sed -e 's|@APP_ID@|com.github.flxzt.rnote|g' \
    -e 's|@APP_IDPATH@|/com/github/flxzt/rnote/|g' \
    -e 's|@APP_NAME@|rnote|g' \
    "${ROOT_DIR}/crates/rnote-ui/data/app.gschema.xml.in" > "${APPDIR}/usr/share/glib-2.0/schemas/com.github.flxzt.rnote.gschema.xml"

glib-compile-schemas "${APPDIR}/usr/share/glib-2.0/schemas"

# 5. AppRun script
cat > "${APPDIR}/AppRun" << 'EOF'
#!/bin/sh
SELF=$(readlink -f "$0")
HERE=${SELF%/*}

export APPDIR="${HERE}"
export PATH="${HERE}/usr/bin:${PATH}"
export LD_LIBRARY_PATH="${HERE}/usr/lib:${HERE}/usr/lib64:${LD_LIBRARY_PATH}"
export XDG_DATA_DIRS="${HERE}/usr/share:${XDG_DATA_DIRS:-/usr/local/share:/usr/share}"
export GSETTINGS_SCHEMA_DIR="${HERE}/usr/share/glib-2.0/schemas:${GSETTINGS_SCHEMA_DIR}"

exec "${HERE}/usr/bin/rnote" "$@"
EOF
chmod +x "${APPDIR}/AppRun"

# 6. Build AppImage using appimagetool
APPIMAGETOOL="/tmp/appimagetool"
if [ ! -f "${APPIMAGETOOL}" ]; then
    echo "Downloading appimagetool..."
    curl -sL https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-x86_64.AppImage -o "${APPIMAGETOOL}"
    chmod +x "${APPIMAGETOOL}"
fi

OUTPUT_APPIMAGE="${ROOT_DIR}/Rnote-x86_64.AppImage"
echo "Packaging into ${OUTPUT_APPIMAGE}..."
ARCH=x86_64 "${APPIMAGETOOL}" --appimage-extract-and-run "${APPDIR}" "${OUTPUT_APPIMAGE}"

echo "=== Successfully built: ${OUTPUT_APPIMAGE} ==="
ls -lh "${OUTPUT_APPIMAGE}"
