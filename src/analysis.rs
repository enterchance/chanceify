//! Tempo, key and waveform, worked out from the sound itself.
//!
//! Spotify no longer gives these to new apps, so for songs that are files on
//! this computer they are measured here. Nothing in this module touches the
//! disk or the network: it takes mono samples and returns numbers, which is
//! also what lets it be tested with tones whose answer is known.
//!
//! * **Tempo** — the track is cut into short frames, the jump in loudness
//!   from one frame to the next (in a low band and a high band) says where
//!   the beats land, and the spacing that repeats most strongly across the
//!   whole song is the tempo. Octave mistakes (60 against 120) are settled
//!   by preferring the range people actually dance and tap to.
//! * **Key** — the energy at each of the twelve notes (C, C sharp, ...) is
//!   added up over the song, and that profile is compared with the shape a
//!   major or minor key has (Krumhansl and Schmuckler's well-known
//!   profiles). The best of the 24 matches wins.
//! * **Waveform** — the loudest sample in each slice of the song.

use std::f32::consts::PI;

/// What was found. Every field is a measurement, not a lookup.
#[derive(Clone, Debug, PartialEq)]
pub struct Analysis {
    /// Beats a minute, to a tenth. `None` for a song with no steady beat.
    pub bpm: Option<f32>,
    /// 0 to 1: how much stronger the chosen tempo was than the rest.
    pub bpm_confidence: f32,
    pub key: Option<Key>,
    /// Slice peaks, 0 to 255, [`WAVEFORM_POINTS`] of them.
    pub waveform: Vec<u8>,
    pub seconds: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Key {
    /// 0 is C, 1 is C sharp, ... 11 is B.
    pub tonic: u8,
    pub minor: bool,
    /// 0 to 1: how far the best key beat the runner-up.
    pub confidence: f32,
}

pub const WAVEFORM_POINTS: usize = 240;

const NOTE_NAMES: [&str; 12] = [
    "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
];

impl Key {
    /// "A minor", "F# major".
    pub fn name(&self) -> String {
        format!(
            "{} {}",
            NOTE_NAMES[usize::from(self.tonic % 12)],
            if self.minor { "minor" } else { "major" }
        )
    }

    /// The Camelot wheel position DJs mix by: "8A" is A minor, "8B" C major.
    pub fn camelot(&self) -> String {
        // Camelot numbers for major keys starting from C: 8B, then up a
        // fifth (7 semitones) adds one. Minor is the relative major's number.
        const MAJOR: [u8; 12] = [8, 3, 10, 5, 12, 7, 2, 9, 4, 11, 6, 1];
        let major_tonic = if self.minor {
            (self.tonic + 3) % 12
        } else {
            self.tonic % 12
        };
        format!(
            "{}{}",
            MAJOR[usize::from(major_tonic)],
            if self.minor { "A" } else { "B" }
        )
    }
}

/// Measures a whole song. `samples` are mono, any rate from 8 kHz up.
pub fn analyze(samples: &[f32], rate: u32) -> Analysis {
    let seconds = samples.len() as f32 / rate.max(1) as f32;
    let waveform = waveform(samples);
    if samples.len() < rate as usize * 8 || rate < 8000 {
        // Under eight seconds there is not enough to call a tempo or a key.
        return Analysis {
            bpm: None,
            bpm_confidence: 0.0,
            key: None,
            waveform,
            seconds,
        };
    }
    let (low, low_rate) = decimate(samples, rate, 11025);
    let (bpm, bpm_confidence) = tempo(&low, low_rate);
    let key = key(&low, low_rate);
    Analysis {
        bpm,
        bpm_confidence,
        key,
        waveform,
        seconds,
    }
}

/// The loudest sample in each of [`WAVEFORM_POINTS`] slices, scaled so the
/// loudest slice of the song is 255.
pub fn waveform(samples: &[f32]) -> Vec<u8> {
    if samples.is_empty() {
        return vec![0; WAVEFORM_POINTS];
    }
    let mut peaks = vec![0.0f32; WAVEFORM_POINTS];
    for (i, peak) in peaks.iter_mut().enumerate() {
        let from = i * samples.len() / WAVEFORM_POINTS;
        let to = ((i + 1) * samples.len() / WAVEFORM_POINTS).max(from + 1).min(samples.len());
        *peak = samples[from..to]
            .iter()
            .fold(0.0f32, |top, s| top.max(s.abs()));
    }
    let top = peaks.iter().copied().fold(0.0f32, f32::max);
    if top <= 0.0 {
        return vec![0; WAVEFORM_POINTS];
    }
    peaks
        .into_iter()
        .map(|p| ((p / top) * 255.0).round().clamp(0.0, 255.0) as u8)
        .collect()
}

/// Averages groups of samples down to about `target` Hz (a crude low-pass,
/// which is all the tempo and key work need).
fn decimate(samples: &[f32], rate: u32, target: u32) -> (Vec<f32>, f32) {
    let factor = ((rate as f32 / target as f32).round() as usize).max(1);
    if factor == 1 {
        return (samples.to_vec(), rate as f32);
    }
    let out: Vec<f32> = samples
        .chunks(factor)
        .filter(|chunk| chunk.len() == factor)
        .map(|chunk| chunk.iter().sum::<f32>() / factor as f32)
        .collect();
    (out, rate as f32 / factor as f32)
}

// ---- tempo -----------------------------------------------------------

/// Beats a minute and how sure.
fn tempo(samples: &[f32], rate: f32) -> (Option<f32>, f32) {
    let hop = (rate / 86.0).round().max(32.0) as usize; // about 11.6 ms
    let frame = hop * 2;
    let fps = rate / hop as f32;
    // Use at most three minutes from the middle, where a song is most
    // itself; intros and outros are often free of a beat.
    let max_len = (rate * 180.0) as usize;
    let (start, end) = if samples.len() > max_len {
        let start = (samples.len() - max_len) / 2;
        (start, start + max_len)
    } else {
        (0, samples.len())
    };
    let samples = &samples[start..end];
    if samples.len() < frame * 8 {
        return (None, 0.0);
    }
    // Two bands: low (kicks, bass) and the rest (snares, hats), by a
    // one-pole low-pass at roughly 200 Hz and what is left after it.
    let alpha = 1.0 - (-2.0 * PI * 200.0 / rate).exp();
    let mut lp = 0.0f32;
    let frames = (samples.len() - frame) / hop;
    let mut low_energy = Vec::with_capacity(frames);
    let mut high_energy = Vec::with_capacity(frames);
    let mut low_sample = Vec::with_capacity(samples.len());
    let mut high_sample = Vec::with_capacity(samples.len());
    for &s in samples {
        lp += alpha * (s - lp);
        low_sample.push(lp);
        high_sample.push(s - lp);
    }
    for f in 0..frames {
        let from = f * hop;
        let to = from + frame;
        let le: f32 = low_sample[from..to].iter().map(|v| v * v).sum::<f32>() / frame as f32;
        let he: f32 = high_sample[from..to].iter().map(|v| v * v).sum::<f32>() / frame as f32;
        low_energy.push(le);
        high_energy.push(he);
    }
    // Onset strength: the rise in log energy, in each band.
    let mut onset = vec![0.0f32; frames];
    for band in [&low_energy, &high_energy] {
        let scale = band.iter().copied().fold(0.0f32, f32::max).max(1e-12);
        let mut previous = (1.0 + 1000.0 * band[0] / scale).ln();
        let mut band_onset = vec![0.0f32; frames];
        for f in 1..frames {
            let now = (1.0 + 1000.0 * band[f] / scale).ln();
            band_onset[f] = (now - previous).max(0.0);
            previous = now;
        }
        // Each band counts equally however loud it is overall.
        let top = band_onset.iter().copied().fold(0.0f32, f32::max);
        if top > 0.0 {
            for (o, b) in onset.iter_mut().zip(&band_onset) {
                *o += b / top;
            }
        }
    }
    // Take away the slow trend so only the pulse is left.
    let window = (fps * 1.5) as usize;
    let mut flattened = vec![0.0f32; frames];
    let mut sum = 0.0f32;
    for f in 0..frames {
        sum += onset[f];
        if f >= window {
            sum -= onset[f - window];
        }
        let n = (f + 1).min(window) as f32;
        flattened[f] = (onset[f] - sum / n).max(0.0);
    }
    let energy: f32 = flattened.iter().map(|v| v * v).sum();
    if energy <= 1e-9 {
        return (None, 0.0);
    }
    // Autocorrelation up to four seconds (the slowest tempo's four beats).
    let max_lag = ((fps * 4.0) as usize + 2).min(frames / 2);
    let mut acf = vec![0.0f32; max_lag + 1];
    for (lag, slot) in acf.iter_mut().enumerate() {
        let mut total = 0.0f32;
        for f in lag..frames {
            total += flattened[f] * flattened[f - lag];
        }
        *slot = total / (frames - lag).max(1) as f32;
    }
    let at = |lag: f32| -> f32 {
        if lag < 0.0 || lag >= max_lag as f32 {
            return 0.0;
        }
        let i = lag.floor() as usize;
        let t = lag - i as f32;
        acf[i] * (1.0 - t) + acf[(i + 1).min(max_lag)] * t
    };
    // Score every tempo from 60 to 200 in half-beat steps: the strength at
    // one beat, two, three and four beats apart, added up.
    let mut best = (0.0f32, 0.0f32);
    let mut scores: Vec<(f32, f32)> = Vec::new();
    let mut bpm = 60.0f32;
    while bpm <= 200.0 {
        let lag = 60.0 / bpm * fps;
        // Also the half-beat: real music has eighth notes between the
        // beats, and a wrong tempo (two thirds of the right one) has
        // nothing happening at its half-beat.
        let harmonic = at(lag)
            + 0.6 * at(lag * 2.0)
            + 0.4 * at(lag * 3.0)
            + 0.3 * at(lag * 4.0)
            + 0.5 * at(lag * 0.5);
        // People tap in a band around 120; this settles 60 against 120.
        let octaves = (bpm / 120.0).log2();
        let prior = (-0.5 * (octaves / 0.9).powi(2)).exp();
        let score = harmonic * prior;
        scores.push((bpm, score));
        if score > best.1 {
            best = (bpm, score);
        }
        bpm += 0.5;
    }
    if best.1 <= 0.0 {
        return (None, 0.0);
    }
    // Refine around the winner: the centre of the scores within 2% of the
    // peak, so 127.9 and 128.1 do not flip with a rounding.
    let near: Vec<&(f32, f32)> = scores
        .iter()
        .filter(|(b, s)| (b - best.0).abs() <= 2.0 && *s >= best.1 * 0.98)
        .collect();
    let weight: f32 = near.iter().map(|(_, s)| s).sum();
    let refined = near.iter().map(|(b, s)| b * s).sum::<f32>() / weight.max(1e-12);
    let mean = scores.iter().map(|(_, s)| s).sum::<f32>() / scores.len() as f32;
    let confidence = (1.0 - mean / best.1).clamp(0.0, 1.0);
    (Some((refined * 10.0).round() / 10.0), confidence)
}

// ---- key -------------------------------------------------------------

const MAJOR_PROFILE: [f32; 12] = [
    6.35, 2.23, 3.48, 2.33, 4.38, 4.09, 2.52, 5.19, 2.39, 3.66, 2.29, 2.88,
];
const MINOR_PROFILE: [f32; 12] = [
    6.33, 2.68, 3.52, 5.38, 2.60, 3.53, 2.54, 4.75, 3.98, 2.69, 3.34, 3.17,
];

/// Energy at one frequency (Goertzel), for a Hann-windowed frame.
fn goertzel(frame: &[f32], window: &[f32], rate: f32, hz: f32) -> f32 {
    let w = 2.0 * PI * hz / rate;
    let coeff = 2.0 * w.cos();
    let (mut s1, mut s2) = (0.0f32, 0.0f32);
    for (x, win) in frame.iter().zip(window) {
        let s0 = x * win + coeff * s1 - s2;
        s2 = s1;
        s1 = s0;
    }
    (s1 * s1 + s2 * s2 - coeff * s1 * s2).max(0.0)
}

fn key(samples: &[f32], rate: f32) -> Option<Key> {
    const FRAME: usize = 4096;
    if samples.len() < FRAME * 4 {
        return None;
    }
    let window: Vec<f32> = (0..FRAME)
        .map(|i| 0.5 - 0.5 * (2.0 * PI * i as f32 / (FRAME - 1) as f32).cos())
        .collect();
    // About 120 frames spread across the whole song.
    let frames = ((samples.len() - FRAME) / FRAME).clamp(1, 120);
    let step = (samples.len() - FRAME) / frames.max(1);
    // Notes from G2 to B6; below that a 4096 frame cannot tell neighbours apart.
    let notes: Vec<(usize, f32)> = (43..=95)
        .map(|midi| (midi % 12, 440.0 * 2.0f32.powf((midi as f32 - 69.0) / 12.0)))
        .filter(|(_, hz)| *hz < rate / 2.0 * 0.9)
        .collect();
    let mut chroma = [0.0f32; 12];
    for f in 0..frames {
        let from = f * step;
        let frame = &samples[from..from + FRAME];
        let mut local = [0.0f32; 12];
        for &(pitch, hz) in &notes {
            local[pitch] += goertzel(frame, &window, rate, hz).sqrt();
        }
        // Every frame counts the same, loud or quiet.
        let total: f32 = local.iter().sum();
        if total > 1e-6 {
            for (c, l) in chroma.iter_mut().zip(&local) {
                *c += l / total;
            }
        }
    }
    let total: f32 = chroma.iter().sum();
    if total <= 1e-6 {
        return None;
    }
    let mut results: Vec<(f32, u8, bool)> = Vec::with_capacity(24);
    for tonic in 0..12usize {
        for minor in [false, true] {
            let profile = if minor { &MINOR_PROFILE } else { &MAJOR_PROFILE };
            let rotated: Vec<f32> = (0..12).map(|i| profile[(i + 12 - tonic) % 12]).collect();
            results.push((correlation(&chroma, &rotated), tonic as u8, minor));
        }
    }
    results.sort_by(|a, b| b.0.total_cmp(&a.0));
    let (best, second) = (results[0], results[1]);
    if best.0 <= 0.0 {
        return None;
    }
    Some(Key {
        tonic: best.1,
        minor: best.2,
        confidence: ((best.0 - second.0) / best.0.abs().max(1e-6) * 4.0).clamp(0.0, 1.0),
    })
}

fn correlation(a: &[f32; 12], b: &[f32]) -> f32 {
    let ma = a.iter().sum::<f32>() / 12.0;
    let mb = b.iter().sum::<f32>() / 12.0;
    let (mut num, mut da, mut db) = (0.0f32, 0.0f32, 0.0f32);
    for i in 0..12 {
        let (x, y) = (a[i] - ma, b[i] - mb);
        num += x * y;
        da += x * x;
        db += y * y;
    }
    if da <= 0.0 || db <= 0.0 {
        0.0
    } else {
        num / (da.sqrt() * db.sqrt())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 22050;

    /// A beat every `60 / bpm` seconds: a short low thump, with a quieter
    /// hat halfway between.
    fn beats(bpm: f32, seconds: f32) -> Vec<f32> {
        let n = (RATE as f32 * seconds) as usize;
        let mut out = vec![0.0f32; n];
        let period = 60.0 / bpm * RATE as f32;
        let mut t = 0.0f32;
        while (t as usize) < n {
            thump(&mut out, t as usize, 1.0);
            thump(&mut out, (t + period / 2.0) as usize, 0.35);
            t += period;
        }
        out
    }

    fn thump(out: &mut [f32], at: usize, level: f32) {
        for i in 0..2000 {
            if let Some(slot) = out.get_mut(at + i) {
                let decay = (-(i as f32) / 350.0).exp();
                *slot += level * decay * (2.0 * PI * 60.0 * i as f32 / RATE as f32).sin();
                *slot += 0.3 * level * decay * (2.0 * PI * 3000.0 * i as f32 / RATE as f32).sin();
            }
        }
    }

    fn notes(midis: &[(i32, f32)], seconds: f32) -> Vec<f32> {
        let n = (RATE as f32 * seconds) as usize;
        let mut out = vec![0.0f32; n];
        for &(midi, level) in midis {
            let hz = 440.0 * 2.0f32.powf((midi as f32 - 69.0) / 12.0);
            for (i, slot) in out.iter_mut().enumerate() {
                let t = i as f32 / RATE as f32;
                *slot += level
                    * ((2.0 * PI * hz * t).sin() + 0.4 * (2.0 * PI * 2.0 * hz * t).sin());
            }
        }
        out
    }

    #[test]
    fn finds_the_tempo_of_a_steady_beat() {
        for bpm in [90.0f32, 110.0, 128.0, 140.0, 174.0] {
            let found = analyze(&beats(bpm, 40.0), RATE).bpm.expect("a tempo");
            assert!((found - bpm).abs() < 1.5, "wanted {bpm}, got {found}");
        }
    }

    #[test]
    fn a_slow_beat_is_not_doubled() {
        let found = analyze(&beats(70.0, 40.0), RATE).bpm.expect("a tempo");
        assert!((found - 70.0).abs() < 1.5 || (found - 140.0).abs() < 1.5, "{found}");
    }

    #[test]
    fn silence_has_no_tempo_or_key() {
        let a = analyze(&vec![0.0; RATE as usize * 20], RATE);
        assert!(a.bpm.is_none());
        assert!(a.key.is_none());
        assert!(a.waveform.iter().all(|&p| p == 0));
    }

    #[test]
    fn finds_c_major() {
        // C major chord with the tonic doubled low and a D and A passing by.
        let tones = notes(
            &[(48, 1.0), (60, 1.0), (64, 0.8), (67, 0.9), (62, 0.3), (69, 0.3), (55, 0.5)],
            20.0,
        );
        let key = analyze(&tones, RATE).key.expect("a key");
        assert_eq!((key.tonic, key.minor), (0, false), "{}", key.name());
        assert_eq!(key.camelot(), "8B");
    }

    #[test]
    fn finds_a_minor() {
        let tones = notes(
            &[(45, 1.0), (57, 1.0), (60, 0.8), (64, 0.9), (62, 0.3), (65, 0.3), (59, 0.3)],
            20.0,
        );
        let key = analyze(&tones, RATE).key.expect("a key");
        assert_eq!((key.tonic, key.minor), (9, true), "{}", key.name());
        assert_eq!(key.camelot(), "8A");
    }

    #[test]
    fn finds_f_sharp_minor() {
        // F# A C# with E and G# colouring.
        let tones = notes(
            &[(42, 1.0), (54, 1.0), (57, 0.8), (61, 0.9), (64, 0.3), (56, 0.3), (59, 0.3)],
            20.0,
        );
        let key = analyze(&tones, RATE).key.expect("a key");
        assert_eq!((key.tonic, key.minor), (6, true), "{}", key.name());
    }

    #[test]
    fn camelot_numbers() {
        let k = |tonic, minor| Key { tonic, minor, confidence: 1.0 };
        assert_eq!(k(0, false).camelot(), "8B");
        assert_eq!(k(7, false).camelot(), "9B");
        assert_eq!(k(2, false).camelot(), "10B");
        assert_eq!(k(9, true).camelot(), "8A");
        assert_eq!(k(4, true).camelot(), "9A");
    }

    #[test]
    fn waveform_follows_loudness() {
        let mut samples = vec![0.1f32; RATE as usize * 10];
        for s in samples.iter_mut().skip(RATE as usize * 5) {
            *s = 0.8;
        }
        let w = waveform(&samples);
        assert_eq!(w.len(), WAVEFORM_POINTS);
        assert!(w[10] < 60);
        assert_eq!(*w.last().unwrap(), 255);
    }
}
