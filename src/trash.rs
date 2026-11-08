//! Sending song files to the Windows Recycle Bin and bringing them back.
//!
//! The Local songs page uses this to take a song file off the computer in a
//! way that can be undone: the file goes to the Recycle Bin, and Ctrl+Z puts
//! it back where it was. Both calls block, so run them off the window's
//! thread. Other systems have no Recycle Bin here and answer with an error.

use std::path::PathBuf;

/// Moves these files to the Recycle Bin, all in one go, without asking and
/// without any window of Windows' own. Windows still warns before deleting
/// a file for good when the Recycle Bin cannot take it (a network drive, or
/// a file too big for the bin), so nothing is lost without a word.
#[cfg(windows)]
pub fn recycle(paths: &[PathBuf]) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx};
    use windows_sys::Win32::UI::Shell::{SHFILEOPSTRUCTW, SHFileOperationW};

    // From shellapi.h. Written out here so the call does not depend on how
    // the bindings type each flag.
    const FO_DELETE: u32 = 0x0003;
    const FOF_SILENT: u16 = 0x0004;
    const FOF_NOCONFIRMATION: u16 = 0x0010;
    const FOF_ALLOWUNDO: u16 = 0x0040;
    const FOF_NOERRORUI: u16 = 0x0400;
    const FOF_WANTNUKEWARNING: u16 = 0x4000;

    if paths.is_empty() {
        return Ok(());
    }
    // Every path whole (the Recycle Bin needs a full path to put a file
    // back), each ended with a NUL and the list with a second one.
    let mut list: Vec<u16> = Vec::new();
    for path in paths {
        let full = std::path::absolute(path).map_err(|error| error.to_string())?;
        list.extend(full.as_os_str().encode_wide());
        list.push(0);
    }
    list.push(0);
    // The shell may load extensions that want COM on this thread. An
    // apartment already set up is kept.
    unsafe { CoInitializeEx(std::ptr::null(), COINIT_APARTMENTTHREADED as u32) };
    let mut operation: SHFILEOPSTRUCTW = unsafe { std::mem::zeroed() };
    operation.wFunc = FO_DELETE as _;
    operation.pFrom = list.as_ptr();
    operation.fFlags =
        (FOF_ALLOWUNDO | FOF_NOCONFIRMATION | FOF_SILENT | FOF_NOERRORUI | FOF_WANTNUKEWARNING) as _;
    let code = unsafe { SHFileOperationW(&mut operation) };
    let aborted = operation.fAnyOperationsAborted;
    if code != 0 {
        return Err(format!("Windows could not move it (error {code:#x})"));
    }
    if aborted != 0 {
        return Err("The move was cancelled".to_string());
    }
    Ok(())
}

/// There is no Recycle Bin to use here.
#[cfg(not(windows))]
pub fn recycle(_paths: &[PathBuf]) -> Result<(), String> {
    Err("Moving files to the Recycle Bin only works on Windows".to_string())
}

/// Puts files back from the Recycle Bin where they were before. For each
/// one the most recently deleted item that came from the same folder under
/// the same name is restored. Whether a file came back is for the caller to
/// check (the file is there again, or not).
#[cfg(windows)]
pub fn restore(paths: &[PathBuf]) -> Result<(), String> {
    use base64::Engine;
    use std::os::windows::process::CommandExt;

    /// Starts PowerShell with no console window.
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    // A long batch is split so no command line grows past what Windows
    // takes (the script is sent as base64 of UTF-16).
    for chunk in paths.chunks(20) {
        let script = restore_script(chunk);
        let bytes: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
        let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
        let output = std::process::Command::new("powershell.exe")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy",
                "Bypass",
                "-EncodedCommand",
                encoded.as_str(),
            ])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .map_err(|error| format!("Could not start PowerShell: {error}"))?;
        if !output.status.success() {
            let said = String::from_utf8_lossy(&output.stderr).trim().to_string();
            log::warn!("restoring from the Recycle Bin: {said}");
        }
    }
    Ok(())
}

/// There is no Recycle Bin to use here.
#[cfg(not(windows))]
pub fn restore(_paths: &[PathBuf]) -> Result<(), String> {
    Err("The Recycle Bin is only used on Windows".to_string())
}

/// A PowerShell string literal holding `text` exactly: single quotes, with
/// every quote inside doubled. PowerShell also takes the curly single
/// quotes as quotes, so those are doubled too.
#[cfg(any(windows, test))]
fn quoted(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('\'');
    for c in text.chars() {
        if matches!(c, '\'' | '\u{2018}' | '\u{2019}' | '\u{201A}' | '\u{201B}') {
            out.push(c);
        }
        out.push(c);
    }
    out.push('\'');
    out
}

/// The PowerShell that restores `paths` through the shell's Recycle Bin
/// folder (Shell.Application, NameSpace 10), the way Explorer's Restore
/// does. An item matches a path when it was deleted from the path's folder
/// and shows the file's name, or its name without the extension (Explorer
/// can hide extensions) while the file in the bin has the same extension.
/// Of several matches the one deleted last is taken. If the shell's
/// Restore leaves no file behind, the item's file is moved back instead.
#[cfg(any(windows, test))]
fn restore_script(paths: &[PathBuf]) -> String {
    let mut script = String::from(
        r#"$ErrorActionPreference = 'SilentlyContinue'
$bin = (New-Object -ComObject Shell.Application).NameSpace(10)
function Restore-One($dir, $name, $stem, $ext) {
  $best = $null
  $bestWhen = [DateTime]::MinValue
  foreach ($item in $bin.Items()) {
    $from = [string]$item.ExtendedProperty('System.Recycle.DeletedFrom')
    if (-not $from) { $from = [string]$bin.GetDetailsOf($item, 1) }
    if ($from.TrimEnd('\') -ne $dir) { continue }
    $shown = [string]$item.Name
    $real = [IO.Path]::GetExtension([string]$item.Path)
    if (-not ($shown -eq $name -or ($shown -eq $stem -and $real -eq $ext))) { continue }
    $when = $item.ExtendedProperty('System.Recycle.DateDeleted')
    if ($when -isnot [DateTime]) { $when = [DateTime]::MinValue }
    if ($null -eq $best -or $when -gt $bestWhen) { $best = $item; $bestWhen = $when }
  }
  if ($null -eq $best) { return }
  $target = Join-Path -Path $dir -ChildPath $name
  $held = [string]$best.Path
  $best.InvokeVerb('undelete')
  for ($i = 0; $i -lt 10 -and -not (Test-Path -LiteralPath $target); $i++) { Start-Sleep -Milliseconds 100 }
  if (-not (Test-Path -LiteralPath $target) -and (Test-Path -LiteralPath $held)) {
    [IO.File]::Move($held, $target)
  }
}
"#,
    );
    for path in paths {
        let full = std::path::absolute(path).unwrap_or_else(|_| path.clone());
        let dir = full
            .parent()
            .map(|dir| dir.to_string_lossy().trim_end_matches(['\\', '/']).to_string())
            .unwrap_or_default();
        let name = file_text(full.file_name());
        let stem = file_text(full.file_stem());
        let ext = full
            .extension()
            .map(|ext| format!(".{}", ext.to_string_lossy()))
            .unwrap_or_default();
        script.push_str(&format!(
            "Restore-One {} {} {} {}\n",
            quoted(&dir),
            quoted(&name),
            quoted(&stem),
            quoted(&ext)
        ));
    }
    script
}

/// A part of a path (a name, a stem) as text.
#[cfg(any(windows, test))]
fn file_text(part: Option<&std::ffi::OsStr>) -> String {
    part.map(|part| part.to_string_lossy().to_string())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quotes_are_doubled_inside_a_literal() {
        assert_eq!(quoted("plain"), "'plain'");
        assert_eq!(quoted("Don't Stop"), "'Don''t Stop'");
        assert_eq!(quoted("Rock ’n’ Roll"), "'Rock ’’n’’ Roll'");
        assert_eq!(quoted(""), "''");
    }

    #[test]
    fn the_script_names_each_file_by_folder_name_stem_and_extension() {
        let folder = std::env::temp_dir().join("Songs");
        let script = restore_script(&[folder.join("It's Me.mp3")]);
        let line = script.lines().last().unwrap_or_default();
        assert!(line.starts_with("Restore-One '"), "{line}");
        assert!(line.ends_with(" 'It''s Me.mp3' 'It''s Me' '.mp3'"), "{line}");
        assert!(line.contains(&quoted(&folder.to_string_lossy())), "{line}");
    }
}
