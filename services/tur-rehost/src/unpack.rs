// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! Opening a package without running it, layer by layer, into a directory
//! the detector can walk (docs/reports/exp-rehost-1.md, 2.2, step 1).
//!
//! The container is recognised by its content, never by its name. Each
//! layer is opened by a Fedora archive tool inside the tool sandbox
//! ([`crate::sandbox::tool_arguments`]): no network, nothing writable but
//! the output directory. No installer is executed; its container format is
//! read.
//!
//! | Container | Tool |
//! |---|---|
//! | zip, tar (also compressed), a `.nupkg` | `bsdtar` |
//! | 7z, an NSIS installer, a 7z self-extracting program, a disk image (`.dmg`) and its partitions | 7-Zip (`7zz`) |
//! | electron-builder's NSIS installer: `$PLUGINSDIR/app-64.7z` inside it | 7-Zip again |
//! | Inno Setup | `innoextract` |
//! | Windows Installer (`.msi`) | `msiextract` (msitools), then 7-Zip |
//! | Squirrel.Windows `Setup.exe` | the zip at its end is cut out here, then `bsdtar`, then each `.nupkg` |
//! | macOS installer package (`.pkg`, xar) | `bsdtar`; a `pbzx` payload is split here into its xz streams, `xz` decompresses, `bsdtar` reads the cpio |
//!
//! Anything else (a program that carries its payload inside, a `.love`, a
//! `.pck`) is copied unchanged, so that the detector always gets a
//! directory.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::codes::{Code, Failure};
use crate::runner::{Invocation, Runner, find_program};
use crate::sandbox::{RootLayout, tool_arguments, tool_environment};
use crate::sniff::{self, Container};

/// How long one archive tool may run: 15 minutes.
pub const TOOL_SECONDS: u64 = 15 * 60;

/// How many layers deep a package is opened.
pub const MAX_LAYERS: usize = 4;

/// The programs that open containers. Each is optional until a package
/// needs it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tools {
    /// bubblewrap.
    pub bwrap: PathBuf,
    /// libarchive's `bsdtar`.
    pub bsdtar: Option<PathBuf>,
    /// 7-Zip: `7zz` (Fedora's `7zip` package), or `7z`.
    pub sevenzip: Option<PathBuf>,
    /// `innoextract`.
    pub innoextract: Option<PathBuf>,
    /// `msiextract` (msitools).
    pub msiextract: Option<PathBuf>,
    /// `xz`.
    pub xz: Option<PathBuf>,
    /// The host's root layout, for the sandbox.
    pub root: RootLayout,
}

impl Tools {
    /// The tools of this machine; `bwrap` may be given (`--sandbox-program`).
    pub fn find(bwrap: Option<PathBuf>) -> Result<Tools, Failure> {
        let bwrap = bwrap.or_else(|| find_program(&["bwrap"])).ok_or_else(|| {
            Failure::new(
                Code::SandboxMissing,
                "bubblewrap (bwrap) is not installed: no archive is opened outside the sandbox",
            )
        })?;
        Ok(Tools {
            bwrap,
            bsdtar: find_program(&["bsdtar"]),
            sevenzip: find_program(&["7zz", "7z", "7za"]),
            innoextract: find_program(&["innoextract"]),
            msiextract: find_program(&["msiextract"]),
            xz: find_program(&["xz"]),
            root: RootLayout::of_host(),
        })
    }
}

impl Tools {
    /// The tools, without requiring bubblewrap: for a package that is a
    /// directory, which is read where it is and runs nothing. A later step
    /// that needs the sandbox then fails to start it
    /// (`TUR-REHOST-HOST-002`).
    pub fn find_lenient(bwrap: Option<PathBuf>) -> Tools {
        match Tools::find(bwrap) {
            Ok(tools) => tools,
            Err(_) => Tools {
                bwrap: PathBuf::from("/usr/bin/bwrap"),
                bsdtar: find_program(&["bsdtar"]),
                sevenzip: find_program(&["7zz", "7z", "7za"]),
                innoextract: find_program(&["innoextract"]),
                msiextract: find_program(&["msiextract"]),
                xz: find_program(&["xz"]),
                root: RootLayout::of_host(),
            },
        }
    }
}

/// A package opened into a directory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Opened {
    /// The directory the detector walks.
    pub root: PathBuf,
    /// The outermost container (`zip`, `pe`, `dmg`, `directory`, ...).
    pub container: String,
    /// What was opened, outermost first (`nsis`, `7z (app-64.7z)`).
    pub layers: Vec<String>,
    /// Containers that could not be opened, in words.
    pub problems: Vec<String>,
}

/// Opens `package` into `work/package`. A directory (an unpacked
/// application, a `.app` bundle) is used where it is.
pub fn open(
    package: &Path,
    work: &Path,
    tools: &Tools,
    runner: &mut dyn Runner,
) -> Result<Opened, Failure> {
    let metadata = std::fs::symlink_metadata(package).map_err(|error| {
        Failure::new(
            Code::Unreadable,
            format!("cannot read {}: {error}", package.display()),
        )
    })?;
    if metadata.is_dir() {
        return Ok(Opened {
            root: package.to_path_buf(),
            container: "directory".to_owned(),
            layers: Vec::new(),
            problems: Vec::new(),
        });
    }
    if !metadata.is_file() {
        return Err(Failure::new(
            Code::Unreadable,
            format!(
                "{} is not a regular file or a directory",
                package.display()
            ),
        ));
    }
    let dest = work.join("package");
    make_dir(&dest)?;
    let mut opener = Opener {
        tools,
        runner,
        work,
        layers: Vec::new(),
        problems: Vec::new(),
        counter: 0,
    };
    let container = opener.unpack(package, &dest, 0)?;
    Ok(Opened {
        root: dest,
        container,
        layers: opener.layers,
        problems: opener.problems,
    })
}

struct Opener<'a> {
    tools: &'a Tools,
    runner: &'a mut dyn Runner,
    work: &'a Path,
    layers: Vec<String>,
    problems: Vec<String>,
    counter: u32,
}

fn make_dir(path: &Path) -> Result<(), Failure> {
    std::fs::create_dir_all(path).map_err(|error| {
        Failure::new(
            Code::StepFailed,
            format!("cannot create {}: {error}", path.display()),
        )
    })
}

fn utf8(path: &Path) -> Result<&str, Failure> {
    path.to_str().ok_or_else(|| {
        Failure::new(
            Code::InputInvalid,
            format!("the path {} is not valid UTF-8", path.display()),
        )
    })
}

fn is_empty_dir(path: &Path) -> bool {
    match std::fs::read_dir(path) {
        Ok(mut entries) => entries.next().is_none(),
        Err(_) => true,
    }
}

fn missing(tool: &str, package: &str) -> Failure {
    Failure::new(
        Code::ProgramMissing,
        format!("{tool} is needed to open {package} and is not installed"),
    )
}

impl Opener<'_> {
    fn scratch(&mut self, label: &str) -> PathBuf {
        self.counter += 1;
        self.work.join(format!("layer-{}-{label}", self.counter))
    }

    /// Runs `program` with `args` in the tool sandbox, reading `inputs`
    /// and writing `output`. Returns whether it exited with 0.
    fn tool(
        &mut self,
        purpose: &str,
        program: &Path,
        args: &[String],
        inputs: &[&Path],
        output: &Path,
    ) -> Result<bool, Failure> {
        make_dir(output)?;
        let input_texts = inputs
            .iter()
            .map(|path| utf8(path))
            .collect::<Result<Vec<&str>, Failure>>()?;
        let mut arguments = tool_arguments(&self.tools.root, &input_texts, utf8(output)?);
        arguments.push(utf8(program)?.to_owned());
        arguments.extend(args.iter().cloned());
        let invocation = Invocation {
            purpose: purpose.to_owned(),
            program: self.tools.bwrap.clone(),
            args: arguments,
            env: tool_environment(),
            cwd: None,
            timeout_seconds: TOOL_SECONDS,
        };
        let outcome = self.runner.run(&invocation).map_err(|error| {
            Failure::new(
                Code::SandboxMissing,
                format!(
                    "the sandbox ({}) cannot be started: {error}",
                    self.tools.bwrap.display()
                ),
            )
        })?;
        if !outcome.succeeded() {
            self.problems.push(format!(
                "{purpose}: {}",
                if outcome.timed_out {
                    "took too long"
                } else {
                    outcome.last_line()
                }
            ));
        }
        Ok(outcome.succeeded())
    }

    fn bsdtar(&mut self, input: &Path, output: &Path) -> Result<bool, Failure> {
        let program = self
            .tools
            .bsdtar
            .clone()
            .ok_or_else(|| missing("bsdtar (libarchive)", "this archive"))?;
        let args = vec![
            "-x".to_owned(),
            "-f".to_owned(),
            utf8(input)?.to_owned(),
            "-C".to_owned(),
            utf8(output)?.to_owned(),
            "--no-same-owner".to_owned(),
            "--no-same-permissions".to_owned(),
        ];
        let ok = self.tool("bsdtar", &program, &args, &[input], output)?;
        Ok(ok || !is_empty_dir(output))
    }

    fn seven(&mut self, input: &Path, output: &Path) -> Result<bool, Failure> {
        let program = self
            .tools
            .sevenzip
            .clone()
            .ok_or_else(|| missing("7-Zip (7zz)", "this archive"))?;
        let args = vec![
            "x".to_owned(),
            "-y".to_owned(),
            "-bso0".to_owned(),
            "-bsp0".to_owned(),
            format!("-o{}", utf8(output)?),
            "--".to_owned(),
            utf8(input)?.to_owned(),
        ];
        // 7-Zip exits with 1 for warnings (for example an NSIS script it
        // cannot decompile): what it extracted is used.
        self.tool("7-Zip", &program, &args, &[input], output)?;
        Ok(!is_empty_dir(output))
    }

    fn unpack(&mut self, path: &Path, dest: &Path, depth: usize) -> Result<String, Failure> {
        let kind = sniff::sniff(path).map_err(|error| {
            Failure::new(
                Code::Unreadable,
                format!("cannot read {}: {error}", path.display()),
            )
        })?;
        let mut opened = false;
        let mut label = kind.as_str().to_owned();
        match kind {
            Container::Zip => {
                let names = sniff::zip_tail(path).and_then(|tail| sniff::zip_names(path, &tail, 200_000));
                let jar = names.as_ref().is_some_and(|names| {
                    names.iter().any(|name| name == "META-INF/MANIFEST.MF")
                        && names.iter().any(|name| name.ends_with(".class"))
                });
                if jar {
                    // A Java archive is the payload itself.
                    label = "jar".to_owned();
                } else {
                    opened = self.bsdtar(path, dest)?;
                }
            }
            Container::Tar | Container::TarCompressed => opened = self.bsdtar(path, dest)?,
            Container::SevenZip => opened = self.seven(path, dest)?,
            Container::Ole => {
                if let Some(program) = self.tools.msiextract.clone() {
                    let args = vec!["-C".to_owned(), utf8(dest)?.to_owned(), utf8(path)?.to_owned()];
                    opened = self.tool("msiextract", &program, &args, &[path], dest)?
                        && !is_empty_dir(dest);
                }
                if !opened {
                    opened = self.seven(path, dest)?;
                }
                label = "msi".to_owned();
            }
            Container::Xar => {
                opened = self.pkg(path, dest)?;
                label = "pkg".to_owned();
            }
            Container::Dmg => opened = self.dmg(path, dest)?,
            Container::Pe => {
                if let Some(installer) = self.pe(path, dest)? {
                    opened = true;
                    label = installer;
                }
            }
            Container::Ar
            | Container::Pck
            | Container::Elf
            | Container::AppImage
            | Container::MachO
            | Container::Other => {}
        }
        if opened {
            self.layers.push(label.clone());
            // electron-builder's NSIS installers and 7z archives carry the
            // application in an archive of its own.
            if matches!(label.as_str(), "7z" | "nsis" | "7z-sfx") {
                self.nested_app_archive(dest)?;
            }
        } else {
            // Not a container this code opens: the file itself goes to the
            // detector, which reads payloads carried inside programs.
            let name = path.file_name().map_or_else(
                || std::ffi::OsString::from("package"),
                std::ffi::OsStr::to_os_string,
            );
            make_dir(dest)?;
            std::fs::copy(path, dest.join(name)).map_err(|error| {
                Failure::new(
                    Code::StepFailed,
                    format!("cannot copy {}: {error}", path.display()),
                )
            })?;
            if matches!(
                kind,
                Container::Zip | Container::Tar | Container::TarCompressed | Container::SevenZip | Container::Ole | Container::Xar | Container::Dmg
            ) && label != "jar"
            {
                self.problems
                    .push(format!("the {} container could not be opened", kind.as_str()));
            }
            self.layers.push(format!("file ({label})"));
            return Ok(label);
        }
        // A lone archive inside an archive (a .dmg in a zip, a tar in a gz).
        if depth + 1 < MAX_LAYERS {
            let entries: Vec<PathBuf> = std::fs::read_dir(dest)
                .map(|listing| {
                    listing
                        .filter_map(Result::ok)
                        .filter(|entry| {
                            let name = entry.file_name();
                            let name = name.to_string_lossy();
                            !name.starts_with("__MACOSX") && !name.starts_with('.')
                        })
                        .map(|entry| entry.path())
                        .collect()
                })
                .unwrap_or_default();
            if let [inner] = entries.as_slice()
                && std::fs::symlink_metadata(inner).is_ok_and(|m| m.is_file())
            {
                let inner_kind = sniff::sniff(inner).unwrap_or(Container::Other);
                if matches!(
                    inner_kind,
                    Container::Zip | Container::Tar | Container::TarCompressed | Container::SevenZip | Container::Dmg | Container::Xar | Container::Ole
                ) {
                    let moved = self.scratch("inner");
                    std::fs::rename(inner, &moved).map_err(|error| {
                        Failure::new(Code::StepFailed, format!("cannot move {}: {error}", inner.display()))
                    })?;
                    let inner_label = self.unpack(&moved, dest, depth + 1)?;
                    let _ = std::fs::remove_file(&moved);
                    return Ok(format!("{label}+{inner_label}"));
                }
            }
        }
        Ok(label)
    }

    /// electron-builder's NSIS installers carry the application as
    /// `$PLUGINSDIR/app-64.7z` (and app-32.7z, app-arm64.7z).
    fn nested_app_archive(&mut self, dest: &Path) -> Result<(), Failure> {
        let Some((directory, archives)) = find_app_archives(dest, 0) else {
            return Ok(());
        };
        let wanted = if archives.iter().any(|name| name == "app-64.7z") {
            "app-64.7z".to_owned()
        } else {
            archives[0].clone()
        };
        if self.seven(&directory.join(&wanted), &dest.join("app"))? {
            self.layers.push(format!("7z ({wanted})"));
        }
        for name in &archives {
            let _ = std::fs::remove_file(directory.join(name));
        }
        Ok(())
    }

    fn dmg(&mut self, path: &Path, dest: &Path) -> Result<bool, Failure> {
        if !self.seven(path, dest)? {
            return Ok(false);
        }
        // 7-Zip may give the partitions as image files: open those too.
        for _ in 0..3 {
            let images: Vec<PathBuf> = std::fs::read_dir(dest)
                .map(|listing| {
                    listing
                        .filter_map(Result::ok)
                        .map(|entry| entry.path())
                        .filter(|path| {
                            let lower = path.to_string_lossy().to_ascii_lowercase();
                            [".hfs", ".hfsx", ".apfs", ".img"]
                                .iter()
                                .any(|suffix| lower.ends_with(suffix))
                                && std::fs::symlink_metadata(path).is_ok_and(|m| m.is_file())
                        })
                        .collect()
                })
                .unwrap_or_default();
            if images.is_empty() {
                break;
            }
            for image in images {
                let mut inner = image.clone().into_os_string();
                inner.push(".d");
                let inner = PathBuf::from(inner);
                if self.seven(&image, &inner)? {
                    let extension = image
                        .extension()
                        .map(|e| e.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    self.layers.push(extension);
                }
                let _ = std::fs::remove_file(&image);
            }
        }
        Ok(!is_empty_dir(dest))
    }

    fn pkg(&mut self, path: &Path, dest: &Path) -> Result<bool, Failure> {
        let holder = self.scratch("xar");
        if !self.bsdtar(path, &holder)? {
            return Ok(false);
        }
        let mut payloads = Vec::new();
        collect_named(&holder, "Payload", 0, &mut payloads);
        let mut count = 0;
        for payload in payloads {
            let relative = payload
                .parent()
                .and_then(|parent| parent.strip_prefix(&holder).ok())
                .map(|relative| relative.to_string_lossy().replace('/', "_"))
                .unwrap_or_default();
            let target = dest.join(if relative.is_empty() { "payload".to_owned() } else { relative });
            let mut head = [0u8; 4];
            let is_pbzx = File::open(&payload)
                .and_then(|mut file| file.read_exact(&mut head))
                .is_ok()
                && &head == b"pbzx";
            let archive = if is_pbzx {
                let streams = self.scratch("pbzx.xz");
                if let Err(problem) = pbzx_to_xz(&payload, &streams) {
                    self.problems.push(format!("pbzx payload: {problem}"));
                    continue;
                }
                let Some(xz) = self.tools.xz.clone() else {
                    return Err(missing("xz", "this installer package"));
                };
                let out_dir = self.scratch("cpio");
                let cpio = out_dir.join("payload.cpio");
                // xz writes to a file in the output directory: through a
                // shell would need a shell; xz -k -d on a copy does it.
                let copy = out_dir.join("payload.cpio.xz");
                make_dir(&out_dir)?;
                std::fs::rename(&streams, &copy).map_err(|error| {
                    Failure::new(Code::StepFailed, format!("cannot move {}: {error}", streams.display()))
                })?;
                let args = vec!["-d".to_owned(), "--".to_owned(), utf8(&copy)?.to_owned()];
                if !self.tool("xz", &xz, &args, &[], &out_dir)? {
                    continue;
                }
                self.layers.push("pbzx".to_owned());
                cpio
            } else {
                payload.clone()
            };
            if self.bsdtar(&archive, &target)? {
                count += 1;
            }
        }
        let _ = std::fs::remove_dir_all(&holder);
        Ok(count > 0)
    }

    /// Opens a Windows program that is an installer. Returns the kind, or
    /// `None` when the program is not a container this code opens.
    fn pe(&mut self, path: &Path, dest: &Path) -> Result<Option<String>, Failure> {
        let Some(info) = sniff::pe_info(path) else {
            return Ok(None);
        };
        let head = read_range(path, info.overlay_offset, info.overlay_size.min(1 << 16));
        let nsis = head.len() >= 12 && find(&head[..head.len().min(4096)], b"NullsoftInst").is_some();
        if nsis {
            if !self.seven(path, dest)? {
                return Ok(None);
            }
            return Ok(Some("nsis".to_owned()));
        }
        if crate::scan::contains(path, b"Inno Setup Setup Data") || crate::scan::contains(path, b"rDlPtS") {
            let program = self
                .tools
                .innoextract
                .clone()
                .ok_or_else(|| missing("innoextract", "this Inno Setup installer"))?;
            let args = vec![
                "-s".to_owned(),
                "-d".to_owned(),
                utf8(dest)?.to_owned(),
                "--".to_owned(),
                utf8(path)?.to_owned(),
            ];
            if self.tool("innoextract", &program, &args, &[path], dest)? && !is_empty_dir(dest) {
                return Ok(Some("inno-setup".to_owned()));
            }
            return Ok(None);
        }
        if head.len() >= 6 && find(&head[..head.len().min(8192)], b"7z\xbc\xaf\x27\x1c").is_some() {
            if self.seven(path, dest)? {
                return Ok(Some("7z-sfx".to_owned()));
            }
            return Ok(None);
        }
        if let Some(tail) = sniff::zip_tail(path) {
            // Squirrel.Windows' Setup.exe ends in a zip holding the
            // application as a NuGet package; anything else that ends in a
            // zip (launch4j, LÖVE) is read in place by the detector.
            let names = sniff::zip_names(path, &tail, 200_000).unwrap_or_default();
            let packages: Vec<String> = names
                .iter()
                .filter(|name| name.ends_with(".nupkg") && !name.contains('/'))
                .cloned()
                .collect();
            if packages.is_empty() {
                return Ok(None);
            }
            let holder_dir = self.scratch("squirrel");
            make_dir(&holder_dir)?;
            let holder = holder_dir.join("setup.zip");
            copy_range(path, tail.start, tail.end, &holder).map_err(|error| {
                Failure::new(Code::StepFailed, format!("cannot cut the archive out of {}: {error}", path.display()))
            })?;
            if !self.bsdtar(&holder, dest)? {
                return Ok(None);
            }
            let _ = std::fs::remove_dir_all(&holder_dir);
            for name in packages {
                let package = dest.join(&name);
                if package.is_file() {
                    if self.bsdtar(&package, &dest.join("nupkg"))? {
                        self.layers.push("nupkg".to_owned());
                    }
                    let _ = std::fs::remove_file(&package);
                }
            }
            return Ok(Some("squirrel-setup".to_owned()));
        }
        Ok(None)
    }
}

/// The directory holding `app-*.7z` archives, and their names, sorted.
fn find_app_archives(directory: &Path, depth: usize) -> Option<(PathBuf, Vec<String>)> {
    if depth > 6 {
        return None;
    }
    let listing = std::fs::read_dir(directory).ok()?;
    let mut archives = Vec::new();
    let mut directories = Vec::new();
    for entry in listing.filter_map(Result::ok) {
        let Ok(metadata) = std::fs::symlink_metadata(entry.path()) else {
            continue;
        };
        let name = entry.file_name().to_string_lossy().into_owned();
        if metadata.is_file() && name.starts_with("app-") && name.ends_with(".7z") {
            archives.push(name);
        } else if metadata.is_dir() {
            directories.push(entry.path());
        }
    }
    if !archives.is_empty() {
        archives.sort();
        return Some((directory.to_path_buf(), archives));
    }
    directories.sort();
    directories
        .iter()
        .find_map(|child| find_app_archives(child, depth + 1))
}

fn collect_named(directory: &Path, wanted: &str, depth: usize, out: &mut Vec<PathBuf>) {
    if depth > 8 || out.len() > 64 {
        return;
    }
    let Ok(listing) = std::fs::read_dir(directory) else {
        return;
    };
    let mut entries: Vec<PathBuf> = listing.filter_map(Result::ok).map(|entry| entry.path()).collect();
    entries.sort();
    for path in entries {
        let Ok(metadata) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        if metadata.is_dir() {
            collect_named(&path, wanted, depth + 1, out);
        } else if metadata.is_file() && path.file_name().is_some_and(|name| name == wanted) {
            out.push(path);
        }
    }
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    haystack.windows(needle.len()).position(|window| window == needle)
}

fn read_range(path: &Path, offset: u64, length: u64) -> Vec<u8> {
    let Ok(mut file) = File::open(path) else {
        return Vec::new();
    };
    if file.seek(SeekFrom::Start(offset)).is_err() {
        return Vec::new();
    }
    let mut out = Vec::new();
    let _ = file.take(length).read_to_end(&mut out);
    out
}

/// Copies bytes `start..end` of `path` into a new file `to`.
pub fn copy_range(path: &Path, start: u64, end: u64, to: &Path) -> std::io::Result<()> {
    let mut source = File::open(path)?;
    source.seek(SeekFrom::Start(start))?;
    let mut out = File::create(to)?;
    let copied = std::io::copy(&mut source.take(end.saturating_sub(start)), &mut out)?;
    if copied != end.saturating_sub(start) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::UnexpectedEof,
            "the file ended early",
        ));
    }
    out.flush()
}

/// Splits Apple's `pbzx` stream (a `.pkg` payload) into the xz streams it
/// is made of, written one after the other into `to`, which `xz -d` then
/// decompresses as one file. The format: `pbzx`, an 8-byte flags field,
/// then chunks of an 8-byte uncompressed size, an 8-byte stored size and the
/// stored bytes, all big-endian. A chunk stored without compression (its
/// stored size equals its size and it does not start with xz's magic) is
/// refused: it cannot join the xz streams.
pub fn pbzx_to_xz(from: &Path, to: &Path) -> std::io::Result<()> {
    const XZ_MAGIC: &[u8; 6] = b"\xfd7zXZ\0";
    const MAX_CHUNK: u64 = 64 << 20;
    let mut source = File::open(from)?;
    let mut header = [0u8; 12];
    source.read_exact(&mut header)?;
    if &header[..4] != b"pbzx" {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "not a pbzx stream"));
    }
    let mut out = File::create(to)?;
    let mut sizes = [0u8; 16];
    let mut chunks = 0u32;
    loop {
        match source.read_exact(&mut sizes) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(error) => return Err(error),
        }
        let stored = u64::from_be_bytes([
            sizes[8], sizes[9], sizes[10], sizes[11], sizes[12], sizes[13], sizes[14], sizes[15],
        ]);
        if stored == 0 || stored > MAX_CHUNK {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "a pbzx chunk has an impossible size"));
        }
        let mut chunk = Vec::new();
        (&mut source).take(stored).read_to_end(&mut chunk)?;
        if chunk.len() as u64 != stored {
            return Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "a pbzx chunk is truncated"));
        }
        if !chunk.starts_with(XZ_MAGIC) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "a pbzx chunk is stored without compression; not supported",
            ));
        }
        out.write_all(&chunk)?;
        chunks += 1;
        if chunks > 1_000_000 {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "too many pbzx chunks"));
        }
    }
    out.flush()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner::{Outcome, RecordingRunner};
    use std::sync::atomic::{AtomicU32, Ordering};

    fn scratch(label: &str) -> PathBuf {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let path = std::env::temp_dir().join(format!(
            "tur-rehost-unpack-{}-{}-{label}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    fn tools() -> Tools {
        Tools {
            bwrap: PathBuf::from("/usr/bin/bwrap"),
            bsdtar: Some(PathBuf::from("/usr/bin/bsdtar")),
            sevenzip: Some(PathBuf::from("/usr/bin/7zz")),
            innoextract: Some(PathBuf::from("/usr/bin/innoextract")),
            msiextract: Some(PathBuf::from("/usr/bin/msiextract")),
            xz: Some(PathBuf::from("/usr/bin/xz")),
            root: RootLayout::merged(),
        }
    }

    #[test]
    fn a_directory_is_used_where_it_is_and_nothing_runs() {
        let directory = scratch("dir");
        let mut runner = RecordingRunner::succeeding();
        let opened = open(&directory, &directory.join("work"), &tools(), &mut runner).unwrap();
        assert_eq!(opened.root, directory);
        assert_eq!(opened.container, "directory");
        assert!(runner.calls.is_empty());
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn a_zip_is_opened_by_bsdtar_inside_the_sandbox() {
        let directory = scratch("zip");
        let package = directory.join("app.zip");
        // An empty zip archive: only the end of central directory.
        let mut bytes = b"PK\x05\x06".to_vec();
        bytes.extend_from_slice(&[0u8; 18]);
        std::fs::write(&package, &bytes).unwrap();
        let mut runner = RecordingRunner::new(|invocation| {
            // Stands in for bsdtar: puts one file into the output.
            let output = invocation.args[invocation.args.iter().position(|a| a == "-C").unwrap() + 1].clone();
            std::fs::write(Path::new(&output).join("Foo.exe"), b"MZ").unwrap();
            Outcome {
                status: Some(0),
                timed_out: false,
                output: String::new(),
            }
        });
        let work = directory.join("work");
        let opened = open(&package, &work, &tools(), &mut runner).unwrap();
        assert_eq!(opened.layers, vec!["zip".to_owned()]);
        assert!(opened.root.join("Foo.exe").is_file());
        let call = &runner.calls[0];
        assert_eq!(call.program, PathBuf::from("/usr/bin/bwrap"));
        assert!(call.args.contains(&"--unshare-all".to_owned()));
        let separator = call.args.iter().position(|a| a == "--").unwrap();
        assert_eq!(call.args[separator + 1], "/usr/bin/bsdtar");
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn pbzx_chunks_become_one_xz_file_and_stored_chunks_are_refused() {
        let directory = scratch("pbzx");
        let source = directory.join("Payload");
        let mut bytes = b"pbzx".to_vec();
        bytes.extend_from_slice(&[0u8; 8]);
        for chunk in [&b"\xfd7zXZ\0first"[..], &b"\xfd7zXZ\0second"[..]] {
            bytes.extend_from_slice(&(1000u64).to_be_bytes());
            bytes.extend_from_slice(&(chunk.len() as u64).to_be_bytes());
            bytes.extend_from_slice(chunk);
        }
        std::fs::write(&source, &bytes).unwrap();
        let out = directory.join("out.xz");
        pbzx_to_xz(&source, &out).unwrap();
        assert_eq!(std::fs::read(&out).unwrap(), b"\xfd7zXZ\0first\xfd7zXZ\0second");

        let mut stored = b"pbzx".to_vec();
        stored.extend_from_slice(&[0u8; 8]);
        stored.extend_from_slice(&(5u64).to_be_bytes());
        stored.extend_from_slice(&(5u64).to_be_bytes());
        stored.extend_from_slice(b"plain");
        std::fs::write(&source, &stored).unwrap();
        assert!(pbzx_to_xz(&source, &out).is_err());
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn the_nsis_application_archive_is_found_and_the_64_bit_one_preferred() {
        let directory = scratch("nsis");
        let plugins = directory.join("$PLUGINSDIR");
        std::fs::create_dir_all(&plugins).unwrap();
        for name in ["app-32.7z", "app-64.7z", "app-arm64.7z"] {
            std::fs::write(plugins.join(name), b"7z").unwrap();
        }
        let (found, archives) = find_app_archives(&directory, 0).unwrap();
        assert_eq!(found, plugins);
        assert!(archives.contains(&"app-64.7z".to_owned()));
        let _ = std::fs::remove_dir_all(&directory);
    }
}
