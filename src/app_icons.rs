//! The icons the window and the taskbar button can wear, and the way to
//! swap one for another while the program runs.
//!
//! Choice 0 is the mark the program has always drawn for itself. The rest
//! are pictures kept in `assets/app-icons`, 256 pixels square, with the
//! transparent ones having no tile behind them.

/// One picture to choose from.
struct Entry {
    name: &'static str,
    png: &'static [u8],
}

static ENTRIES: [Entry; 3] = [
    Entry {
        name: "Vinyl",
        png: include_bytes!("../assets/app-icons/01-vinyl.png"),
    },
    Entry {
        name: "Night",
        png: include_bytes!("../assets/app-icons/02-night.png"),
    },
    Entry {
        name: "Transparent",
        png: include_bytes!("../assets/app-icons/03-transparent.png"),
    },
];

/// The choice that wears the Vinyl icon in the colours of the album playing.
pub const FOLLOWS_ALBUM: u8 = 4;

/// The choice that is the transparent vinyl, turning while a song plays.
pub const SPINNING: u8 = 5;

/// How many pictures one turn of the spinning vinyl is cut into.
pub const SPIN_FRAMES: u32 = 24;

/// How many icons there are to choose from: the classic mark, the three
/// pictures, and the one that follows the album.
pub fn count() -> usize {
    ENTRIES.len() + 3
}

/// The name of a choice, for the list in Settings.
pub fn name(choice: u8) -> &'static str {
    if choice == FOLLOWS_ALBUM {
        return "Follows the album colours";
    }
    if choice == SPINNING {
        return "Spinning vinyl (transparent)";
    }
    match (choice as usize).checked_sub(1).and_then(|at| ENTRIES.get(at)) {
        Some(entry) => entry.name,
        None => "Classic",
    }
}

const SIZE: usize = 256;

fn decode(entry: &Entry) -> Option<image::RgbaImage> {
    let decoded = image::load_from_memory_with_format(entry.png, image::ImageFormat::Png).ok()?;
    Some(resize_clean(&decoded.to_rgba8(), SIZE as u32))
}

/// Resizes a picture with see-through parts without the dark or light
/// fringe that resizing the colours of transparent pixels leaves: the
/// colours are weighted by their opacity while they are mixed.
fn resize_clean(source: &image::RgbaImage, size: u32) -> image::RgbaImage {
    if source.width() == size && source.height() == size {
        return source.clone();
    }
    let mut weighted = source.clone();
    for pixel in weighted.pixels_mut() {
        let alpha = f32::from(pixel[3]) / 255.0;
        for channel in 0..3 {
            pixel[channel] = (f32::from(pixel[channel]) * alpha).round() as u8;
        }
    }
    let mut out = image::imageops::resize(&weighted, size, size, image::imageops::FilterType::Lanczos3);
    for pixel in out.pixels_mut() {
        let alpha = f32::from(pixel[3]) / 255.0;
        if alpha > 0.0 {
            for channel in 0..3 {
                pixel[channel] = (f32::from(pixel[channel]) / alpha).round().min(255.0) as u8;
            }
        }
    }
    out
}

/// The transparent vinyl turned by `frame` steps of a full turn, for the
/// window icon while a song plays. Frame 0 is the picture as it is.
pub fn spun(frame: u32) -> egui::IconData {
    static BASE: std::sync::OnceLock<Option<image::RgbaImage>> = std::sync::OnceLock::new();
    const OUT: u32 = 128;
    let base = BASE.get_or_init(|| {
        let entry = ENTRIES.get(2)?;
        let decoded =
            image::load_from_memory_with_format(entry.png, image::ImageFormat::Png).ok()?;
        Some(resize_clean(&decoded.to_rgba8(), OUT))
    });
    let Some(base) = base else {
        return icon_data(SPINNING);
    };
    let angle = (frame % SPIN_FRAMES) as f32 / SPIN_FRAMES as f32 * std::f32::consts::TAU;
    let (sin, cos) = angle.sin_cos();
    let centre = (OUT as f32 - 1.0) / 2.0;
    let mut rgba = vec![0u8; (OUT * OUT * 4) as usize];
    for y in 0..OUT {
        for x in 0..OUT {
            // Where this spot came from before the turn.
            let (dx, dy) = (x as f32 - centre, y as f32 - centre);
            let source_x = (cos * dx + sin * dy + centre).round();
            let source_y = (-sin * dx + cos * dy + centre).round();
            if source_x < 0.0 || source_y < 0.0 || source_x >= OUT as f32 || source_y >= OUT as f32 {
                continue;
            }
            let pixel = base.get_pixel(source_x as u32, source_y as u32);
            let at = ((y * OUT + x) * 4) as usize;
            rgba[at..at + 4].copy_from_slice(&pixel.0);
        }
    }
    egui::IconData {
        rgba,
        width: OUT,
        height: OUT,
    }
}

/// The Vinyl icon at any size, as straight RGBA, for the tray and the logo
/// the app draws. The classic mark if the picture will not decode.
pub fn vinyl_rgba(size: usize) -> Vec<u8> {
    let size = size.max(1);
    if let Ok(decoded) =
        image::load_from_memory_with_format(ENTRIES[2].png, image::ImageFormat::Png)
    {
        return resize_clean(&decoded.to_rgba8(), size as u32).into_raw();
    }
    crate::util::app_icon_rgba(size)
}

/// The window icon for a choice. A choice that does not exist, or a picture
/// that will not decode, gives the classic mark rather than nothing. The
/// album-following choice gives the plain Vinyl until a colour is known (see
/// [`tinted`]).
pub fn icon_data(choice: u8) -> egui::IconData {
    let at = if choice == FOLLOWS_ALBUM {
        Some(0)
    } else if choice == SPINNING {
        Some(2)
    } else {
        (choice as usize).checked_sub(1)
    };
    if let Some(entry) = at.and_then(|at| ENTRIES.get(at))
        && let Some(small) = decode(entry)
    {
        return egui::IconData {
            rgba: small.into_raw(),
            width: SIZE as u32,
            height: SIZE as u32,
        };
    }
    egui::IconData {
        rgba: crate::util::app_icon_rgba(SIZE),
        width: SIZE as u32,
        height: SIZE as u32,
    }
}

fn to_hsv(r: u8, g: u8, b: u8) -> (f32, f32, f32) {
    let (r, g, b) = (f32::from(r) / 255.0, f32::from(g) / 255.0, f32::from(b) / 255.0);
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let delta = max - min;
    let hue = if delta <= f32::EPSILON {
        0.0
    } else if max == r {
        60.0 * (((g - b) / delta).rem_euclid(6.0))
    } else if max == g {
        60.0 * ((b - r) / delta + 2.0)
    } else {
        60.0 * ((r - g) / delta + 4.0)
    };
    let saturation = if max <= f32::EPSILON { 0.0 } else { delta / max };
    (hue.rem_euclid(360.0), saturation, max)
}

fn from_hsv(h: f32, s: f32, v: f32) -> [u8; 3] {
    let c = v * s;
    let x = c * (1.0 - ((h / 60.0).rem_euclid(2.0) - 1.0).abs());
    let m = v - c;
    let (r, g, b) = match (h.rem_euclid(360.0) / 60.0) as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let byte = |value: f32| ((value + m).clamp(0.0, 1.0) * 255.0).round() as u8;
    [byte(r), byte(g), byte(b)]
}

/// The Vinyl icon with its colours turned to a cover's: every hue is
/// rotated by the same amount, so the picture keeps its shading and its
/// rainbow, only on the album's colour. A grey cover gives a quieter icon.
pub fn tinted(cover: [u8; 3]) -> egui::IconData {
    let base = icon_data(1);
    let (cover_hue, cover_saturation, _) = to_hsv(cover[0], cover[1], cover[2]);
    // The picture's own main hue: the circular mean of its colourful pixels.
    let (mut sin, mut cos) = (0.0f32, 0.0f32);
    for pixel in base.rgba.chunks_exact(4) {
        let (hue, saturation, value) = to_hsv(pixel[0], pixel[1], pixel[2]);
        if pixel[3] > 200 && saturation > 0.35 && value > 0.25 {
            let weight = saturation * value;
            sin += weight * hue.to_radians().sin();
            cos += weight * hue.to_radians().cos();
        }
    }
    let own_hue = if sin == 0.0 && cos == 0.0 {
        cover_hue
    } else {
        sin.atan2(cos).to_degrees().rem_euclid(360.0)
    };
    // A grey cover has no hue to turn to: keep the picture's own, quieter.
    let turn = if cover_saturation < 0.15 {
        0.0
    } else {
        cover_hue - own_hue
    };
    let strength = (0.35 + cover_saturation * 0.9).clamp(0.35, 1.0);
    let mut rgba = base.rgba;
    for pixel in rgba.chunks_exact_mut(4) {
        let (hue, saturation, value) = to_hsv(pixel[0], pixel[1], pixel[2]);
        if saturation < 0.04 {
            continue;
        }
        let [r, g, b] = from_hsv(hue + turn, (saturation * strength).clamp(0.0, 1.0), value);
        pixel[0] = r;
        pixel[1] = g;
        pixel[2] = b;
    }
    egui::IconData {
        rgba,
        width: base.width,
        height: base.height,
    }
}

/// A choice as a picture to show in Settings, 128 pixels square.
pub fn thumbnail(choice: u8) -> egui::ColorImage {
    let icon = if choice == FOLLOWS_ALBUM {
        // One sample colour, to show that this one changes.
        tinted([255, 120, 60])
    } else {
        icon_data(choice)
    };
    egui::ColorImage::from_rgba_unmultiplied(
        [icon.width as usize, icon.height as usize],
        &icon.rgba,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_choice_decodes_to_a_square_icon() {
        for choice in 0..count() as u8 {
            let icon = icon_data(choice);
            assert_eq!(icon.width, icon.height);
            assert_eq!(icon.rgba.len(), (icon.width * icon.height * 4) as usize);
        }
    }

    #[test]
    fn a_tinted_icon_keeps_its_size_and_changes_its_colour() {
        let plain = icon_data(1);
        let red = tinted([220, 30, 30]);
        assert_eq!(red.width, plain.width);
        assert_eq!(red.rgba.len(), plain.rgba.len());
        assert_ne!(red.rgba, plain.rgba);
        let grey = tinted([128, 128, 128]);
        assert_eq!(grey.rgba.len(), plain.rgba.len());
    }

    #[test]
    fn the_vinyl_comes_at_any_size() {
        for size in [16, 32, 84, 256, 300] {
            assert_eq!(vinyl_rgba(size).len(), size * size * 4);
        }
    }

    #[test]
    fn hsv_round_trips() {
        for rgb in [[200u8, 30, 90], [10, 250, 120], [128, 128, 128], [0, 0, 0]] {
            let (h, s, v) = to_hsv(rgb[0], rgb[1], rgb[2]);
            let back = from_hsv(h, s, v);
            for i in 0..3 {
                assert!((i32::from(back[i]) - i32::from(rgb[i])).abs() <= 1);
            }
        }
    }

    #[test]
    fn a_choice_that_does_not_exist_is_the_classic_mark() {
        assert_eq!(name(200), "Classic");
        assert_eq!(icon_data(200).rgba, icon_data(0).rgba);
    }
}
