#!/bin/sh
# Regenerates the decode fixtures: 440 Hz sine, 5 s unless noted. Needs ffmpeg with libmp3lame, libopus, libvorbis.
set -e
cd "$(dirname "$0")"
tone="-f lavfi -i sine=frequency=440:sample_rate=44100:duration=5"
ff="ffmpeg -hide_banner -loglevel error -y"
$ff $tone -ac 2 -c:a libmp3lame -b:a 64k tone.mp3
$ff $tone -ac 2 -c:a aac -b:a 64k -f adts tone.aac
$ff $tone -ac 2 -c:a libopus -b:a 48k tone.opus
$ff $tone -ac 2 -c:a libvorbis -q:a 0 tone.ogg
$ff $tone -ac 2 -sample_fmt s16 -c:a flac tone.flac
$ff -f lavfi -i sine=frequency=440:sample_rate=22050:duration=5 -ac 1 -c:a libmp3lame -b:a 32k mono22k.mp3
# 2 s, tone on front left/right only: six non-silent 5 s FLAC channels would not fit under 100 KB.
$ff -f lavfi -i sine=frequency=440:sample_rate=48000:duration=2 -af "pan=5.1|c0=c0|c1=c0" -sample_fmt s16 -compression_level 12 -c:a flac surround.flac
