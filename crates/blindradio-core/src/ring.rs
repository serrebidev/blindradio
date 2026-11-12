use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::{Duration, Instant};

use parking_lot::Mutex;

/// How long a push into a full ring waits before trying again.
const PUSH_WAIT: Duration = Duration::from_millis(5);

/// Audio a ring holds before it starts playing, after a cold start, a warm swap or running
/// dry. Some servers dribble their first seconds in lumps up to a second apart (amperwave);
/// burst servers reach it within a few ms of first audio, so it costs them almost nothing.
pub const CUSHION_MS: u32 = 1000;
/// The most a stream that keeps running dry must refill before it may sound again, by default.
pub const REFILL_LIMIT_MS: u32 = 15_000;
/// How many segments behind the newest a live HLS stream starts, by default.
pub const HLS_BEHIND: u32 = 2;

static START_NOW: AtomicU32 = AtomicU32::new(CUSHION_MS);
static REFILL_NOW: AtomicU32 = AtomicU32::new(REFILL_LIMIT_MS);
static HLS_BEHIND_NOW: AtomicU32 = AtomicU32::new(HLS_BEHIND);

/// The Buffer settings. Process-wide, as the streams and the mixer that read them are made in
/// many places; a stream already open keeps the ring it was made with.
pub fn set_buffers(start_ms: u32, refill_limit_ms: u32, hls_behind: u32) {
	START_NOW.store(start_ms, Ordering::Relaxed);
	REFILL_NOW.store(refill_limit_ms, Ordering::Relaxed);
	HLS_BEHIND_NOW.store(hls_behind, Ordering::Relaxed);
}

/// Audio a ring holds before it starts playing.
pub fn start_ms() -> u32 {
	START_NOW.load(Ordering::Relaxed)
}

pub fn refill_limit_ms() -> u32 {
	REFILL_NOW.load(Ordering::Relaxed)
}

pub fn hls_behind() -> usize {
	HLS_BEHIND_NOW.load(Ordering::Relaxed) as usize
}

/// Interleaved stereo f32 buffer; `rate` is frames per second.
pub struct PcmRing {
	rate: u32,
	capacity: usize,
	// ponytail: mutex shared with audio callback, held microseconds; lock-free ring if underruns show up
	buf: Mutex<VecDeque<f32>>,
	closed: AtomicBool,
	arrival: Mutex<Arrival>,
	/// The dip an earlier ring for this station learned; see `seed_dip`.
	seed: AtomicU32,
}

/// How audio has been arriving, measured at each push.
#[derive(Default)]
struct Arrival {
	/// When the last push ended.
	end: Option<Instant>,
	/// The longest wait between pushes, ms.
	max_gap: u32,
	/// First push of this connection; the clock audio is measured against.
	start: Option<Instant>,
	/// Time spent waiting for room, which is no delay in the stream.
	waited: Duration,
	/// Frames pushed since `start`.
	frames: u64,
	/// Most audio ever held ahead of real time, ms.
	peak: i64,
	/// Deepest fall below `peak`: what playback must hold in hand to ride out this server's
	/// slow stretches. Kept across reconnects.
	dip: u32,
}

impl PcmRing {
	pub fn new(rate: u32, seconds: f32) -> PcmRing {
		let rate = rate.max(1);
		let capacity = (rate as f32 * seconds) as usize * 2;
		PcmRing {
			rate,
			capacity,
			buf: Mutex::new(VecDeque::with_capacity(capacity)),
			closed: AtomicBool::new(false),
			arrival: Mutex::new(Arrival::default()),
			seed: AtomicU32::new(0),
		}
	}

	/// Whole frames only: a trailing odd sample is dropped so left and right never swap.
	/// A full ring makes the caller wait for room, so no audio is ever thrown away: streams
	/// that deliver in lumps (HLS segments, large Ogg pages, connect bursts) stay continuous,
	/// and a warm neighbour pauses its connection instead of skipping ahead. Once the ring is
	/// closed, what does not fit is discarded so the producer can finish.
	pub fn push(&self, stereo: &[f32]) {
		let now = Instant::now();
		let mut rest = &stereo[..stereo.len() & !1];
		{
			let mut a = self.arrival.lock();
			if let Some(end) = a.end {
				let gap = (now - end).as_millis().min(u128::from(u32::MAX)) as u32;
				a.max_gap = a.max_gap.max(gap);
			}
			// Audio ahead of real time, before and after this push. A server that sends a
			// burst and then dips below its bitrate (TopRadio) shows as a fall from the high.
			let start = *a.start.get_or_insert(now);
			let clock = (now - start).saturating_sub(a.waited).as_millis() as i64;
			let held = |frames: u64| (frames * 1000 / self.rate as u64) as i64 - clock;
			let fall = a.peak - held(a.frames);
			a.dip = a.dip.max(fall.clamp(0, i64::from(u32::MAX)) as u32);
			a.frames += rest.len() as u64 / 2;
			a.peak = a.peak.max(held(a.frames));
		}
		let mut waited = Duration::ZERO;
		loop {
			{
				let mut buf = self.buf.lock();
				let n = (self.capacity - buf.len()).min(rest.len());
				buf.extend(&rest[..n]);
				rest = &rest[n..];
			}
			if rest.is_empty() || self.closed.load(Ordering::Relaxed) {
				break;
			}
			let t = Instant::now();
			std::thread::sleep(PUSH_WAIT);
			waited += t.elapsed();
		}
		// After the wait for room: a full ring is no gap in the stream.
		let mut a = self.arrival.lock();
		a.end = Some(Instant::now());
		a.waited += waited;
	}

	/// A new connection feeds this ring: its connect burst and the outage before it are not
	/// the server's pace. What was learned about its slow stretches stays.
	pub fn new_connection(&self) {
		let mut a = self.arrival.lock();
		*a = Arrival {
			dip: a.dip,
			max_gap: a.max_gap,
			..Arrival::default()
		};
	}

	/// What an earlier ring for this station learned of its dips, so the first slow stretch
	/// is not met with only the connect burst in hand.
	pub fn seed_dip(&self, ms: u32) {
		self.seed.store(ms, Ordering::Relaxed);
	}

	/// The deepest the stream has fallen behind its own best lead, ms, or the seed if deeper;
	/// see `Arrival::dip`.
	pub fn dip_ms(&self) -> u32 {
		self.arrival.lock().dip.max(self.seed.load(Ordering::Relaxed))
	}

	/// What to remember of this server's dips. After a `long` listen that never fell as deep as
	/// the seed, an eighth less: one bad evening fades, a server that keeps dipping measures it
	/// back. A short visit, warm or played, has not seen enough to lower it.
	pub fn dip_to_keep(&self, long: bool) -> u32 {
		let seed = self.seed.load(Ordering::Relaxed);
		let seed = if long { seed - seed / 8 } else { seed };
		self.arrival.lock().dip.max(seed)
	}

	/// Audio to hold before this stream sounds: its dip and a quarter more, as the next slow
	/// stretch can run a little deeper than any so far. Below the start cushion for a server
	/// that sends evenly, so it costs those nothing.
	pub fn hold_ms(&self) -> u32 {
		let dip = self.dip_ms();
		dip.saturating_add(dip / 4)
	}

	/// No one will read this ring again: pushes stop waiting.
	pub fn close(&self) {
		self.closed.store(true, Ordering::Relaxed);
	}

	pub fn pop_into(&self, out: &mut [f32]) -> usize {
		let mut buf = self.buf.lock();
		let n = out.len().min(buf.len()) & !1;
		for (o, s) in out.iter_mut().zip(buf.drain(..n)) {
			*o = s;
		}
		n
	}

	pub fn len_ms(&self) -> u32 {
		(self.frames() * 1000 / self.rate as u64) as u32
	}

	pub fn frames(&self) -> u64 {
		self.buf.lock().len() as u64 / 2
	}

	/// Drops all but the newest `keep_ms`, or more if this stream arrives in lumps further apart
	/// than that (HLS segments) or has dipped below real time by more: trimming below either
	/// only buys a gap, and each gap makes the mixer demand a deeper refill.
	pub fn trim_to_ms(&self, keep_ms: u32) {
		let max_gap = self.arrival.lock().max_gap;
		let keep_ms = keep_ms.max(max_gap).max(self.hold_ms().min(refill_limit_ms()));
		let keep = (self.rate as u64 * keep_ms as u64 / 1000) as usize * 2;
		let mut buf = self.buf.lock();
		let excess = buf.len().saturating_sub(keep);
		buf.drain(..excess);
	}

	pub fn clear(&self) {
		self.buf.lock().clear();
	}

	#[cfg(test)]
	pub(crate) fn is_closed(&self) -> bool {
		self.closed.load(Ordering::Relaxed)
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn ramp(from: usize, frames: usize) -> Vec<f32> {
		(from..from + frames)
			.flat_map(|i| [i as f32, i as f32])
			.collect()
	}

	#[test]
	fn closed_ring_discards_what_does_not_fit() {
		let r = PcmRing::new(1000, 3.0);
		r.close();
		r.push(&ramp(0, 4000));
		assert_eq!(r.len_ms(), 3000);
		let mut out = [0.0; 2];
		r.pop_into(&mut out);
		assert_eq!(out, [0.0, 0.0], "the oldest audio is kept");
	}

	#[test]
	fn full_ring_waits_until_closed() {
		let r = std::sync::Arc::new(PcmRing::new(10, 1.0));
		let pusher = {
			let r = r.clone();
			std::thread::spawn(move || r.push(&ramp(0, 30)))
		};
		std::thread::sleep(Duration::from_millis(50));
		assert!(!pusher.is_finished(), "a warm ring pauses its producer");
		r.close();
		pusher.join().unwrap();
		assert_eq!(r.len_ms(), 1000);
	}

	#[test]
	fn trim_keeps_newest() {
		let r = PcmRing::new(1000, 3.0);
		r.push(&ramp(0, 1000));
		r.trim_to_ms(250);
		assert_eq!(r.len_ms(), 250);
		let mut out = [0.0; 2];
		r.pop_into(&mut out);
		assert_eq!(out[0], 750.0);
	}

	#[test]
	fn trim_keeps_enough_to_reach_the_next_lump() {
		let r = PcmRing::new(1000, 3.0);
		r.push(&ramp(0, 1000));
		std::thread::sleep(Duration::from_millis(400));
		r.push(&ramp(1000, 1000));
		r.trim_to_ms(100);
		let held = r.len_ms();
		assert!(held >= 400, "held {held} ms, against a 400 ms wait for audio");
		assert!(held < 1000, "held {held} ms: still trimmed");
	}

	#[test]
	fn dip_is_the_fall_behind_real_time() {
		let r = PcmRing::new(1000, 5.0);
		// A 2 s burst, then 100 ms of audio every 200 ms: half speed for a second.
		r.push(&ramp(0, 2000));
		for k in 0..5 {
			std::thread::sleep(Duration::from_millis(200));
			r.push(&ramp(2000 + k * 100, 100));
		}
		let dip = r.dip_ms();
		assert!((500..900).contains(&dip), "dip {dip} ms, against 600");
		r.trim_to_ms(100);
		assert!(r.len_ms() >= 500, "a trim keeps the dip in hand");
	}

	#[test]
	fn a_seed_fades_only_after_a_long_listen() {
		let r = PcmRing::new(1000, 5.0);
		r.seed_dip(8000);
		assert!(r.hold_ms() >= 8000, "the seed is held from the start");
		assert_eq!(r.dip_to_keep(false), 8000);
		assert_eq!(r.dip_to_keep(true), 7000);
	}

	#[test]
	fn waiting_for_room_is_no_dip() {
		let r = std::sync::Arc::new(PcmRing::new(100, 1.0));
		r.push(&ramp(0, 100));
		let pusher = {
			let r = r.clone();
			std::thread::spawn(move || r.push(&ramp(100, 50)))
		};
		std::thread::sleep(Duration::from_millis(500));
		let mut out = vec![0.0; 200];
		r.pop_into(&mut out);
		pusher.join().unwrap();
		r.push(&ramp(150, 1));
		assert!(r.dip_ms() < 200, "dip {} ms", r.dip_ms());
	}

	#[test]
	fn a_new_connection_keeps_the_dip_not_the_outage() {
		let r = PcmRing::new(1000, 5.0);
		r.push(&ramp(0, 1000));
		std::thread::sleep(Duration::from_millis(300));
		r.push(&ramp(1000, 1));
		let dip = r.dip_ms();
		assert!(dip >= 300, "dip {dip} ms");
		r.new_connection();
		std::thread::sleep(Duration::from_millis(600));
		r.push(&ramp(1001, 10));
		r.push(&ramp(1011, 10));
		assert!(r.dip_ms() < 600, "the outage is not the server's pace");
		assert!(r.dip_ms() >= dip, "the dip stays");
	}

	#[test]
	fn pop_partial_returns_count() {
		let r = PcmRing::new(1000, 1.0);
		r.push(&[1.0, 2.0, 3.0, 4.0]);
		let mut out = [9.0; 6];
		assert_eq!(r.pop_into(&mut out), 4);
		assert_eq!(out, [1.0, 2.0, 3.0, 4.0, 9.0, 9.0]);
		r.clear();
		assert_eq!(r.pop_into(&mut out), 0);
	}

	#[test]
	fn len_ms_counts_stereo_frames() {
		let r = PcmRing::new(48000, 3.0);
		// 24000 frames of stereo is half a second at 48 kHz.
		r.push(&vec![0.0; 24000 * 2]);
		assert_eq!(r.len_ms(), 500);
	}

	#[test]
	fn playing_ring_waits_instead_of_dropping() {
		let r = std::sync::Arc::new(PcmRing::new(10, 1.0));
		let mut out = [0.0; 2];
		let pusher = {
			let r = r.clone();
			std::thread::spawn(move || r.push(&ramp(0, 30)))
		};
		let mut got = Vec::new();
		while got.len() < 60 {
			let n = r.pop_into(&mut out);
			got.extend_from_slice(&out[..n]);
			std::thread::sleep(Duration::from_millis(1));
		}
		pusher.join().unwrap();
		assert_eq!(got, ramp(0, 30), "every frame arrives, in order");
	}

	#[test]
	fn odd_lengths_keep_whole_frames() {
		let r = PcmRing::new(1000, 1.0);
		r.push(&[1.0, 2.0, 3.0]);
		r.push(&[4.0, 5.0]);
		let mut out = [0.0; 3];
		assert_eq!(r.pop_into(&mut out), 2);
		assert_eq!(out[..2], [1.0, 2.0]);
		assert_eq!(r.pop_into(&mut out), 2);
		assert_eq!(out[..2], [4.0, 5.0], "left stays left");
		assert_eq!(PcmRing::new(0, 1.0).len_ms(), 0);
	}
}
