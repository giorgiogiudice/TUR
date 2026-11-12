// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The document `bootc status --format=json` prints.
//!
//! These types mirror the definitions in bootc's own source at tag `v1.17.1`
//! (`crates/lib/src/spec.rs`), the version in the base image, member for
//! member and with the same rules for what may be absent:
//!
//! * a member that is an `Option` in bootc may be absent or `null`;
//! * a member bootc gives a default (`softRebootCapable`, `downloadOnly`,
//!   `store`, `otherDeployments`, `rollbackQueued`, `readOnly`, `spec`,
//!   `status`, `bootOrder`) may be absent;
//! * `incompatible` and `pinned` of a boot entry, and `imageDigest`,
//!   `architecture` and the reference of an image, are required, as they are
//!   in bootc;
//! * a member this version does not know is passed over, as bootc does.
//!
//! The shape was checked against real output: of the TUR test image booted
//! in a virtual machine (one deployment, a staged deployment, after an
//! automatic fallback, after a good update), of the base image run as a
//! container, and of bootc's own samples. `tests/fixtures/provenance.json`
//! says where each came from.
//!
//! Nothing here interprets the document. [`crate::state`] does.

use std::fmt;

use serde::Deserialize;

/// The `apiVersion` bootc 1.17.1 writes.
pub const API_VERSION: &str = "org.containers.bootc/v1";

/// The `apiVersion` of bootc's own older samples, which it still reads. The
/// members are the same.
pub const API_VERSION_ALPHA: &str = "org.containers.bootc/v1alpha1";

/// The `kind` of the document.
pub const KIND: &str = "BootcHost";

/// The whole document.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Host {
    /// `org.containers.bootc/v1`.
    pub api_version: String,
    /// `BootcHost`.
    pub kind: String,
    /// What the host is asked to be.
    #[serde(default)]
    pub spec: HostSpec,
    /// What the host is.
    #[serde(default)]
    pub status: HostStatus,
}

/// Which deployment the next boot starts.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BootOrder {
    /// The staged or booted deployment.
    #[default]
    Default,
    /// The rollback deployment.
    Rollback,
}

/// `spec`: the image the host follows and the boot order.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HostSpec {
    /// The image the host follows. `null` when the host is not a bootc host.
    #[serde(default)]
    pub image: Option<ImageReference>,
    /// Which deployment the next boot starts.
    #[serde(default)]
    pub boot_order: BootOrder,
}

/// How an image is verified when it is fetched.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ImageSignature {
    /// The OSTree commit is verified with the named OSTree remote.
    OstreeRemote(String),
    /// The `containers-policy.json` of the system decides.
    ContainerPolicy,
    /// Nothing is verified.
    Insecure,
}

/// An image reference with its transport.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageReference {
    /// The reference, for example `quay.io/example/os:stable`.
    pub image: String,
    /// The transport, for example `registry` or `oci`.
    pub transport: String,
    /// How the image is verified. Absent when no verification is configured.
    #[serde(default)]
    pub signature: Option<ImageSignature>,
}

/// An image as it was fetched.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageStatus {
    /// Where it came from.
    pub image: ImageReference,
    /// The version label of the image, if it has one.
    #[serde(default)]
    pub version: Option<String>,
    /// When the image was built, RFC 3339, if it says.
    #[serde(default)]
    pub timestamp: Option<String>,
    /// The manifest digest, `sha256:…`.
    pub image_digest: String,
    /// The hardware architecture.
    pub architecture: String,
}

/// The OSTree side of a boot entry.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BootEntryOstree {
    /// The name of the storage for `/etc` and `/var`.
    pub stateroot: String,
    /// The OSTree commit.
    pub checksum: String,
    /// The deployment serial.
    pub deploy_serial: u32,
}

/// One deployment.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BootEntry {
    /// The image it was made from. `null` for a deployment that is not a
    /// plain container image.
    #[serde(default)]
    pub image: Option<ImageStatus>,
    /// What the last `bootc upgrade --check` found for it.
    #[serde(default)]
    pub cached_update: Option<ImageStatus>,
    /// It has changes bootc does not understand (layered packages).
    pub incompatible: bool,
    /// It is exempt from garbage collection.
    pub pinned: bool,
    /// Relative to the booted system, it could be entered by a soft reboot.
    #[serde(default)]
    pub soft_reboot_capable: bool,
    /// It is staged but will not be applied at shutdown.
    #[serde(default)]
    pub download_only: bool,
    /// The storage backend, `ostreeContainer` in 1.17.1.
    #[serde(default)]
    pub store: Option<String>,
    /// The OSTree state, for the OSTree backend.
    #[serde(default)]
    pub ostree: Option<BootEntryOstree>,
    /// The composefs state, for the composefs backend. Only its presence is
    /// used: TUR is on the OSTree backend (ADR-0014).
    #[serde(default)]
    pub composefs: Option<serde_json::Value>,
}

/// The overlay mounted on `/usr`, if any (`bootc usr-overlay`).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FilesystemOverlay {
    /// `readOnly` or `readWrite`.
    pub access_mode: String,
    /// `transient` or `persistent`.
    pub persistence: String,
}

/// `status`: the deployments.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HostStatus {
    /// The deployment queued for the next boot.
    #[serde(default)]
    pub staged: Option<BootEntry>,
    /// The running deployment. `null` when the host is not a bootc host.
    #[serde(default)]
    pub booted: Option<BootEntry>,
    /// The other deployment: the previous one, as a rule.
    #[serde(default)]
    pub rollback: Option<BootEntry>,
    /// Further deployments: pinned ones, as a rule. Left out when empty.
    #[serde(default)]
    pub other_deployments: Vec<BootEntry>,
    /// The rollback deployment is before the booted one in the boot order.
    #[serde(default)]
    pub rollback_queued: bool,
    /// `bootcHost` when the booted deployment is a container image.
    #[serde(default, rename = "type")]
    pub host_type: Option<String>,
    /// The overlay on `/usr`, if one is mounted.
    #[serde(default)]
    pub usr_overlay: Option<FilesystemOverlay>,
    /// The physical root is on a read-only medium.
    #[serde(default)]
    pub read_only: bool,
}

/// Why a document could not be used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    /// It is not JSON, or does not have the members bootc 1.17.1 writes.
    Shape(String),
    /// It is a document of another format version or kind.
    Format {
        /// The `apiVersion` found.
        api_version: String,
        /// The `kind` found.
        kind: String,
    },
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ParseError::Shape(message) => write!(f, "not a bootc status document: {message}"),
            ParseError::Format { api_version, kind } => write!(
                f,
                "apiVersion {api_version:?} and kind {kind:?} are not {API_VERSION:?} and {KIND:?}"
            ),
        }
    }
}

impl std::error::Error for ParseError {}

/// Reads the JSON `bootc status --format=json` prints.
///
/// # Errors
///
/// The text is not JSON, lacks a member bootc always writes, or is of a
/// format version or kind this code does not know.
pub fn parse_status(text: &str) -> Result<Host, ParseError> {
    let host: Host =
        serde_json::from_str(text).map_err(|error| ParseError::Shape(error.to_string()))?;
    let known_version = host.api_version == API_VERSION || host.api_version == API_VERSION_ALPHA;
    if !known_version || host.kind != KIND {
        return Err(ParseError::Format {
            api_version: host.api_version,
            kind: host.kind,
        });
    }
    Ok(host)
}

#[cfg(test)]
mod tests {
    use super::*;

    const NULL_HOST: &str = r#"{"apiVersion":"org.containers.bootc/v1","kind":"BootcHost",
        "metadata":{"name":"host"},"spec":{"image":null,"bootOrder":"default"},
        "status":{"staged":null,"booted":null,"rollback":null,"rollbackQueued":false,"type":null}}"#;

    #[test]
    fn a_host_that_is_not_a_bootc_host_is_read() {
        let host = parse_status(NULL_HOST).unwrap();
        assert_eq!(host.api_version, API_VERSION);
        assert_eq!(host.spec, HostSpec::default());
        assert_eq!(host.status, HostStatus::default());
        assert!(host.status.booted.is_none());
    }

    #[test]
    fn members_bootc_defaults_may_be_absent() {
        let host = parse_status(
            r#"{"apiVersion":"org.containers.bootc/v1alpha1","kind":"BootcHost",
                "status":{"booted":{"incompatible":false,"pinned":true}}}"#,
        )
        .unwrap();
        let booted = host.status.booted.unwrap();
        assert!(booted.pinned);
        assert!(!booted.download_only);
        assert!(!booted.soft_reboot_capable);
        assert!(booted.image.is_none() && booted.ostree.is_none() && booted.store.is_none());
        assert_eq!(host.spec.boot_order, BootOrder::Default);
        assert!(host.status.other_deployments.is_empty());
    }

    #[test]
    fn members_bootc_requires_are_required() {
        // `pinned` is not optional in bootc, so a document without it is not
        // one bootc wrote.
        let error = parse_status(
            r#"{"apiVersion":"org.containers.bootc/v1","kind":"BootcHost",
                "status":{"booted":{"incompatible":false}}}"#,
        )
        .unwrap_err();
        assert!(matches!(&error, ParseError::Shape(message) if message.contains("pinned")));
        // An image without its digest.
        let error = parse_status(
            r#"{"apiVersion":"org.containers.bootc/v1","kind":"BootcHost",
                "status":{"booted":{"incompatible":false,"pinned":false,
                "image":{"image":{"image":"x","transport":"registry"},"architecture":"amd64"}}}}"#,
        )
        .unwrap_err();
        assert!(matches!(&error, ParseError::Shape(message) if message.contains("imageDigest")));
    }

    #[test]
    fn the_signature_has_the_three_forms_bootc_writes() {
        let read = |signature: &str| -> Option<ImageSignature> {
            let text = format!(
                r#"{{"apiVersion":"org.containers.bootc/v1","kind":"BootcHost",
                    "spec":{{"image":{{"image":"x","transport":"registry","signature":{signature}}}}}}}"#
            );
            parse_status(&text).unwrap().spec.image.unwrap().signature
        };
        assert_eq!(read("\"insecure\""), Some(ImageSignature::Insecure));
        assert_eq!(
            read("\"containerPolicy\""),
            Some(ImageSignature::ContainerPolicy)
        );
        assert_eq!(
            read(r#"{"ostreeRemote":"fedora"}"#),
            Some(ImageSignature::OstreeRemote("fedora".to_owned()))
        );
        assert_eq!(read("null"), None);
    }

    #[test]
    fn another_format_is_not_interpreted() {
        for (version, kind) in [
            ("org.containers.bootc/v2", "BootcHost"),
            ("org.containers.bootc/v1", "Something"),
            ("v1", "BootcHost"),
        ] {
            let text = format!(r#"{{"apiVersion":"{version}","kind":"{kind}"}}"#);
            assert_eq!(
                parse_status(&text),
                Err(ParseError::Format {
                    api_version: version.to_owned(),
                    kind: kind.to_owned()
                })
            );
        }
        let message = parse_status(r#"{"apiVersion":"v9","kind":"BootcHost"}"#)
            .unwrap_err()
            .to_string();
        assert!(message.contains("v9"), "{message}");
    }

    #[test]
    fn text_that_is_not_the_document_is_refused() {
        for text in ["", "not json", "{}", "[]", r#"{"apiVersion":1,"kind":"BootcHost"}"#] {
            assert!(
                matches!(parse_status(text), Err(ParseError::Shape(_))),
                "{text:?}"
            );
        }
        // A boot order bootc does not have.
        assert!(matches!(
            parse_status(
                r#"{"apiVersion":"org.containers.bootc/v1","kind":"BootcHost",
                    "spec":{"bootOrder":"sideways"}}"#
            ),
            Err(ParseError::Shape(_))
        ));
    }
}
