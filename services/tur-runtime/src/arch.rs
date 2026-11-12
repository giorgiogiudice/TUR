// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! Processor architectures and byte order, shared by every format.

use std::fmt;

use serde::Serialize;

/// A processor architecture, named the way Linux names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
pub enum Arch {
    /// 64-bit x86 (AMD64, Intel 64).
    #[serde(rename = "x86_64")]
    X86_64,
    /// 32-bit x86.
    #[serde(rename = "i386")]
    I386,
    /// 64-bit ARM.
    #[serde(rename = "aarch64")]
    Aarch64,
    /// 32-bit ARM.
    #[serde(rename = "arm")]
    Arm,
    /// 64-bit RISC-V.
    #[serde(rename = "riscv64")]
    Riscv64,
    /// 32-bit RISC-V.
    #[serde(rename = "riscv32")]
    Riscv32,
    /// 64-bit PowerPC.
    #[serde(rename = "ppc64")]
    Ppc64,
    /// 32-bit PowerPC.
    #[serde(rename = "ppc")]
    Ppc,
    /// IBM Z, 64-bit.
    #[serde(rename = "s390x")]
    S390x,
    /// 32-bit MIPS.
    #[serde(rename = "mips")]
    Mips,
    /// 64-bit MIPS.
    #[serde(rename = "mips64")]
    Mips64,
    /// 64-bit LoongArch.
    #[serde(rename = "loongarch64")]
    Loongarch64,
    /// SPARC, any width.
    #[serde(rename = "sparc")]
    Sparc,
    /// Intel Itanium.
    #[serde(rename = "ia64")]
    Ia64,
    /// An architecture this crate does not name. The raw machine number is
    /// reported next to it.
    #[serde(rename = "other")]
    Other,
}

impl Arch {
    /// The architecture's name, for example `x86_64`.
    pub const fn as_str(self) -> &'static str {
        match self {
            Arch::X86_64 => "x86_64",
            Arch::I386 => "i386",
            Arch::Aarch64 => "aarch64",
            Arch::Arm => "arm",
            Arch::Riscv64 => "riscv64",
            Arch::Riscv32 => "riscv32",
            Arch::Ppc64 => "ppc64",
            Arch::Ppc => "ppc",
            Arch::S390x => "s390x",
            Arch::Mips => "mips",
            Arch::Mips64 => "mips64",
            Arch::Loongarch64 => "loongarch64",
            Arch::Sparc => "sparc",
            Arch::Ia64 => "ia64",
            Arch::Other => "other",
        }
    }

    /// Parses an architecture name, accepting common aliases such as `amd64`
    /// and `arm64`. Returns `None` for a name that is not recognised.
    pub fn from_name(name: &str) -> Option<Arch> {
        Some(match name.to_ascii_lowercase().as_str() {
            "x86_64" | "x86-64" | "amd64" | "x64" => Arch::X86_64,
            "i386" | "i486" | "i586" | "i686" | "x86" => Arch::I386,
            "aarch64" | "arm64" => Arch::Aarch64,
            "arm" | "armv7" | "armhf" => Arch::Arm,
            "riscv64" => Arch::Riscv64,
            "riscv32" => Arch::Riscv32,
            "ppc64" | "ppc64le" | "powerpc64" => Arch::Ppc64,
            "ppc" | "powerpc" => Arch::Ppc,
            "s390x" => Arch::S390x,
            "mips" => Arch::Mips,
            "mips64" => Arch::Mips64,
            "loongarch64" => Arch::Loongarch64,
            "sparc" | "sparc64" => Arch::Sparc,
            "ia64" => Arch::Ia64,
            _ => return None,
        })
    }
}

impl fmt::Display for Arch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Byte order of multi-byte fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Endianness {
    /// Least significant byte first.
    Little,
    /// Most significant byte first.
    Big,
}

impl Endianness {
    /// A label for reports: `little-endian` or `big-endian`.
    pub const fn label(self) -> &'static str {
        match self {
            Endianness::Little => "little-endian",
            Endianness::Big => "big-endian",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_round_trip() {
        for arch in [
            Arch::X86_64,
            Arch::I386,
            Arch::Aarch64,
            Arch::Arm,
            Arch::Riscv64,
            Arch::Riscv32,
            Arch::Ppc64,
            Arch::Ppc,
            Arch::S390x,
            Arch::Mips,
            Arch::Mips64,
            Arch::Loongarch64,
            Arch::Sparc,
            Arch::Ia64,
        ] {
            assert_eq!(Arch::from_name(arch.as_str()), Some(arch));
            let json = serde_json::to_value(arch).unwrap();
            assert_eq!(json, serde_json::Value::String(arch.as_str().to_owned()));
        }
    }

    #[test]
    fn aliases_and_unknown_names() {
        assert_eq!(Arch::from_name("AMD64"), Some(Arch::X86_64));
        assert_eq!(Arch::from_name("arm64"), Some(Arch::Aarch64));
        assert_eq!(Arch::from_name("i686"), Some(Arch::I386));
        assert_eq!(Arch::from_name("other"), None);
        assert_eq!(Arch::from_name("pdp11"), None);
        assert_eq!(Arch::from_name(""), None);
    }
}
