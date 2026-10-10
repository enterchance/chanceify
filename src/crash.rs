//! A black box for crashes that leave no panic behind.
//!
//! A Rust panic is written to `panic.log` and, while drawing, caught. What
//! that cannot see is the process being ended from outside the language: a
//! memory failure, a graphics driver giving up, an abort. Those leave
//! nothing at all, which is why "it just closed" was impossible to chase.
//!
//! Everything lives in the state folder, and **nothing is ever thrown away
//! by starting again**: a run that did not end cleanly is copied into
//! `crash-history.log` the next time the program starts.
//!
//! * `stage.log` is one line, rewritten at every step of drawing a frame
//!   (the panel, the bars, the flow, the swirl, an action being applied...).
//!   When the program dies, the line says which step it was in.
//! * `trail.log` holds what the person did and what went wrong: settings
//!   moved, clicks, panics, huge or failed memory requests, a frozen frame.
//! * `running.flag` exists while the program runs and is removed on a clean
//!   exit; finding it at the next start means the last run did not finish.
//! * On Windows a hook notes the code and address of a fatal exception.
//! * A watcher thread notes a frame that has been going for too long.

use std::alloc::{GlobalAlloc, Layout, System};
use std::io::{Seek, SeekFrom, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

static DIR: OnceLock<PathBuf> = OnceLock::new();
static STAGE_FILE: Mutex<Option<std::fs::File>> = Mutex::new(None);
static NOTE_FILE: OnceLock<std::fs::File> = OnceLock::new();
static START: OnceLock<Instant> = OnceLock::new();
static FRAMES: AtomicU64 = AtomicU64::new(0);
static FRAME_STARTED_MS: AtomicU64 = AtomicU64::new(0);
static IN_FRAME: AtomicBool = AtomicBool::new(false);
static MEMORY_NOTES: AtomicU32 = AtomicU32::new(0);
static LAST_STAGE: Mutex<String> = Mutex::new(String::new());
static PREVIOUS: OnceLock<Previous> = OnceLock::new();

/// A memory request this large is worth a line, even if it succeeds.
const BIG_REQUEST: usize = 768 * 1024 * 1024;

/// What the run before this one left behind.
pub struct Previous {
    /// It did not end cleanly.
    pub dirty: bool,
    /// The last step of drawing it reached.
    pub stage: String,
    /// The tail of what it did, as lines.
    pub trail: String,
}

impl Previous {
    /// The last run died while drawing or handling the visualizer.
    pub fn looks_like_visualizer(&self) -> bool {
        let stage = self.stage.to_lowercase();
        let in_vis = ["vis", "swirl", "flow", "bars", "panel", "menu"]
            .iter()
            .any(|word| stage.contains(word));
        let did_vis = self.trail.lines().rev().take(12).any(|line| {
            ["action SetVis", "action ToggleVis", "action SetSwirl", "action ToggleSwirl",
             "action SetFlow", "action ToggleBarSide", "action ResetSwirl"]
                .iter()
                .any(|prefix| line.contains(prefix))
        });
        self.dirty && (in_vis || did_vis)
    }
}

/// What the previous run left, once this one has started.
pub fn previous() -> Option<&'static Previous> {
    PREVIOUS.get()
}

fn millis() -> u64 {
    START.get().map_or(0, |start| start.elapsed().as_millis() as u64)
}

fn stamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_secs())
}

fn read_text(path: &std::path::Path) -> String {
    let bytes = std::fs::read(path).unwrap_or_default();
    String::from_utf8_lossy(&bytes)
        .chars()
        .filter(|c| *c != '\0')
        .collect::<String>()
}

fn tail_lines(text: &str, count: usize) -> String {
    let lines: Vec<&str> = text.lines().collect();
    lines[lines.len().saturating_sub(count)..].join("\n")
}

/// Looks at what the last run left, files it away, and starts recording.
pub fn install(state: PathBuf) {
    let _ = START.set(Instant::now());
    let _ = std::fs::create_dir_all(&state);
    let flag = state.join("running.flag");
    let dirty = flag.exists();
    let last_stage = read_text(&state.join("stage.log")).trim().to_string();
    let last_trail = tail_lines(&read_text(&state.join("trail.log")), 60);
    if dirty {
        // The history keeps every unfinished run, so starting again never
        // erases the evidence.
        let history = state.join("crash-history.log");
        if std::fs::metadata(&history).is_ok_and(|meta| meta.len() > 512 * 1024) {
            let _ = std::fs::remove_file(&history);
        }
        if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(&history) {
            let _ = writeln!(
                file,
                "=== a run did not end cleanly (found at {}) ===\nlast step: {last_stage}\nwhat it was doing:\n{last_trail}\n",
                stamp()
            );
        }
    }
    let _ = PREVIOUS.set(Previous { dirty, stage: last_stage, trail: last_trail });
    let _ = std::fs::write(&flag, stamp().to_string());
    let _ = std::fs::write(state.join("trail.log"), b"");
    if let Ok(file) = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(state.join("stage.log"))
    {
        *STAGE_FILE.lock().unwrap_or_else(|p| p.into_inner()) = Some(file);
    }
    if let Ok(file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(state.join("trail.log"))
    {
        let _ = NOTE_FILE.set(file);
    }
    let _ = DIR.set(state);
    #[cfg(windows)]
    native::install();
    stage("started");
    std::thread::Builder::new()
        .name("black-box-watch".into())
        .spawn(watch)
        .ok();
}

/// Call after the logger is set up: panics are copied into the trail too.
pub fn chain_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let place = info
            .location()
            .map_or_else(String::new, |at| format!("{}:{}", at.file(), at.line()));
        let message = if let Some(text) = info.payload().downcast_ref::<&str>() {
            (*text).to_string()
        } else if let Some(text) = info.payload().downcast_ref::<String>() {
            text.clone()
        } else {
            String::new()
        };
        let message: String = message.chars().take(160).collect();
        let last = LAST_STAGE.lock().map(|s| s.clone()).unwrap_or_default();
        trail(&format!("PANIC at {place} after step '{last}': {message}"));
        previous(info);
    }));
}

/// The program is closing on its own terms.
pub fn clean_exit() {
    stage("clean exit");
    if let Some(dir) = DIR.get() {
        let _ = std::fs::remove_file(dir.join("running.flag"));
    }
}

/// A new frame begins; the watcher times it.
pub fn frame_begin() {
    FRAMES.fetch_add(1, Ordering::Relaxed);
    FRAME_STARTED_MS.store(millis(), Ordering::Relaxed);
    IN_FRAME.store(true, Ordering::Relaxed);
}

/// The frame finished.
pub fn frame_end() {
    IN_FRAME.store(false, Ordering::Relaxed);
}

/// Where the drawing has got to. Cheap: one short write into a fixed spot.
pub fn stage(name: &str) {
    if let Ok(mut last) = LAST_STAGE.try_lock() {
        last.clear();
        last.push_str(name);
    }
    let Ok(mut slot) = STAGE_FILE.try_lock() else {
        return;
    };
    let Some(file) = slot.as_mut() else {
        return;
    };
    let mut line = format!("{:>8}ms frame {} {}", millis(), FRAMES.load(Ordering::Relaxed), name);
    line.truncate(110);
    while line.len() < 110 {
        line.push(' ');
    }
    line.push('\n');
    if file.seek(SeekFrom::Start(0)).is_ok() {
        let _ = file.write_all(line.as_bytes());
    }
}

/// Notes something the person did, so a crash can be placed after it.
pub fn trail(text: &str) {
    if let Some(mut file) = NOTE_FILE.get() {
        let _ = writeln!(file, "{} +{}ms {text}", stamp(), millis());
    }
}

/// The start of a value's `Debug` text, cut short so a big one costs nothing.
pub fn describe(value: &dyn std::fmt::Debug) -> String {
    struct Capped(String);
    impl std::fmt::Write for Capped {
        fn write_str(&mut self, text: &str) -> std::fmt::Result {
            for c in text.chars() {
                if self.0.chars().count() >= 80 {
                    return Err(std::fmt::Error);
                }
                self.0.push(c);
            }
            Ok(())
        }
    }
    let mut out = Capped(String::new());
    let _ = std::fmt::write(&mut out, format_args!("{value:?}"));
    out.0
}

/// A frame that runs for seconds is a frozen program: say where.
fn watch() {
    let mut reported = 0u64;
    loop {
        std::thread::sleep(std::time::Duration::from_secs(1));
        if !IN_FRAME.load(Ordering::Relaxed) {
            continue;
        }
        let started = FRAME_STARTED_MS.load(Ordering::Relaxed);
        if millis().saturating_sub(started) > 4000 && reported != started {
            reported = started;
            let last = LAST_STAGE.lock().map(|s| s.clone()).unwrap_or_default();
            trail(&format!("FROZEN: one frame has run for over 4 seconds, in step '{last}'"));
        }
        // A freeze inside the visualizer is not waited out: the program
        // closes itself, and the next start comes up with the visualizer off.
        if millis().saturating_sub(started) > 9000 {
            let last = LAST_STAGE.lock().map(|s| s.clone()).unwrap_or_default();
            if ["menu", "visualizer", "panel", "swirl", "flow", "bars"]
                .iter()
                .any(|word| last.contains(word))
            {
                trail(&format!(
                    "FROZEN for 9 seconds in step '{last}': closing, so the next start is safe"
                ));
                std::process::exit(86);
            }
        }
    }
}

/// Watches every memory request. A failed or enormous one is written down
/// without needing any memory itself, because after a failed request the
/// program is about to be ended by the system without a word.
pub struct Watch;

fn note_memory(size: usize, failed: bool) {
    if MEMORY_NOTES.fetch_add(1, Ordering::Relaxed) >= 30 {
        return;
    }
    let Some(mut file) = NOTE_FILE.get() else {
        return;
    };
    let mut buffer = [0u8; 96];
    let mut at = 0usize;
    let mut put = |bytes: &[u8]| {
        for byte in bytes {
            if at < buffer.len() {
                buffer[at] = *byte;
                at += 1;
            }
        }
    };
    let heading: &[u8] = if failed {
        b"MEMORY REQUEST FAILED, bytes="
    } else {
        b"huge memory request, bytes="
    };
    put(heading);
    let mut digits = [0u8; 20];
    let mut count = 0;
    let mut rest = size;
    loop {
        digits[count] = b'0' + (rest % 10) as u8;
        count += 1;
        rest /= 10;
        if rest == 0 {
            break;
        }
    }
    while count > 0 {
        count -= 1;
        put(&[digits[count]]);
    }
    put(b"\n");
    let _ = file.write_all(&buffer[..at]);
}

// SAFETY: every call is passed straight to the system allocator; only the
// size is looked at afterwards.
unsafe impl GlobalAlloc for Watch {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let pointer = unsafe { System.alloc(layout) };
        if pointer.is_null() || layout.size() > BIG_REQUEST {
            note_memory(layout.size(), pointer.is_null());
        }
        pointer
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let pointer = unsafe { System.alloc_zeroed(layout) };
        if pointer.is_null() || layout.size() > BIG_REQUEST {
            note_memory(layout.size(), pointer.is_null());
        }
        pointer
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        unsafe { System.dealloc(pointer, layout) }
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let moved = unsafe { System.realloc(pointer, layout, new_size) };
        if moved.is_null() || new_size > BIG_REQUEST {
            note_memory(new_size, moved.is_null());
        }
        moved
    }
}

#[cfg(windows)]
mod native {
    use windows_sys::Win32::System::Diagnostics::Debug::{
        AddVectoredExceptionHandler, EXCEPTION_POINTERS,
    };

    /// Exceptions that end a program. (Others, such as the ones a debugger
    /// or a language runtime uses, pass by all the time and mean nothing.)
    const FATAL: [u32; 8] = [
        0xC000_0005, // access violation
        0xC000_00FD, // stack overflow
        0xC000_0409, // fast fail (an abort)
        0xC000_0374, // heap corruption
        0xC000_001D, // illegal instruction
        0xC000_0094, // integer divide by zero
        0xC000_0096, // privileged instruction
        0xC000_0025, // cannot continue
    ];

    unsafe extern "system" fn handler(info: *mut EXCEPTION_POINTERS) -> i32 {
        // SAFETY: Windows hands over a valid pointer, or null, which `as_ref` handles.
        let Some(info) = (unsafe { info.as_ref() }) else {
            return 0;
        };
        // SAFETY: as above.
        let Some(record) = (unsafe { record_of(info) }) else {
            return 0;
        };
        let code = record.ExceptionCode as u32;
        if FATAL.contains(&code) {
            let last = super::LAST_STAGE.try_lock().map(|s| s.clone()).unwrap_or_default();
            let line = format!(
                "NATIVE ERROR code=0x{code:08X} address={:p} in step '{last}'",
                record.ExceptionAddress
            );
            super::trail(&line);
            if let Some(dir) = super::DIR.get()
                && let Ok(mut file) = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(dir.join("crash.log"))
            {
                use std::io::Write;
                let _ = writeln!(file, "{} {line}", super::stamp());
            }
        }
        // Keep looking: this only writes the note down.
        0
    }

    unsafe fn record_of(
        info: &EXCEPTION_POINTERS,
    ) -> Option<&windows_sys::Win32::System::Diagnostics::Debug::EXCEPTION_RECORD> {
        // SAFETY: the record pointer is valid for the call, or null.
        unsafe { info.ExceptionRecord.as_ref() }
    }

    pub(super) fn install() {
        // SAFETY: registers a handler that only appends to files.
        unsafe {
            AddVectoredExceptionHandler(1, Some(handler));
        }
    }
}
