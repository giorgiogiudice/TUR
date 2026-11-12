// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! Identifiers of memory entries, schedules and run records.
//!
//! An identifier is a fixed prefix and a sequence number in lower-case
//! hexadecimal, for example `m-0000002a`. It is also the stem of the file that
//! holds the record, so its form is checked wherever one is read: only
//! lower-case letters, digits and the hyphen, which cannot name another
//! directory. Sequence numbers are never reused while the store's counter
//! file survives, so a forgotten entry's identifier does not come back
//! attached to something else.

use std::fmt;

use serde::{Deserialize, Serialize};

// Declares one identifier type. The three types are distinct so that a memory
// identifier cannot be passed where a schedule identifier is expected.
macro_rules! id_type {
    ($(#[$doc:meta])* $name:ident, $prefix:literal) => {
        $(#[$doc])*
        #[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
        #[serde(try_from = "String", into = "String")]
        pub struct $name(String);

        impl $name {
            /// The prefix every identifier of this kind starts with.
            pub const PREFIX: &'static str = $prefix;

            /// Parse an identifier, checking its form: the prefix, then 8 to
            /// 16 lower-case hexadecimal digits.
            pub fn parse(text: &str) -> Option<Self> {
                let digits = text.strip_prefix(Self::PREFIX)?;
                let well_formed = (8..=16).contains(&digits.len())
                    && digits
                        .bytes()
                        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte));
                well_formed.then(|| $name(text.to_owned()))
            }

            /// The identifier for a sequence number.
            pub(crate) fn from_sequence(sequence: u64) -> Self {
                $name(format!("{}{sequence:08x}", Self::PREFIX))
            }

            /// The identifier as text.
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl TryFrom<String> for $name {
            type Error = String;

            fn try_from(text: String) -> Result<Self, Self::Error> {
                Self::parse(&text).ok_or_else(|| {
                    format!("\"{text}\" is not an identifier of the form {}<hex>", Self::PREFIX)
                })
            }
        }

        impl From<$name> for String {
            fn from(id: $name) -> String {
                id.0
            }
        }
    };
}

id_type!(
    /// The identifier of a memory entry, for example `m-0000002a`.
    EntryId,
    "m-"
);

id_type!(
    /// The identifier of a schedule, for example `s-00000003`.
    ScheduleId,
    "s-"
);

id_type!(
    /// The identifier of a run record, for example `r-00000140`.
    RunId,
    "r-"
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sequence_numbers_round_trip() {
        let id = EntryId::from_sequence(42);
        assert_eq!(id.as_str(), "m-0000002a");
        assert_eq!(EntryId::parse("m-0000002a"), Some(id));
        assert_eq!(
            ScheduleId::from_sequence(u64::MAX).as_str(),
            "s-ffffffffffffffff"
        );
        assert!(ScheduleId::parse("s-ffffffffffffffff").is_some());
        assert_eq!(RunId::from_sequence(1).to_string(), "r-00000001");
    }

    #[test]
    fn malformed_identifiers_are_rejected() {
        let malformed = [
            "",
            "m-",
            "m-1",
            "m-0000002A",
            "m-0000002g",
            "s-0000002a",
            "m-0000002a ",
            " m-0000002a",
            "m-0000002a.json",
            "m-../../etc",
            "m-00000000000000000",
            "../m-0000002a",
            "m-0000002a/x",
        ];
        for text in malformed {
            assert_eq!(EntryId::parse(text), None, "{text:?}");
        }
    }

    #[test]
    fn serde_checks_the_form() {
        let id: EntryId = serde_json::from_str("\"m-0000002a\"").expect("a valid identifier");
        assert_eq!(id.as_str(), "m-0000002a");
        assert_eq!(
            serde_json::to_string(&id).expect("serialises"),
            "\"m-0000002a\""
        );
        assert!(serde_json::from_str::<EntryId>("\"s-0000002a\"").is_err());
        assert!(serde_json::from_str::<EntryId>("\"m-../../x\"").is_err());
        assert!(serde_json::from_str::<ScheduleId>("7").is_err());
    }
}
