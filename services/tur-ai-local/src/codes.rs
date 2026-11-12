// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The `TUR-AI-LOCAL-*` error-code registry (specification §29.3).
//!
//! The codes belong to the AI family (`TUR-AI-*`) of `tur-core`, under the
//! topic `LOCAL`: local AI is part of the AI control layer, and no new family
//! was needed.
//!
//! Every code has one registry entry with a title, severity, detection
//! criteria, user message, technical explanation and recovery action.
//! [`AiLocalCode::entry`] is an exhaustive `match`, so a code without an
//! entry does not compile. The meaning of a released code never changes.
//!
//! No text in this registry tells a person to bypass a check; a test scans
//! the registry for such wording.

use serde::Serialize;
use tur_core::{ErrorCode, Family};

string_enum! {
    /// How serious the condition behind a code is.
    pub enum Severity {
        /// Nothing is wrong; the code records a normal outcome.
        Info => "info",
        /// The person did not get what was asked for, by design, and nothing is wrong.
        Notice => "notice",
        /// Something needs the person's attention.
        Warning => "warning",
        /// The operation could not be carried out.
        Error => "error",
    }
}

string_enum! {
    /// A stable local-AI code.
    pub enum AiLocalCode {
        /// The memory of this computer could not be read.
        HardwareUnreadable => "TUR-AI-LOCAL-001",
        /// The model catalogue could not be read or breaks a rule.
        CatalogueInvalid => "TUR-AI-LOCAL-002",
        /// The model named is not in the catalogue.
        UnknownModel => "TUR-AI-LOCAL-003",
        /// The model fits in the memory free now.
        Fits => "TUR-AI-LOCAL-004",
        /// The model needs more memory than this computer has.
        TooLarge => "TUR-AI-LOCAL-005",
        /// The model fits this computer, but not in the memory free now.
        NotNow => "TUR-AI-LOCAL-006",
        /// No model in the catalogue fits this computer.
        NoModelFits => "TUR-AI-LOCAL-007",
        /// The download could not be completed.
        DownloadFailed => "TUR-AI-LOCAL-008",
        /// The file does not have the size the catalogue gives.
        SizeMismatch => "TUR-AI-LOCAL-009",
        /// The file does not have the SHA-256 digest the catalogue gives.
        DigestMismatch => "TUR-AI-LOCAL-010",
        /// The models folder could not be written.
        StorageFailed => "TUR-AI-LOCAL-011",
        /// The model is not on this computer.
        NotDownloaded => "TUR-AI-LOCAL-012",
        /// The model's licence restricts use and was not accepted.
        LicenceNotAccepted => "TUR-AI-LOCAL-013",
        /// The inference engine is not installed.
        EngineMissing => "TUR-AI-LOCAL-014",
        /// The inference engine did not start, or stopped by itself.
        EngineFailed => "TUR-AI-LOCAL-015",
        /// Listening on an address other devices can reach was refused.
        NetworkBindRefused => "TUR-AI-LOCAL-016",
        /// The port is used by another program.
        PortInUse => "TUR-AI-LOCAL-017",
        /// A model is already being served.
        AlreadyRunning => "TUR-AI-LOCAL-018",
        /// No model is being served.
        NotRunning => "TUR-AI-LOCAL-019",
        /// The model was stopped because it was not used.
        StoppedIdle => "TUR-AI-LOCAL-020",
        /// The model was stopped on request.
        Stopped => "TUR-AI-LOCAL-021",
        /// There is no runtime folder for the service's state.
        RuntimeDirMissing => "TUR-AI-LOCAL-022",
        /// The model was downloaded and verified.
        Installed => "TUR-AI-LOCAL-023",
        /// The model was removed from this computer.
        Removed => "TUR-AI-LOCAL-024",
        /// A downloaded model no longer matches the catalogue.
        FileChanged => "TUR-AI-LOCAL-025",
        /// The model is being served.
        Serving => "TUR-AI-LOCAL-026",
        /// Whether a model is being served cannot be seen from here.
        StateNotVisible => "TUR-AI-LOCAL-027",
    }
}

/// A registry entry: everything specification §29.3 requires for a code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct CodeEntry {
    /// The code.
    pub code: AiLocalCode,
    /// A short title.
    pub title: &'static str,
    /// How serious the condition is.
    pub severity: Severity,
    /// The facts that cause this code to be reported.
    pub detection: &'static str,
    /// What the person is told happened.
    pub user_message: &'static str,
    /// Why, in technical terms.
    pub technical_explanation: &'static str,
    /// What the person can safely do about it.
    pub recovery_action: &'static str,
}

impl AiLocalCode {
    /// The registry entry for this code.
    pub const fn entry(self) -> CodeEntry {
        match self {
            AiLocalCode::HardwareUnreadable => CodeEntry {
                code: self,
                title: "Memory could not be read",
                severity: Severity::Error,
                detection: "/proc/meminfo could not be read, or has no MemTotal or MemAvailable \
                            line.",
                user_message: "TUR could not find out how much memory this computer has.",
                technical_explanation: "Whether a model fits is decided from the total and the \
                                        available memory the kernel reports. Without them no \
                                        model is recommended and none is started.",
                recovery_action: "Try again. If it keeps happening, create a support bundle.",
            },
            AiLocalCode::CatalogueInvalid => CodeEntry {
                code: self,
                title: "Model catalogue not valid",
                severity: Severity::Error,
                detection: "A file of the model catalogue could not be read, is not valid TOML, \
                            has a field the format does not define, or breaks one of the \
                            catalogue's rules.",
                user_message: "The list of models TUR offers could not be read.",
                technical_explanation: "Every entry names its publisher's file, its size and \
                                        its SHA-256 digest. An entry that cannot be read whole \
                                        is not offered, because a model is only downloaded and \
                                        checked against what the catalogue says.",
                recovery_action: "Install the latest TUR updates. If it keeps happening, create \
                                  a support bundle.",
            },
            AiLocalCode::UnknownModel => CodeEntry {
                code: self,
                title: "Model not in the catalogue",
                severity: Severity::Error,
                detection: "The model identifier given is not the id of any catalogue entry.",
                user_message: "TUR does not know this model.",
                technical_explanation: "Models are downloaded only from an entry of the \
                                        catalogue, which gives the publisher's file and its \
                                        digest.",
                recovery_action: "Choose a model from the list that tur-ai-local models shows.",
            },
            AiLocalCode::Fits => CodeEntry {
                code: self,
                title: "The model fits",
                severity: Severity::Info,
                detection: "The memory the model needs, plus the reserve kept for the rest of \
                            the system, is no more than the memory available now.",
                user_message: "This model fits in the memory of this computer.",
                technical_explanation: "The memory a model needs is its files, the cache of its \
                                        context and the engine's working memory. A reserve is \
                                        kept free so that the rest of the system stays usable.",
                recovery_action: "No action is needed.",
            },
            AiLocalCode::TooLarge => CodeEntry {
                code: self,
                title: "The model is too large for this computer",
                severity: Severity::Error,
                detection: "The memory the model needs, plus the reserve kept for the rest of \
                            the system, is more than the total memory of this computer.",
                user_message: "This model needs more memory than this computer has. It was not \
                               started.",
                technical_explanation: "Starting it would push the rest of the system out of \
                                        memory and could make the computer unusable until the \
                                        model is stopped.",
                recovery_action: "Choose a smaller model. tur-ai-local recommend names one \
                                  that fits.",
            },
            AiLocalCode::NotNow => CodeEntry {
                code: self,
                title: "Not enough free memory now",
                severity: Severity::Warning,
                detection: "The model and the reserve fit in the total memory of this computer, \
                            but not in the memory available now.",
                user_message: "This model fits this computer, but not in the memory that is \
                               free now. It was not started.",
                technical_explanation: "Other programs are using the memory the model would \
                                        need. Starting it now would make the system swap or \
                                        stop programs.",
                recovery_action: "Close some programs and try again, or choose a smaller model.",
            },
            AiLocalCode::NoModelFits => CodeEntry {
                code: self,
                title: "No model fits this computer",
                severity: Severity::Notice,
                detection: "No catalogue model that can hold a conversation fits in the memory \
                            available now with the reserve.",
                user_message: "None of the models TUR offers fits in the memory of this \
                               computer now.",
                technical_explanation: "The smallest model needs its files, its context cache \
                                        and the engine's working memory, plus the reserve kept \
                                        for the rest of the system.",
                recovery_action: "Close some programs and ask again. Local AI needs more memory \
                                  than is free now.",
            },
            AiLocalCode::DownloadFailed => CodeEntry {
                code: self,
                title: "Download not completed",
                severity: Severity::Error,
                detection: "The publisher's server could not be reached, answered with an \
                            error, or the connection broke before the whole file arrived.",
                user_message: "The model could not be downloaded.",
                technical_explanation: "The partial file was deleted. Nothing that was not \
                                        checked is kept.",
                recovery_action: "Check the network connection and try again.",
            },
            AiLocalCode::SizeMismatch => CodeEntry {
                code: self,
                title: "Wrong file size",
                severity: Severity::Error,
                detection: "The file announced or received is not the size in bytes the \
                            catalogue gives for it.",
                user_message: "The model file is not the one TUR expected. It was deleted and \
                               will not be used.",
                technical_explanation: "The catalogue records the exact size of the publisher's \
                                        file. A file of another size is a different file, an \
                                        incomplete one or a damaged one.",
                recovery_action: "Try the download again. If it keeps happening, the publisher \
                                  may have changed the file; install the latest TUR updates.",
            },
            AiLocalCode::DigestMismatch => CodeEntry {
                code: self,
                title: "Wrong file digest",
                severity: Severity::Error,
                detection: "The SHA-256 digest of the whole file is not the digest the \
                            catalogue gives for it.",
                user_message: "The model file is not the one TUR expected. It was deleted and \
                               will not be used.",
                technical_explanation: "The digest identifies the publisher's file exactly. A \
                                        file with another digest was damaged on the way or \
                                        replaced, and could behave differently from the model \
                                        that was reviewed.",
                recovery_action: "Try the download again. If it keeps happening, create a \
                                  support bundle.",
            },
            AiLocalCode::StorageFailed => CodeEntry {
                code: self,
                title: "Models folder could not be written",
                severity: Severity::Error,
                detection: "A file under the models folder could not be created, written, \
                            renamed or removed, or the disk is full.",
                user_message: "TUR could not store the model on this computer.",
                technical_explanation: "Models are kept in the person's data folder, \
                                        $XDG_DATA_HOME/tur/models. Nothing was left half \
                                        written.",
                recovery_action: "Free some disk space and try again.",
            },
            AiLocalCode::NotDownloaded => CodeEntry {
                code: self,
                title: "Model not downloaded",
                severity: Severity::Notice,
                detection: "The model asked for has no verified file in the models folder.",
                user_message: "This model is not on this computer yet.",
                technical_explanation: "A model is usable only after its file was downloaded \
                                        and its size and digest were checked.",
                recovery_action: "Download it first with tur-ai-local pull.",
            },
            AiLocalCode::LicenceNotAccepted => CodeEntry {
                code: self,
                title: "Licence not accepted",
                severity: Severity::Notice,
                detection: "The catalogue lists use restrictions for this model and the request \
                            did not say the person accepts them.",
                user_message: "This model's licence restricts how it may be used. It was not \
                               downloaded.",
                technical_explanation: "Restrictions are shown before a download, never \
                                        hidden. The person decides.",
                recovery_action: "Read the licence. If you accept it, ask again with \
                                  --accept-licence.",
            },
            AiLocalCode::EngineMissing => CodeEntry {
                code: self,
                title: "Inference engine not installed",
                severity: Severity::Error,
                detection: "The engine program (llama-server) does not exist at the path the \
                            service uses.",
                user_message: "The program that runs local models is not installed.",
                technical_explanation: "TUR runs models with llama.cpp's llama-server, \
                                        installed from Fedora's repositories.",
                recovery_action: "Install the latest TUR updates. If it keeps happening, create \
                                  a support bundle.",
            },
            AiLocalCode::EngineFailed => CodeEntry {
                code: self,
                title: "Inference engine failed",
                severity: Severity::Error,
                detection: "The engine exited before it was ready, did not become ready in \
                            time, or exited while it was serving.",
                user_message: "The local model could not be run.",
                technical_explanation: "The last lines the engine printed are in the technical \
                                        detail and in its log in the runtime folder.",
                recovery_action: "Try again. If it keeps happening, choose another model or \
                                  create a support bundle.",
            },
            AiLocalCode::NetworkBindRefused => CodeEntry {
                code: self,
                title: "Network access not allowed",
                severity: Severity::Error,
                detection: "The address to listen on is not a loopback address and \
                            --allow-network was not given.",
                user_message: "Local AI listens only on this computer unless you allow other \
                               devices to reach it.",
                technical_explanation: "On a loopback address only programs on this computer \
                                        can connect. Any other address can be reached from the \
                                        network, so it needs the person's explicit choice.",
                recovery_action: "Use the loopback address 127.0.0.1, or allow network access \
                                  in Settings (--allow-network on the command line) if you mean \
                                  other devices to use it.",
            },
            AiLocalCode::PortInUse => CodeEntry {
                code: self,
                title: "Port in use",
                severity: Severity::Error,
                detection: "The port to listen on could not be bound because another program \
                            uses it.",
                user_message: "Another program is using the port local AI listens on.",
                technical_explanation: "Two programs cannot listen on the same address and \
                                        port.",
                recovery_action: "Choose another port with --port, or stop the other program.",
            },
            AiLocalCode::AlreadyRunning => CodeEntry {
                code: self,
                title: "A model is already running",
                severity: Severity::Notice,
                detection: "The state file names a serving process that is still running.",
                user_message: "Local AI is already running a model.",
                technical_explanation: "This slice serves one model at a time, so that the \
                                        memory check stays true.",
                recovery_action: "Stop it with tur-ai-local stop, then start the model you \
                                  want.",
            },
            AiLocalCode::NotRunning => CodeEntry {
                code: self,
                title: "Not running",
                severity: Severity::Info,
                detection: "There is no state file, or the process it names has ended.",
                user_message: "Local AI is not running a model. Nothing is using memory.",
                technical_explanation: "Models run only when asked for and stop when they are \
                                        not used.",
                recovery_action: "No action is needed.",
            },
            AiLocalCode::StoppedIdle => CodeEntry {
                code: self,
                title: "Stopped when not used",
                severity: Severity::Info,
                detection: "The engine processed no request for the idle time set when it was \
                            started.",
                user_message: "Local AI stopped the model because it was not used. Its memory \
                               is free again.",
                technical_explanation: "Activity is read from the engine's own counters of \
                                        processed tokens and requests in progress.",
                recovery_action: "No action is needed. Start the model again when you need it.",
            },
            AiLocalCode::Stopped => CodeEntry {
                code: self,
                title: "Stopped on request",
                severity: Severity::Info,
                detection: "tur-ai-local stop asked the serving process to stop.",
                user_message: "Local AI stopped the model. Its memory is free again.",
                technical_explanation: "The engine process was ended and the state and key \
                                        files were removed.",
                recovery_action: "No action is needed.",
            },
            AiLocalCode::RuntimeDirMissing => CodeEntry {
                code: self,
                title: "No runtime folder",
                severity: Severity::Error,
                detection: "XDG_RUNTIME_DIR is not set and no --runtime-dir was given.",
                user_message: "Local AI could not find the folder where it keeps its state \
                               while it runs.",
                technical_explanation: "The state file, the engine's log and the access key \
                                        live in the session's runtime folder, which only the \
                                        person can read and which is emptied at log-out.",
                recovery_action: "Start it from a desktop session. If it keeps happening, \
                                  create a support bundle.",
            },
            AiLocalCode::Installed => CodeEntry {
                code: self,
                title: "Model ready",
                severity: Severity::Info,
                detection: "Every file of the model has the size and SHA-256 digest the \
                            catalogue gives.",
                user_message: "The model is downloaded and checked, and ready to use.",
                technical_explanation: "The files were written under temporary names, checked, \
                                        and only then given their final names.",
                recovery_action: "No action is needed.",
            },
            AiLocalCode::Removed => CodeEntry {
                code: self,
                title: "Model removed",
                severity: Severity::Info,
                detection: "The model's folder was deleted from the models folder.",
                user_message: "The model was removed from this computer.",
                technical_explanation: "Its files and its record of verification were deleted.",
                recovery_action: "No action is needed.",
            },
            AiLocalCode::FileChanged => CodeEntry {
                code: self,
                title: "Downloaded model changed",
                severity: Severity::Error,
                detection: "A downloaded model's record of verification is missing or names \
                            another digest, or its file no longer has the size the catalogue \
                            gives.",
                user_message: "A downloaded model was changed or damaged. It will not be used.",
                technical_explanation: "A model is used only while it is the file that was \
                                        checked. A change can come from a disk fault, a \
                                        partial copy or another program.",
                recovery_action: "Remove the model and download it again.",
            },
            AiLocalCode::Serving => CodeEntry {
                code: self,
                title: "Serving",
                severity: Severity::Info,
                detection: "The engine answered its health check and the state file was \
                            written.",
                user_message: "Local AI is running a model on this computer.",
                technical_explanation: "The engine listens on the address in the state file and \
                                        asks for the access key kept in the runtime folder.",
                recovery_action: "No action is needed.",
            },
            AiLocalCode::StateNotVisible => CodeEntry {
                code: self,
                title: "State not visible here",
                severity: Severity::Notice,
                detection: "The runtime folder that holds the state file is not set or cannot \
                            be read, as in the assistant's read-only sandbox.",
                user_message: "Whether local AI is running a model cannot be seen from here.",
                technical_explanation: "The state lives in the session's runtime folder, which \
                                        the assistant's sandbox does not show. The local AI \
                                        service will publish it over D-Bus in a later slice.",
                recovery_action: "Open Settings, AI, to see whether a model is running.",
            },
        }
    }

    /// The code as a validated [`ErrorCode`].
    ///
    /// Returns `None` only if the text form is not a valid TUR AI code; a
    /// unit test checks that this never happens for a registered code.
    pub fn error_code(self) -> Option<ErrorCode> {
        self.as_str()
            .parse::<ErrorCode>()
            .ok()
            .filter(|code| code.family() == Family::Ai)
    }
}

/// The whole registry, in code order.
pub fn registry() -> Vec<CodeEntry> {
    AiLocalCode::all().iter().map(|code| code.entry()).collect()
}

/// Wording that must never appear in a registry entry: it would steer a
/// person towards bypassing a check instead of fixing its cause.
pub const FORBIDDEN_WORDING: &[&str] =
    &["force", "override", "ignore", "skip", "disable", "anyway"];

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn every_code_parses_as_an_ai_code_with_the_local_topic() {
        for &code in AiLocalCode::all() {
            let parsed = code
                .error_code()
                .unwrap_or_else(|| panic!("{code} does not parse"));
            assert_eq!(parsed.family(), Family::Ai);
            assert!(code.as_str().starts_with("TUR-AI-LOCAL-"), "{code}");
            assert_eq!(AiLocalCode::from_text(code.as_str()), Some(code));
        }
    }

    #[test]
    fn codes_are_unique_and_numbered_in_order() {
        let texts: BTreeSet<&str> = AiLocalCode::all()
            .iter()
            .map(|code| code.as_str())
            .collect();
        assert_eq!(texts.len(), AiLocalCode::all().len());
        for (index, code) in AiLocalCode::all().iter().enumerate() {
            let parsed = code.error_code().expect("a valid code");
            assert_eq!(usize::from(parsed.number()), index + 1, "{code}");
        }
    }

    #[test]
    fn every_entry_is_complete() {
        let entries = registry();
        assert_eq!(entries.len(), AiLocalCode::all().len());
        for entry in entries {
            assert_eq!(entry.code.entry(), entry);
            for text in [
                entry.title,
                entry.detection,
                entry.user_message,
                entry.technical_explanation,
                entry.recovery_action,
            ] {
                assert!(!text.trim().is_empty(), "{}", entry.code);
                assert!(
                    !text.contains("  "),
                    "{}: double space in {text:?}",
                    entry.code
                );
                assert!(!text.chars().any(char::is_control), "{}", entry.code);
            }
            assert!(entry.user_message.ends_with('.'), "{}", entry.code);
            assert!(entry.recovery_action.ends_with('.'), "{}", entry.code);
            assert!(entry.detection.ends_with('.'), "{}", entry.code);
            assert!(entry.technical_explanation.ends_with('.'), "{}", entry.code);
            assert!(!entry.title.ends_with('.'), "{}", entry.code);
        }
    }

    #[test]
    fn the_registry_never_suggests_bypassing_a_check() {
        for entry in registry() {
            for text in [
                entry.title,
                entry.detection,
                entry.user_message,
                entry.technical_explanation,
                entry.recovery_action,
            ] {
                let lower = text.to_lowercase();
                for word in FORBIDDEN_WORDING {
                    assert!(
                        !lower.contains(word),
                        "{}: {word:?} in {text:?}",
                        entry.code
                    );
                }
            }
        }
    }

    #[test]
    fn refusals_say_that_nothing_was_started() {
        for code in [AiLocalCode::TooLarge, AiLocalCode::NotNow] {
            assert!(
                code.entry().user_message.ends_with("It was not started."),
                "{code}"
            );
        }
        for code in [AiLocalCode::SizeMismatch, AiLocalCode::DigestMismatch] {
            assert!(
                code.entry().user_message.contains("will not be used"),
                "{code}"
            );
        }
    }
}
