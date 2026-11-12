// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The receiver's stable refusal codes, `TUR-DIAG-RECEIVER-*`
//! (specification §29.3). Each has an HTTP status and a short message. The
//! message never quotes anything from the request.

use serde::Serialize;

/// Why a request was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Refusal {
    /// The path is not one the receiver serves.
    NotFound,
    /// The method is not accepted on this path.
    MethodNotAllowed,
    /// The upload is larger than the receiver accepts, sent or unpacked.
    TooLarge,
    /// The upload is not of the bundle media type.
    UnsupportedMediaType,
    /// The upload is not a bundle that verifies.
    NotABundle,
    /// The bundle has no valid `report.json`.
    NotAReport,
    /// The bundle was made with the technical redaction profile.
    TechnicalProfile,
    /// Too many uploads from one address.
    AddressLimit,
    /// Too many uploads from everyone.
    GlobalLimit,
    /// Too many uploads are being processed at once.
    Busy,
    /// No report has this status token.
    UnknownToken,
    /// The report could not be stored.
    Internal,
    /// The upload was cut short or could not be read in time.
    Incomplete,
    /// Too many status lookups from one address.
    LookupLimit,
}

impl Refusal {
    /// Every refusal, in code order.
    pub const ALL: [Refusal; 14] = [
        Refusal::NotFound,
        Refusal::MethodNotAllowed,
        Refusal::TooLarge,
        Refusal::UnsupportedMediaType,
        Refusal::NotABundle,
        Refusal::NotAReport,
        Refusal::TechnicalProfile,
        Refusal::AddressLimit,
        Refusal::GlobalLimit,
        Refusal::Busy,
        Refusal::UnknownToken,
        Refusal::Internal,
        Refusal::Incomplete,
        Refusal::LookupLimit,
    ];

    /// The stable code. The meaning of a released code never changes.
    pub const fn code(self) -> &'static str {
        match self {
            Refusal::NotFound => "TUR-DIAG-RECEIVER-001",
            Refusal::MethodNotAllowed => "TUR-DIAG-RECEIVER-002",
            Refusal::TooLarge => "TUR-DIAG-RECEIVER-003",
            Refusal::UnsupportedMediaType => "TUR-DIAG-RECEIVER-004",
            Refusal::NotABundle => "TUR-DIAG-RECEIVER-005",
            Refusal::NotAReport => "TUR-DIAG-RECEIVER-006",
            Refusal::TechnicalProfile => "TUR-DIAG-RECEIVER-007",
            Refusal::AddressLimit => "TUR-DIAG-RECEIVER-008",
            Refusal::GlobalLimit => "TUR-DIAG-RECEIVER-009",
            Refusal::Busy => "TUR-DIAG-RECEIVER-010",
            Refusal::UnknownToken => "TUR-DIAG-RECEIVER-011",
            Refusal::Internal => "TUR-DIAG-RECEIVER-012",
            Refusal::Incomplete => "TUR-DIAG-RECEIVER-013",
            Refusal::LookupLimit => "TUR-DIAG-RECEIVER-014",
        }
    }

    /// The HTTP status it is answered with.
    pub const fn status(self) -> u16 {
        match self {
            Refusal::NotFound | Refusal::UnknownToken => 404,
            Refusal::MethodNotAllowed => 405,
            Refusal::TooLarge => 413,
            Refusal::UnsupportedMediaType => 415,
            Refusal::NotABundle | Refusal::NotAReport | Refusal::TechnicalProfile => 422,
            Refusal::AddressLimit | Refusal::GlobalLimit | Refusal::LookupLimit => 429,
            Refusal::Busy => 503,
            Refusal::Internal => 500,
            Refusal::Incomplete => 400,
        }
    }

    /// The message for the person, a short sentence.
    pub const fn message(self) -> &'static str {
        match self {
            Refusal::NotFound => "There is nothing at this address.",
            Refusal::MethodNotAllowed => "This method is not accepted at this address.",
            Refusal::TooLarge => "The report is larger than this receiver accepts.",
            Refusal::UnsupportedMediaType => {
                "Reports are sent as application/vnd.tur.diag-bundle+zstd."
            }
            Refusal::NotABundle => "The upload is not a TUR diagnostic bundle that verifies.",
            Refusal::NotAReport => {
                "The bundle is not a problem report: report.json is missing or not valid."
            }
            Refusal::TechnicalProfile => {
                "Reports must be made with the default redaction profile."
            }
            Refusal::AddressLimit => "Too many reports from this address. Try again later.",
            Refusal::GlobalLimit => "The receiver is taking no more reports for now. Try again later.",
            Refusal::Busy => "The receiver is busy. Try again in a minute.",
            Refusal::UnknownToken => "No report has this status token.",
            Refusal::Internal => "The receiver could not store the report. Nothing was kept.",
            Refusal::Incomplete => "The upload was incomplete or too slow.",
            Refusal::LookupLimit => "Too many status requests from this address. Try again later.",
        }
    }
}

/// The body of a refusal (`schemas/report-receiver.schema.json`, `error`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ErrorBody {
    /// Version of the answer format.
    pub schema_version: u32,
    /// The stable code.
    pub code: &'static str,
    /// The short message.
    pub message: &'static str,
    /// For a bundle that did not verify, its `TUR-DIAG-BUNDLE-*` code.
    pub detail_code: Option<String>,
}

impl ErrorBody {
    /// The body for `refusal`.
    pub fn new(refusal: Refusal, detail_code: Option<String>) -> Self {
        ErrorBody {
            schema_version: 1,
            code: refusal.code(),
            message: refusal.message(),
            detail_code,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use tur_core::{ErrorCode, Family};

    use super::*;

    #[test]
    fn every_code_is_a_diag_code_and_unique() {
        let codes: BTreeSet<&str> = Refusal::ALL.iter().map(|r| r.code()).collect();
        assert_eq!(codes.len(), Refusal::ALL.len());
        for refusal in Refusal::ALL {
            let code: ErrorCode = refusal.code().parse().expect("parses");
            assert_eq!(code.family(), Family::Diag);
            assert!(refusal.code().starts_with("TUR-DIAG-RECEIVER-"));
            assert!(refusal.status() >= 400);
            assert!(!refusal.message().is_empty());
        }
    }

    #[test]
    fn released_codes_keep_their_meaning() {
        let pinned = [
            (Refusal::NotFound, "TUR-DIAG-RECEIVER-001", 404),
            (Refusal::MethodNotAllowed, "TUR-DIAG-RECEIVER-002", 405),
            (Refusal::TooLarge, "TUR-DIAG-RECEIVER-003", 413),
            (Refusal::UnsupportedMediaType, "TUR-DIAG-RECEIVER-004", 415),
            (Refusal::NotABundle, "TUR-DIAG-RECEIVER-005", 422),
            (Refusal::NotAReport, "TUR-DIAG-RECEIVER-006", 422),
            (Refusal::TechnicalProfile, "TUR-DIAG-RECEIVER-007", 422),
            (Refusal::AddressLimit, "TUR-DIAG-RECEIVER-008", 429),
            (Refusal::GlobalLimit, "TUR-DIAG-RECEIVER-009", 429),
            (Refusal::Busy, "TUR-DIAG-RECEIVER-010", 503),
            (Refusal::UnknownToken, "TUR-DIAG-RECEIVER-011", 404),
            (Refusal::Internal, "TUR-DIAG-RECEIVER-012", 500),
            (Refusal::Incomplete, "TUR-DIAG-RECEIVER-013", 400),
            (Refusal::LookupLimit, "TUR-DIAG-RECEIVER-014", 429),
        ];
        assert_eq!(pinned.len(), Refusal::ALL.len());
        for (refusal, code, status) in pinned {
            assert_eq!(refusal.code(), code);
            assert_eq!(refusal.status(), status);
        }
    }

    #[test]
    fn the_error_body_matches_the_schema() {
        let schema: serde_json::Value = serde_json::from_str(include_str!(
            "../../../schemas/report-receiver.schema.json"
        ))
        .expect("JSON");
        let body = serde_json::to_value(ErrorBody::new(Refusal::NotABundle, None)).expect("JSON");
        let node = &schema["$defs"]["error"];
        let properties: BTreeSet<&String> = node["properties"]
            .as_object()
            .expect("properties")
            .keys()
            .collect();
        let keys: BTreeSet<&String> = body.as_object().expect("object").keys().collect();
        assert_eq!(properties, keys);
    }
}
