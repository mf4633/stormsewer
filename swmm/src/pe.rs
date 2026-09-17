// SPDX-License-Identifier: GPL-3.0-or-later

//! Reads the machine type out of a PE (Windows executable) header.
//!
//! This exists because architecture is the whole reason engines run
//! out-of-process: EPA's stock Windows build of SWMM 5.2.4 is 32-bit, and a
//! 64-bit host cannot load it in-process. Showing the user which architecture
//! each registered engine is turns that from a mystery into a label.
//!
//! Only the few bytes that matter are read: `e_lfanew` at 0x3C points at the
//! PE signature, and the machine word is the two bytes just past it.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Arch {
    X86,
    X64,
    Arm64,
    /// A PE file with a machine type we do not have a name for.
    Other(u16),
    /// Not a PE file at all — an ELF or Mach-O binary on Linux or macOS.
    NotPe,
}

impl Arch {
    pub fn label(&self) -> String {
        match self {
            Self::X86 => "32-bit (x86)".into(),
            Self::X64 => "64-bit (x64)".into(),
            Self::Arm64 => "64-bit (ARM64)".into(),
            Self::Other(m) => format!("machine 0x{m:04x}"),
            Self::NotPe => "native".into(),
        }
    }

    /// True when this engine cannot be loaded in-process by a 64-bit host, and
    /// so must be run as a subprocess. Currently that is every 32-bit build.
    pub fn requires_subprocess_from_x64(&self) -> bool {
        matches!(self, Self::X86)
    }
}

/// Read the machine type of an executable. Anything that is not a well-formed
/// PE file reports [`Arch::NotPe`] rather than erroring: on Linux and macOS
/// that is the normal answer.
pub fn arch_of(path: &Path) -> std::io::Result<Arch> {
    let mut f = File::open(path)?;
    let mut mz = [0u8; 2];
    if f.read_exact(&mut mz).is_err() || &mz != b"MZ" {
        return Ok(Arch::NotPe);
    }

    f.seek(SeekFrom::Start(0x3C))?;
    let mut lfanew = [0u8; 4];
    if f.read_exact(&mut lfanew).is_err() {
        return Ok(Arch::NotPe);
    }
    let offset = u32::from_le_bytes(lfanew) as u64;

    if f.seek(SeekFrom::Start(offset)).is_err() {
        return Ok(Arch::NotPe);
    }
    let mut sig_and_machine = [0u8; 6];
    if f.read_exact(&mut sig_and_machine).is_err() || &sig_and_machine[..4] != b"PE\0\0" {
        return Ok(Arch::NotPe);
    }

    let machine = u16::from_le_bytes([sig_and_machine[4], sig_and_machine[5]]);
    Ok(match machine {
        0x014c => Arch::X86,
        0x8664 => Arch::X64,
        0xaa64 => Arch::Arm64,
        other => Arch::Other(other),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// Assemble the smallest byte sequence that carries a machine type, so the
    /// parser can be checked on every platform without a real executable.
    fn fake_pe(dir: &Path, name: &str, machine: u16) -> std::path::PathBuf {
        let mut bytes = vec![0u8; 0x80];
        bytes[0] = b'M';
        bytes[1] = b'Z';
        bytes[0x3C..0x40].copy_from_slice(&0x40u32.to_le_bytes());
        bytes[0x40..0x44].copy_from_slice(b"PE\0\0");
        bytes[0x44..0x46].copy_from_slice(&machine.to_le_bytes());
        let path = dir.join(name);
        File::create(&path).unwrap().write_all(&bytes).unwrap();
        path
    }

    fn scratch() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join("stormsewer-swmm-tests");
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn reads_machine_types() {
        let dir = scratch();
        assert_eq!(arch_of(&fake_pe(&dir, "x86.exe", 0x014c)).unwrap(), Arch::X86);
        assert_eq!(arch_of(&fake_pe(&dir, "x64.exe", 0x8664)).unwrap(), Arch::X64);
        assert_eq!(arch_of(&fake_pe(&dir, "arm.exe", 0xaa64)).unwrap(), Arch::Arm64);
        assert_eq!(
            arch_of(&fake_pe(&dir, "odd.exe", 0x1234)).unwrap(),
            Arch::Other(0x1234)
        );
    }

    /// The 32-bit case is the one that forces subprocess execution.
    #[test]
    fn only_x86_forces_subprocess() {
        assert!(Arch::X86.requires_subprocess_from_x64());
        assert!(!Arch::X64.requires_subprocess_from_x64());
        assert!(!Arch::NotPe.requires_subprocess_from_x64());
    }

    #[test]
    fn non_pe_files_are_not_an_error() {
        let dir = scratch();
        let elf = dir.join("not-a-pe");
        std::fs::write(&elf, b"\x7fELF and then some").unwrap();
        assert_eq!(arch_of(&elf).unwrap(), Arch::NotPe);

        let empty = dir.join("empty-file");
        std::fs::write(&empty, b"").unwrap();
        assert_eq!(arch_of(&empty).unwrap(), Arch::NotPe);

        // Truncated right after "MZ": no e_lfanew to read.
        let stub = dir.join("stub.exe");
        std::fs::write(&stub, b"MZ").unwrap();
        assert_eq!(arch_of(&stub).unwrap(), Arch::NotPe);
    }
}
