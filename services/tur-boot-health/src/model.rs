// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! What this program reads from the machine, as plain data: the kernel
//! arguments GRUB computed (design section 5.4), GRUB's environment block
//! (5.2 and 5.3) and `bootc status` (3.3).

use std::collections::BTreeMap;

use crate::json::{self, Value};

/// The contract version this program implements. The GRUB fragment says which
/// version it implements through `tur.boot_contract=` on the command line.
pub(crate) const CONTRACT_VERSION: &str = "1";

/// The counter variable in GRUB's environment block.
pub(crate) const VAR_COUNTER: &str = "boot_counter";

/// The variable that carries the extra kernel arguments of a trial boot.
pub(crate) const VAR_TRIAL_ARGS: &str = "tur_trial_args";

/// What GRUB said about this boot through `tur.boot=`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BootArg {
    /// No `tur.boot` argument: an ordinary boot.
    None,
    /// `tur.boot=trial`: a counted attempt of a new deployment.
    Trial,
    /// `tur.boot=fallback`: the attempts ran out; GRUB chose the second entry.
    Fallback,
}

impl BootArg {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            BootArg::None => "none",
            BootArg::Trial => "trial",
            BootArg::Fallback => "fallback",
        }
    }
}

/// The boot-health part of the kernel command line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct KernelArgs {
    /// `tur.boot_contract=`, if present.
    pub(crate) contract: Option<String>,
    /// `tur.boot=`.
    pub(crate) boot: BootArg,
    /// `tur.boot_tries_left=`, if present and a number.
    pub(crate) tries_left: Option<i64>,
    /// `ostree=`: the deployment the initramfs was told to start.
    pub(crate) ostree: Option<String>,
}

impl KernelArgs {
    /// Reads the arguments this program cares about from `/proc/cmdline`.
    pub(crate) fn parse(cmdline: &str) -> KernelArgs {
        let mut args = KernelArgs {
            contract: None,
            boot: BootArg::None,
            tries_left: None,
            ostree: None,
        };
        for word in cmdline.split_whitespace() {
            let (key, value) = match word.split_once('=') {
                Some((key, value)) => (key, value),
                None => (word, ""),
            };
            match key {
                "tur.boot_contract" => args.contract = Some(value.to_owned()),
                "tur.boot" => {
                    args.boot = match value {
                        "trial" => BootArg::Trial,
                        "fallback" => BootArg::Fallback,
                        _ => BootArg::None,
                    }
                }
                "tur.boot_tries_left" => args.tries_left = value.parse::<i64>().ok(),
                "ostree" => args.ostree = Some(value.to_owned()),
                _ => {}
            }
        }
        args
    }

    /// Whether the installed `grub.cfg` implements the contract this program
    /// speaks. Without it nothing counts attempts (design section 5.5, step 2).
    pub(crate) fn contract_ok(&self) -> bool {
        self.contract.as_deref() == Some(CONTRACT_VERSION)
    }
}

/// GRUB's environment block as `grub2-editenv list` prints it.
pub(crate) type EnvBlock = BTreeMap<String, String>;

/// Reads the output of `grub2-editenv list`: one `name=value` per line.
pub(crate) fn parse_env_list(text: &str) -> EnvBlock {
    let mut block = EnvBlock::new();
    for line in text.lines() {
        if let Some((name, value)) = line.split_once('=')
            && !name.is_empty()
        {
            block.insert(name.to_owned(), value.to_owned());
        }
    }
    block
}

/// One deployment as `bootc status` describes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Deployment {
    /// Image reference, for example `localhost/tur:ci`.
    pub(crate) image: String,
    /// Transport of the reference, for example `registry` or `oci`.
    pub(crate) transport: String,
    /// Manifest digest of the image, `sha256:…`.
    pub(crate) digest: String,
    /// The image's version label.
    pub(crate) version: String,
    /// OSTree commit of the deployment.
    pub(crate) checksum: String,
    /// OSTree deployment serial.
    pub(crate) serial: i64,
    /// OSTree stateroot.
    pub(crate) stateroot: String,
    /// Staged, but not to be applied at shutdown.
    pub(crate) download_only: bool,
    /// TUR build ID, where it could be read from the deployment.
    pub(crate) build_id: Option<String>,
}

impl Deployment {
    fn from_json(value: &Value) -> Option<Deployment> {
        if value.is_null() {
            return None;
        }
        let image = value.get("image");
        let reference = image.and_then(|image| image.get("image"));
        let ostree = value.get("ostree");
        let text = |from: Option<&Value>, key: &str| -> String {
            from.map(|from| from.str_or_empty(key)).unwrap_or_default()
        };
        let checksum = text(ostree, "checksum");
        if checksum.is_empty() {
            return None;
        }
        Some(Deployment {
            image: text(reference, "image"),
            transport: text(reference, "transport"),
            digest: text(image, "imageDigest"),
            version: text(image, "version"),
            checksum,
            serial: ostree
                .and_then(|ostree| ostree.get("deploySerial"))
                .and_then(Value::as_i64)
                .unwrap_or(0),
            stateroot: text(ostree, "stateroot"),
            download_only: value
                .get("downloadOnly")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            build_id: None,
        })
    }

    /// `checksum.serial`, the name of the deployment's directory.
    pub(crate) fn name(&self) -> String {
        format!("{}.{}", self.checksum, self.serial)
    }

    /// The version as a person would want to read it.
    pub(crate) fn label(&self) -> String {
        let version = if self.version.is_empty() {
            "(unknown version)"
        } else {
            self.version.as_str()
        };
        match &self.build_id {
            Some(build) if !build.is_empty() => format!("{version} (build {build})"),
            _ => version.to_owned(),
        }
    }

    pub(crate) fn to_json(&self) -> Value {
        Value::object(vec![
            ("image", Value::from(self.image.as_str())),
            ("transport", Value::from(self.transport.as_str())),
            ("digest", Value::from(self.digest.as_str())),
            ("version", Value::from(self.version.as_str())),
            ("checksum", Value::from(self.checksum.as_str())),
            ("serial", Value::Int(self.serial)),
            ("stateroot", Value::from(self.stateroot.as_str())),
            ("build_id", Value::opt_str(self.build_id.as_deref())),
        ])
    }

    /// Reads what [`Deployment::to_json`] wrote.
    pub(crate) fn from_record(value: &Value) -> Option<Deployment> {
        if value.is_null() {
            return None;
        }
        let checksum = value.str_or_empty("checksum");
        if checksum.is_empty() {
            return None;
        }
        Some(Deployment {
            image: value.str_or_empty("image"),
            transport: value.str_or_empty("transport"),
            digest: value.str_or_empty("digest"),
            version: value.str_or_empty("version"),
            checksum,
            serial: value.get("serial").and_then(Value::as_i64).unwrap_or(0),
            stateroot: value.str_or_empty("stateroot"),
            download_only: false,
            build_id: value
                .get("build_id")
                .and_then(Value::as_str)
                .map(str::to_owned),
        })
    }

    /// Whether two descriptions name the same deployment: the same OSTree
    /// commit, and the same serial.
    pub(crate) fn same_as(&self, other: &Deployment) -> bool {
        self.checksum == other.checksum && self.serial == other.serial
    }
}

/// The part of `bootc status --format=json` this program uses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BootcStatus {
    /// The running deployment.
    pub(crate) booted: Option<Deployment>,
    /// A deployment waiting to be finalised at shutdown.
    pub(crate) staged: Option<Deployment>,
    /// The other deployment.
    pub(crate) rollback: Option<Deployment>,
    /// True when the booted deployment is not the first in the boot order.
    pub(crate) rollback_queued: bool,
}

impl BootcStatus {
    /// Reads the JSON `bootc status --format=json` prints.
    pub(crate) fn parse(text: &str) -> Result<BootcStatus, String> {
        let value = json::parse(text).map_err(|error| format!("bootc status: {error}"))?;
        let api = value.str_or_empty("apiVersion");
        if !api.starts_with("org.containers.bootc/") {
            return Err(format!("bootc status: unexpected apiVersion {api:?}"));
        }
        let Some(status) = value.get("status") else {
            return Err("bootc status: no status member".to_owned());
        };
        let deployment =
            |key: &str| -> Option<Deployment> { status.get(key).and_then(Deployment::from_json) };
        Ok(BootcStatus {
            booted: deployment("booted"),
            staged: deployment("staged"),
            rollback: deployment("rollback"),
            rollback_queued: status
                .get("rollbackQueued")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_ordinary_command_line_has_only_the_contract() {
        let args = KernelArgs::parse(
            "BOOT_IMAGE=(hd0,gpt3)/boot/ostree/x/vmlinuz root=UUID=1 rw tur.boot_contract=1 \
             console=ttyS0 ostree=/ostree/boot.1/default/abc/0",
        );
        assert_eq!(args.boot, BootArg::None);
        assert_eq!(args.contract.as_deref(), Some("1"));
        assert!(args.contract_ok());
        assert_eq!(args.tries_left, None);
        assert_eq!(args.ostree.as_deref(), Some("/ostree/boot.1/default/abc/0"));
    }

    #[test]
    fn a_trial_command_line_carries_the_tries_left() {
        let args = KernelArgs::parse(
            "root=UUID=1 tur.boot_contract=1 tur.boot=trial tur.boot_tries_left=2 panic=30",
        );
        assert_eq!(args.boot, BootArg::Trial);
        assert_eq!(args.tries_left, Some(2));
    }

    #[test]
    fn a_fallback_command_line_is_recognised() {
        let args = KernelArgs::parse("tur.boot_contract=1 tur.boot=fallback");
        assert_eq!(args.boot, BootArg::Fallback);
        assert_eq!(args.boot.as_str(), "fallback");
    }

    #[test]
    fn a_command_line_without_the_fragment_has_no_contract() {
        let args = KernelArgs::parse("root=UUID=1 rw $tur_boot_args quiet");
        assert!(!args.contract_ok());
        assert_eq!(args.boot, BootArg::None);
        let other = KernelArgs::parse("tur.boot_contract=2 tur.boot=bogus");
        assert!(!other.contract_ok());
        assert_eq!(other.boot, BootArg::None);
    }

    #[test]
    fn the_environment_list_is_read_line_by_line() {
        let block = parse_env_list(
            "boot_counter=2\ntur_trial_args=panic=30 rd.shell=0 rd.emergency=reboot\n\n=x\n",
        );
        assert_eq!(block.get(VAR_COUNTER).map(String::as_str), Some("2"));
        assert_eq!(
            block.get(VAR_TRIAL_ARGS).map(String::as_str),
            Some("panic=30 rd.shell=0 rd.emergency=reboot")
        );
        assert_eq!(block.len(), 2);
    }

    const STATUS: &str = r#"{
      "apiVersion": "org.containers.bootc/v1", "kind": "BootcHost",
      "status": {
        "booted": {"downloadOnly": false,
          "image": {"image": {"image": "localhost/tur-boot-test:ci", "transport": "registry"},
                    "imageDigest": "sha256:aaa", "version": "0.1.0-dev"},
          "ostree": {"checksum": "1111", "deploySerial": 0, "stateroot": "default"}},
        "staged": {"downloadOnly": true,
          "image": {"image": {"image": "/run/images:good", "transport": "oci"},
                    "imageDigest": "sha256:bbb", "version": "0.2.0"},
          "ostree": {"checksum": "2222", "deploySerial": 1, "stateroot": "default"}},
        "rollback": null,
        "rollbackQueued": true
      }
    }"#;

    #[test]
    fn bootc_status_is_read() {
        let status = BootcStatus::parse(STATUS).unwrap();
        let booted = status.booted.unwrap();
        assert_eq!(booted.image, "localhost/tur-boot-test:ci");
        assert_eq!(booted.digest, "sha256:aaa");
        assert_eq!(booted.name(), "1111.0");
        assert!(!booted.download_only);
        let staged = status.staged.unwrap();
        assert_eq!(staged.transport, "oci");
        assert!(staged.download_only);
        assert_eq!(staged.serial, 1);
        assert!(status.rollback.is_none());
        assert!(status.rollback_queued);
    }

    #[test]
    fn something_that_is_not_bootc_status_is_refused() {
        assert!(BootcStatus::parse("{}").is_err());
        assert!(BootcStatus::parse("not json").is_err());
        assert!(BootcStatus::parse(r#"{"apiVersion": "org.containers.bootc/v1"}"#).is_err());
    }

    #[test]
    fn a_deployment_survives_the_trial_record() {
        let mut booted = BootcStatus::parse(STATUS).unwrap().booted.unwrap();
        booted.build_id = Some("20261112.gabc".to_owned());
        let back = Deployment::from_record(&booted.to_json()).unwrap();
        assert_eq!(back, booted);
        assert!(back.same_as(&booted));
        assert_eq!(back.label(), "0.1.0-dev (build 20261112.gabc)");
        assert!(Deployment::from_record(&Value::Null).is_none());
    }
}
