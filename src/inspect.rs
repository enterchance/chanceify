//! Says what is under the pointer, in a box you can select and copy.
//!
//! "It looks wrong" is the hardest kind of bug report to act on, because
//! the next question is always *what is it actually drawing*. This module
//! answers that: arm it from the player bar's right-click menu, move the
//! mouse over whatever is misbehaving, and the words under the pointer —
//! along with the egui layer they belong to — appear in a panel with a text
//! field, which can be selected, copied and pasted into a bug report.
//!
//! Nothing is recorded while it is off. The recording is two places deep:
//! [`note`] is called by the text helpers in `theme` as they draw, and
//! [`panel`] reads what was noted and asks egui what layer the pointer is
//! over.

use std::sync::Mutex;

/// Whether anything is being recorded. Off until a reader asks for it, and
/// off again the moment they have copied what they needed.
static ARMED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// The last frame's text, as `(where it was drawn, what it said)`. Cleared
/// by [`panel`] each frame it runs, so it holds one frame and no more.
static SEEN: Mutex<Vec<(egui::Rect, String)>> = Mutex::new(Vec::new());

/// Starts or stops recording.
pub fn set_armed(armed: bool) {
    ARMED.store(armed, std::sync::atomic::Ordering::Relaxed);
    if !armed {
        SEEN.lock().map(|mut seen| seen.clear()).ok();
    }
}

/// Whether it is recording.
pub fn armed() -> bool {
    ARMED.load(std::sync::atomic::Ordering::Relaxed)
}

/// Whether the selector is open. Shift+Q.
static SELECTOR: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Whether the reading is frozen. See [`toggle_lock`].
static LOCKED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// The last reading that actually found something under the pointer.
///
/// This is kept across frames on purpose. The report box sits under the
/// pointer the moment it opens, so a live reading taken while the box is
/// open describes the box and nothing else — which is exactly what it did,
/// reporting `pointer 1063, 740` and a layer id while the reader was
/// trying to point at a playlist name. The last good reading is therefore
/// held, so moving the pointer onto the box does not throw away the thing
/// it just passed over.
static LAST: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// Freezes the current reading so the pointer can be moved away to type.
///
/// Locking is the whole point of the box: the reader points at something,
/// presses the key, and can then move the pointer anywhere — including off
/// the thing they were describing — while writing the note. Without it the
/// reading changes under the cursor as they type, and there is no way to
/// write down what was there a moment ago.
pub fn toggle_lock() {
    let locked = !locked();
    LOCKED.store(locked, std::sync::atomic::Ordering::Relaxed);
    if !locked {
        // Unlocking goes back to following the pointer, so whatever it is
        // over now becomes the held reading again.
        clear_last();
    }
}

/// Whether the reading is frozen.
pub fn locked() -> bool {
    LOCKED.load(std::sync::atomic::Ordering::Relaxed)
}

/// Forgets the held reading.
pub fn clear_last() {
    if let Ok(mut last) = LAST.lock() {
        last.clear();
    }
}

/// The held reading, for the tests and for the box.
fn last() -> Vec<String> {
    LAST.lock().map(|last| last.clone()).unwrap_or_default()
}

/// Remembers a reading, if it found anything.
fn remember(lines: &[String]) {
    // The two header lines are always there; anything past them is a real
    // thing under the pointer.
    if lines.len() <= 2 {
        return;
    }
    if let Ok(mut last) = LAST.lock() {
        *last = lines.to_vec();
    }
}

/// Opens or closes the selector.
pub fn set_selector(open: bool) {
    SELECTOR.store(open, std::sync::atomic::Ordering::Relaxed);
    if open {
        // The report box is about what is under the pointer, so opening it
        // starts the recording: the words on screen are the ones the report
        // needs.
        ARMED.store(true, std::sync::atomic::Ordering::Relaxed);
    } else {
        ARMED.store(false, std::sync::atomic::Ordering::Relaxed);
    }
}

/// What the reader has typed. Held here rather than in [`crate::app::App`]
/// because the box is a debugging aid: it must not add a field every reader
/// of [`crate::app::App`] has to know about.
static REPORT: Mutex<String> = Mutex::new(String::new());

/// What is in the report box, emptied first if asked.
pub fn report() -> String {
    REPORT.lock().map(|text| text.clone()).unwrap_or_default()
}

/// Puts text in the report box.
pub fn set_report(text: impl Into<String>) {
    if let Ok(mut held) = REPORT.lock() {
        *held = text.into();
    }
}

/// Whether the selector is open.
pub fn selector() -> bool {
    SELECTOR.load(std::sync::atomic::Ordering::Relaxed)
}

/// Called by every text helper as it draws. A no-op, and a cheap one, when
/// nothing is watching.
pub fn note(rect: egui::Rect, text: &str) {
    if !armed() || text.is_empty() {
        return;
    }
    if let Ok(mut seen) = SEEN.lock()
        && seen.len() < 200
    {
        seen.push((rect, text.to_string()));
    }
}

/// The panel, drawn once a frame over everything else when armed.
///
/// It lists, top to bottom: the egui layer under the pointer, then each
/// string drawn there with where on screen it sits. That is enough to tell
/// "the column is empty" from "the column is showing something that is not
/// a number", which is most of what a report like that turns on.
pub fn panel(app: &mut crate::app::App, ui: &mut egui::Ui) {
    let ctx = ui.ctx().clone();
    let palette = app.palette;
    if selector() {
        egui::Area::new(egui::Id::new("chanceify-report"))
            .fixed_pos(egui::pos2(16.0, 64.0))
            .order(egui::Order::Tooltip)
            .show(&ctx, |ui| report_box(app, ui, &ctx));
        return;
    }
    if !armed() {
        return;
    }
    let lines = hover_lines(&ctx);
    // A frame on the right of the window so the pointer can still be over
    // whatever is being inspected.
    let Some(area) = ctx.input(|input| input.raw.screen_rect) else {
        return;
    };
    egui::Area::new(egui::Id::new("chanceify-inspector"))
        .fixed_pos(egui::pos2(area.right() - 340.0, area.top() + 8.0))
        .order(egui::Order::Tooltip)
        .show(&ctx, |ui| {
            let frame = egui::Frame::new()
                .fill(palette.overlay)
                .stroke(egui::Stroke::new(1.0, palette.outline))
                .corner_radius(egui::CornerRadius::same(6))
                .inner_margin(egui::Margin::same(8));
            frame.show(ui, |ui| {
                ui.set_max_width(320.0);
                ui.horizontal(|ui| {
                    ui.label("Inspect — F2");
                    if ui.button("Stop").clicked() {
                        set_armed(false);
                    }
                    if ui.button("Copy").clicked() {
                        ui.ctx().copy_text(lines.join("\n"));
                    }
                });
                ui.separator();
                for line in &lines {
                    ui.monospace(line);
                }
            });
        });
}

/// What is under the pointer right now, as lines fit to be read aloud.
///
/// Takes the frame's recording with it, so calling this twice in a frame
/// shows the second caller nothing. That is deliberate: one reader per
/// frame, and which reader it is does not depend on drawing order.
fn hover_lines(ctx: &egui::Context) -> Vec<String> {
    let seen = SEEN
        .lock()
        .map(|mut seen| std::mem::take(&mut *seen))
        .unwrap_or_default();
    let Some(pointer) = ctx.pointer_hover_pos() else {
        return Vec::new();
    };
    let layer = ctx.layer_id_at(pointer);
    let mut lines: Vec<String> = vec![
        format!("pointer  {:.0}, {:.0}", pointer.x, pointer.y),
        format!("layer    {layer:?}"),
    ];
    for (rect, text) in seen {
        if rect.contains(pointer) {
            lines.push(format!("at {:.0},{:.0}  {text}", rect.left(), rect.top()));
        }
    }
    remember(&lines);
    lines
}

/// The report box: what the reader is typing about what is under the
/// pointer.
///
/// Shift+Q used to open a list of switches, which was the wrong shape for
/// the job it was given — a reader who has pressed it has something wrong
/// on screen and wants to write down what. So this is a text box, with the
/// pointer's own words kept live above it so the note says which thing is
/// meant, and Copy putting both on the clipboard at once.
fn report_box(app: &mut crate::app::App, ui: &mut egui::Ui, ctx: &egui::Context) {
    let palette = app.palette;
    // Locked, the reading stands still. Unlocked, it follows the pointer
    // but falls back to the last good one rather than to nothing, because
    // the box itself is under the pointer now and has nothing to report.
    let lines = if locked() {
        let held = last();
        if held.is_empty() {
            hover_lines(ctx)
        } else {
            held
        }
    } else {
        let live = hover_lines(ctx);
        if live.len() > 2 {
            live
        } else {
            let held = last();
            if held.is_empty() { live } else { held }
        }
    };
    egui::Frame::new()
        .fill(palette.overlay)
        .stroke(egui::Stroke::new(1.0, palette.outline))
        .corner_radius(egui::CornerRadius::same(6))
        .inner_margin(egui::Margin::same(10))
        .show(ui, |ui| {
            ui.set_max_width(340.0);
            ui.horizontal(|ui| {
                ui.strong("Report — Shift+Q");
                ui.label(
                    egui::RichText::new(if locked() {
                        "reading locked — Ctrl+L to follow the pointer again"
                    } else {
                        "move the mouse over what is wrong, then Ctrl+L"
                    })
                    .small()
                    .color(palette.dim),
                );
            });
            if locked() {
                ui.separator();
                ui.label(
                    egui::RichText::new("locked")
                        .small()
                        .strong()
                        .color(palette.accent),
                );
            }
            ui.separator();
            for line in &lines {
                ui.monospace(egui::RichText::new(line).color(palette.secondary));
            }
            ui.separator();
            let mut text = report();
            let response = ui.add(
                egui::TextEdit::multiline(&mut text)
                    .id(egui::Id::new("chanceify-report-text"))
                    .desired_rows(6)
                    .desired_width(320.0)
                    .hint_text("what is wrong with the thing above"),
            );
            set_report(text.clone());
            // The box is the first thing here to want the keyboard, so it
            // asks for it the moment it opens rather than waiting for a
            // click that a reader in a hurry will not make.
            if response.has_focus() {
                ui.ctx()
                    .memory_mut(|memory| memory.request_focus(response.id));
            }
            ui.horizontal(|ui| {
                if ui
                    .button(if locked() { "Unlock" } else { "Lock" })
                    .clicked()
                {
                    toggle_lock();
                }
                if ui.button("Copy").clicked() {
                    let mut all = lines.clone();
                    all.push(String::new());
                    all.push(text);
                    ui.ctx().copy_text(all.join("\n"));
                }
                if ui.button("Clear").clicked() {
                    set_report(String::new());
                    clear_last();
                }
                if ui.button("Close").clicked() {
                    set_selector(false);
                }
            });
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_is_recorded_until_it_is_armed() {
        // These globals are shared by every test in the process, so each of
        // these starts from a known state rather than from whichever test
        // happened to run before it.
        set_armed(false);
        clear_last();
        SEEN.lock().unwrap().clear();
        note(
            egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(10.0, 10.0)),
            "quiet",
        );
        assert!(
            SEEN.lock().unwrap().is_empty(),
            "an unarmed panel costs nothing"
        );
        set_armed(true);
        SEEN.lock().unwrap().clear();
        note(
            egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(10.0, 10.0)),
            "loud",
        );
        assert_eq!(SEEN.lock().unwrap().len(), 1);
        // And stopping clears it, so the next arming starts from nothing
        // rather than from a frame of someone else's words.
        set_armed(false);
        assert!(SEEN.lock().unwrap().is_empty());
    }

    /// A reading that found something is held, so moving the pointer onto
    /// the report box — which is what happens the instant it opens — does not
    /// throw away the thing the reader was pointing at.
    #[test]
    fn a_reading_is_held_across_frames() {
        clear_last();
        assert!(last().is_empty(), "nothing is held to begin with");

        remember(&[
            "pointer 10, 20".to_string(),
            "layer Background".to_string(),
            "at 4,16  EVENT HORIZON".to_string(),
        ]);
        assert_eq!(last().len(), 3, "a real reading is held");

        // A frame where the pointer found nothing changes nothing.
        remember(&["pointer 1, 1".to_string(), "layer Background".to_string()]);
        assert_eq!(
            last()[2],
            "at 4,16  EVENT HORIZON",
            "an empty reading must not wipe the held one"
        );
        clear_last();
    }

    /// Locking is what makes the box usable at all: the reader points at
    /// something, locks, and can then move the pointer anywhere — including
    /// onto the box — while writing the note.
    #[test]
    fn locking_toggles_and_unlocking_forgets_the_held_reading() {
        clear_last();
        LOCKED.store(false, std::sync::atomic::Ordering::Relaxed);
        assert!(!locked());

        remember(&[
            "pointer 10, 20".to_string(),
            "layer Background".to_string(),
            "at 4,16  SCOURGE".to_string(),
        ]);
        toggle_lock();
        assert!(locked(), "the reading stands still once locked");
        assert_eq!(last()[2], "at 4,16  SCOURGE", "locking keeps the reading");

        toggle_lock();
        assert!(!locked(), "unlocking goes back to following the pointer");
        assert!(last().is_empty(), "unlocking forgets the held reading");
    }
}

/// Save a debug report to a local file for later review
pub fn save_report_to_file(lines: &[String], note: &str) {
    use std::fs::OpenOptions;
    use std::io::Write;
    use std::path::PathBuf;
    
    let debug_dir = PathBuf::from(format!(
        "{}\\AppData\\Local\\paolino\\chanceify\\debug",
        std::env::var("USERPROFILE").unwrap_or_default()
    ));
    
    // Create directory if it doesn't exist
    let _ = std::fs::create_dir_all(&debug_dir);
    
    let log_file = debug_dir.join("inspector_notes.txt");
    if let Ok(mut file) = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_file)
    {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        
        let _ = writeln!(file, "\n=== Flag Report {} ===", timestamp);
        for line in lines {
            let _ = writeln!(file, "{}", line);
        }
        let _ = writeln!(file, "Note: {}", note);
    }
}
