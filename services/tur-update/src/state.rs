// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The typed model of system state: the versions on this computer and what
//! is known about an update.
//!
//! [`SystemState::from_host`] interprets the document of [`crate::bootc`].
//! It is a pure function: the same document always gives the same state.

use std::fmt;

use serde::Serialize;

use crate::bootc::{BootEntry, BootOrder, Host, ImageSignature, ImageStatus};

string_enum! {
    /// Where a deployment stands in the boot order.
    pub enum Slot {
        /// Queued for the next boot.
        Staged => "staged",
        /// Running.
        Booted => "booted",
        /// The deployment `bootc rollback` would make the default: the
        /// previous version, as a rule.
        Rollback => "rollback",
        /// Any further deployment: a kept one, as a rule.
        Other => "other",
    }
}

string_enum! {
    /// How an image is verified when it is fetched, as bootc reports it.
    pub enum SignatureKind {
        /// No verification is configured for the image reference.
        None => "none",
        /// The system's `containers-policy.json` decides.
        ContainerPolicy => "container-policy",
        /// An OSTree remote's keys verify the commit.
        OstreeRemote => "ostree-remote",
        /// Verification is explicitly switched off.
        Insecure => "insecure",
    }
}

/// How an image is verified, with the name of the OSTree remote where one
/// is used.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Signature {
    /// The kind of verification.
    pub kind: SignatureKind,
    /// The OSTree remote, for [`SignatureKind::OstreeRemote`].
    pub remote: Option<String>,
}

impl Signature {
    fn from_bootc(signature: Option<&ImageSignature>) -> Signature {
        match signature {
            None => Signature {
                kind: SignatureKind::None,
                remote: None,
            },
            Some(ImageSignature::ContainerPolicy) => Signature {
                kind: SignatureKind::ContainerPolicy,
                remote: None,
            },
            Some(ImageSignature::Insecure) => Signature {
                kind: SignatureKind::Insecure,
                remote: None,
            },
            Some(ImageSignature::OstreeRemote(remote)) => Signature {
                kind: SignatureKind::OstreeRemote,
                remote: Some(remote.clone()),
            },
        }
    }

    /// Whether fetching this image verifies a signature.
    pub fn verifies(&self) -> bool {
        matches!(
            self.kind,
            SignatureKind::ContainerPolicy | SignatureKind::OstreeRemote
        )
    }
}

/// A reference to an image: where the system looks for it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ImageRef {
    /// The reference, for example `quay.io/example/os:stable`.
    pub reference: String,
    /// The transport, for example `registry` or `oci`.
    pub transport: String,
    /// How the image is verified when it is fetched.
    pub signature: Signature,
}

/// An image as it was fetched: one version of the system.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Image {
    /// The reference it was fetched from.
    pub reference: String,
    /// The transport of the reference.
    pub transport: String,
    /// How it was verified.
    pub signature: Signature,
    /// The manifest digest, `sha256:…`. This is the identity of a version:
    /// two images with the same digest are the same version.
    pub digest: String,
    /// The version label, if the image has one.
    pub version: Option<String>,
    /// When the image was built, RFC 3339 as bootc prints it, if it says.
    pub timestamp: Option<String>,
    /// The hardware architecture.
    pub architecture: String,
}

impl Image {
    fn from_bootc(status: &ImageStatus) -> Image {
        Image {
            reference: status.image.image.clone(),
            transport: status.image.transport.clone(),
            signature: Signature::from_bootc(status.image.signature.as_ref()),
            digest: status.image_digest.clone(),
            version: status.version.clone().filter(|version| !version.is_empty()),
            timestamp: status.timestamp.clone().filter(|stamp| !stamp.is_empty()),
            architecture: status.architecture.clone(),
        }
    }

    /// The name of this version for a person: `TUR 0.4.0`, or, for an image
    /// without a version label, `TUR (sha256:1234abcd…)`.
    pub fn label(&self) -> String {
        match &self.version {
            Some(version) => format!("TUR {}", printable(version)),
            None => format!("TUR ({})", short_digest(&self.digest)),
        }
    }
}

/// The first twelve hexadecimal digits of a digest, with its algorithm.
pub fn short_digest(digest: &str) -> String {
    let digest = printable(digest);
    match digest.split_once(':') {
        Some((algorithm, hex)) if hex.chars().count() > 12 => {
            let head: String = hex.chars().take(12).collect();
            format!("{algorithm}:{head}…")
        }
        _ => digest,
    }
}

/// Text from a document, made safe to print: control characters are
/// replaced, and it is cut at 200 characters.
pub fn printable(text: &str) -> String {
    text.chars()
        .take(200)
        .map(|character| {
            if character.is_control() {
                '\u{fffd}'
            } else {
                character
            }
        })
        .collect()
}

/// The OSTree identity of a deployment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Ostree {
    /// The OSTree commit.
    pub checksum: String,
    /// The deployment serial.
    pub deploy_serial: u32,
    /// The stateroot.
    pub stateroot: String,
}

/// One deployment: one version of the system on this computer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Deployment {
    /// Where it stands.
    pub slot: Slot,
    /// The image it was made from. `None` for a deployment that is not a
    /// plain container image.
    pub image: Option<Image>,
    /// It has local changes bootc does not understand.
    pub incompatible: bool,
    /// It is kept: never removed when updates are installed.
    pub pinned: bool,
    /// Relative to the booted system, it could be entered by a soft reboot.
    /// TUR does not use soft reboots for updates (ADR-0014); this is reported
    /// and plays no part in any decision.
    pub soft_reboot_capable: bool,
    /// It is staged but held: it will not be used at the next restart.
    pub download_only: bool,
    /// The storage backend bootc names.
    pub store: Option<String>,
    /// The OSTree identity, on the OSTree backend.
    pub ostree: Option<Ostree>,
    /// Whether bootc reports composefs state for it.
    pub composefs: bool,
}

impl Deployment {
    fn from_bootc(slot: Slot, entry: &BootEntry) -> Deployment {
        Deployment {
            slot,
            image: entry.image.as_ref().map(Image::from_bootc),
            incompatible: entry.incompatible,
            pinned: entry.pinned,
            soft_reboot_capable: entry.soft_reboot_capable,
            download_only: entry.download_only,
            store: entry.store.clone(),
            ostree: entry.ostree.as_ref().map(|ostree| Ostree {
                checksum: ostree.checksum.clone(),
                deploy_serial: ostree.deploy_serial,
                stateroot: ostree.stateroot.clone(),
            }),
            composefs: entry.composefs.is_some(),
        }
    }

    /// Whether bootc can update or roll back to this deployment: it is a
    /// container image with no local changes.
    pub fn managed(&self) -> bool {
        self.image.is_some() && !self.incompatible
    }

    /// The manifest digest of its image, if it has one.
    pub fn digest(&self) -> Option<&str> {
        self.image.as_ref().map(|image| image.digest.as_str())
    }

    /// The name of this version for a person.
    pub fn label(&self) -> String {
        match &self.image {
            Some(image) => image.label(),
            None => match &self.ostree {
                Some(ostree) => {
                    let head: String = printable(&ostree.checksum).chars().take(12).collect();
                    format!("a version that is not an image ({head})")
                }
                None => "a version that is not an image".to_owned(),
            },
        }
    }
}

/// What is known about an update, from the last check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateKnowledge {
    /// bootc holds no result of a check for the booted deployment.
    NotChecked,
    /// The last check found the image that is already running.
    UpToDate,
    /// The last check found another image.
    Available(Image),
}

impl UpdateKnowledge {
    /// The stable text form.
    pub fn as_str(&self) -> &'static str {
        match self {
            UpdateKnowledge::NotChecked => "not-checked",
            UpdateKnowledge::UpToDate => "up-to-date",
            UpdateKnowledge::Available(_) => "available",
        }
    }

    /// The image found, if one was.
    pub fn image(&self) -> Option<&Image> {
        match self {
            UpdateKnowledge::Available(image) => Some(image),
            UpdateKnowledge::NotChecked | UpdateKnowledge::UpToDate => None,
        }
    }
}

impl Serialize for UpdateKnowledge {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut fields = serializer.serialize_struct("UpdateKnowledge", 2)?;
        fields.serialize_field("status", self.as_str())?;
        fields.serialize_field("image", &self.image())?;
        fields.end()
    }
}

string_enum! {
    /// The one state Settings ▸ Updates shows (desktop design, section 11.1),
    /// as far as it can be told from the deployments alone. Checking,
    /// downloading and verifying are states of an operation in progress and
    /// belong to the service that runs it.
    pub enum UpdateState {
        /// A staged update will be used at the next restart.
        Ready => "ready",
        /// A staged update is held.
        Held => "held",
        /// An update is known and not staged.
        Available => "available",
        /// The last check found nothing newer.
        UpToDate => "up-to-date",
        /// No check has been made.
        NotChecked => "not-checked",
    }
}

/// The overlay on `/usr`, if one is mounted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UsrOverlay {
    /// `readOnly` or `readWrite`, as bootc writes it.
    pub access_mode: String,
    /// `transient` or `persistent`, as bootc writes it.
    pub persistence: String,
}

/// Why a document does not describe a system TUR can update.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StateError {
    /// bootc reports no booted deployment.
    NotBootcHost,
}

impl fmt::Display for StateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StateError::NotBootcHost => {
                f.write_str("bootc reports no booted deployment (status.booted is null)")
            }
        }
    }
}

impl std::error::Error for StateError {}

/// The state of the system, as far as updates are concerned.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SystemState {
    /// The image the system follows: where updates are looked for.
    pub follows: Option<ImageRef>,
    /// The running deployment.
    pub booted: Deployment,
    /// The deployment queued for the next restart.
    pub staged: Option<Deployment>,
    /// The previous version: what `bootc rollback` would make the default.
    pub rollback: Option<Deployment>,
    /// Further deployments, kept ones as a rule.
    pub others: Vec<Deployment>,
    /// The next restart starts the rollback deployment.
    pub rollback_queued: bool,
    /// The system runs from a read-only medium and cannot be changed.
    pub read_only: bool,
    /// The overlay on `/usr`, if one is mounted.
    pub usr_overlay: Option<UsrOverlay>,
    /// What the last check found.
    pub update: UpdateKnowledge,
}

impl SystemState {
    /// Interprets a `bootc status` document.
    ///
    /// # Errors
    ///
    /// bootc reports no booted deployment: the system is a container, or was
    /// not installed from an image.
    pub fn from_host(host: &Host) -> Result<SystemState, StateError> {
        let status = &host.status;
        let Some(booted_entry) = status.booted.as_ref() else {
            return Err(StateError::NotBootcHost);
        };
        let booted = Deployment::from_bootc(Slot::Booted, booted_entry);
        let update = match (&booted_entry.cached_update, booted.digest()) {
            (None, _) | (Some(_), None) => UpdateKnowledge::NotChecked,
            (Some(cached), Some(running)) if cached.image_digest == running => {
                UpdateKnowledge::UpToDate
            }
            (Some(cached), Some(_)) => UpdateKnowledge::Available(Image::from_bootc(cached)),
        };
        Ok(SystemState {
            follows: host.spec.image.as_ref().map(|image| ImageRef {
                reference: image.image.clone(),
                transport: image.transport.clone(),
                signature: Signature::from_bootc(image.signature.as_ref()),
            }),
            booted,
            staged: status
                .staged
                .as_ref()
                .map(|entry| Deployment::from_bootc(Slot::Staged, entry)),
            rollback: status
                .rollback
                .as_ref()
                .map(|entry| Deployment::from_bootc(Slot::Rollback, entry)),
            others: status
                .other_deployments
                .iter()
                .map(|entry| Deployment::from_bootc(Slot::Other, entry))
                .collect(),
            // The two say the same thing in bootc 1.17.1; either is enough.
            rollback_queued: status.rollback_queued || host.spec.boot_order == BootOrder::Rollback,
            read_only: status.read_only,
            usr_overlay: status.usr_overlay.as_ref().map(|overlay| UsrOverlay {
                access_mode: overlay.access_mode.clone(),
                persistence: overlay.persistence.clone(),
            }),
            update,
        })
    }

    /// The deployment in a slot that holds at most one.
    pub fn deployment(&self, slot: Slot) -> Option<&Deployment> {
        match slot {
            Slot::Booted => Some(&self.booted),
            Slot::Staged => self.staged.as_ref(),
            Slot::Rollback => self.rollback.as_ref(),
            Slot::Other => None,
        }
    }

    /// Every deployment, in the order a person reads them: the one waiting,
    /// the one in use, the previous one, then the others.
    pub fn deployments(&self) -> Vec<&Deployment> {
        let mut all: Vec<&Deployment> = Vec::new();
        all.extend(self.staged.as_ref());
        all.push(&self.booted);
        all.extend(self.rollback.as_ref());
        all.extend(self.others.iter());
        all
    }

    /// How many deployments are kept.
    pub fn kept(&self) -> usize {
        self.deployments()
            .iter()
            .filter(|deployment| deployment.pinned)
            .count()
    }

    /// The one state the Updates page shows.
    pub fn update_state(&self) -> UpdateState {
        match (&self.staged, &self.update) {
            (Some(staged), _) if staged.download_only => UpdateState::Held,
            (Some(staged), UpdateKnowledge::Available(image))
                if staged.digest() != Some(image.digest.as_str()) =>
            {
                UpdateState::Available
            }
            (Some(_), _) => UpdateState::Ready,
            (None, UpdateKnowledge::Available(_)) => UpdateState::Available,
            (None, UpdateKnowledge::UpToDate) => UpdateState::UpToDate,
            (None, UpdateKnowledge::NotChecked) => UpdateState::NotChecked,
        }
    }

    /// The sentence the Updates page shows for [`SystemState::update_state`],
    /// in the wording of the desktop design, section 11.1.
    pub fn update_message(&self) -> String {
        let running = self.booted.label();
        let version = running.strip_prefix("TUR ").unwrap_or(&running).to_owned();
        match self.update_state() {
            UpdateState::Ready => match &self.staged {
                Some(staged) => format!("{} is ready. Restart to use it.", staged.label()),
                None => "The update is ready. Restart to use it.".to_owned(),
            },
            UpdateState::Held => match &self.staged {
                Some(staged) => format!(
                    "{} has been downloaded and is waiting to be released.",
                    staged.label()
                ),
                None => "The update has been downloaded and is waiting to be released.".to_owned(),
            },
            UpdateState::Available => match self.update.image() {
                Some(image) => format!("{} is available.", image.label()),
                None => "An update is available.".to_owned(),
            },
            UpdateState::UpToDate => format!("TUR is up to date. Version {version}."),
            UpdateState::NotChecked => {
                format!("TUR has not checked for updates yet. Version {version}.")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bootc::parse_status;

    fn entry(digest: &str, version: Option<&str>, extra: &str) -> String {
        let version = match version {
            Some(version) => format!("\"{version}\""),
            None => "null".to_owned(),
        };
        format!(
            r#"{{"image":{{"image":{{"image":"example.org/tur:stable","transport":"registry"}},
                "imageDigest":"{digest}","architecture":"amd64","version":{version},
                "timestamp":"2026-11-12T08:53:00Z"}},
                "incompatible":false,"pinned":false,
                "ostree":{{"checksum":"c{digest}","deploySerial":0,"stateroot":"default"}}{extra}}}"#
        )
    }

    fn state(status: &str) -> SystemState {
        let text = format!(
            r#"{{"apiVersion":"org.containers.bootc/v1","kind":"BootcHost",
                "spec":{{"image":{{"image":"example.org/tur:stable","transport":"registry"}}}},
                "status":{status}}}"#
        );
        SystemState::from_host(&parse_status(&text).unwrap()).unwrap()
    }

    const A: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaa";
    const B: &str = "sha256:bbbbbbbbbbbbbbbbbbbbbbbb";
    const C: &str = "sha256:cccccccccccccccccccccccc";

    fn cached(digest: &str, version: &str) -> String {
        format!(
            r#","cachedUpdate":{{"image":{{"image":"example.org/tur:stable","transport":"registry"}},
                "imageDigest":"{digest}","architecture":"amd64","version":"{version}"}}"#
        )
    }

    #[test]
    fn no_booted_deployment_is_not_a_bootc_host() {
        let host = parse_status(
            r#"{"apiVersion":"org.containers.bootc/v1","kind":"BootcHost",
                "status":{"booted":null,"staged":null,"rollback":null}}"#,
        )
        .unwrap();
        assert_eq!(
            SystemState::from_host(&host),
            Err(StateError::NotBootcHost)
        );
        assert!(StateError::NotBootcHost.to_string().contains("status.booted"));
    }

    #[test]
    fn update_knowledge_comes_from_the_cached_update_of_the_booted_deployment() {
        let none = state(&format!(r#"{{"booted":{}}}"#, entry(A, Some("0.3.2"), "")));
        assert_eq!(none.update, UpdateKnowledge::NotChecked);
        assert_eq!(none.update_state(), UpdateState::NotChecked);
        assert_eq!(
            none.update_message(),
            "TUR has not checked for updates yet. Version 0.3.2."
        );

        let same = state(&format!(
            r#"{{"booted":{}}}"#,
            entry(A, Some("0.3.2"), &cached(A, "0.3.2"))
        ));
        assert_eq!(same.update, UpdateKnowledge::UpToDate);
        assert_eq!(same.update_state(), UpdateState::UpToDate);
        assert_eq!(same.update_message(), "TUR is up to date. Version 0.3.2.");

        let newer = state(&format!(
            r#"{{"booted":{}}}"#,
            entry(A, Some("0.3.2"), &cached(B, "0.4.0"))
        ));
        let image = newer.update.image().expect("an update");
        assert_eq!(image.digest, B);
        assert_eq!(image.label(), "TUR 0.4.0");
        assert_eq!(newer.update_state(), UpdateState::Available);
        assert_eq!(newer.update_message(), "TUR 0.4.0 is available.");
        assert_eq!(newer.update.as_str(), "available");
    }

    #[test]
    fn the_staged_deployment_decides_the_state() {
        // Staged, and it is the update that was found: ready.
        let ready = state(&format!(
            r#"{{"booted":{},"staged":{}}}"#,
            entry(A, Some("0.3.2"), &cached(B, "0.4.0")),
            entry(B, Some("0.4.0"), "")
        ));
        assert_eq!(ready.update_state(), UpdateState::Ready);
        assert_eq!(ready.update_message(), "TUR 0.4.0 is ready. Restart to use it.");
        // Staged with nothing known about a check: still ready.
        let unchecked = state(&format!(
            r#"{{"booted":{},"staged":{}}}"#,
            entry(A, Some("0.3.2"), ""),
            entry(B, Some("0.4.0"), "")
        ));
        assert_eq!(unchecked.update_state(), UpdateState::Ready);
        // Staged and held.
        let held = state(&format!(
            r#"{{"booted":{},"staged":{}}}"#,
            entry(A, Some("0.3.2"), &cached(B, "0.4.0")),
            entry(B, Some("0.4.0"), r#","downloadOnly":true"#)
        ));
        assert_eq!(held.update_state(), UpdateState::Held);
        assert_eq!(
            held.update_message(),
            "TUR 0.4.0 has been downloaded and is waiting to be released."
        );
        // Staged, and a newer one than that was found since.
        let superseded = state(&format!(
            r#"{{"booted":{},"staged":{}}}"#,
            entry(A, Some("0.3.2"), &cached(C, "0.4.1")),
            entry(B, Some("0.4.0"), "")
        ));
        assert_eq!(superseded.update_state(), UpdateState::Available);
        assert_eq!(superseded.update_message(), "TUR 0.4.1 is available.");
    }

    #[test]
    fn deployments_are_listed_in_reading_order_with_their_slots() {
        let all = state(&format!(
            r#"{{"booted":{},"staged":{},"rollback":{},"otherDeployments":[{}],
                "rollbackQueued":false}}"#,
            entry(B, Some("0.4.0"), ""),
            entry(C, Some("0.4.1"), ""),
            entry(A, Some("0.3.2"), r#","pinned":true"#).replace(r#""pinned":false,"#, ""),
            entry(A, None, "")
        ));
        let slots: Vec<Slot> = all.deployments().iter().map(|d| d.slot).collect();
        assert_eq!(
            slots,
            [Slot::Staged, Slot::Booted, Slot::Rollback, Slot::Other]
        );
        assert_eq!(all.kept(), 1);
        assert_eq!(all.deployment(Slot::Rollback).unwrap().label(), "TUR 0.3.2");
        assert_eq!(all.deployment(Slot::Booted).unwrap().digest(), Some(B));
        assert!(all.deployment(Slot::Other).is_none());
        // An image without a version label is named by its digest.
        assert_eq!(all.others[0].label(), "TUR (sha256:aaaaaaaaaaaa…)");
        assert_eq!(all.follows.as_ref().unwrap().reference, "example.org/tur:stable");
        assert_eq!(all.follows.as_ref().unwrap().signature.kind, SignatureKind::None);
        assert!(!all.follows.as_ref().unwrap().signature.verifies());
    }

    #[test]
    fn a_queued_rollback_is_read_from_either_member() {
        let by_status = state(&format!(
            r#"{{"booted":{},"rollback":{},"rollbackQueued":true}}"#,
            entry(B, Some("0.4.0"), ""),
            entry(A, Some("0.3.2"), "")
        ));
        assert!(by_status.rollback_queued);
        let text = format!(
            r#"{{"apiVersion":"org.containers.bootc/v1","kind":"BootcHost",
                "spec":{{"bootOrder":"rollback"}},"status":{{"booted":{}}}}}"#,
            entry(B, Some("0.4.0"), "")
        );
        let by_spec = SystemState::from_host(&parse_status(&text).unwrap()).unwrap();
        assert!(by_spec.rollback_queued);
        assert!(by_spec.follows.is_none());
    }

    #[test]
    fn a_deployment_that_is_not_an_image_is_not_managed() {
        let text = r#"{"apiVersion":"org.containers.bootc/v1","kind":"BootcHost",
            "status":{"booted":{"image":null,"incompatible":false,"pinned":false,
              "ostree":{"checksum":"f9fa3a553ceaaaf30cf85bfe","deploySerial":0,"stateroot":"default"}},
              "staged":{"image":null,"incompatible":true,"pinned":false}}}"#;
        let state = SystemState::from_host(&parse_status(text).unwrap()).unwrap();
        assert!(!state.booted.managed());
        assert_eq!(state.booted.digest(), None);
        assert_eq!(
            state.booted.label(),
            "a version that is not an image (f9fa3a553cea)"
        );
        assert_eq!(state.update, UpdateKnowledge::NotChecked);
        let staged = state.staged.unwrap();
        assert!(staged.incompatible && !staged.managed());
        assert_eq!(staged.label(), "a version that is not an image");
    }

    #[test]
    fn text_from_the_document_is_made_printable() {
        assert_eq!(printable("0.4.0\u{1b}[31m\n"), "0.4.0\u{fffd}[31m\u{fffd}");
        assert_eq!(printable(&"x".repeat(500)).chars().count(), 200);
        assert_eq!(short_digest("sha256:0123456789abcdef"), "sha256:0123456789ab…");
        assert_eq!(short_digest("sha256:aaa"), "sha256:aaa");
        assert_eq!(short_digest("nodigest"), "nodigest");
    }

    #[test]
    fn the_report_form_of_update_knowledge() {
        let newer = state(&format!(
            r#"{{"booted":{}}}"#,
            entry(A, Some("0.3.2"), &cached(B, "0.4.0"))
        ));
        let value = serde_json::to_value(&newer.update).unwrap();
        assert_eq!(value["status"], "available");
        assert_eq!(value["image"]["digest"], B);
        let none = serde_json::to_value(UpdateKnowledge::NotChecked).unwrap();
        assert_eq!(none["status"], "not-checked");
        assert!(none["image"].is_null());
        assert_eq!(serde_json::to_value(Slot::Rollback).unwrap(), "rollback");
    }
}
