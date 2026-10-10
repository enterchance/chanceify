//! A tempo detector that listens to what is playing.
//!
//! Deezer knows the tempo of only a fraction of Spotify's catalogue, so most
//! rows in the BPM column stayed empty. This works it out from the audio
//! itself while a song plays: the signal is reduced to a loudness envelope
//! at about 200 values a second, the envelope's rises are the onsets, and
//! the lag at which the onsets best repeat is the beat.

/// Envelope values per second.
const RATE: f64 = 200.0;
/// Seconds of onsets kept for the analysis.
const KEEP_SECONDS: usize = 24;
/// Seconds of audio needed before an answer is given.
const NEEDED_SECONDS: usize = 12;
/// Tempo range searched, before octave folding.
const MIN_BPM: f64 = 70.0;
const MAX_BPM: f64 = 180.0;

pub struct BeatDetector {
    sample_rate: f64,
    hop: usize,
    acc: f64,
    count: usize,
    last: f64,
    onsets: Vec<f32>,
    total: usize,
    result: Option<f32>,
    since_check: usize,
}

impl BeatDetector {
    pub fn new(sample_rate: f64) -> Self {
        Self {
            sample_rate,
            hop: (sample_rate / RATE) as usize,
            acc: 0.0,
            count: 0,
            last: 0.0,
            onsets: Vec::new(),
            total: 0,
            result: None,
            since_check: 0,
        }
    }

    /// Forgets the song, for the start of a new one.
    pub fn reset(&mut self) {
        *self = Self::new(self.sample_rate);
    }

    /// The tempo, once enough audio has been heard.
    pub fn bpm(&self) -> Option<f32> {
        self.result
    }

    /// Feeds interleaved stereo (or any channel count) samples, at the
    /// stream's own speed.
    pub fn push(&mut self, interleaved: &[f64], channels: usize) {
        let channels = channels.max(1);
        for frame in interleaved.chunks_exact(channels) {
            let mono = frame.iter().sum::<f64>() / channels as f64;
            self.acc += mono * mono;
            self.count += 1;
            if self.count >= self.hop {
                let energy = (self.acc / self.count as f64).sqrt();
                // Compressed, so a loud chorus does not drown a quiet verse.
                let level = (1.0 + 20.0 * energy).ln();
                let rise = (level - self.last).max(0.0);
                self.last = level;
                self.acc = 0.0;
                self.count = 0;
                self.onsets.push(rise as f32);
                self.total += 1;
                self.since_check += 1;
                let cap = KEEP_SECONDS * RATE as usize;
                if self.onsets.len() > cap {
                    let extra = self.onsets.len() - cap;
                    self.onsets.drain(..extra);
                }
            }
        }
        // Re-estimate every few seconds, so the answer settles as more is
        // heard rather than being fixed by the first intro.
        if self.total >= NEEDED_SECONDS * RATE as usize && self.since_check >= 3 * RATE as usize {
            self.since_check = 0;
            if let Some(bpm) = estimate(&self.onsets) {
                self.result = Some(bpm);
            }
        }
    }
}

/// The tempo whose beat period the onsets repeat at most strongly.
fn estimate(onsets: &[f32]) -> Option<f32> {
    let n = onsets.len();
    let mean = onsets.iter().sum::<f32>() / n.max(1) as f32;
    let centred: Vec<f32> = onsets.iter().map(|v| v - mean).collect();
    let lo = (RATE * 60.0 / MAX_BPM) as usize;
    let hi = (RATE * 60.0 / MIN_BPM) as usize;
    if n < hi * 4 {
        return None;
    }
    let mut best = (f32::MIN, 0usize);
    let mut scores = vec![0.0f32; hi + 2];
    for lag in lo..=hi {
        let mut sum = 0.0f32;
        // Add the double and triple lag too: a real beat repeats at its
        // multiples, a coincidence does not.
        for multiple in 1..=3 {
            let l = lag * multiple;
            if l >= n {
                break;
            }
            let mut dot = 0.0f32;
            for i in l..n {
                dot += centred[i] * centred[i - l];
            }
            sum += dot / (n - l) as f32;
        }
        // A gentle preference for the tempos songs actually have.
        let bpm = RATE * 60.0 / lag as f64;
        let prior = (-0.5 * ((bpm / 120.0).ln() / 0.45).powi(2)).exp() as f32;
        let score = sum * (0.6 + 0.4 * prior);
        scores[lag] = score;
        if score > best.0 {
            best = (score, lag);
        }
    }
    if best.0 <= 0.0 {
        return None;
    }
    // Parabolic refinement between neighbouring lags.
    let lag = best.1;
    let mut refined = lag as f64;
    if lag > lo && lag < hi {
        let (a, b, c) = (scores[lag - 1] as f64, scores[lag] as f64, scores[lag + 1] as f64);
        let denom = a - 2.0 * b + c;
        if denom.abs() > 1e-12 {
            refined += 0.5 * (a - c) / denom;
        }
    }
    let bpm = RATE * 60.0 / refined;
    Some((bpm * 10.0).round() as f32 / 10.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A click track at a known tempo must be found.
    #[test]
    fn finds_a_steady_click() {
        let rate = 44100.0;
        let mut detector = BeatDetector::new(rate);
        let period = (rate * 60.0 / 128.0) as usize;
        let mut samples = vec![0.0f64; rate as usize * 20 * 2];
        let mut at = 0;
        while at * 2 + 400 < samples.len() {
            for k in 0..400 {
                samples[(at + k) * 2] = 0.8 * (1.0 - k as f64 / 400.0);
                samples[(at + k) * 2 + 1] = samples[(at + k) * 2];
            }
            at += period;
        }
        detector.push(&samples, 2);
        let bpm = detector.bpm().expect("a tempo");
        assert!((bpm - 128.0).abs() < 3.0, "heard {bpm}");
    }
}
