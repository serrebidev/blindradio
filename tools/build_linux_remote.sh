#!/bin/sh
# Builds BlindRadio's Linux assets on serrebiradio.com in the ubuntu container and copies them
# back into target/dist. Needs ssh to root@serrebiradio.com and docker there.
set -eu

host="${BLINDRADIO_LINUX_HOST:-root@serrebiradio.com}"
# The home directory, not /tmp: that is a tmpfs there and a wx build fills it.
remote="$HOME/blindradio-build"
here=$(cd "$(dirname "$0")/.." && pwd)
dist="$here/target/dist"

mkdir -p "$dist"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

# The committed tree, not the working copy.
git -C "$here" archive --format=tar.gz --prefix=blindradio/ HEAD > "$tmp/src.tar.gz"

echo "copying the source to $host:$remote"
ssh "$host" "rm -rf $remote && mkdir -p $remote/src"
scp -q "$tmp/src.tar.gz" "$host:$remote/src.tar.gz"
ssh "$host" "tar -xzf $remote/src.tar.gz -C $remote/src"

echo "building in the container"
ssh "$host" "docker build -t blindradio-build $remote/src/blindradio/tools/linux \
	&& docker run --rm -v $remote/src/blindradio:/work -v $remote/src/blindradio/target:/work/target \
		blindradio-build sh -c 'cargo build --release -p blindradio && cargo xtask dist-linux'"

echo "checking the AppImage starts"
ssh "$host" "cd $remote/src/blindradio && timeout 15 xvfb-run -a ./target/dist/BlindRadio.AppImage; \
	code=\$?; if [ \$code -ne 124 ]; then echo \"AppImage exited with \$code\"; exit 1; fi"

echo "copying the assets back"
scp -q "$host:$remote/src/blindradio/target/dist/BlindRadio.AppImage" "$dist/"
scp -q "$host:$remote/src/blindradio/target/dist/BlindRadio.tar.gz" "$dist/"
echo "Linux assets are in $dist"
