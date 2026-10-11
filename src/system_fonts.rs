//! Fonts installed on this computer that the full-screen visualizer can draw
//! its title in.

use std::path::PathBuf;

/// Faces offered for the title in the full-screen visualizer: a name to show
/// and the file in Windows' font folder. The first entry is the app's own.
pub const VIS_FONTS: [(&str, &str); 28] = [
    ("Inter (default)", ""),
    ("Impact", "impact.ttf"),
    ("Georgia", "georgia.ttf"),
    ("Segoe Script", "segoesc.ttf"),
    ("Bahnschrift", "bahnschrift.ttf"),
    ("Consolas", "consola.ttf"),
    ("Palatino", "pala.ttf"),
    ("Gabriola", "gabriola.ttf"),
    ("Ink Free", "Inkfree.ttf"),
    ("Times", "times.ttf"),
    ("Comic Sans", "comic.ttf"),
    ("Trebuchet", "trebuc.ttf"),
    ("Lucida Handwriting", "LHANDW.TTF"),
    ("Brush Script", "BRUSHSCI.TTF"),
    ("Segoe Print", "segoepr.ttf"),
    ("Arial Black", "ariblk.ttf"),
    ("Rockwell", "ROCK.TTF"),
    ("Candara", "candara.ttf"),
    ("Constantia", "constan.ttf"),
    ("Corbel", "corbel.ttf"),
    ("Cooper Black", "COOPBL.TTF"),
    ("Bauhaus 93", "BAUHS93.TTF"),
    ("Jokerman", "JOKERMAN.TTF"),
    ("Chiller", "CHILLER.TTF"),
    ("Old English", "OLDENGL.TTF"),
    ("Stencil", "STENCIL.TTF"),
    ("Papyrus", "PAPYRUS.TTF"),
    ("Mistral", "MISTRAL.TTF"),
];

/// The bytes of visualizer font `index`, when this computer has it.
pub fn vis_font_bytes(index: usize) -> Option<Vec<u8>> {
    let (_, file) = VIS_FONTS.get(index)?;
    if file.is_empty() {
        return None;
    }
    let windows = std::env::var_os("WINDIR").map_or_else(|| PathBuf::from("C:\\Windows"), PathBuf::from);
    std::fs::read(windows.join("Fonts").join(file)).ok()
}
