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
# HLS: 1 s segments; 4.99 s avoids a one-frame tail segment. live.m3u8 is ffmpeg's playlist (ends with ENDLIST).
# hls/master.m3u8 is hand-written: a lower-bandwidth video variant and the audio-only live.m3u8.
hls_tone="-f lavfi -i sine=frequency=440:sample_rate=44100:duration=4.99"
mkdir -p hls
$ff $hls_tone -ac 2 -c:a aac -b:a 64k -f hls -hls_time 1 -hls_list_size 0 -hls_segment_type mpegts -hls_segment_filename hls/seg%d.ts hls/live.m3u8
$ff $hls_tone -ac 2 -c:a aac -b:a 64k -f segment -segment_time 1 -segment_format adts hls/aac%d.aac
# AC-3 and E-AC-3: elementary streams, and 5.1 with the tone on the centre channel only, which a
# front-pair downmix would lose. Oxideav's decoder is checked against these, never shipped with them.
short="-f lavfi -i sine=frequency=440:sample_rate=48000:duration=3"
$ff $short -ac 2 -c:a ac3 -b:a 96k tone.ac3
$ff $short -ac 2 -c:a eac3 -b:a 96k tone.eac3
$ff $short -af "pan=5.1|c2=c0" -c:a ac3 -b:a 192k center.ac3
# TS: AC-3 under ATSC's stream type, AC-3 and E-AC-3 under DVB's descriptors, AAC in LATM.
$ff $short -ac 2 -c:a ac3 -b:a 96k -f mpegts ts_ac3_atsc.ts
$ff $short -ac 2 -c:a ac3 -b:a 96k -mpegts_flags system_b -f mpegts ts_ac3_dvb.ts
$ff $short -ac 2 -c:a eac3 -b:a 96k -mpegts_flags system_b -f mpegts ts_eac3_dvb.ts
$ff $short -ac 2 -c:a aac -b:a 64k -mpegts_flags latm -f mpegts ts_latm.ts
# HLS in fragmented MP4: 1 s AAC segments and their init segment.
$ff $short -ac 2 -c:a aac -b:a 64k -f hls -hls_time 1 -hls_list_size 0 -hls_segment_type fmp4 -hls_fmp4_init_filename init.mp4 -hls_segment_filename hls/f%d.m4s hls/fmp4.m3u8
# Two Opus songs with their tags, chained as Icecast sends them (a test joins them).
$ff $short -ac 2 -c:a libopus -b:a 48k -metadata artist="Lucid" -metadata title="End Of Time" song1.opus
$ff $short -ac 2 -c:a libopus -b:a 48k -metadata artist="Mara" -metadata title="Second Song" song2.opus
# TS whose SDT names its service, for Name Automatically.
$ff -f lavfi -i sine=frequency=440:sample_rate=48000:duration=1 -ac 2 -c:a aac -b:a 64k -metadata service_name="Test Channel" -metadata service_provider="Test" -f mpegts ts_named.ts
