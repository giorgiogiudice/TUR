// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The inference engine: llama.cpp's `llama-server`.
//!
//! Why this engine, and which build, is in `docs/architecture/local-ai.md`,
//! section 6. This module knows three things about it: which backends a
//! build has (from the backend libraries beside it), which addresses it may
//! listen on, and the command line it is started with.
//!
//! **Where it listens.** Only on a loopback address (127.0.0.0/8, ::1) or a
//! Unix socket, unless the person allows network access explicitly
//! (`--allow-network`). The unspecified addresses `0.0.0.0` and `::` listen
//! on every interface and are refused like any other non-loopback address.
//!
//! **What it is told.** The model file and its projector, the context size,
//! one request at a time, an access key from a file only the person can
//! read, its metrics (the service reads them to see whether it is idle),
//! and to stay offline: no web interface, no slot inspection, no download
//! of anything. Its environment is cleared, so that no `LLAMA_ARG_*`
//! variable of the caller can change any of this.

use std::ffi::OsString;
use std::fs;
use std::net::IpAddr;
use std::path::{Path, PathBuf};

use crate::catalogue::{Capability, Model, Pooling};

/// The engine program of an installed system. Never looked up along a
/// search path.
pub const DEFAULT_ENGINE: &str = "/usr/bin/llama-server";

/// The port used unless another is asked for.
pub const DEFAULT_PORT: u16 = 39281;

/// The environment the engine is started with.
pub const ENGINE_ENVIRONMENT: [(&str, &str); 2] = [("PATH", "/usr/bin:/bin"), ("LANG", "C.UTF-8")];

string_enum! {
    /// Whether a model may think before it answers.
    pub enum Reasoning {
        /// Let it think.
        On => "on",
        /// Ask it to answer directly. The default: thinking makes a small
        /// model on a processor much slower.
        Off => "off",
        /// Leave it to the model's chat template.
        Auto => "auto",
    }
}

/// The backends a build of the engine has, from the `libggml-*.so`
/// libraries beside the program or in the library directory next to its
/// own: for example `cpu`, `cpu-haswell`, `vulkan`, `hip`.
///
/// A link is followed first: on TUR the engine is `/usr/bin/llama-server`,
/// a link to the directory the image build puts llama.cpp in, with its
/// backends (`/usr/libexec/tur/llama.cpp/`; image/README.md, "The local AI
/// engine").
pub fn backends(program: &Path) -> Vec<String> {
    let program = fs::canonicalize(program).unwrap_or_else(|_| program.to_path_buf());
    let Some(bin) = program.parent() else {
        return Vec::new();
    };
    let mut directories = vec![bin.to_path_buf()];
    if let Some(prefix) = bin.parent() {
        directories.push(prefix.join("lib64"));
        directories.push(prefix.join("lib"));
        directories.push(prefix.join("lib/ollama"));
    }
    let mut found: Vec<String> = Vec::new();
    for directory in directories {
        let Ok(entries) = fs::read_dir(&directory) else {
            continue;
        };
        for entry in entries.filter_map(Result::ok) {
            let name = entry.file_name().to_string_lossy().into_owned();
            let Some(rest) = name.strip_prefix("libggml-") else {
                continue;
            };
            let Some(backend) = rest.split(".so").next() else {
                continue;
            };
            if backend == "base" || backend.is_empty() || !rest.contains(".so") {
                continue;
            }
            if !found.iter().any(|known| known == backend) {
                found.push(backend.to_owned());
            }
        }
    }
    found.sort();
    found
}

/// Where the engine listens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Listen {
    /// An address and a port.
    Tcp {
        /// The address.
        address: IpAddr,
        /// The port.
        port: u16,
    },
    /// A Unix socket; llama-server listens on one when the host ends in
    /// `.sock`.
    Socket(PathBuf),
}

impl Listen {
    /// The base URL a client uses, for a TCP address.
    pub fn base_url(&self) -> Option<String> {
        match self {
            Listen::Tcp { address, port } => Some(match address {
                IpAddr::V4(v4) => format!("http://{v4}:{port}"),
                IpAddr::V6(v6) => format!("http://[{v6}]:{port}"),
            }),
            Listen::Socket(_) => None,
        }
    }

    /// Only programs on this computer can connect.
    pub fn is_local(&self) -> bool {
        match self {
            Listen::Tcp { address, .. } => address.is_loopback(),
            Listen::Socket(_) => true,
        }
    }
}

/// The address to listen on, checked: a loopback address, or with
/// `allow_network` any address. `localhost` means 127.0.0.1.
pub fn check_host(host: &str, allow_network: bool) -> Result<IpAddr, String> {
    let address: IpAddr = if host == "localhost" {
        IpAddr::from([127, 0, 0, 1])
    } else {
        host.trim_start_matches('[')
            .trim_end_matches(']')
            .parse()
            .map_err(|_| format!("{host:?} is not an IP address"))?
    };
    if address.is_loopback() || allow_network {
        Ok(address)
    } else {
        Err(format!(
            "{address} is not a loopback address: other devices could connect to it"
        ))
    }
}

/// A Unix socket path the engine accepts: absolute, ending in `.sock`.
pub fn check_socket(path: &Path) -> Result<PathBuf, String> {
    let text = path.to_string_lossy();
    if !path.is_absolute() || !text.ends_with(".sock") {
        return Err(format!("{text} is not an absolute path ending in .sock"));
    }
    Ok(path.to_path_buf())
}

/// Everything the engine is started with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Launch {
    /// The engine program.
    pub program: PathBuf,
    /// The catalogue id, given to the API as the model's name.
    pub model_id: String,
    /// The model file.
    pub model_path: PathBuf,
    /// The image projector, for a model with image input.
    pub projector_path: Option<PathBuf>,
    /// Where it listens.
    pub listen: Listen,
    /// The context size.
    pub context: u32,
    /// The file holding the access key.
    pub api_key_file: PathBuf,
    /// Whether the model may think first. Not given to an embeddings model.
    pub reasoning: Option<Reasoning>,
    /// The pooling of an embeddings model.
    pub pooling: Option<Pooling>,
}

/// What the engine is told about the model's kind: whether it may think
/// first, and the pooling of an embeddings model.
pub fn model_options(model: &Model, reasoning: Reasoning) -> (Option<Reasoning>, Option<Pooling>) {
    if model.can(Capability::Embeddings) {
        (None, model.pooling)
    } else {
        (Some(reasoning), None)
    }
}

impl Launch {
    /// The arguments after the program name.
    pub fn arguments(&self) -> Vec<OsString> {
        let mut arguments: Vec<OsString> =
            vec!["--model".into(), self.model_path.clone().into_os_string()];
        if let Some(projector) = &self.projector_path {
            arguments.push("--mmproj".into());
            arguments.push(projector.clone().into_os_string());
        }
        let mut text: Vec<String> = Vec::new();
        let mut push = |value: String| text.push(value);
        push("--alias".to_owned());
        push(self.model_id.clone());
        match &self.listen {
            Listen::Tcp { address, port } => {
                push("--host".to_owned());
                push(address.to_string());
                push("--port".to_owned());
                push(port.to_string());
            }
            Listen::Socket(path) => {
                push("--host".to_owned());
                push(path.to_string_lossy().into_owned());
            }
        }
        push("--ctx-size".to_owned());
        push(self.context.to_string());
        for fixed in [
            // One request at a time: the memory check counts one context.
            "--parallel",
            "1",
            // No host-memory cache of other prompts (8 GiB by default),
            // so that memory use stays what was checked.
            "--cache-ram",
            "0",
            "--metrics",
            "--no-webui",
            "--no-slots",
            "--offline",
        ] {
            push(fixed.to_owned());
        }
        push("--api-key-file".to_owned());
        push(self.api_key_file.to_string_lossy().into_owned());
        if let Some(reasoning) = self.reasoning {
            push("--reasoning".to_owned());
            push(reasoning.as_str().to_owned());
        }
        if let Some(pooling) = self.pooling {
            push("--embedding".to_owned());
            push("--pooling".to_owned());
            push(pooling.engine_value().to_owned());
            // An embedding is computed in one batch: the whole input.
            push("--ubatch-size".to_owned());
            push(self.context.to_string());
        }
        arguments.extend(text.into_iter().map(OsString::from));
        arguments
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_loopback_unless_allowed() {
        assert_eq!(
            check_host("127.0.0.1", false),
            Ok(IpAddr::from([127, 0, 0, 1]))
        );
        assert!(check_host("localhost", false).is_ok());
        assert!(check_host("::1", false).is_ok());
        assert!(check_host("[::1]", false).is_ok());
        assert!(check_host("127.0.0.2", false).is_ok());
        for refused in ["0.0.0.0", "::", "192.168.1.10", "10.0.0.1", "fe80::1"] {
            assert!(check_host(refused, false).is_err(), "{refused}");
            assert!(check_host(refused, true).is_ok(), "{refused}");
        }
        assert!(check_host("example.org", true).is_err());
        assert!(check_socket(Path::new("/run/user/1000/tur-ai-local/api.sock")).is_ok());
        assert!(check_socket(Path::new("relative.sock")).is_err());
        assert!(check_socket(Path::new("/tmp/not-a-socket")).is_err());
    }

    #[test]
    fn the_backends_are_found_beside_the_program_a_link_points_to() {
        let root =
            std::env::temp_dir().join(format!("tur-ai-local-backends-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let real = root.join("libexec/llama.cpp");
        let bin = root.join("bin");
        fs::create_dir_all(&real).expect("a directory");
        fs::create_dir_all(&bin).expect("a directory");
        for name in [
            "llama-server",
            "libggml-base.so.0",
            "libggml-cpu-haswell.so",
            "libggml-cpu-x64.so",
            "libggml-vulkan.so",
        ] {
            fs::write(real.join(name), b"").expect("a file");
        }
        std::os::unix::fs::symlink("../libexec/llama.cpp/llama-server", bin.join("llama-server"))
            .expect("a link");
        let expected = ["cpu-haswell", "cpu-x64", "vulkan"];
        assert_eq!(backends(&bin.join("llama-server")), expected);
        assert_eq!(backends(&real.join("llama-server")), expected);
        // A program that is not there is looked for where it was named.
        assert!(backends(&bin.join("absent")).is_empty());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn the_command_line_binds_where_it_was_told_and_stays_offline() {
        let launch = Launch {
            program: PathBuf::from(DEFAULT_ENGINE),
            model_id: "example-1b".to_owned(),
            model_path: PathBuf::from("/m/example-1b/Example.gguf"),
            projector_path: None,
            listen: Listen::Tcp {
                address: IpAddr::from([127, 0, 0, 1]),
                port: DEFAULT_PORT,
            },
            context: 8192,
            api_key_file: PathBuf::from("/run/user/1000/tur-ai-local/api-key"),
            reasoning: Some(Reasoning::Off),
            pooling: None,
        };
        let line: Vec<String> = launch
            .arguments()
            .iter()
            .map(|argument| argument.to_string_lossy().into_owned())
            .collect();
        let joined = line.join(" ");
        assert!(joined.starts_with("--model /m/example-1b/Example.gguf --alias example-1b"));
        assert!(joined.contains("--host 127.0.0.1 --port 39281"));
        for expected in [
            "--ctx-size 8192",
            "--parallel 1",
            "--cache-ram 0",
            "--metrics",
            "--no-webui",
            "--no-slots",
            "--offline",
            "--api-key-file /run/user/1000/tur-ai-local/api-key",
            "--reasoning off",
        ] {
            assert!(joined.contains(expected), "{expected} missing: {joined}");
        }
        assert!(!joined.contains("--embedding"));
        assert_eq!(line.iter().filter(|a| *a == "--host").count(), 1);
        assert_eq!(
            launch.listen.base_url().as_deref(),
            Some("http://127.0.0.1:39281")
        );
        assert!(launch.listen.is_local());
    }
}
