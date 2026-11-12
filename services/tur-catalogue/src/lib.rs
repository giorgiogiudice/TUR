// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The TUR application catalogue and router.
//!
//! TUR is app-centric (ADR-0007): the user asks for an application and the
//! system chooses the best route to run it. This crate holds the two pieces
//! that decision needs:
//!
//! * the **catalogue**: one entry per application, listing the editions it
//!   exists in, what each edition requires, and what is known about how well
//!   each route runs it ([`model`], [`validate`], [`catalogue`]);
//! * the **router**: [`route()`], a pure function from an entry, facts about
//!   the machine and the user's preferences to an ordered list of
//!   [`RouteOption`]s, following the routing ladder of ADR-0007.
//!
//! [`search()`] finds entries by name and alias. Nothing in this crate
//! installs or launches an application, and nothing uses the network.
//!
//! This crate handles "the user named an application". The case "the user
//! has a file" belongs to the runtime manager's inspection (`tur-runtime`),
//! which this crate does not depend on.
//!
//! ```
//! use tur_catalogue::{HostFacts, Preferences, Route, RouteRequest, parse_entry, route};
//!
//! let entry = parse_entry(
//!     r#"
//!     schema_version = 1
//!     id = "org.example.editor"
//!     name = "Example Editor"
//!     publisher = "Example Project"
//!     category = "productivity"
//!
//!     [[editions]]
//!     kind = "flatpak"
//!     flatpak_id = "org.example.Editor"
//!
//!     [[editions]]
//!     kind = "windows"
//!     obtain = "publisher-page"
//!     download_page = "https://example.org/download"
//!     "#,
//! )
//! .unwrap();
//!
//! let options = route(
//!     &RouteRequest::new(&entry),
//!     &HostFacts::default(),
//!     &Preferences::default(),
//! );
//! // The native Linux edition comes first; Wine follows; no virtual machine
//! // is offered, because nothing is known against Wine.
//! let routes: Vec<Route> = options.iter().map(|option| option.route).collect();
//! assert_eq!(routes, [Route::Flatpak, Route::Wine]);
//! assert!(options[0].recommended);
//! ```

#![forbid(unsafe_code)]

pub mod catalogue;
pub mod codes;
pub mod date;
pub mod host;
pub mod model;
pub mod report;
pub mod route;
pub mod search;
pub mod url;
pub mod validate;

pub use catalogue::{Catalogue, Statistics};
pub use codes::{Code, Problem};
pub use date::Date;
pub use host::{Arch, GpuClass, HostFacts, Preferences, parse_host_facts, parse_preferences};
pub use model::{
    AndroidStore, Category, Edition, EditionKind, Entry, EntryKind, Installer, Licence, Obtain,
    Packaging, Rating, Record, Reference, Reporter, Requirement, Route, SCHEMA_VERSION,
    parse_entry,
};
pub use route::{
    ALTERNATIVE_TEXT_IDS, Alternative, AlternativeKind, CAVEAT_TEXT_IDS, Caveat, CaveatCode,
    Outlook, Params, Prerequisite, PrerequisiteCode, RatingSource, ReasonCode, ReasonPart,
    RouteOption, RouteRequest, Severity, recommended, requirement_severity, route,
};
pub use search::{MatchKind, SearchHit, search};
pub use validate::{validate_catalogue, validate_entry};
