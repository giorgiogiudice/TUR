// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! Android's binary XML, read as far as a package's identity needs: the
//! package name, its version, the Android versions it asks for and the
//! hardware it requires.
//!
//! The input is untrusted: every offset is checked, counts are capped, and a
//! file that does not follow the format is refused, never guessed at.

use serde::Serialize;

const RES_XML_TYPE: u16 = 0x0003;
const RES_STRING_POOL_TYPE: u16 = 0x0001;
const RES_XML_RESOURCE_MAP_TYPE: u16 = 0x0180;
const RES_XML_START_ELEMENT_TYPE: u16 = 0x0102;
const RES_XML_END_ELEMENT_TYPE: u16 = 0x0103;
const UTF8_FLAG: u32 = 1 << 8;

const TYPE_STRING: u8 = 0x03;
const TYPE_INT_DEC: u8 = 0x10;
const TYPE_INT_HEX: u8 = 0x11;
const TYPE_INT_BOOLEAN: u8 = 0x12;

const ATTR_NAME: u32 = 0x0101_0003;
const ATTR_MIN_SDK: u32 = 0x0101_020c;
const ATTR_VERSION_CODE: u32 = 0x0101_021b;
const ATTR_VERSION_NAME: u32 = 0x0101_021c;
const ATTR_TARGET_SDK: u32 = 0x0101_0270;
const ATTR_REQUIRED: u32 = 0x0101_028e;

const MAX_STRINGS: usize = 200_000;
const MAX_STRING_UNITS: usize = 65_536;
const MAX_FEATURES: usize = 64;

/// What a manifest says about its package.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub(crate) struct ManifestFacts {
    /// The package name, such as `org.example.app`.
    pub(crate) package: String,
    /// The version code, when it is a number in the manifest.
    pub(crate) version_code: Option<u64>,
    /// The version name, when it is text in the manifest.
    pub(crate) version_name: Option<String>,
    /// The lowest Android API level the package runs on.
    pub(crate) min_sdk: Option<u32>,
    /// The API level the package was built for.
    pub(crate) target_sdk: Option<u32>,
    /// Hardware or software features the package requires
    /// (`uses-feature` without `required="false"`).
    pub(crate) required_features: Vec<String>,
}

fn u16_at(data: &[u8], offset: usize) -> Option<u16> {
    let bytes = data.get(offset..offset.checked_add(2)?)?;
    Some(u16::from_le_bytes([bytes[0], bytes[1]]))
}

fn u32_at(data: &[u8], offset: usize) -> Option<u32> {
    let bytes = data.get(offset..offset.checked_add(4)?)?;
    Some(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

fn usize_at(data: &[u8], offset: usize) -> Option<usize> {
    usize::try_from(u32_at(data, offset)?).ok()
}

/// The string pool of a document.
#[derive(Debug, Default)]
struct Strings {
    values: Vec<Option<String>>,
}

impl Strings {
    fn get(&self, index: u32) -> Option<&str> {
        let index = usize::try_from(index).ok()?;
        self.values.get(index)?.as_deref()
    }

    fn parse(chunk: &[u8], header_size: usize) -> Result<Strings, String> {
        let count = usize_at(chunk, 8).ok_or("string pool header is truncated")?;
        let flags = u32_at(chunk, 16).ok_or("string pool header is truncated")?;
        let strings_start = usize_at(chunk, 20).ok_or("string pool header is truncated")?;
        if count > MAX_STRINGS {
            return Err(format!("string pool holds {count} strings"));
        }
        let utf8 = flags & UTF8_FLAG != 0;
        let mut values = Vec::with_capacity(count);
        for index in 0..count {
            let at = header_size
                .checked_add(index.checked_mul(4).ok_or("string index overflows")?)
                .ok_or("string index overflows")?;
            let offset = usize_at(chunk, at).ok_or("string offsets are truncated")?;
            let start = strings_start
                .checked_add(offset)
                .ok_or("string offset overflows")?;
            values.push(if utf8 {
                utf8_string(chunk, start)
            } else {
                utf16_string(chunk, start)
            });
        }
        Ok(Strings { values })
    }
}

fn utf8_length(data: &[u8], at: usize) -> Option<(usize, usize)> {
    let first = *data.get(at)?;
    if first & 0x80 == 0 {
        Some((usize::from(first), 1))
    } else {
        let second = *data.get(at.checked_add(1)?)?;
        Some(((usize::from(first & 0x7f) << 8) | usize::from(second), 2))
    }
}

fn utf8_string(data: &[u8], start: usize) -> Option<String> {
    // The length in UTF-16 units, then the length in bytes, then the bytes.
    let (_, skip) = utf8_length(data, start)?;
    let at = start.checked_add(skip)?;
    let (bytes, skip) = utf8_length(data, at)?;
    let from = at.checked_add(skip)?;
    let raw = data.get(from..from.checked_add(bytes)?)?;
    Some(String::from_utf8_lossy(raw).into_owned())
}

fn utf16_string(data: &[u8], start: usize) -> Option<String> {
    let first = usize::from(u16_at(data, start)?);
    let (units, mut at) = if first & 0x8000 == 0 {
        (first, start.checked_add(2)?)
    } else {
        let second = usize::from(u16_at(data, start.checked_add(2)?)?);
        (((first & 0x7fff) << 16) | second, start.checked_add(4)?)
    };
    if units > MAX_STRING_UNITS {
        return None;
    }
    let mut text = Vec::with_capacity(units);
    for _ in 0..units {
        text.push(u16_at(data, at)?);
        at = at.checked_add(2)?;
    }
    Some(String::from_utf16_lossy(&text))
}

/// One attribute of an element: its resource identifier, its name and its
/// value.
#[derive(Debug)]
struct Attribute {
    id: Option<u32>,
    name: String,
    text: Option<String>,
    number: Option<u64>,
    boolean: Option<bool>,
}

impl Attribute {
    fn is(&self, id: u32, name: &str) -> bool {
        self.id == Some(id) || (self.id.is_none() && self.name == name)
    }
}

fn attributes(
    chunk: &[u8],
    header_size: usize,
    strings: &Strings,
    ids: &[u32],
) -> Result<(String, Vec<Attribute>), String> {
    let base = header_size;
    let name = u32_at(chunk, base + 4).ok_or("element is truncated")?;
    let start = usize::from(u16_at(chunk, base + 8).ok_or("element is truncated")?);
    let size = usize::from(u16_at(chunk, base + 10).ok_or("element is truncated")?);
    let count = usize::from(u16_at(chunk, base + 12).ok_or("element is truncated")?);
    if size < 20 {
        return Err("attribute size is too small".to_owned());
    }
    let element = strings.get(name).unwrap_or_default().to_owned();
    let mut list = Vec::with_capacity(count);
    for index in 0..count {
        let at = base + start + index * size;
        let name_index = u32_at(chunk, at + 4).ok_or("attribute is truncated")?;
        let raw = u32_at(chunk, at + 8).ok_or("attribute is truncated")?;
        let data_type = *chunk.get(at + 15).ok_or("attribute is truncated")?;
        let data = u32_at(chunk, at + 16).ok_or("attribute is truncated")?;
        let id = usize::try_from(name_index)
            .ok()
            .and_then(|i| ids.get(i).copied());
        let text = match data_type {
            TYPE_STRING => strings.get(data).map(ToOwned::to_owned),
            _ if raw != u32::MAX => strings.get(raw).map(ToOwned::to_owned),
            _ => None,
        };
        let number = match data_type {
            TYPE_INT_DEC | TYPE_INT_HEX => Some(u64::from(data)),
            _ => None,
        };
        let boolean = (data_type == TYPE_INT_BOOLEAN).then_some(data != 0);
        list.push(Attribute {
            id,
            name: strings.get(name_index).unwrap_or_default().to_owned(),
            text,
            number,
            boolean,
        });
    }
    Ok((element, list))
}

fn sdk(attribute: &Attribute) -> Option<u32> {
    attribute
        .number
        .and_then(|value| u32::try_from(value).ok())
        .or_else(|| attribute.text.as_deref().and_then(|text| text.parse().ok()))
}

/// Reads the facts of a binary `AndroidManifest.xml`.
pub(crate) fn parse(data: &[u8]) -> Result<ManifestFacts, String> {
    if u16_at(data, 0) != Some(RES_XML_TYPE) {
        return Err("not Android binary XML".to_owned());
    }
    let first = usize::from(u16_at(data, 2).ok_or("header is truncated")?);
    let mut offset = first;
    let mut strings = Strings::default();
    let mut ids: Vec<u32> = Vec::new();
    let mut depth = 0usize;
    let mut facts = ManifestFacts::default();
    let mut seen_manifest = false;
    while offset < data.len() {
        let kind = u16_at(data, offset).ok_or("chunk header is truncated")?;
        let header_size = usize::from(u16_at(data, offset + 2).ok_or("chunk header is truncated")?);
        let size = usize_at(data, offset + 4).ok_or("chunk header is truncated")?;
        if size < 8 || header_size < 8 || header_size > size {
            return Err(format!("chunk at {offset} has an impossible size"));
        }
        let end = offset.checked_add(size).ok_or("chunk size overflows")?;
        let chunk = data.get(offset..end).ok_or("chunk runs past the end")?;
        match kind {
            RES_STRING_POOL_TYPE => strings = Strings::parse(chunk, header_size)?,
            RES_XML_RESOURCE_MAP_TYPE => {
                let (words, _) = chunk[header_size..].as_chunks::<4>();
                ids = words.iter().map(|word| u32::from_le_bytes(*word)).collect();
            }
            RES_XML_START_ELEMENT_TYPE => {
                depth += 1;
                let (element, list) = attributes(chunk, header_size, &strings, &ids)?;
                match (depth, element.as_str()) {
                    (1, "manifest") => {
                        seen_manifest = true;
                        for attribute in &list {
                            if attribute.id.is_none() && attribute.name == "package" {
                                facts.package = attribute.text.clone().unwrap_or_default();
                            } else if attribute.is(ATTR_VERSION_CODE, "versionCode") {
                                facts.version_code = attribute.number.or_else(|| {
                                    attribute.text.as_deref().and_then(|t| t.parse().ok())
                                });
                            } else if attribute.is(ATTR_VERSION_NAME, "versionName") {
                                facts.version_name = attribute.text.clone();
                            }
                        }
                    }
                    (2, "uses-sdk") => {
                        for attribute in &list {
                            if attribute.is(ATTR_MIN_SDK, "minSdkVersion") {
                                facts.min_sdk = sdk(attribute);
                            } else if attribute.is(ATTR_TARGET_SDK, "targetSdkVersion") {
                                facts.target_sdk = sdk(attribute);
                            }
                        }
                    }
                    (2, "uses-feature") => {
                        let required = list
                            .iter()
                            .find(|a| a.is(ATTR_REQUIRED, "required"))
                            .and_then(|a| a.boolean)
                            .unwrap_or(true);
                        let name = list
                            .iter()
                            .find(|a| a.is(ATTR_NAME, "name"))
                            .and_then(|a| a.text.clone());
                        if let Some(name) = name
                            && required
                            && facts.required_features.len() < MAX_FEATURES
                        {
                            facts.required_features.push(name);
                        }
                    }
                    _ => {}
                }
            }
            RES_XML_END_ELEMENT_TYPE => depth = depth.saturating_sub(1),
            _ => {}
        }
        offset = end;
    }
    if !seen_manifest {
        return Err("the document has no manifest element".to_owned());
    }
    if facts.package.is_empty() {
        return Err("the manifest names no package".to_owned());
    }
    Ok(facts)
}

/// Builds binary XML for tests: a string pool, a resource map and elements.
#[cfg(test)]
pub(crate) mod build {
    /// An attribute value.
    pub(crate) enum Value {
        Text(&'static str),
        Int(u32),
        Bool(bool),
    }

    fn chunk(kind: u16, header: &[u8], body: &[u8]) -> Vec<u8> {
        let header_size = 8 + header.len();
        let size = header_size + body.len();
        let mut out = Vec::new();
        out.extend_from_slice(&kind.to_le_bytes());
        out.extend_from_slice(&u16::try_from(header_size).unwrap().to_le_bytes());
        out.extend_from_slice(&u32::try_from(size).unwrap().to_le_bytes());
        out.extend_from_slice(header);
        out.extend_from_slice(body);
        out
    }

    /// A document. `strings` is the pool; `ids` the resource map for its
    /// first strings; `elements` are (depth change, name, attributes) where
    /// a name of `None` closes an element.
    pub(crate) fn document(
        strings: &[&str],
        ids: &[u32],
        elements: &[(Option<u32>, Vec<(u32, u32, Value)>)],
        utf8: bool,
    ) -> Vec<u8> {
        // String pool.
        let mut offsets = Vec::new();
        let mut data = Vec::new();
        for text in strings {
            offsets.extend_from_slice(&u32::try_from(data.len()).unwrap().to_le_bytes());
            if utf8 {
                let units = text.encode_utf16().count();
                data.push(u8::try_from(units).unwrap());
                data.push(u8::try_from(text.len()).unwrap());
                data.extend_from_slice(text.as_bytes());
                data.push(0);
            } else {
                let units: Vec<u16> = text.encode_utf16().collect();
                data.extend_from_slice(&u16::try_from(units.len()).unwrap().to_le_bytes());
                for unit in units {
                    data.extend_from_slice(&unit.to_le_bytes());
                }
                data.extend_from_slice(&[0, 0]);
            }
        }
        while data.len() % 4 != 0 {
            data.push(0);
        }
        let count = u32::try_from(strings.len()).unwrap();
        let mut header = Vec::new();
        header.extend_from_slice(&count.to_le_bytes());
        header.extend_from_slice(&0u32.to_le_bytes());
        header.extend_from_slice(&(if utf8 { super::UTF8_FLAG } else { 0 }).to_le_bytes());
        let strings_start = 28 + offsets.len();
        header.extend_from_slice(&u32::try_from(strings_start).unwrap().to_le_bytes());
        header.extend_from_slice(&0u32.to_le_bytes());
        let mut body = offsets;
        body.extend_from_slice(&data);
        let mut document = chunk(super::RES_STRING_POOL_TYPE, &header, &body);

        // Resource map.
        let mut map = Vec::new();
        for id in ids {
            map.extend_from_slice(&id.to_le_bytes());
        }
        document.extend(chunk(super::RES_XML_RESOURCE_MAP_TYPE, &[], &map));

        // Elements.
        for (name, attributes) in elements {
            let mut header = Vec::new();
            header.extend_from_slice(&1u32.to_le_bytes()); // line
            header.extend_from_slice(&u32::MAX.to_le_bytes()); // comment
            match name {
                Some(name) => {
                    let mut body = Vec::new();
                    body.extend_from_slice(&u32::MAX.to_le_bytes()); // namespace
                    body.extend_from_slice(&name.to_le_bytes());
                    body.extend_from_slice(&20u16.to_le_bytes()); // attribute start
                    body.extend_from_slice(&20u16.to_le_bytes()); // attribute size
                    let n = u16::try_from(attributes.len()).unwrap();
                    body.extend_from_slice(&n.to_le_bytes());
                    body.extend_from_slice(&[0; 6]); // id, class, style
                    for (namespace, attribute, value) in attributes {
                        body.extend_from_slice(&namespace.to_le_bytes());
                        body.extend_from_slice(&attribute.to_le_bytes());
                        let (raw, kind, data) = match value {
                            Value::Text(text) => {
                                let index = strings.iter().position(|s| s == text).unwrap();
                                let index = u32::try_from(index).unwrap();
                                (index, super::TYPE_STRING, index)
                            }
                            Value::Int(number) => (u32::MAX, super::TYPE_INT_DEC, *number),
                            Value::Bool(flag) => (
                                u32::MAX,
                                super::TYPE_INT_BOOLEAN,
                                if *flag { u32::MAX } else { 0 },
                            ),
                        };
                        body.extend_from_slice(&raw.to_le_bytes());
                        body.extend_from_slice(&8u16.to_le_bytes());
                        body.push(0);
                        body.push(kind);
                        body.extend_from_slice(&data.to_le_bytes());
                    }
                    document.extend(chunk(super::RES_XML_START_ELEMENT_TYPE, &header, &body));
                }
                None => {
                    let mut body = Vec::new();
                    body.extend_from_slice(&u32::MAX.to_le_bytes());
                    body.extend_from_slice(&0u32.to_le_bytes());
                    document.extend(chunk(super::RES_XML_END_ELEMENT_TYPE, &header, &body));
                }
            }
        }
        chunk(super::RES_XML_TYPE, &[], &document)
    }

    /// A manifest like a real one: the attribute names are resolved through
    /// the resource map, as aapt2 writes them.
    pub(crate) fn manifest(package: &'static str, min_sdk: u32, utf8: bool) -> Vec<u8> {
        let strings = [
            "versionCode",
            "versionName",
            "minSdkVersion",
            "targetSdkVersion",
            "name",
            "required",
            "manifest",
            "package",
            package,
            "1.98",
            "uses-sdk",
            "uses-feature",
            "android.hardware.camera",
            "android.hardware.touchscreen",
        ];
        let ids = [
            super::ATTR_VERSION_CODE,
            super::ATTR_VERSION_NAME,
            super::ATTR_MIN_SDK,
            super::ATTR_TARGET_SDK,
            super::ATTR_NAME,
            super::ATTR_REQUIRED,
        ];
        let android = 99;
        document(
            &strings,
            &ids,
            &[
                (
                    Some(6),
                    vec![
                        (android, 0, Value::Int(198)),
                        (android, 1, Value::Text("1.98")),
                        (u32::MAX, 7, Value::Text(package)),
                    ],
                ),
                (
                    Some(10),
                    vec![
                        (android, 2, Value::Int(min_sdk)),
                        (android, 3, Value::Int(35)),
                    ],
                ),
                (None, vec![]),
                (
                    Some(11),
                    vec![(android, 4, Value::Text("android.hardware.camera"))],
                ),
                (None, vec![]),
                (
                    Some(11),
                    vec![
                        (android, 4, Value::Text("android.hardware.touchscreen")),
                        (android, 5, Value::Bool(false)),
                    ],
                ),
                (None, vec![]),
                (None, vec![]),
            ],
            utf8,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_manifest_in_utf16_and_utf8() {
        for utf8 in [false, true] {
            let facts = parse(&build::manifest("org.billthefarmer.editor", 21, utf8)).unwrap();
            assert_eq!(facts.package, "org.billthefarmer.editor");
            assert_eq!(facts.version_code, Some(198));
            assert_eq!(facts.version_name.as_deref(), Some("1.98"));
            assert_eq!(facts.min_sdk, Some(21));
            assert_eq!(facts.target_sdk, Some(35));
            // The camera is required; the touchscreen is declared optional.
            assert_eq!(facts.required_features, ["android.hardware.camera"]);
        }
    }

    #[test]
    fn refuses_what_is_not_binary_xml() {
        assert!(parse(b"<?xml version=\"1.0\"?><manifest/>").is_err());
        assert!(parse(&[]).is_err());
    }

    #[test]
    fn refuses_truncated_documents_without_panicking() {
        let whole = build::manifest("org.example.app", 21, false);
        for length in 0..whole.len() {
            let _ = parse(&whole[..length]);
        }
        // Every byte changed in turn: never a panic.
        for index in 0..whole.len() {
            let mut damaged = whole.clone();
            damaged[index] ^= 0xff;
            let _ = parse(&damaged);
        }
    }

    #[test]
    fn a_document_without_a_package_is_refused() {
        let document = build::document(
            &["manifest", "x"],
            &[],
            &[(Some(0), vec![]), (None, vec![])],
            false,
        );
        assert_eq!(
            parse(&document).unwrap_err(),
            "the manifest names no package"
        );
    }
}
