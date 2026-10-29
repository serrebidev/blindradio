# Encoded ring buffer (DVBViewer-style timeshift)

Status: in progress on branch `timeshift-size-limit`. Opt-in feature,
off by default; Brandon's call, 2026-10-29: go straight to the encoded
ring, do not ship the decoded-audio size cap, and make it optional.

## Why

The standard timeshift buffer holds decoded PCM at about 11 MB a
minute, so its depth is set in minutes (30 by default, 240 max).
DVBViewer instead rings the stream's own encoded bytes: a 200 MB
budget covers roughly 3.5 hours at 128 kbps, and recording can start
anywhere inside the buffer. This adds that model as an alternative.

## Settings

`TimeshiftSettings.encoded_ring` (default false) turns it on;
`max_mb` is the ring budget (0 = the 200 MB default). With the ring
off, everything is exactly as before. Both live in the Settings
window next to Buffer Minutes.

## Pieces

- `encoded_ring::EncodedRing` (done, unit-tested): byte-budget ring of
  encoded chunks, each stamped by the caller with the stream time of
  its first byte; whole-chunk trim from the front; `span(from, to)`
  for recording, `chunk_at(time)` for replay.
- Tee and stamps (done): the byte pump pushes every clean encoded
  chunk into the ring through `stream::RingTap`. Stamps are the
  station session's elapsed time when the chunk arrives — chunks are
  network-paced, so the axis is stream time within the first burst's
  skew, and an outage honestly stretches it. One tap serves the whole
  session across reconnects.
- Replay feed (done, `ringfeed::RingFeed`): a second `Decoder` over
  ring bytes, resampled into a 180-second `ShiftBuffer` window whose
  frame 0 maps to a session time (`WindowMap`), so the mixer,
  `ShiftStatus`, seek steps, pause and the status bar are unchanged.
  The feed throttles so the window never slides out from under the
  mixer's cursor. The tuner intercepts seek/pause/live in ring mode:
  a seek or resume landing before the window, or going live,
  re-primes the window at the target and re-attaches.  Reaching the
  window's edge hands back to the live path, as before.
- Format gate (done): replay starts mid-ring, so only
  self-synchronizing codecs use the ring — MPEG audio (MP1/MP2/MP3)
  and ADTS AAC.
  A cold stream starts before its codec is known, so the ring fills
  from the first chunk while the decoded buffer plays; the format
  event picks the ring, or leaves the decoded buffer if the codec
  needs its opening headers. A chained stream that switches codec
  re-picks on the format event.
- Recording from the buffer (done): recording while time-shifted
  starts at the playhead. WAV/MP3/FLAC take the mixer's own shifted
  frames — what the listener hears, silence while paused — through
  `MixerCmd::RecordSlot`; Original appends finished ring chunks as
  the cursor passes them, paced by the feed
  (`chunks_ended_after_seq`). The stream-side taps yield while
  `ShiftStatus` shows a shifted playhead, and the `Recorder` itself
  ignores the wrong write kind, so the paths never mix. Seams at the
  shifted/live boundary can repeat or drop a fraction of a second.

## Risks

- Codec resync: MP3 and ADTS AAC frames resync cleanly; a few frames
  can be lost at a seek because the seek target lies inside a chunk.
  Ogg, FLAC and MP4 streams need their opening headers and never use
  the ring; they keep the decoded-minutes buffer.
- HLS: chunks are whole segments there; stamps still apply.
- The app crate cannot be compile-tested on the dev VM; the Windows
  CI gate (full suite) is the verifier, as with every BlindRadio
  release, plus Brandon's ears on a real stream before it ships.
