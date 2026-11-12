// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The text a person reads and accepts before P@x is switched on, and its
//! version.
//!
//! Enabling P@x with a cloud provider sends data off the machine, so nothing
//! is sent before the person has read what will be sent, to whom, and what
//! the provider keeps (addendum A section 8). The acceptance is recorded
//! with the version of the text and the provider it named
//! ([`crate::settings::ConsentRecord`]). If the text changes, its version
//! changes, and P@x asks again before its next task. Another provider needs
//! its own acceptance.
//!
//! The wording is the consent screen of the desktop design
//! (`docs/architecture/desktop-experience.md`, section 9.12, step 3), with
//! what later sections of addendum A require the text to say:
//!
//! * section 18, item 5: entries read somewhere and kept are sent again
//!   whenever they matter;
//! * section 19, item 1: other software in the session can see the screen
//!   and use the keyboard and pointer too;
//! * section 21, item 6: a scheduled task runs with nobody there, also
//!   while the session is locked, and what it reads goes to the provider.
//!
//! One paragraph is this build's own: it says that this version cannot see
//! or use the screen. And one sentence of the design is left out: the stop
//! key, which this build does not have.
//!
//! **What a provider keeps** is said per provider (ADR-0004, amended 12
//! November 2026), from its entry's data terms: whether it keeps what it is
//! sent, for how long, whether it trains on it, and where, with the day TUR
//! read its terms. Where TUR has not read them, and always for a provider
//! the person added by hand, the text says "not reviewed by TUR". A model
//! that runs on this computer is said to: nothing goes to a company.

use tur_pax_providers::ProviderId;

use crate::settings::DataTerms;

/// The version of the consent text. It changes whenever the text below
/// changes in a way a person should read again. Version 2 says what each
/// provider's terms say, or that TUR has not reviewed them.
pub const CONSENT_TEXT_VERSION: &str = "2";

/// The words that say TUR has not read a provider's terms.
pub const NOT_REVIEWED: &str = "not reviewed by TUR";

/// The name of the company behind an adapter family, as a person reads it.
/// Used for a provider of that family that the model list does not name.
/// The OpenAI-compatible family has no company: a provider of it is always
/// an entry with a name of its own, and this is only its family's name.
pub const fn family_name(adapter: ProviderId) -> &'static str {
    match adapter {
        ProviderId::Anthropic => "Anthropic",
        ProviderId::OpenAi => "OpenAI",
        ProviderId::Google => "Google",
        ProviderId::OpenAiChat => "an OpenAI-compatible service",
    }
}

/// The provider a consent text is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Subject<'a> {
    /// Its name, as a person reads it.
    pub name: &'a str,
    /// The adapter family P@x talks to it with.
    pub adapter: ProviderId,
    /// Whether it is the family's own company at its own public address,
    /// whose interface P@x's adapter was written for.
    pub native: bool,
    /// Where it says how it handles data sent to its interface.
    pub terms_url: Option<&'a str>,
    /// Whether the person added it by hand, so that TUR has said nothing
    /// about it at all.
    pub by_hand: bool,
    /// Its data terms as TUR read them, if TUR read them. Never given for a
    /// provider added by hand.
    pub terms: Option<&'a DataTerms>,
    /// Whether the model runs on this computer, at an address of this
    /// computer.
    pub runs_locally: bool,
}

/// What P@x asks of a provider about storing conversations. A statement
/// about what the adapter sends (`docs/architecture/pax-providers.md`,
/// section 9), not about the provider's terms.
fn storing(subject: &Subject<'_>) -> String {
    let name = subject.name;
    match (subject.adapter, subject.native) {
        (ProviderId::OpenAiChat, _) => format!(
            "P@x sends {name} the whole conversation with each request. The interface P@x \
             uses has no setting that asks a service not to store a conversation."
        ),
        (ProviderId::Anthropic, true) => format!(
            "P@x sends {name} the whole conversation with each request. The interface P@x \
             uses has no setting for storing a conversation, so there is nothing for P@x to \
             switch off."
        ),
        (_, true) => format!(
            "{name}'s interface stores what it is sent unless it is asked not to. P@x asks it \
             not to, with every request."
        ),
        (ProviderId::Anthropic, false) => format!(
            "P@x sends {name} the whole conversation with each request. The interface P@x \
             uses has no setting for storing a conversation. Whether {name} keeps it is not \
             known to TUR."
        ),
        (_, false) => format!(
            "P@x asks {name} not to store the conversation, with every request, in the way \
             the interface it uses allows. Whether {name} honours that is not known to TUR."
        ),
    }
}

/// The statement of the terms, or that the terms do not say it.
fn said(statement: Option<&String>) -> &str {
    statement.map_or("not stated in what TUR read", String::as_str)
}

/// What the provider's terms say, as TUR read them, or that TUR has not.
fn terms_summary(subject: &Subject<'_>) -> String {
    let name = subject.name;
    match subject.terms.filter(|_| !subject.by_hand) {
        Some(terms) if terms.reviewed_on.is_some() => format!(
            "What {name}'s terms say, as TUR read them on {date}:\n\
             - whether it keeps what it is sent: {stores}\n\
             - for how long: {retention}\n\
             - whether it trains models on it: {trains}\n\
             - where it processes it: {location}\n\
             This is TUR's summary. {name}'s own terms govern, and they can change.\n",
            date = terms.reviewed_on.as_deref().unwrap_or_default(),
            stores = said(terms.stores_requests.as_ref()),
            retention = said(terms.retention.as_ref()),
            trains = said(terms.trains_on_data.as_ref()),
            location = said(terms.processing_location.as_ref()),
        ),
        _ => format!(
            "{name}'s terms are {NOT_REVIEWED}: TUR has not read what {name} keeps, for how \
             long, whether it trains on it, or where it processes it. Read them before you go \
             on.\n"
        ),
    }
}

/// The consent text for a provider.
///
/// The address of its terms and what they say are configuration, from the
/// model list or the person's own entry; none is compiled in.
pub fn consent_text(subject: &Subject<'_>) -> String {
    let name = subject.name;
    let terms = match subject.terms_url {
        Some(url) => format!("{name}'s data terms: {url}"),
        None => format!("{name}'s data terms are on its website."),
    };
    let terms = if subject.by_hand {
        format!(
            "{terms}\nYou added {name} yourself. TUR has not reviewed its terms and knows \
             nothing of how it handles what it is sent ({NOT_REVIEWED})."
        )
    } else {
        terms
    };
    let local = subject.runs_locally && !subject.by_hand;
    let how = if local {
        format!("{name}, which runs on this computer")
    } else {
        format!("{name} over an encrypted connection")
    };
    let keeps = if local {
        format!(
            "{name} runs on this computer. What P@x sends it stays on this computer: no \
             company receives it. TUR checked that its address is this computer; what \
             {name} itself keeps on this computer is up to how it was set up.\n"
        )
    } else {
        format!(
            "P@x asks every provider not to store your conversations where the provider \
             offers that choice. {storing}\n\
             {summary}\
             What {name} keeps anyway, and for how long, is governed by {name}'s terms, not by \
             TUR. TUR cannot control it.\n\
             {terms}\n",
            storing = storing(subject),
            summary = terms_summary(subject),
        )
    };
    format!(
        "What P@x sends to {name}\n\
         \n\
         To do what you ask, P@x sends these to {how}:\n\
         - what you ask, as text;\n\
         - pictures of the window P@x is working in, or of the screen when a task needs it;\n\
         - the names and states of buttons and other controls in that window;\n\
         - the results of the tools and commands P@x runs for you;\n\
         - the things P@x remembers that matter to the task.\n\
         \n\
         Never sent:\n\
         - the sound of your voice (speech is turned into text on this computer);\n\
         - the trusted screen, sign-in prompts and password managers;\n\
         - windows of applications you exclude;\n\
         - passwords and keys stored by the system;\n\
         - support bundles, unless you attach one yourself.\n\
         \n\
         In this version\n\
         P@x cannot see or use the screen yet, so no pictures and no names of controls are \
         sent. It uses TUR's own tools and commands that only read.\n\
         \n\
         Reminders and scheduled tasks\n\
         A scheduled task runs when you may not be there, also while the screen is locked. It \
         can look things up and make changes that can be undone. What it reads is sent to \
         {name} like the results of any other task. Anything else it would need waits for \
         you.\n\
         \n\
         Memory\n\
         Memory stays on this computer. When something P@x remembers matters to a task, it is \
         sent to {name} with that task. That includes things P@x read somewhere and you chose \
         to keep: they are sent again whenever they matter.\n\
         \n\
         Other programs\n\
         On this desktop, any program you run can see your screen and use the keyboard and \
         pointer. TUR limits what P@x may do. It cannot limit other programs you start.\n\
         \n\
         What {name} keeps\n\
         {keeps}\
         \n\
         You can see everything P@x did in Settings \u{25b8} P@x \u{25b8} Activity, and stop \
         P@x at any time.\n",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const COMPANIES: [ProviderId; 3] = [
        ProviderId::Anthropic,
        ProviderId::OpenAi,
        ProviderId::Google,
    ];

    fn native(adapter: ProviderId) -> Subject<'static> {
        Subject {
            name: family_name(adapter),
            adapter,
            native: true,
            terms_url: Some("https://example.org/data-terms"),
            by_hand: false,
            terms: None,
            runs_locally: false,
        }
    }

    fn reviewed() -> DataTerms {
        DataTerms {
            reviewed_on: Some("2026-11-12".to_owned()),
            runs_locally: false,
            stores_requests: Some("yes, as long as the account exists".to_owned()),
            retention: Some("as long as necessary".to_owned()),
            trains_on_data: Some("yes, unless you opt out".to_owned()),
            processing_location: Some("the People's Republic of China".to_owned()),
            terms_url: Some("https://example.org/privacy".to_owned()),
            sources: vec!["https://example.org/privacy".to_owned()],
        }
    }

    #[test]
    fn the_text_names_the_provider_and_says_what_the_design_requires() {
        for adapter in COMPANIES {
            let subject = native(adapter);
            let name = subject.name;
            let text = consent_text(&subject);
            assert!(
                text.starts_with(&format!("What P@x sends to {name}\n")),
                "{name}"
            );
            for phrase in [
                // The desktop design's consent screen.
                "what you ask, as text",
                "the results of the tools and commands P@x runs for you",
                "the things P@x remembers that matter to the task",
                "Never sent:",
                "passwords and keys stored by the system",
                "P@x asks every provider not to store your conversations",
                "TUR cannot control it.",
                "https://example.org/data-terms",
                "over an encrypted connection",
                // Addendum A section 18, item 5.
                "they are sent again whenever they matter",
                // Section 19, item 1.
                "any program you run can see your screen",
                // Section 21, item 6.
                "also while the screen is locked",
                // This build.
                "P@x cannot see or use the screen yet",
                // The terms of the three companies were not read.
                "not reviewed by TUR",
            ] {
                assert!(text.contains(phrase), "{name}: {phrase}");
            }
            assert!(
                text.contains(&format!("governed by {name}'s terms")),
                "{name}"
            );
            // No other provider is named in a text.
            for other in COMPANIES {
                if other != adapter {
                    assert!(!text.contains(family_name(other)), "{name}");
                }
            }
            // Without an address the text still says where the terms are.
            let without = consent_text(&Subject {
                terms_url: None,
                ..subject
            });
            assert!(without.contains(&format!("{name}'s data terms are on its website.")));
            assert!(!without.contains("https://"));
        }
    }

    #[test]
    fn the_paragraph_on_storing_differs_by_provider() {
        let anthropic = consent_text(&native(ProviderId::Anthropic));
        let openai = consent_text(&native(ProviderId::OpenAi));
        let google = consent_text(&native(ProviderId::Google));
        assert!(anthropic.contains("has no setting for storing a conversation"));
        assert!(openai.contains("P@x asks it not to, with every request"));
        assert!(google.contains("P@x asks it not to, with every request"));
        assert!(!anthropic.contains("asks it not to"));
        // The compatible family sends no such setting, and does not say it does.
        let compatible = consent_text(&Subject {
            name: "DeepSeek",
            adapter: ProviderId::OpenAiChat,
            native: false,
            ..native(ProviderId::OpenAi)
        });
        assert!(compatible.contains("has no setting that asks a service not to store"));
        assert!(!compatible.contains("asks DeepSeek not to store"));
        // The version is a short token a caller can send back.
        assert!(!CONSENT_TEXT_VERSION.is_empty());
        assert!(
            CONSENT_TEXT_VERSION
                .bytes()
                .all(|byte| byte.is_ascii_graphic())
        );
    }

    #[test]
    fn reviewed_terms_are_shown_with_their_date_and_unreviewed_ones_say_so() {
        let terms = reviewed();
        let subject = Subject {
            name: "DeepSeek",
            adapter: ProviderId::OpenAiChat,
            native: false,
            terms_url: Some("https://example.org/privacy"),
            by_hand: false,
            terms: Some(&terms),
            runs_locally: false,
        };
        let text = consent_text(&subject);
        assert!(text.starts_with("What P@x sends to DeepSeek\n"));
        for phrase in [
            "What DeepSeek's terms say, as TUR read them on 2026-11-12:",
            "- whether it keeps what it is sent: yes, as long as the account exists",
            "- for how long: as long as necessary",
            "- whether it trains models on it: yes, unless you opt out",
            "- where it processes it: the People's Republic of China",
            "DeepSeek's own terms govern, and they can change.",
            "DeepSeek's data terms: https://example.org/privacy",
        ] {
            assert!(text.contains(phrase), "{phrase}\n{text}");
        }
        assert!(!text.contains(NOT_REVIEWED));
        assert!(!text.contains("OpenAI"));

        // A statement the terms do not make is said to be missing.
        let partial = DataTerms {
            retention: None,
            ..reviewed()
        };
        let text = consent_text(&Subject {
            terms: Some(&partial),
            ..subject
        });
        assert!(text.contains("- for how long: not stated in what TUR read"));

        // Not reviewed: said in those words.
        let text = consent_text(&Subject {
            terms: None,
            ..subject
        });
        assert!(text.contains("DeepSeek's terms are not reviewed by TUR"));
        assert!(!text.contains("as TUR read them"));
    }

    #[test]
    fn a_provider_added_by_hand_is_never_shown_as_reviewed() {
        let terms = reviewed();
        let text = consent_text(&Subject {
            name: "my-gateway",
            adapter: ProviderId::OpenAiChat,
            native: false,
            terms_url: None,
            by_hand: true,
            // Whatever the person's entry claims.
            terms: Some(&terms),
            runs_locally: true,
        });
        assert!(text.contains("You added my-gateway yourself. TUR has not reviewed its terms"));
        assert!(text.contains(NOT_REVIEWED));
        assert!(!text.contains("as TUR read them"));
        assert!(!text.contains("runs on this computer"));
        // Through another family, as before.
        let anthropic_family = consent_text(&Subject {
            name: "Elsewhere",
            adapter: ProviderId::Anthropic,
            native: false,
            terms_url: None,
            by_hand: false,
            terms: None,
            runs_locally: false,
        });
        assert!(anthropic_family.contains("Whether Elsewhere keeps it is not known to TUR."));
    }

    #[test]
    fn a_model_on_this_computer_is_said_to_stay_on_it() {
        let local = DataTerms {
            runs_locally: true,
            ..DataTerms::default()
        };
        let text = consent_text(&Subject {
            name: "Ollama",
            adapter: ProviderId::OpenAiChat,
            native: false,
            terms_url: None,
            by_hand: false,
            terms: Some(&local),
            runs_locally: true,
        });
        assert!(text.contains("P@x sends these to Ollama, which runs on this computer:"));
        assert!(text.contains("What P@x sends it stays on this computer: no company receives it."));
        assert!(!text.contains("encrypted connection"));
        assert!(!text.contains(NOT_REVIEWED));
    }
}
