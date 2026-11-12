// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! Reading and writing the JSON documents of the store.

use serde::Serialize;
use serde_json::error::Category;

/// Why a document could not be read, without quoting it.
///
/// A `serde_json` error message can repeat a value from the document
/// ("invalid type: string \"...\""). The value may be the text of a memory
/// entry, so the message is not passed on: only the kind of failure and its
/// position are.
pub(crate) fn describe_error(err: &serde_json::Error) -> String {
    let what = match err.classify() {
        Category::Io => "could not be read",
        Category::Syntax => "is not valid JSON",
        Category::Data => "does not have the expected fields and types",
        Category::Eof => "ends before the JSON document is complete",
    };
    format!(
        "the file {what} (line {}, column {})",
        err.line(),
        err.column()
    )
}

/// A value as the bytes of a stored document: compact JSON and a line break.
pub(crate) fn to_document<T: Serialize>(value: &T) -> Result<Vec<u8>, String> {
    let mut bytes =
        serde_json::to_vec(value).map_err(|_| "the record could not be serialised".to_owned())?;
    bytes.push(b'\n');
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_error_description_never_quotes_the_document() {
        let secret = "hunter2-do-not-print";
        for document in [
            format!("{{\"n\": \"{secret}\"}}"),
            format!("{{\"n\": \"{secret}"),
            format!("[\"{secret}\"]"),
            format!("\"{secret}\""),
        ] {
            let err = serde_json::from_str::<std::collections::BTreeMap<String, u32>>(&document)
                .expect_err("not a map of numbers");
            let described = describe_error(&err);
            assert!(!described.contains(secret), "{described}");
            assert!(described.starts_with("the file "), "{described}");
        }
    }

    #[test]
    fn a_document_ends_with_a_line_break() {
        assert_eq!(to_document(&[1, 2]).expect("serialises"), b"[1,2]\n");
    }
}
