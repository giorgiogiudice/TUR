// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! Building a native Node module from its npm source on the person's
//! computer, for one Electron version: the `source-build` tier.
//!
//! The experiment built 53 of its 138 foreign modules this way, 1 to 43 s
//! each (docs/reports/exp-rehost-1.md, section 4). Where it happens is a
//! decision (docs/architecture/rehost.md, section 6): on the person's
//! computer, in a container of Fedora's own toolchain, **without network**,
//! from inputs TUR's index pins, and only when the person allowed it.
//!
//! 1. **The build environment**, once: a container image built from
//!    Fedora 45's base image, pinned by digest, with Fedora's compiler,
//!    Node.js and npm (whose bundled node-gyp does the build) and the
//!    headers the experiment's modules needed (X11, xkbfile, libsecret,
//!    fontconfig). This is the one step that uses the network for a build:
//!    the base image from Fedora's registry and the packages from Fedora's
//!    repositories, both verified by Fedora's own signatures.
//! 2. **The inputs**, fetched and checked against the index on the host:
//!    the module's npm tarball, the build-time packages it needs
//!    (`node-addon-api`, `nan`, at pinned versions) and Electron's headers.
//!    They are unpacked by `bsdtar` in the tool sandbox.
//! 3. **The build**: `podman run --network=none --read-only --cap-drop=all`
//!    with the source directory the only thing writable. The module's
//!    `binding.gyp` can run commands; they run there, with nothing to reach.
//! 4. **The result**: only ELF x86-64 `.node` files from `build/Release` are
//!    taken; everything else is deleted with the directory.

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::codes::{Code, Failure};
use crate::runner::{Invocation, Runner};

/// Fedora 45's base image, by digest (tag 45 on 13 November 2026), the one
/// `.github/workflows/exp-rehost.yml` and `windows-drivers.yml` use.
pub const BASE_IMAGE: &str = "registry.fedoraproject.org/fedora:45@sha256:fa773d10382c082ff2cccaf7982a5c162f11f296f91274b994357f2d3d473770";

/// The image the build environment is kept as. The suffix changes when
/// [`BUILD_PACKAGES`] or [`containerfile`] change.
pub const BUILD_IMAGE: &str = "localhost/tur-rehost-build:fedora45-1";

/// Fedora's packages in the build environment. `nodejs-npm` carries
/// node-gyp (`/usr/lib/node_modules*/npm/node_modules/node-gyp`).
pub const BUILD_PACKAGES: [&str; 9] = [
    "gcc-c++",
    "make",
    "python3",
    "nodejs",
    "nodejs-npm",
    "libX11-devel",
    "libxkbfile-devel",
    "libsecret-devel",
    "fontconfig-devel",
];

/// Where node-gyp is linked inside the image.
pub const NODE_GYP: &str = "/usr/local/bin/node-gyp.js";

/// How long the environment may take to build: 30 minutes.
pub const ENVIRONMENT_SECONDS: u64 = 30 * 60;

/// How long one module may take to build: 10 minutes (the experiment's
/// longest took 55 s on a GitHub runner).
pub const MODULE_SECONDS: u64 = 10 * 60;

/// The build environment's container description.
pub fn containerfile() -> String {
    format!(
        "# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0\n\
         # Written by tur-rehost: the environment native Node modules are built in.\n\
         FROM {BASE_IMAGE}\n\
         RUN dnf -y --setopt=install_weak_deps=False install {} && dnf clean all\n\
         RUN set -e; gyp=$(ls -d /usr/lib/node_modules*/npm/node_modules/node-gyp/bin/node-gyp.js | head -n 1); \
         test -f \"$gyp\"; ln -s \"$gyp\" {NODE_GYP}\n",
        BUILD_PACKAGES.join(" ")
    )
}

/// The command that says whether the environment exists.
pub fn image_exists(podman: &Path) -> Invocation {
    Invocation {
        purpose: "look for the build environment".to_owned(),
        program: podman.to_path_buf(),
        args: vec!["image".to_owned(), "exists".to_owned(), BUILD_IMAGE.to_owned()],
        env: podman_environment(),
        cwd: None,
        timeout_seconds: 60,
    }
}

/// The command that builds the environment from `context` (a directory
/// holding `Containerfile`).
pub fn build_environment(podman: &Path, context: &str) -> Invocation {
    Invocation {
        purpose: "build the build environment (Fedora's compiler; downloads from Fedora once)".to_owned(),
        program: podman.to_path_buf(),
        args: vec![
            "build".to_owned(),
            "--pull=missing".to_owned(),
            "--tag".to_owned(),
            BUILD_IMAGE.to_owned(),
            "--file".to_owned(),
            format!("{context}/Containerfile"),
            context.to_owned(),
        ],
        env: podman_environment(),
        cwd: None,
        timeout_seconds: ENVIRONMENT_SECONDS,
    }
}

/// The command that builds the module in `source` against the headers in
/// `headers` (the directory that holds `include/node`).
pub fn build_module(podman: &Path, source: &str, headers: &str) -> Invocation {
    Invocation {
        purpose: "build a native module (no network)".to_owned(),
        program: podman.to_path_buf(),
        args: vec![
            "run".to_owned(),
            "--rm".to_owned(),
            "--network=none".to_owned(),
            "--read-only".to_owned(),
            "--tmpfs".to_owned(),
            "/tmp".to_owned(),
            "--cap-drop=all".to_owned(),
            "--security-opt=no-new-privileges".to_owned(),
            "--userns=keep-id".to_owned(),
            "--volume".to_owned(),
            format!("{source}:/build:Z"),
            "--volume".to_owned(),
            format!("{headers}:/headers:ro,Z"),
            "--workdir".to_owned(),
            "/build".to_owned(),
            "--env".to_owned(),
            "HOME=/tmp".to_owned(),
            BUILD_IMAGE.to_owned(),
            "node".to_owned(),
            NODE_GYP.to_owned(),
            "rebuild".to_owned(),
            "--nodedir=/headers".to_owned(),
            "--arch=x64".to_owned(),
            "--jobs=max".to_owned(),
        ],
        env: podman_environment(),
        cwd: None,
        timeout_seconds: MODULE_SECONDS,
    }
}

/// podman runs as the person: it needs their home and runtime directory to
/// find its storage, and nothing else.
fn podman_environment() -> Vec<(String, String)> {
    let mut env = vec![("PATH".to_owned(), "/usr/bin:/bin".to_owned())];
    for name in ["HOME", "XDG_RUNTIME_DIR", "XDG_DATA_HOME", "XDG_CONFIG_HOME", "USER"] {
        if let Ok(value) = std::env::var(name)
            && !value.chars().any(char::is_control)
        {
            env.push((name.to_owned(), value));
        }
    }
    env
}

/// What a build produced.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Built {
    /// The module, `name@version`.
    pub identity: String,
    /// The `.node` files taken, by name.
    pub files: Vec<String>,
    /// The last line of the build's output.
    pub detail: String,
}

/// Makes sure the build environment exists, building it once.
pub fn ensure_environment(
    runner: &mut dyn Runner,
    podman: &Path,
    scratch: &Path,
) -> Result<(), Failure> {
    let exists = runner.run(&image_exists(podman)).map_err(|error| {
        Failure::new(
            Code::ProgramMissing,
            format!("podman cannot be started ({}): {error}", podman.display()),
        )
    })?;
    if exists.succeeded() {
        return Ok(());
    }
    std::fs::create_dir_all(scratch).map_err(|error| {
        Failure::new(Code::StepFailed, format!("cannot create {}: {error}", scratch.display()))
    })?;
    std::fs::write(scratch.join("Containerfile"), containerfile()).map_err(|error| {
        Failure::new(Code::StepFailed, format!("cannot write the build description: {error}"))
    })?;
    let context = scratch.to_str().ok_or_else(|| {
        Failure::new(Code::InputInvalid, "the cache directory's path is not valid UTF-8")
    })?;
    let outcome = runner.run(&build_environment(podman, context)).map_err(|error| {
        Failure::new(Code::ProgramMissing, format!("podman cannot be started: {error}"))
    })?;
    let _ = std::fs::remove_dir_all(scratch);
    if outcome.succeeded() {
        Ok(())
    } else {
        Err(Failure::new(
            Code::BuildFailed,
            format!(
                "the build environment could not be made: {}",
                if outcome.timed_out { "it took too long" } else { outcome.last_line() }
            ),
        ))
    }
}

/// Runs the build of the module unpacked in `source`, with Electron's
/// headers unpacked in `headers`, and returns the Linux `.node` files of
/// `build/Release`.
pub fn run_build(
    runner: &mut dyn Runner,
    podman: &Path,
    identity: &str,
    source: &Path,
    headers: &Path,
) -> Result<(crate::natives::Extracted, Built), Failure> {
    let text = |path: &Path| {
        path.to_str().map(str::to_owned).ok_or_else(|| {
            Failure::new(Code::InputInvalid, "the build directory's path is not valid UTF-8")
        })
    };
    let invocation = build_module(podman, &text(source)?, &text(headers)?);
    let outcome = runner.run(&invocation).map_err(|error| {
        Failure::new(Code::ProgramMissing, format!("podman cannot be started: {error}"))
    })?;
    if !outcome.succeeded() {
        return Err(Failure::new(
            Code::BuildFailed,
            format!(
                "{identity} did not build: {}",
                if outcome.timed_out { "it took too long" } else { outcome.last_line() }
            ),
        ));
    }
    let release: PathBuf = source.join("build").join("Release");
    let mut files = Vec::new();
    if let Ok(listing) = std::fs::read_dir(&release) {
        let mut paths: Vec<PathBuf> = listing.filter_map(Result::ok).map(|entry| entry.path()).collect();
        paths.sort();
        for path in paths {
            let Some(name) = path.file_name().and_then(|name| name.to_str()).map(str::to_owned) else {
                continue;
            };
            if !name.ends_with(".node") {
                continue;
            }
            let is_file = std::fs::symlink_metadata(&path).is_ok_and(|m| m.is_file() && m.len() <= 256 << 20);
            if !is_file {
                continue;
            }
            let linux = crate::sniff::binary_kind(&path).is_some_and(|kind| kind.is_linux_x86_64());
            if !linux {
                continue;
            }
            let data = std::fs::read(&path).map_err(|error| {
                Failure::new(Code::StepFailed, format!("cannot read {}: {error}", path.display()))
            })?;
            files.push((name, data));
        }
    }
    if files.is_empty() {
        return Err(Failure::new(
            Code::BuildFailed,
            format!("{identity} built, but no Linux .node file came out (a target for another platform only)"),
        ));
    }
    let built = Built {
        identity: identity.to_owned(),
        files: files.iter().map(|(name, _)| name.clone()).collect(),
        detail: outcome.last_line().to_owned(),
    };
    Ok((files, built))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_environment_is_fedora_s_pinned_base_and_fedora_s_packages() {
        let text = containerfile();
        assert!(text.contains(&format!("FROM {BASE_IMAGE}\n")));
        assert!(BASE_IMAGE.contains("@sha256:"));
        assert!(text.contains("dnf -y"));
        for package in BUILD_PACKAGES {
            assert!(text.contains(package), "{package}");
        }
        // Nothing from npm is installed into the environment: node-gyp is
        // the one Fedora's npm package carries.
        assert!(!text.contains("npm install"));
        assert!(!text.contains("npx"));
    }

    #[test]
    fn a_module_is_built_without_network_and_with_nothing_else_writable() {
        let invocation = build_module(Path::new("/usr/bin/podman"), "/c/builds/x/src", "/c/builds/x/headers");
        let args = &invocation.args;
        for required in ["--network=none", "--read-only", "--cap-drop=all", "--security-opt=no-new-privileges", "--rm"] {
            assert!(args.contains(&required.to_owned()), "{required}");
        }
        assert!(args.contains(&"/c/builds/x/src:/build:Z".to_owned()));
        assert!(args.contains(&"/c/builds/x/headers:/headers:ro,Z".to_owned()));
        let volumes = args.iter().filter(|a| *a == "--volume").count();
        assert_eq!(volumes, 2);
        assert!(args.contains(&"--nodedir=/headers".to_owned()));
        // Never a download of headers during the build.
        assert!(!args.iter().any(|a| a.starts_with("--dist-url")));
    }
}
