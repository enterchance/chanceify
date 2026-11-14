//! The Windows volume, read a few times a second on a thread of its own, so
//! the loudness reading and limit follow what reaches the headphones.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Once;

/// The Windows volume in dB below its top, 0 at full.
static LEVEL_DB: AtomicU64 = AtomicU64::new(0);
static MUTED: AtomicBool = AtomicBool::new(false);
static START: Once = Once::new();

/// The Windows volume in dB (0 at full, negative below), or `None` while
/// Windows is muted. 0 where it cannot be read.
pub fn db() -> Option<f64> {
    START.call_once(start);
    if MUTED.load(Ordering::Relaxed) {
        return None;
    }
    Some(f64::from_bits(LEVEL_DB.load(Ordering::Relaxed)))
}

#[cfg(windows)]
fn start() {
    let spawned = std::thread::Builder::new()
        .name("system-volume".into())
        .spawn(|| {
            // SAFETY: COM is set up once for this thread, which only reads
            // the default output's volume.
            unsafe {
                let _ = windows::Win32::System::Com::CoInitializeEx(
                    None,
                    windows::Win32::System::Com::COINIT_MULTITHREADED,
                );
            }
            loop {
                match read() {
                    Ok((db, muted)) => {
                        LEVEL_DB.store(f64::from(db).min(0.0).to_bits(), Ordering::Relaxed);
                        MUTED.store(muted, Ordering::Relaxed);
                    }
                    Err(error) => log::debug!("system volume unreadable: {error}"),
                }
                std::thread::sleep(std::time::Duration::from_millis(250));
            }
        });
    if let Err(error) = spawned {
        log::warn!("system volume thread: {error}");
    }
}

/// The default output device's volume and mute, asked fresh each time so a
/// newly plugged headset is followed.
#[cfg(windows)]
fn read() -> windows::core::Result<(f32, bool)> {
    use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
    use windows::Win32::Media::Audio::{IMMDeviceEnumerator, MMDeviceEnumerator, eConsole, eRender};
    use windows::Win32::System::Com::{CLSCTX_ALL, CoCreateInstance};
    // SAFETY: plain COM calls on interfaces this function owns.
    unsafe {
        let devices: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
        let device = devices.GetDefaultAudioEndpoint(eRender, eConsole)?;
        let volume: IAudioEndpointVolume = device.Activate(CLSCTX_ALL, None)?;
        // The slider's position, not the dB the device reports: many
        // headsets report a narrow dB range that barely moves while the
        // sound changes a lot. A cubic taper, the usual shape of a volume
        // knob, follows what is heard much more closely.
        let scalar = volume.GetMasterVolumeLevelScalar()?;
        let db = (60.0 * f64::from(scalar).max(1e-6).log10()).max(-96.0);
        let muted = volume.GetMute()?.as_bool();
        Ok((db as f32, muted))
    }
}

#[cfg(not(windows))]
fn start() {}
