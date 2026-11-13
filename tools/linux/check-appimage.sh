#!/bin/bash
# Check the packaged payload, not the libraries installed on the build runner.
set -euo pipefail
image=$(readlink -f "$1")
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
cd "$work"
"$image" --appimage-extract >/dev/null
libs="$work/squashfs-root/usr/bin"
for name in libtiff.so.5 libjpeg.so.8 libwebp.so.7 libzstd.so.1 liblzma.so.5 libjbig.so.0 libdeflate.so.0 libz.so.1 libnotify.so.4; do
	test -f "$libs/$name" || { echo "Missing bundled library: $name" >&2; exit 1; }
done
highest=$(objdump -T "$libs/blindradio" "$libs/"*.so.* | grep -oE 'GLIBC_[0-9]+\.[0-9]+' | sort -Vu | tail -1)
test "$(printf '%s\n' GLIBC_2.35 "$highest" | sort -V | tail -1)" = GLIBC_2.35 || {
	echo "AppImage requires $highest; maximum is GLIBC_2.35" >&2
	exit 1
}
echo "AppImage bundles TIFF, codecs and libnotify; maximum required glibc: $highest"
