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
- Tee and stamps (next): the byte pipe feeding the live `Decoder`
  also pushes into the ring. Stamps come from the live decoder's
  progress — bytes consumed so far against frames decoded so far,
  checkpointed per chunk. Accuracy within a chunk is fine: replay
  resynchronizes and discards to the exact frame.
- Replay pump (next): a second `Decoder` over ring bytes starting at
  `chunk_at(cursor)`, decoding forward into the station's existing
  `ShiftBuffer` window, so the mixer, `ShiftStatus`, seek steps,
  pause and the status bar are unchanged. On a seek, the pump restarts
  at the new position and the window re-primes (a short rebuffer, as
  in DVBViewer). Reaching the live edge hands back to the live path.
- Recording from the buffer (next): Original format writes
  `span(start, end)` bytes directly; WAV/MP3/FLAC decode that span
  with a throwaway `Decoder` and encode as today.

## Risks

- Codec resync: MP3/AAC frames resync cleanly; chained Ogg and some
  AAC-in-LATM streams may lose a few frames at a seek. Acceptable,
  noted in the changelog when it ships.
- HLS: chunks are whole segments there; stamps still apply.
- The app crate cannot be compile-tested on the dev VM; the Windows
  CI gate (full suite) is the verifier, as with every BlindRadio
  release, plus Brandon's ears on a real stream before it ships.
