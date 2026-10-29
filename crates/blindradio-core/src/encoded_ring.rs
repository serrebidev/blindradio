use std::collections::VecDeque;

use parking_lot::Mutex;

/// The stream's own encoded bytes, kept up to a byte budget with the
/// oldest dropped first — the DVBViewer timeshift model. The caller
/// stamps each chunk with the stream time (milliseconds since the
/// stream started) of its first byte, taken from the live decoder's
/// progress, so any position in the ring can later be replayed through
/// a fresh decoder or written out as the start of a recording.
///
/// Chunk starts are not codec-frame aligned; a decoder resynchronizes
/// within a chunk, and the replay pump discards audio up to the exact
/// target. Chunks arrive from one producer and their stamps never go
/// backwards; a stamp that would is clamped to the previous one.
pub struct EncodedRing {
	cap_bytes: usize,
	inner: Mutex<RingInner>,
}

struct RingInner {
	chunks: VecDeque<Chunk>,
	bytes: usize,
	next_seq: u64,
}

struct Chunk {
	seq: u64,
	start_ms: u64,
	data: Vec<u8>,
}

impl EncodedRing {
	pub fn new(cap_bytes: usize) -> EncodedRing {
		EncodedRing {
			cap_bytes: cap_bytes.max(1),
			inner: Mutex::new(RingInner {
				chunks: VecDeque::new(),
				bytes: 0,
				next_seq: 0,
			}),
		}
	}

	pub fn push(&self, start_ms: u64, data: &[u8]) {
		if data.is_empty() {
			return;
		}
		let mut inner = self.inner.lock();
		let start_ms = inner
			.chunks
			.back()
			.map_or(start_ms, |c| start_ms.max(c.start_ms));
		inner.bytes += data.len();
		let seq = inner.next_seq;
		inner.next_seq += 1;
		inner.chunks.push_back(Chunk {
			seq,
			start_ms,
			data: data.to_vec(),
		});
		// Drop the oldest whole chunks until the budget holds; the
		// newest chunk is always kept, even if it alone is over budget.
		while inner.bytes > self.cap_bytes && inner.chunks.len() > 1 {
			let gone = inner.chunks.pop_front().unwrap();
			inner.bytes -= gone.data.len();
		}
	}

	/// Stream time of the oldest byte still held; None while empty.
	pub fn start_ms(&self) -> Option<u64> {
		self.inner.lock().chunks.front().map(|c| c.start_ms)
	}

	/// Stream time of the newest chunk's first byte; None while empty.
	pub fn newest_ms(&self) -> Option<u64> {
		self.inner.lock().chunks.back().map(|c| c.start_ms)
	}

	pub fn len_bytes(&self) -> usize {
		self.inner.lock().bytes
	}

	pub fn is_empty(&self) -> bool {
		self.inner.lock().chunks.is_empty()
	}

	/// Where the chunk containing `time_ms` begins, as an offset into
	/// the ring's concatenated bytes, with that chunk's start time.
	/// The replay pump opens a decoder at this chunk and discards up to
	/// the exact target. None when `time_ms` predates the ring.
	pub fn chunk_at(&self, time_ms: u64) -> Option<(usize, u64)> {
		let inner = self.inner.lock();
		let mut offset = 0usize;
		let mut found = None;
		for c in &inner.chunks {
			if c.start_ms > time_ms {
				break;
			}
			found = Some((offset, c.start_ms));
			offset += c.data.len();
		}
		found
	}

	/// Bytes from the chunk containing `time_ms` to the live edge,
	/// with the sequence number of the last chunk served: a reader
	/// following the stream starts here, then continues with
	/// `bytes_after_seq`. None when `time_ms` predates the ring.
	pub fn serve_from(&self, time_ms: u64) -> Option<(Vec<u8>, u64)> {
		let inner = self.inner.lock();
		// The containing chunk is the last one starting at or before
		// the target; the newest chunk counts as containing any later
		// time, since it is still growing toward the live edge.
		let start = inner.chunks.iter().rposition(|c| c.start_ms <= time_ms)?;
		let mut out = Vec::new();
		let mut last_seq = 0;
		for c in inner.chunks.iter().skip(start) {
			out.extend_from_slice(&c.data);
			last_seq = c.seq;
		}
		Some((out, last_seq))
	}

	/// Bytes in chunks pushed after sequence `seq`, with the newest
	/// sequence served (unchanged when nothing new arrived).
	pub fn bytes_after_seq(&self, seq: u64) -> (Vec<u8>, u64) {
		let inner = self.inner.lock();
		let mut out = Vec::new();
		let mut last = seq;
		for c in &inner.chunks {
			if c.seq > seq {
				out.extend_from_slice(&c.data);
				last = c.seq;
			}
		}
		(out, last)
	}

	/// The sequence number of the chunk containing `time_ms`.
	pub fn seq_at(&self, time_ms: u64) -> Option<u64> {
		let inner = self.inner.lock();
		inner
			.chunks
			.iter()
			.rev()
			.find(|c| c.start_ms <= time_ms)
			.map(|c| c.seq)
	}

	/// Whole chunks after `seq` that have ENDED by `time_ms` — a chunk
	/// ends where the next one begins, so the newest chunk never
	/// qualifies. Returns the bytes and the last sequence included.
	/// Paces an Original recording to a playhead: finished chunks are
	/// written exactly once, in order.
	pub fn chunks_ended_after_seq(&self, seq: u64, time_ms: u64) -> (Vec<u8>, u64) {
		let inner = self.inner.lock();
		let mut out = Vec::new();
		let mut last = seq;
		let n = inner.chunks.len();
		for (i, c) in inner.chunks.iter().enumerate() {
			if c.seq <= seq || i + 1 >= n {
				continue;
			}
			if inner.chunks[i + 1].start_ms <= time_ms {
				out.extend_from_slice(&c.data);
				last = c.seq;
			} else {
				break;
			}
		}
		(out, last)
	}

	/// The encoded bytes covering `[from_ms, to_ms)`, whole chunks
	/// only: every chunk intersecting the range, so the chunk holding
	/// the end comes along whole. `to_ms` of `u64::MAX` runs to the
	/// live edge. Empty when the range predates the ring.
	pub fn span(&self, from_ms: u64, to_ms: u64) -> Vec<u8> {
		let inner = self.inner.lock();
		let mut out = Vec::new();
		let n = inner.chunks.len();
		for (i, c) in inner.chunks.iter().enumerate() {
			let end = if i + 1 < n {
				inner.chunks[i + 1].start_ms
			} else {
				u64::MAX
			};
			if end > from_ms && c.start_ms < to_ms {
				out.extend_from_slice(&c.data);
			}
		}
		out
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn ring(cap: usize) -> EncodedRing {
		EncodedRing::new(cap)
	}

	#[test]
	fn push_and_read_back() {
		let r = ring(1000);
		assert!(r.is_empty());
		r.push(0, &[1u8; 100]);
		r.push(1000, &[2u8; 100]);
		assert_eq!(r.start_ms(), Some(0));
		assert_eq!(r.newest_ms(), Some(1000));
		assert_eq!(r.len_bytes(), 200);
		assert_eq!(r.span(0, u64::MAX).len(), 200);
		// Whole chunks intersecting the range: both chunks touch
		// [500, 1500), so both come back.
		assert_eq!(r.span(500, 1500).len(), 200);
		assert_eq!(r.span(500, 1000), vec![1u8; 100]);
		assert_eq!(r.span(1000, u64::MAX), vec![2u8; 100]);
	}

	#[test]
	fn trim_drops_oldest_over_budget() {
		let r = ring(100);
		r.push(0, &[1u8; 40]);
		r.push(1000, &[2u8; 40]);
		r.push(2000, &[3u8; 40]);
		// 120 > 100, so the first chunk is gone.
		assert_eq!(r.start_ms(), Some(1000));
		assert_eq!(r.len_bytes(), 80);
		assert!(r.span(0, 500).is_empty());
		assert_eq!(r.chunk_at(500), None);
		assert_eq!(r.chunk_at(1500), Some((0, 1000)));
		assert_eq!(r.chunk_at(2500), Some((40, 2000)));
	}

	#[test]
	fn serve_from_and_follow_by_seq() {
		let r = ring(1000);
		r.push(0, &[1u8; 10]);
		r.push(1000, &[2u8; 10]);
		r.push(2000, &[3u8; 10]);
		let (bytes, seq) = r.serve_from(500).unwrap();
		assert_eq!(bytes.len(), 30);
		assert_eq!(seq, 2);
		let (more, seq) = r.bytes_after_seq(seq);
		assert!(more.is_empty());
		assert_eq!(seq, 2);
		r.push(3000, &[4u8; 10]);
		let (more, seq) = r.bytes_after_seq(seq);
		assert_eq!(more, vec![4u8; 10]);
		assert_eq!(seq, 3);
		// A time past the newest stamp serves the newest chunk; a time
		// before the ring's first chunk serves nothing.
		assert_eq!(r.serve_from(99_000).unwrap().0, vec![4u8; 10]);
		let r2 = ring(1000);
		r2.push(5000, &[1u8; 10]);
		assert_eq!(r2.serve_from(100), None);
	}

	#[test]
	fn chunks_end_only_when_the_next_begins() {
		let r = ring(1000);
		r.push(0, &[1u8; 10]);
		r.push(1000, &[2u8; 10]);
		r.push(2000, &[3u8; 10]);
		assert_eq!(r.seq_at(1500), Some(1));
		assert_eq!(r.seq_at(0), Some(0));
		// After chunk 0, only chunk 1 has ended by 2500 — chunk 2 is
		// the newest and never ends.
		let (bytes, last) = r.chunks_ended_after_seq(0, 2500);
		assert_eq!(bytes, vec![2u8; 10]);
		assert_eq!(last, 1);
		// Nothing new ends by 1500 on a second call.
		let (bytes, last) = r.chunks_ended_after_seq(last, 1500);
		assert!(bytes.is_empty());
		assert_eq!(last, 1);
	}

	#[test]
	fn newest_chunk_kept_even_when_oversized() {
		let r = ring(10);
		r.push(0, &[7u8; 50]);
		assert_eq!(r.len_bytes(), 50);
		assert_eq!(r.start_ms(), Some(0));
	}

	#[test]
	fn stamps_never_go_backwards() {
		let r = ring(1000);
		r.push(5000, &[1u8; 10]);
		r.push(4000, &[2u8; 10]);
		assert_eq!(r.newest_ms(), Some(5000));
		// Both chunks sit at 5000 after the clamp; the later one is the
		// replay start (less to discard).
		assert_eq!(r.chunk_at(5000), Some((10, 5000)));
	}
}
