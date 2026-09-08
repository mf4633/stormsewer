// SPDX-License-Identifier: GPL-3.0-or-later

//! Software OpenGL fallback for Windows machines with no usable GPU driver.
//!
//! Linux and macOS supply a software rasteriser (Mesa llvmpipe) when there is
//! no GPU; Windows does not, and a machine with no display driver — a bare
//! VM, Microsoft's winget validation sandbox — offers only OpenGL 1.1, below
//! what egui needs. The Windows installer therefore ships Mesa's llvmpipe
//! build (`opengl32.dll` + `libgallium_wgl.dll`) in a `mesa` folder beside the
//! executable, **together with a second copy of the executable**, and this
//! module runs that copy only after the hardware renderers have failed.
//!
//! Why a copy of the executable: `StormSewer.exe` imports `opengl32.dll`
//! statically (through glutin's WGL bindings), so the system copy is mapped
//! before `main` runs and nothing done later — `SetDllDirectory`, a preload by
//! absolute path — can replace it: Windows resolves any further load of that
//! module name to the copy already in memory. The one place the loader looks
//! before System32 for a statically imported, non-KnownDLL module is the
//! application directory. So the fallback process lives in `mesa\`, where
//! Mesa's `opengl32.dll` *is* the application-directory copy.

use std::path::{Path, PathBuf};

/// Set on the fallback process (and honoured on the main one as "skip the
/// GPU and go straight to the software build").
pub const ENV: &str = "STORMSEWER_SOFTWARE_GL";

const DLLS: [&str; 2] = ["opengl32.dll", "libgallium_wgl.dll"];

fn exe_name() -> Option<std::ffi::OsString> {
    std::env::current_exe()
        .ok()?
        .file_name()
        .map(|n| n.to_os_string())
}

/// The bundled Mesa folder next to this executable, if it holds the two Mesa
/// DLLs and a copy of the executable to run them with.
pub fn bundled_dir() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?.join("mesa");
    let complete = DLLS.iter().all(|d| dir.join(d).is_file()) && dir.join(exe_name()?).is_file();
    complete.then_some(dir)
}

/// True when this process *is* the fallback copy (its own directory holds
/// the Mesa DLLs), whether launched by [`reexec`] or by hand.
pub fn running_from_bundle() -> bool {
    let Some(dir) = std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(Path::to_path_buf))
    else {
        return false;
    };
    DLLS.iter().all(|d| dir.join(d).is_file())
}

/// `STORMSEWER_SOFTWARE_GL=1`: the user or a test asked for software GL.
pub fn requested() -> bool {
    std::env::var(ENV).map(|v| v == "1").unwrap_or(false)
}

/// Full path of the `opengl32.dll` this process actually loaded, so the
/// self-test can prove the bundled one is in use.
#[cfg(windows)]
pub fn loaded_opengl32() -> Option<PathBuf> {
    use std::os::windows::ffi::OsStringExt;
    use windows_sys::Win32::System::LibraryLoader::{GetModuleFileNameW, GetModuleHandleW};
    let name: Vec<u16> = "opengl32.dll"
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    // SAFETY: NUL-terminated name; the buffer is sized and the length checked.
    unsafe {
        let h = GetModuleHandleW(name.as_ptr());
        if h.is_null() {
            return None;
        }
        let mut buf = [0u16; 1024];
        let n = GetModuleFileNameW(h, buf.as_mut_ptr(), buf.len() as u32) as usize;
        (n > 0 && n < buf.len()).then(|| PathBuf::from(std::ffi::OsString::from_wide(&buf[..n])))
    }
}

#[cfg(not(windows))]
pub fn loaded_opengl32() -> Option<PathBuf> {
    None
}

/// Prepare the fallback process: pin the Gallium driver and confirm the
/// application-directory `opengl32.dll` is the one that got mapped.
pub fn activate() -> Result<PathBuf, String> {
    // llvmpipe is the only Gallium driver shipped; say so explicitly so a
    // stray environment cannot pick a different one.
    std::env::set_var("GALLIUM_DRIVER", "llvmpipe");
    let dir = std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(Path::to_path_buf))
        .ok_or("cannot locate the executable")?;
    match loaded_opengl32() {
        Some(p) if p.parent() == Some(dir.as_path()) => Ok(p),
        Some(p) => Err(format!(
            "the loader mapped {} instead of the bundled Mesa in {}",
            p.display(),
            dir.display()
        )),
        None => Err("opengl32.dll is not loaded".into()),
    }
}

/// Run the bundled copy with the same arguments and [`ENV`] set, wait for
/// it, and return its exit status. `None` if there is no bundle or it could
/// not be spawned.
pub fn reexec(args: &[String]) -> Option<std::process::ExitStatus> {
    let dir = bundled_dir()?;
    std::process::Command::new(dir.join(exe_name()?))
        .args(args)
        .env(ENV, "1")
        .status()
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_checkout_has_no_bundle() {
        assert!(bundled_dir().is_none());
        assert!(!running_from_bundle());
    }

    #[test]
    fn requested_reads_the_env() {
        std::env::remove_var(ENV);
        assert!(!requested());
        std::env::set_var(ENV, "1");
        assert!(requested());
        std::env::remove_var(ENV);
    }
}
