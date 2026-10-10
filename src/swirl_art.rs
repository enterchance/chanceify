//! The album artwork, smeared into a soft diagonal swirl backdrop.
//!
//! The first swirl this app drew was rings: closed curves whose radius came
//! from the spectrum, stacked around the transport. It was a waveform that
//! happened to be round. What a swirl actually looks like is the artwork
//! itself dragged around a centre — the same thing a desktop engine does
//! with whatever is on its screen — so this is that: the cover, turned more
//! the further out from the centre it is, carried sideways across the whole
//! width of the bar.
//!
//! **Low resolution on purpose.** A swirl is a smear. Warping it at cover
//! resolution costs a resample per pixel for detail nobody can see through
//! the distortion, so the warp runs on a small image and the bar scales it
//! up. It is also what keeps this affordable: a few thousand pixels, once
//! per song rather than once per frame.

use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::{Duration, Instant};

use image::RgbaImage;

/// How many pixels across the warped cover is kept. Small enough that the
/// resample is free, large enough that the swirl has something to bend.
const W: u32 = 960;
const H: u32 = 540;
/// Everything about the backdrop's picture that is baked into the texture;
/// change any of it and the texture is made again.
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub struct Look {
    /// waves, peaks, cross waves %, tilt degrees, shift %, softness,
    /// richness %, contrast %.
    pub v: [i16; 8],
    /// Plain cover art, mirrored and scrolling, instead of the waves.
    pub art: bool,
    /// With `art`: the cover mirrored every other copy (true) or repeated.
    pub mirror: bool,
    /// With `art`: how far the soft blur reaches in from the edges where one
    /// cover meets the next, 0 (sharp) to 100 (blurred almost to the middle).
    pub edge: i16,
}

/// How many backdrops are being made right now.
static BUILDING: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// How long a failed fetch waits before asking again.
const RETRY: Duration = Duration::from_secs(5);
/// The largest artwork this will decode. A cover is a few hundred
/// kilobytes; anything past this is not one, and decoding it to throw the
/// pixels away is the one way this could cost real memory.
const MAX_ART_BYTES: usize = 24 * 1024 * 1024;

/// The swirled cover for whichever song is playing, and the machinery to
/// fetch the next one without blocking a frame.
///
/// The bytes are fetched rather than borrowed from whatever cover is on
/// screen: a cover releases its bytes as soon as its texture exists, and
/// this would then wait for a song that never changes.
#[derive(Default)]
pub struct SwirlArt {
    uri: Option<String>,
    texture: Option<egui::TextureHandle>,
    pending: Option<Receiver<Result<Option<Backdrop>, String>>>,
    accent: Option<egui::Color32>,
    colours: Vec<egui::Color32>,
    previous: Option<egui::TextureHandle>,
    fade_from: Option<Instant>,
    /// A new song's backdrop fades in; a changed setting swaps at once.
    fade_next: bool,
    requested: bool,
    retry_at: Option<Instant>,
    /// Whether the backdrop is bright overall, so text on it can go dark.
    light: bool,
    /// The (waves, peaks) the current texture was made with.
    params: Look,
    /// The settings the reader has moved to, and since when; the backdrop is
    /// only rebuilt once they have held still, so a drag stays smooth.
    wanted: Option<(Look, Instant)>,
}

impl SwirlArt {
    /// Whether the backdrop is bright enough that light text would vanish.
    pub fn is_light(&self) -> bool {
        self.light
    }

    /// The backdrop of the song before, and how opaque it still is, while it
    /// fades out over the new one.
    pub fn previous(&mut self, ctx: &egui::Context) -> Option<(egui::TextureId, f32)> {
        const FADE: f32 = 1.1;
        let started = self.fade_from?;
        let t = started.elapsed().as_secs_f32() / FADE;
        if t >= 1.0 {
            self.previous = None;
            self.fade_from = None;
            return None;
        }
        ctx.request_repaint();
        let eased = 1.0 - t * t * (3.0 - 2.0 * t);
        self.previous.as_ref().map(|handle| (handle.id(), eased))
    }

    /// The cover's vivid colours, most prominent first.
    pub fn colours(&self) -> &[egui::Color32] {
        &self.colours
    }

    /// A vivid colour drawn from the cover, for the bars.
    pub fn accent(&self) -> Option<egui::Color32> {
        self.accent
    }

    /// The texture for `uri`, asking for it if it is not here yet.
    pub fn texture(
        &mut self,
        ctx: &egui::Context,
        loader: &crate::images::ArtLoader,
        uri: Option<&str>,
        params: Look,
    ) -> Option<&egui::TextureHandle> {
        if self.uri.as_deref() != uri {
            *self = Self {
                // The old swirl stays until the new one lands, so a change
                // of song does not flash the bar empty for a second.
                texture: self.texture.take().filter(|_| uri.is_some()),
                accent: self.accent.filter(|_| uri.is_some()),
                colours: if uri.is_some() { self.colours.clone() } else { Vec::new() },
                uri: uri.map(str::to_owned),
                params,
                fade_next: true,
                ..Default::default()
            };
        }
        let uri = uri?;
        if params != self.params {
            // New wave settings: rebuild once the sliders have stopped.
            let since = match self.wanted {
                Some((seen, at)) if seen == params => at,
                _ => {
                    let now = Instant::now();
                    self.wanted = Some((params, now));
                    now
                }
            };
            let wait = Duration::from_millis(40);
            if since.elapsed() >= wait {
                self.params = params;
                self.wanted = None;
                self.requested = false;
                self.pending = None;
                self.retry_at = None;
            } else {
                ctx.request_repaint_after(wait);
            }
        }
        let due = self.retry_at.is_none_or(|at| Instant::now() >= at);
        // Only a couple of backdrops are ever being made at once: moving a
        // slider back and forth would otherwise start a heavy job on every
        // pause, piling them up on the processor and in memory.
        let busy = BUILDING.load(std::sync::atomic::Ordering::Relaxed) >= 2;
        if busy && !self.requested {
            ctx.request_repaint_after(Duration::from_millis(150));
        }
        if !self.requested && due && !busy && uri.starts_with("http") {
            self.requested = true;
            self.retry_at = None;
            let look = self.params;
            self.pending = Some(loader.spawn_processed(ctx, uri, move |bytes| {
                BUILDING.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                let made = std::panic::catch_unwind(|| backdrop(bytes, look)).unwrap_or(None);
                BUILDING.fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
                made
            }));
        }
        if let Some(receiver) = &self.pending {
            match receiver.try_recv() {
                Ok(Ok(made)) => {
                    self.light = made.as_ref().is_some_and(|made| is_light(&made.image));
                    self.accent = made.as_ref().map(|made| made.accent);
                    self.colours = made
                        .as_ref()
                        .map(|made| made.colours.clone())
                        .unwrap_or_default();
                    let before = self.texture.take();
                    self.texture = made.map(|made| {
                        let image = made.image;
                        ctx.load_texture(
                            "swirl-art",
                            image,
                            egui::TextureOptions {
                                wrap_mode: egui::TextureWrapMode::Repeat,
                                ..egui::TextureOptions::LINEAR
                            },
                        )
                    });
                    if self.texture.is_some() && before.is_some() && self.fade_next {
                        // The old backdrop stays up and fades out over the
                        // new one, rather than switching in a frame.
                        self.previous = before;
                        self.fade_from = Some(Instant::now());
                    } else if self.texture.is_none() {
                        self.texture = before;
                    }
                    self.fade_next = false;
                    self.pending = None;
                }
                Ok(Err(_)) | Err(TryRecvError::Disconnected) => {
                    self.pending = None;
                    self.requested = false;
                    self.retry_at = Some(Instant::now() + RETRY);
                    ctx.request_repaint_after(RETRY);
                }
                Err(TryRecvError::Empty) => {}
            }
        }
        self.texture.as_ref()
    }
}

/// What one cover becomes: the soft backdrop and the colour for the bars.
pub struct Backdrop {
    pub image: egui::ColorImage,
    pub accent: egui::Color32,
    /// Up to three distinct vivid colours, most prominent first.
    pub colours: Vec<egui::Color32>,
}

/// The cover's most prominent *colours*, kept vivid, most prominent first.
/// Pixels count by how saturated and bright they are, so a red cover with a
/// lot of black gives red, not grey; a cover with no colour at all gives a
/// single light grey.
fn colours_of(source: &RgbaImage) -> Vec<egui::Color32> {
    use egui::ecolor::Hsva;
    const BUCKETS: usize = 12;
    let mut weight = [0.0f32; BUCKETS];
    let mut sat = [0.0f32; BUCKETS];
    let mut val = [0.0f32; BUCKETS];
    // The pixels that make up each colour, weighted towards the vivid ones,
    // so the colour reported is one the cover really contains and not the
    // quantised centre of a hue slice or a washed-out average of it.
    let mut rgb = [[0.0f32; 3]; BUCKETS];
    let mut rgb_weight = [0.0f32; BUCKETS];
    let mut grey = 0.0f32;
    for p in source.pixels() {
        let [r, g, b, a] = p.0;
        let hsva = Hsva::from_srgba_unmultiplied([r, g, b, a]);
        grey += hsva.v;
        if hsva.s > 0.14 && hsva.v > 0.15 {
            let bucket = ((hsva.h * BUCKETS as f32) as usize).min(BUCKETS - 1);
            let w = hsva.s * hsva.v;
            weight[bucket] += w;
            sat[bucket] += hsva.s * w;
            val[bucket] += hsva.v * w;
            let sharp = w * w;
            rgb[bucket][0] += f32::from(r) * sharp;
            rgb[bucket][1] += f32::from(g) * sharp;
            rgb[bucket][2] += f32::from(b) * sharp;
            rgb_weight[bucket] += sharp;
        }
    }
    let total: f32 = weight.iter().sum();
    if total < 0.2 {
        let average = grey / source.pixels().len().max(1) as f32;
        let mid = (average.clamp(0.45, 0.85) * 255.0) as u8;
        return vec![
            egui::Color32::from_gray(mid),
            egui::Color32::from_gray(mid.saturating_sub(70).max(90)),
        ];
    }
    let mut order: Vec<usize> = (0..BUCKETS).collect();
    order.sort_by(|a, b| weight[*b].total_cmp(&weight[*a]));
    let mut picked: Vec<usize> = Vec::new();
    for bucket in order {
        if weight[bucket] < total * 0.02 || picked.len() == 4 {
            break;
        }
        // Not a neighbour of one already taken: two shades of the same
        // colour are not a choice.
        let near = picked.iter().any(|other| {
            let gap = (*other as i32 - bucket as i32).rem_euclid(BUCKETS as i32);
            gap <= 1 || gap >= BUCKETS as i32 - 1
        });
        if !near {
            picked.push(bucket);
        }
    }
    let mut found: Vec<egui::Color32> = picked
        .iter()
        .map(|&best| {
            let n = rgb_weight[best].max(f32::EPSILON);
            let real = Hsva::from_srgba_unmultiplied([
                (rgb[best][0] / n) as u8,
                (rgb[best][1] / n) as u8,
                (rgb[best][2] / n) as u8,
                255,
            ]);
            let _ = (&sat, &val);
            // Only lifted when it would vanish: dark or grey colours get a
            // little help, a colour the cover really has is left alone.
            egui::Color32::from(Hsva::new(
                real.h,
                real.s.max(0.4),
                real.v.max(0.6),
                1.0,
            ))
        })
        .collect();
    // A one-colour cover still gives the pickers two choices: a neighbouring
    // hue, so the second pick is never plain white.
    if found.len() < 2 {
        let best = picked[0];
        let h = ((best as f32 + 0.5) / BUCKETS as f32 + 0.09).rem_euclid(1.0);
        found.push(egui::Color32::from(Hsva::new(h, 0.6, 0.95, 1.0)));
    }
    found
}

/// The one colour a page header is tinted with: the cover's most prominent
/// colour, found the same way the visualizer finds its own, so a page and the
/// bars agree about what colour a cover is.
pub fn accent_of(bytes: &[u8]) -> Option<[u8; 3]> {
    let decoded = image::load_from_memory(bytes).ok()?;
    let small: RgbaImage = decoded.thumbnail(64, 64).to_rgba8();
    let first = colours_of(&small).first().copied()?;
    Some([first.r(), first.g(), first.b()])
}

/// Mean brightness above which the backdrop counts as light.
fn is_light(image: &egui::ColorImage) -> bool {
    let sum: f32 = image
        .pixels
        .iter()
        .map(|c| 0.299 * f32::from(c.r()) + 0.587 * f32::from(c.g()) + 0.114 * f32::from(c.b()))
        .sum();
    sum / image.pixels.len().max(1) as f32 > 150.0
}

/// The cover smeared along a diagonal into a soft swirl, `W` by `H`.
///
/// Every output pixel averages a run of samples taken along a diagonal
/// through the cover, and the run bends in a slow wave, so the colours of
/// the art come out as long blurred streaks with a swirl in them, which is
/// what a media-player backdrop looks like. The picture is periodic in both
/// directions, so sliding it sideways never shows a seam.
pub fn backdrop(bytes: &[u8], look: Look) -> Option<Backdrop> {
    if bytes.len() > MAX_ART_BYTES {
        return None;
    }
    let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .ok()?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(8192);
    limits.max_image_height = Some(8192);
    limits.max_alloc = Some(64 * 1024 * 1024);
    reader.limits(limits);
    let decoded = reader.decode().ok()?;
    // A bigger thumbnail than before: the swirl is a warp of this picture,
    // and a 64px source is what made the old one blocky and foggy.
    // The plain-cover mode shows the picture itself, so it keeps more of it.
    // In art mode the strip is drawn small (a bar is about 80 pixels tall),
    // so it is made close to that size. A big texture shrunk that far
    // shimmers while it slides, which showed up as flicker.
    let (out_w, out_h): (u32, u32) = if look.art { (576, 288) } else { (W, H) };
    let size = if look.art { 288u32 } else { 256u32 };
    let source: RgbaImage = decoded.thumbnail(size, size).to_rgba8();
    let (sw, sh) = (source.width().max(1), source.height().max(1));
    // A smooth (bilinear) sample from the cover.
    let at = |u: f32, v: f32| -> [f32; 3] {
        let fx = u.clamp(0.0, 1.0) * (sw - 1) as f32;
        let fy = v.clamp(0.0, 1.0) * (sh - 1) as f32;
        let (x0, y0) = (fx.floor() as u32, fy.floor() as u32);
        let (x1, y1) = ((x0 + 1).min(sw - 1), (y0 + 1).min(sh - 1));
        let (tx, ty) = (fx - x0 as f32, fy - y0 as f32);
        let px = |x: u32, y: u32| -> [f32; 3] {
            let p = source.get_pixel(x, y);
            [f32::from(p.0[0]), f32::from(p.0[1]), f32::from(p.0[2])]
        };
        let (a, b, c, d) = (px(x0, y0), px(x1, y0), px(x0, y1), px(x1, y1));
        let mut o = [0.0f32; 3];
        for i in 0..3 {
            let top = a[i] + (b[i] - a[i]) * tx;
            let bottom = c[i] + (d[i] - c[i]) * tx;
            o[i] = top + (bottom - top) * ty;
        }
        o
    };
    // A smooth fold with period 1 (0 -> 1 -> 0): every colour of the cover is
    // used, and the picture is periodic, so the backdrop tiles with no seam.
    let mirror = |x: f32| 0.5 - 0.5 * (std::f32::consts::TAU * x).cos();
    let tau = std::f32::consts::TAU;
    let (width, height) = (out_w as usize, out_h as usize);
    // Stage 1: the liquid wave. Every point of the cover is pushed along a
    // direction at right angles to a slow sine wave, and a second gentler wave
    // runs across the first so several colours are always in play. Both waves
    // have a whole number of cycles across the picture, which is what makes
    // the result tile: the right edge continues into the left edge.
    // `waves` is how many ripples fit across the picture; `peaks` is how
    // far they push the colours, as a percentage.
    let [waves, peaks, cross, tilt, shift, softness, mut richness, mut contrast] = look.v;
    if look.art {
        // The cover as it is: no extra colour or contrast on top of it.
        richness = 100;
        contrast = 100;
    }
    let cycles = f32::from(waves.clamp(1, 8));
    let rise = f32::from(peaks.clamp(0, 100)) / 50.0;
    let cross = f32::from(cross.clamp(0, 200)) / 100.0;
    let angle = (-0.316f32).atan2(-0.949f32) + f32::from(tilt.clamp(-45, 45)).to_radians();
    let phase = 0.7 + tau * f32::from(shift.clamp(0, 100)) / 100.0;
    let (dir_x, dir_y) = (angle.cos(), angle.sin());
    let (off_x, off_y) = (dir_y, -dir_x);
    let mut warped = vec![[0.0f32; 3]; width * height];
    for py in 0..height {
        for px in 0..width {
            let u = px as f32 / out_w as f32;
            let v = py as f32 / out_h as f32;
            let w1 = (tau * (-cycles * u - 1.0 * v) + phase).sin();
            let w2 = (tau * (1.0 * u - 1.0 * v) + 2.1).sin();
            let x = u + (w1 * off_x) * 0.55 * rise + w2 * 0.12 * rise * cross;
            let y = v + (w1 * off_y) * 0.55 * rise - w2 * 0.09 * rise * cross;
            warped[py * width + px] = if look.art {
                // The cover itself, whole: forwards across the first half of
                // the picture and backwards across the second, which is what
                // makes the strip tile with no seam, and top to bottom with
                // no fold, so none of it is ever cut off.
                if look.mirror { at(1.0 - (2.0 * u - 1.0).abs(), v) } else { at(u, v) }
            } else {
                at(mirror(x), mirror(y))
            };
        }
    }
    // Stage 2: a wide Gaussian blur, in two passes, so the streaks come out
    // smooth with no pixels in them at any resolution.
    let sigma = if look.art { 0.0 } else { f32::from(softness.clamp(0, 30)) };
    let radius = if sigma < 0.5 { 0 } else { ((sigma * 2.4).ceil() as i32).min(72) };
    let sigma = sigma.max(0.5);
    let mut kernel: Vec<f32> = (-radius..=radius)
        .map(|i| (-(i * i) as f32 / (2.0 * sigma * sigma)).exp())
        .collect();
    let total: f32 = kernel.iter().sum();
    for weight in &mut kernel {
        *weight /= total;
    }
    let mut across = vec![[0.0f32; 3]; width * height];
    for py in 0..height {
        for px in 0..width {
            let mut acc = [0.0f32; 3];
            for (k, weight) in kernel.iter().enumerate() {
                let sx = (px as i32 + k as i32 - radius).rem_euclid(width as i32) as usize;
                let sample = warped[py * width + sx];
                for c in 0..3 {
                    acc[c] += sample[c] * weight;
                }
            }
            across[py * width + px] = acc;
        }
    }
    // Plain-cover mode: soften the places where one cover meets the next, so
    // the same square is not seen again and again with hard sides.
    if look.art && look.edge > 0 {
        soften_seams(&mut across, width, height, look);
    }
    let mut out = RgbaImage::new(out_w, out_h);
    for py in 0..height {
        for px in 0..width {
            let mut acc = [0.0f32; 3];
            for (k, weight) in kernel.iter().enumerate() {
                let sy = (py as i32 + k as i32 - radius).rem_euclid(height as i32) as usize;
                let sample = across[sy * width + px];
                for c in 0..3 {
                    acc[c] += sample[c] * weight;
                }
            }
            // Stage 3: rich and contrasty, so the ribbons read as distinct
            // colours rather than fog.
            let lum = 0.299 * acc[0] + 0.587 * acc[1] + 0.114 * acc[2];
            let mut rgb = [0u8; 3];
            for c in 0..3 {
                let rich = lum + (acc[c] - lum) * (f32::from(richness.clamp(50, 250)) / 100.0);
                let contrast = (rich - 128.0) * (f32::from(contrast.clamp(60, 200)) / 100.0) + 128.0;
                rgb[c] = contrast.clamp(0.0, 255.0) as u8;
            }
            out.put_pixel(px as u32, py as u32, image::Rgba([rgb[0], rgb[1], rgb[2], 255]));
        }
    }
    let colours = colours_of(&source);
    // A cover with no colour in it at all (fully clear, say) still gets a backdrop.
    let colours = if colours.is_empty() {
        vec![egui::Color32::from_rgb(128, 128, 140)]
    } else {
        colours
    };
    Some(Backdrop {
        image: egui::ColorImage::from_rgba_unmultiplied([out_w as usize, out_h as usize], &out.into_raw()),
        accent: colours[0],
        colours,
    })
}

/// Blurs the strip sideways, more the closer a pixel is to a seam between
/// two covers. The strip wraps, so a seam at the very edge blends with the
/// far side too.
fn soften_seams(pixels: &mut [[f32; 3]], width: usize, height: usize, look: Look) {
    // Covers per texture width: two when mirrored (the second is the first
    // backwards), otherwise one.
    let period = if look.mirror { width / 2 } else { width }.max(2);
    let reach = (period as f32 * 0.5 * (look.edge.clamp(0, 100) as f32 / 100.0)).max(1.0);
    // How wide the blur itself is: wider at the seam than farther in.
    let widest = ((period as f32) * 0.16 * (look.edge.clamp(0, 100) as f32 / 100.0)).round() as i32;
    if widest < 1 {
        return;
    }
    // A blurred copy (box blur, run twice, wrapping) to blend towards.
    let blur = |source: &[[f32; 3]]| -> Vec<[f32; 3]> {
        let mut out = vec![[0.0f32; 3]; source.len()];
        let span = (2 * widest + 1) as f32;
        for row in 0..height {
            let base = row * width;
            let mut acc = [0.0f32; 3];
            for k in -widest..=widest {
                let x = k.rem_euclid(width as i32) as usize;
                for c in 0..3 {
                    acc[c] += source[base + x][c];
                }
            }
            for x in 0..width {
                for c in 0..3 {
                    out[base + x][c] = acc[c] / span;
                }
                let leave = (x as i32 - widest).rem_euclid(width as i32) as usize;
                let enter = (x as i32 + widest + 1).rem_euclid(width as i32) as usize;
                for c in 0..3 {
                    acc[c] += source[base + enter][c] - source[base + leave][c];
                }
            }
        }
        out
    };
    let once = blur(pixels);
    let twice = blur(&once);
    for row in 0..height {
        for x in 0..width {
            let r = (x % period) as f32;
            let distance = r.min(period as f32 - r);
            let t = (1.0 - distance / reach).clamp(0.0, 1.0);
            let weight = t * t * (3.0 - 2.0 * t);
            if weight <= 0.0 {
                continue;
            }
            let i = row * width + x;
            for c in 0..3 {
                pixels[i][c] += (twice[i][c] - pixels[i][c]) * weight;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn split() -> Vec<u8> {
        let mut image = image::RgbaImage::new(16, 16);
        for (x, _, pixel) in image.enumerate_pixels_mut() {
            *pixel = if x < 8 {
                image::Rgba([255, 0, 0, 255])
            } else {
                image::Rgba([0, 0, 255, 255])
            };
        }
        let mut bytes = Vec::new();
        image
            .write_to(
                &mut std::io::Cursor::new(&mut bytes),
                image::ImageFormat::Png,
            )
            .expect("encode");
        bytes
    }

    #[test]
    fn a_cover_becomes_a_wide_backdrop() {
        let b = backdrop(&split(), Look { v: [2, 10, 100, 0, 0, 7, 140, 120], art: false, mirror: true, edge: 0 }).expect("a backdrop");
        assert_eq!(b.image.width(), W as usize);
        assert_eq!(b.image.height(), H as usize);
    }

    #[test]
    fn the_backdrop_keeps_the_cover_colours() {
        let b = backdrop(&split(), Look { v: [2, 10, 100, 0, 0, 7, 140, 120], art: false, mirror: true, edge: 0 }).expect("a backdrop");
        assert!(b.image.pixels.iter().any(|c| c.r() > c.b()));
        assert!(b.image.pixels.iter().any(|c| c.b() > c.r()));
    }

    #[test]
    fn something_that_is_not_a_cover_yields_nothing() {
        assert!(backdrop(b"not an image at all", Look::default()).is_none());
    }
}
