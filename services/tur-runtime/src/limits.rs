// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! Hard caps applied while inspecting untrusted input.
//!
//! Every value read from a file is attacker-controlled. These limits bound the
//! memory and I/O one inspection can cost, whatever the file claims.

/// Total bytes one inspection may read from a single file.
pub const MAX_TOTAL_READ: u64 = 32 * 1024 * 1024;

/// Largest single read, and so the largest single allocation.
pub const MAX_SINGLE_READ: usize = 16 * 1024 * 1024;

/// Bytes read from the start of a file to identify it.
pub const HEAD_LEN: usize = 4096;

/// ELF program headers examined. A real program has a few dozen at most.
pub const MAX_PROGRAM_HEADERS: u16 = 512;

/// Longest accepted ELF interpreter path, in bytes.
pub const MAX_INTERPRETER_LEN: u64 = 4096;

/// Bytes of an ELF dynamic section examined.
pub const MAX_DYNAMIC_BYTES: u64 = 16 * 1024;

/// Architectures accepted in a universal Mach-O header.
///
/// Java class files share the universal magic number and store their version
/// where the architecture count lives; the lowest class-file major version is
/// 45, so a count below that cannot be a class file.
pub const MAX_FAT_ARCHS: u32 = 44;

/// Entries accepted in a zip central directory.
pub const MAX_ZIP_ENTRIES: u64 = 100_000;

/// Bytes of zip central directory read.
pub const MAX_CENTRAL_DIRECTORY: u64 = 16 * 1024 * 1024;

/// Nested package names kept from an archive of Android packages.
pub const MAX_LISTED_PACKAGES: usize = 64;

/// Longest archive member name kept in a report, in bytes.
pub const MAX_LISTED_NAME_LEN: usize = 256;

/// Longest shebang line examined, in bytes (the Linux kernel reads 256).
pub const MAX_SHEBANG_LEN: usize = 256;

/// Bytes of `Info.plist` read from a macOS application bundle.
pub const MAX_PLIST_BYTES: u64 = 1024 * 1024;

/// String values kept from an `Info.plist`.
pub const MAX_PLIST_KEYS: usize = 256;

/// Longest `Info.plist` value kept, in bytes.
pub const MAX_PLIST_VALUE_LEN: usize = 512;

/// Directory entries examined under `Contents/MacOS`.
pub const MAX_BUNDLE_DIR_ENTRIES: usize = 256;

/// Largest application profile accepted, in bytes.
pub const MAX_PROFILE_BYTES: usize = 1024 * 1024;

/// Largest prefix metadata file accepted, in bytes.
pub const MAX_METADATA_BYTES: usize = 256 * 1024;

/// Host directories one application may be granted.
pub const MAX_GRANTS: usize = 64;

/// Runtime migrations remembered in a prefix's metadata; older ones are
/// dropped.
pub const MAX_MIGRATIONS: usize = 32;

/// Longest path accepted for a grant, a binary or a runtime directory, in
/// bytes.
pub const MAX_PATH_BYTES: usize = 4096;

/// Arguments passed on to an application in one launch.
pub const MAX_LAUNCH_ARGUMENTS: usize = 1024;

/// Extra environment variables accepted for one launch.
pub const MAX_EXTRA_ENVIRONMENT: usize = 64;

/// Longest value of one environment variable, in bytes.
pub const MAX_ENVIRONMENT_VALUE_BYTES: usize = 8192;

/// Bytes of a runtime's `--version` output examined.
pub const MAX_VERSION_OUTPUT_BYTES: usize = 512;

/// Bytes of a file examined by a verification check.
pub const MAX_CHECK_BYTES: usize = 64 * 1024;

/// Bytes of an application's output kept in the log of one launch.
pub const MAX_LOG_BYTES: usize = 64 * 1024 * 1024;

/// Bytes of a failed command's output kept in the error message.
pub const MAX_ERROR_OUTPUT_BYTES: usize = 2000;

/// Bytes of a time-limited command's output kept in memory, for the
/// operations log. Later output is read and dropped.
pub const MAX_COMMAND_OUTPUT_BYTES: usize = 1024 * 1024;

/// The longest an installer or uninstaller may be given, in seconds.
pub const MAX_INSTALLER_SECONDS: u64 = 24 * 60 * 60;
