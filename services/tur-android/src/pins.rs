// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The Android images this system may install, pinned by digest
//! (android-runtime.md, section 8.3, rule 2), and the explanation shown to
//! the person before anything is downloaded.
//!
//! The pins are a file of the system image,
//! `/usr/share/tur/android/images.json`. TUR does not ship the Android
//! images: the person's computer fetches the ones named here when the
//! person switches Android on, and installs nothing else.

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

/// Where the system image keeps the pins.
pub(crate) const PINS_PATH: &str = "/usr/share/tur/android/images.json";

/// Space kept free beyond the images, for Android's own data and the
/// container's overlay.
pub(crate) const SPARE_BYTES: u64 = 1024 * 1024 * 1024;

/// One pinned image archive.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ImagePin {
    /// `system` or `vendor`.
    pub(crate) name: String,
    /// The archive's file name, as upstream publishes it.
    pub(crate) file: String,
    /// Where upstream publishes it.
    pub(crate) url: String,
    /// The SHA-256 of the archive.
    pub(crate) sha256: String,
    /// The size of the archive in bytes.
    pub(crate) size: u64,
    /// The image inside the archive: `system.img` or `vendor.img`.
    pub(crate) member: String,
    /// The size of that image in bytes.
    pub(crate) unpacked_size: u64,
}

/// The pinned set of Android images.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Pins {
    /// The version of this file's format: 1.
    pub(crate) schema_version: u64,
    /// A name for this set of images, used as its directory name.
    pub(crate) id: String,
    /// The Android version, such as `13`.
    pub(crate) android_version: String,
    /// Its API level, such as 33.
    pub(crate) api_level: u32,
    /// Who built the images, what they are and under which terms, in a
    /// sentence for the person.
    pub(crate) description: String,
    /// Where the pins were read from upstream.
    pub(crate) source: String,
    /// The licences of the images, in a sentence.
    pub(crate) licence: String,
    /// Memory a running Android system was measured to use, with where it
    /// was measured; `None` until it has been measured.
    pub(crate) memory_measured: Option<String>,
    /// The two archives.
    pub(crate) images: Vec<ImagePin>,
}

impl Pins {
    /// Reads and checks the pins at `path`.
    pub(crate) fn load(path: &Path) -> Result<Pins, String> {
        let text = fs::read_to_string(path)
            .map_err(|error| format!("{} cannot be read: {error}", path.display()))?;
        let pins: Pins = serde_json::from_str(&text)
            .map_err(|error| format!("{} is not valid: {error}", path.display()))?;
        pins.check()?;
        Ok(pins)
    }

    fn check(&self) -> Result<(), String> {
        if self.schema_version != 1 {
            return Err(format!("schema_version {} is not 1", self.schema_version));
        }
        if !is_plain_name(&self.id) {
            return Err(format!("id {:?} is not a plain name", self.id));
        }
        let names: Vec<&str> = self.images.iter().map(|i| i.name.as_str()).collect();
        if names != ["system", "vendor"] {
            return Err(format!("images are {names:?}, not system and vendor"));
        }
        for image in &self.images {
            if image.sha256.len() != 64
                || !image
                    .sha256
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            {
                return Err(format!(
                    "{}: sha256 is not a SHA-256 in hexadecimal",
                    image.name
                ));
            }
            if !image.url.starts_with("https://") {
                return Err(format!("{}: the address is not https", image.name));
            }
            if image.member != format!("{}.img", image.name) || !is_plain_name(&image.file) {
                return Err(format!("{}: file or member is not plain", image.name));
            }
            if image.size == 0 || image.unpacked_size == 0 {
                return Err(format!("{}: a size is zero", image.name));
            }
        }
        Ok(())
    }

    /// Bytes downloaded when Android is switched on.
    pub(crate) fn download_bytes(&self) -> u64 {
        self.images.iter().map(|i| i.size).sum()
    }

    /// Bytes the unpacked images take on disk.
    pub(crate) fn disk_bytes(&self) -> u64 {
        self.images.iter().map(|i| i.unpacked_size).sum()
    }

    /// Free space needed to switch Android on: the archives and the images
    /// side by side while unpacking, and room for Android's data.
    pub(crate) fn needed_bytes(&self) -> u64 {
        self.download_bytes() + self.disk_bytes() + SPARE_BYTES
    }
}

/// A name with no path in it.
fn is_plain_name(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-+".contains(&b))
}

/// A size for people: `1.04 GB`, `562 MB`.
pub(crate) fn human(bytes: u64) -> String {
    const GB: f64 = 1_000_000_000.0;
    const MB: f64 = 1_000_000.0;
    let value = bytes as f64;
    if value >= GB {
        format!("{:.2} GB", value / GB)
    } else {
        format!("{:.0} MB", value / MB)
    }
}

/// What switching Android on means, in plain words, before anything is done.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct Explanation {
    /// What is downloaded, and from whom.
    pub(crate) what: String,
    /// Bytes downloaded.
    pub(crate) download_bytes: u64,
    /// Bytes on disk once installed.
    pub(crate) disk_bytes: u64,
    /// Free bytes needed while switching on.
    pub(crate) needed_free_bytes: u64,
    /// The memory it costs.
    pub(crate) memory: String,
    /// The paragraphs shown to the person, in order.
    pub(crate) paragraphs: Vec<String>,
}

/// The explanation for these pins.
pub(crate) fn explain(pins: &Pins) -> Explanation {
    let download = pins.download_bytes();
    let disk = pins.disk_bytes();
    let memory = match &pins.memory_measured {
        Some(measured) => format!(
            "{measured} Android uses memory only while an Android application is open or the \
             Android system is running; switching Android off gives all of it back."
        ),
        None => "Not yet measured on TUR. Android uses memory only while an Android application \
                 is open or the Android system is running; switching Android off gives all of it \
                 back."
            .to_owned(),
    };
    let paragraphs = vec![
        format!(
            "Switching Android on downloads an Android {} system ({}) from its publisher to this \
             computer: {} to download, {} on disk once unpacked. TUR does not ship it; your \
             computer fetches exactly the files TUR has pinned and checks each one before using \
             it.",
            pins.android_version,
            pins.description,
            human(download),
            human(disk)
        ),
        format!("Memory: {memory}"),
        "Nothing from Google is included: no Google services, no Play Store. Applications that \
         need them, or that need a certified phone (banking, payment, streaming in HD, many \
         games), will not work."
            .to_owned(),
        format!(
            "Android {} no longer receives security fixes from Google. Android applications run \
             in a container managed by a service with administrator rights; they are less \
             contained than on a phone. They can reach the internet, but not this computer's \
             own services or the devices of your local network. Install only applications you \
             trust.",
            pins.android_version
        ),
        format!("Licences: {}", pins.licence),
        "You can switch Android off at any time: nothing of Android then runs or stays mounted, \
         and your Android applications are kept until you remove them."
            .to_owned(),
    ];
    Explanation {
        what: format!("Android {} ({})", pins.android_version, pins.description),
        download_bytes: download,
        disk_bytes: disk,
        needed_free_bytes: pins.needed_bytes(),
        memory,
        paragraphs,
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// The pins the image ships, read from the repository.
    pub(crate) fn shipped() -> Pins {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/data/images.json");
        Pins::load(Path::new(path)).unwrap()
    }

    #[test]
    fn the_shipped_pins_are_valid_and_name_no_google_image() {
        let pins = shipped();
        assert_eq!(pins.api_level, 33);
        assert_eq!(pins.android_version, "13");
        for image in &pins.images {
            assert!(!image.file.contains("GAPPS"), "{}", image.file);
            assert!(image.url.ends_with(&format!("{}/download", image.file)));
        }
        assert!(pins.images[0].file.contains("VANILLA"));
        assert_eq!(pins.download_bytes(), 847_449_341 + 189_818_691);
        assert_eq!(pins.disk_bytes(), 1_827_717_120 + 562_388_992);
    }

    #[test]
    fn the_explanation_states_size_memory_and_limits() {
        let explanation = explain(&shipped());
        let text = explanation.paragraphs.join("\n");
        assert!(text.contains("1.04 GB to download"), "{text}");
        assert!(text.contains("2.39 GB on disk"), "{text}");
        assert!(text.contains("Memory:"));
        assert!(text.contains("no Play Store"));
        assert!(text.contains("no longer receives security fixes"));
        assert!(text.contains("switch Android off"));
    }

    #[test]
    fn pins_that_are_not_plain_are_refused() {
        let mut pins = shipped();
        pins.images[0].file = "../system.zip".to_owned();
        assert!(pins.check().is_err());
        let mut pins = shipped();
        pins.images[1].sha256 = "00".to_owned();
        assert!(pins.check().is_err());
        let mut pins = shipped();
        pins.images[0].url = "http://example.org/system.zip".to_owned();
        assert!(pins.check().is_err());
        let mut pins = shipped();
        pins.images.swap(0, 1);
        assert!(pins.check().is_err());
    }

    #[test]
    fn sizes_for_people() {
        assert_eq!(human(1_037_268_032), "1.04 GB");
        assert_eq!(human(562_388_992), "562 MB");
    }
}
