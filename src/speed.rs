//! Playback speed, as a pitch-preserving time stretch on the audio thread.
//!
//! librespot has no rate control of its own, so speed is applied where every
//! player does it: to the decoded samples. This used to read them faster or
//! slower, which moved the pitch with the tempo. It now uses WSOLA
//! (waveform-similarity overlap-add): short Hann-windowed slices of the input
//! are laid down at a fixed output hop, and each slice start is nudged to
//! wherever it lines up best with the previous one, so the waveform joins
//! without a click and the pitch stays where it was.
//!
//! The stretcher is stateful because rodio hands the sink whatever a packet
//! holds. It keeps the unread input between calls.

use std::sync::{Arc, Mutex};

/// How fast or slow the stream may run.
pub const MIN_SPEED: f32 = 0.75;
/// The fastest. Past this the pitch is a novelty rather than a sound.
pub const MAX_SPEED: f32 = 1.5;

/// The speed the window sets and the audio thread reads.
pub type SharedSpeed = Arc<Mutex<f32>>;

/// A new handle, reading as normal speed until told otherwise.
pub fn shared() -> SharedSpeed {
    Arc::new(Mutex::new(1.0))
}

/// Pulls a rate out of whatever the reader put in the settings file.
pub fn clamped(speed: f32) -> f32 {
    if speed.is_finite() {
        speed.clamp(MIN_SPEED, MAX_SPEED)
    } else {
        1.0
    }
}

/// A speed with a name and, where the scene calls for it, the equalizer
/// curve that goes with it. The pairing is the point: "hardtekk" is a fast
/// track with a bass-heavy curve, not a fast track.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Preset {
    pub name: &'static str,
    pub speed: f32,
    /// An [`crate::eq::PRESETS`] name, or `None` to leave the curve alone.
    pub eq: Option<&'static str>,
}

/// The presets, in the order the button cycles them. Normal is first so one
/// press of the key puts a nightcore back to how it was.
pub const PRESETS: &[Preset] = &[
    Preset {
        name: "Normal",
        speed: 1.0,
        eq: Some("Flat"),
    },
    Preset {
        name: "Warm",
        speed: 1.0,
        eq: Some("Full Bass"),
    },
    Preset {
        name: "Punch",
        speed: 1.0,
        eq: Some("Bass Booster"),
    },
    Preset {
        name: "Nightcore",
        speed: 1.25,
        eq: Some("Laptop Speakers / Headphones"),
    },
    Preset {
        name: "Hardtekk",
        speed: 1.35,
        eq: Some("Techno"),
    },
    Preset {
        name: "Gabber",
        speed: 1.08,
        eq: Some("Jump"),
    },
    Preset {
        name: "Slowed",
        speed: 0.8,
        eq: Some("Soft"),
    },
    Preset {
        name: "Half speed",
        speed: 0.75,
        eq: Some("Soft Rock"),
    },
];

/// One preset's name and speed.
pub fn preset(index: usize) -> &'static Preset {
    &PRESETS[index % PRESETS.len()]
}

/// The index of the preset whose speed is nearest `speed`, so a slider left
/// at 1.35 and a click on "Hardtekk" agree about where they are.
pub fn index_of_speed(speed: f32) -> usize {
    PRESETS
        .iter()
        .position(|preset| (preset.speed - speed).abs() < f32::EPSILON)
        .unwrap_or(0)
}

/// Frames per analysis window.
const WINDOW: usize = 1024;
/// Output frames produced per step. Half a window, so the Hann slices sum to 1.
const HOP: usize = WINDOW / 2;
/// How far either side of the nominal position a slice may slide to find its
/// best join, in frames.
const SEEK: usize = 220;

/// Changes tempo without changing pitch, on interleaved samples.
///
/// One instance per output stream, on the audio thread.
pub struct Resampler {
    /// Input consumed per output frame. Above 1 is faster.
    ratio: f64,
    channels: usize,
    /// Unread input, interleaved. Frame 0 is absolute frame `base`.
    input: Vec<f64>,
    base: usize,
    /// Nominal absolute analysis position of the next slice.
    position: f64,
    /// Absolute start of the previous slice, once there is one.
    last_start: Option<usize>,
    /// Overlap-add accumulator, `WINDOW` frames, interleaved.
    pending: Vec<f64>,
    window: Vec<f64>,
}

impl Resampler {
    /// A stretcher at the given rate, where 1.0 is the stream's own speed.
    pub fn new(speed: f32) -> Self {
        Self::with_channels(speed, 1)
    }

    /// A stretcher for an interleaved stream of `channels` channels.
    pub fn with_channels(speed: f32, channels: usize) -> Self {
        let channels = channels.max(1);
        let window = (0..WINDOW)
            .map(|i| 0.5 - 0.5 * (std::f64::consts::TAU * i as f64 / WINDOW as f64).cos())
            .collect();
        Self {
            ratio: f64::from(clamped(speed)),
            channels,
            input: Vec::new(),
            base: 0,
            position: 0.0,
            last_start: None,
            pending: vec![0.0; WINDOW * channels],
            window,
        }
    }

    /// Changes rate without a click: the read position is kept.
    pub fn set_speed(&mut self, speed: f32) {
        self.ratio = f64::from(clamped(speed));
    }

    /// How fast this is running, 1.0 being the stream's own rate.
    pub fn speed(&self) -> f32 {
        self.ratio as f32
    }

    /// Drops all buffered audio, for a seek or a new stream.
    pub fn reset(&mut self) {
        self.input.clear();
        self.base = 0;
        self.position = 0.0;
        self.last_start = None;
        self.pending.iter_mut().for_each(|v| *v = 0.0);
    }

    /// Stretches `input` and returns the new samples (possibly none yet).
    ///
    /// At 1.0 this is the identity, so a reader who never touches the speed
    /// pays nothing for the feature existing.
    pub fn process(&mut self, input: &[f64]) -> Vec<f64> {
        if (self.ratio - 1.0).abs() < 1e-4 {
            if !self.input.is_empty() || self.last_start.is_some() {
                self.reset();
            }
            return input.to_vec();
        }
        let ch = self.channels;
        self.input.extend_from_slice(input);
        let mut out = Vec::with_capacity(input.len() + HOP * ch);
        loop {
            let frames = self.input.len() / ch;
            let target = self.position.round() as usize;
            // Everything this step may read must already be here.
            if target + SEEK + WINDOW > self.base + frames {
                break;
            }
            let start = match self.last_start {
                None => target.max(self.base),
                Some(previous) => self.best_start(previous, target),
            };
            self.overlap_add(start);
            out.extend_from_slice(&self.pending[..HOP * ch]);
            self.pending.drain(..HOP * ch);
            self.pending.resize(WINDOW * ch, 0.0);
            self.last_start = Some(start);
            self.position += HOP as f64 * self.ratio;
            // Forget what neither the next template nor the next search
            // window can reach.
            let keep_from = (start + HOP).min(self.position.round() as usize)
                .saturating_sub(SEEK)
                .max(self.base);
            let drop = keep_from - self.base;
            if drop > 0 {
                self.input.drain(..drop * ch);
                self.base = keep_from;
            }
        }
        out
    }

    /// The mono value of an absolute frame.
    fn mono(&self, frame: usize) -> f64 {
        let at = (frame - self.base) * self.channels;
        self.input[at..at + self.channels].iter().sum::<f64>() / self.channels as f64
    }

    /// The start near `target` whose first half best continues the audio
    /// that followed the previous slice.
    fn best_start(&self, previous: usize, target: usize) -> usize {
        let template_at = (previous + HOP).max(self.base);
        let lo = target.saturating_sub(SEEK).max(self.base);
        let hi = target + SEEK;
        let step = 2;
        let len = HOP / step;
        let template: Vec<f64> = (0..len).map(|i| self.mono(template_at + i * step)).collect();
        let mut best = (f64::MIN, target.max(self.base));
        for candidate in lo..=hi {
            let mut dot = 0.0;
            let mut energy = 1e-9;
            for (i, t) in template.iter().enumerate() {
                let v = self.mono(candidate + i * step);
                dot += t * v;
                energy += v * v;
            }
            let score = dot / energy.sqrt();
            if score > best.0 {
                best = (score, candidate);
            }
        }
        best.1
    }

    fn overlap_add(&mut self, start: usize) {
        let ch = self.channels;
        let from = (start - self.base) * ch;
        for frame in 0..WINDOW {
            let w = self.window[frame];
            for c in 0..ch {
                self.pending[frame * ch + c] += w * self.input[from + frame * ch + c];
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(len: usize, hz: f64) -> Vec<f64> {
        (0..len)
            .map(|i| (std::f64::consts::TAU * hz * i as f64 / 44100.0).sin() * 0.5)
            .collect()
    }

    #[test]
    fn one_to_one_is_the_identity() {
        let mut stretcher = Resampler::new(1.0);
        let input = sine(64, 440.0);
        assert_eq!(stretcher.process(&input), input);
    }

    #[test]
    fn faster_gives_proportionally_fewer_samples() {
        let mut stretcher = Resampler::new(1.5);
        let out = stretcher.process(&sine(44100, 440.0));
        let expected = 44100.0 / 1.5;
        assert!(
            (out.len() as f64 - expected).abs() < expected * 0.15,
            "1.5x produced {} samples",
            out.len()
        );
    }

    #[test]
    fn slower_gives_more_samples() {
        let mut stretcher = Resampler::new(0.75);
        let out = stretcher.process(&sine(44100, 440.0));
        assert!(out.len() > 44100, "0.75x produced {} samples", out.len());
    }

    /// The point of the change: the pitch stays put.
    #[test]
    fn pitch_is_preserved() {
        let mut stretcher = Resampler::new(1.4);
        let out = stretcher.process(&sine(44100, 440.0));
        let crossings = out.windows(2).filter(|w| w[0] <= 0.0 && w[1] > 0.0).count();
        let seconds = out.len() as f64 / 44100.0;
        let hz = crossings as f64 / seconds;
        assert!((hz - 440.0).abs() < 25.0, "pitch drifted to {hz}");
    }

    #[test]
    fn packet_boundaries_do_not_change_the_length_much() {
        let whole = sine(20000, 330.0);
        let mut together = Resampler::new(1.25);
        let a = together.process(&whole).len();
        let mut apart = Resampler::new(1.25);
        let mut b = apart.process(&whole[..5000]).len();
        b += apart.process(&whole[5000..]).len();
        assert!(a.abs_diff(b) <= HOP, "{a} vs {b}");
    }

    #[test]
    fn an_impossible_speed_is_normal_speed() {
        assert_eq!(clamped(f32::NAN), 1.0);
        assert_eq!(clamped(0.0), MIN_SPEED);
        assert_eq!(clamped(99.0), MAX_SPEED);
        assert_eq!(clamped(1.25), 1.25);
    }

    #[test]
    fn every_preset_names_a_curve_and_a_speed() {
        for preset in PRESETS {
            if let Some(name) = preset.eq {
                assert!(
                    crate::eq::PRESETS.iter().any(|known| known.name == name),
                    "{} names a curve that does not exist: {name}",
                    preset.name
                );
            }
            assert!((MIN_SPEED..=MAX_SPEED).contains(&preset.speed));
        }
        assert!(PRESETS.iter().any(|p| p.speed > 1.25));
        assert!(PRESETS.iter().any(|p| p.speed < 1.0));
    }
}
