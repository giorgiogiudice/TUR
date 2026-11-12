// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The text a person reads and accepts before P@x is switched on, and its
//! version.
//!
//! Enabling P@x with a cloud provider sends data off the machine, so nothing
//! is sent before the person has read what will be sent, to whom, and what
//! the provider keeps (addendum A section 8). The acceptance is recorded
//! with the version of the text and the provider it named
//! ([`crate::settings::ConsentRecord`]). If the text changes, its version
//! changes, and P@x asks again before its next task.
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
//! **The providers' terms have not been reviewed by the project**
//! (ADR-0004). The text says what P@x does and points at the provider's own
//! terms; it makes no statement about what those terms say.

use tur_pax_providers::ProviderId;

/// The version of the consent text. It changes whenever the text below
/// changes in a way a person should read again.
pub const CONSENT_TEXT_VERSION: &str = "1";

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
}

/// What P@x asks of a provider about storing conversations. A statement
/// about what the adapter sends (`docs/architecture/pax-providers.md`,
/// section 9), not about the provider's terms.
fn storing(subject: &Subject<'_>) -> String {
    let name = subject.name;
    match (subject.adapter, subject.native) {
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

/// The consent text for a provider.
///
/// The address of its terms is configuration, from the model list or the
/// person's own entry; none is compiled in.
pub fn consent_text(subject: &Subject<'_>) -> String {
    let name = subject.name;
    let terms = match subject.terms_url {
        Some(url) => format!("{name}'s data terms: {url}"),
        None => format!("{name}'s data terms are on its website."),
    };
    let terms = if subject.by_hand {
        format!(
            "{terms}\nYou added {name} yourself. TUR has not reviewed its terms and knows \
             nothing of how it handles what it is sent."
        )
    } else {
        terms
    };
    format!(
        "What P@x sends to {name}\n\
         \n\
         To do what you ask, P@x sends these to {name} over an encrypted connection:\n\
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
         P@x asks every provider not to store your conversations where the provider offers \
         that choice. {storing}\n\
         What {name} keeps anyway, and for how long, is governed by {name}'s terms, not by \
         TUR. TUR cannot control it.\n\
         {terms}\n\
         \n\
         You can see everything P@x did in Settings \u{25b8} P@x \u{25b8} Activity, and stop \
         P@x at any time.\n",
        storing = storing(subject),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn native(adapter: ProviderId) -> Subject<'static> {
        Subject {
            name: family_name(adapter),
            adapter,
            native: true,
            terms_url: Some("https://example.org/data-terms"),
            by_hand: false,
        }
    }

    #[test]
    fn the_text_names_the_provider_and_says_what_the_design_requires() {
        for adapter in ProviderId::ALL {
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
                // Addendum A section 18, item 5.
                "they are sent again whenever they matter",
                // Section 19, item 1.
                "any program you run can see your screen",
                // Section 21, item 6.
                "also while the screen is locked",
                // This build.
                "P@x cannot see or use the screen yet",
            ] {
                assert!(text.contains(phrase), "{name}: {phrase}");
            }
            assert!(
                text.contains(&format!("governed by {name}'s terms")),
                "{name}"
            );
            // No other provider is named in a text.
            for other in ProviderId::ALL {
                if other != adapter {
                    assert!(!text.contains(family_name(other)), "{name}");
                }
            }
            assert!(!text.contains("not reviewed"), "{name}");
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
        // The version is a short token a caller can send back.
        assert!(!CONSENT_TEXT_VERSION.is_empty());
        assert!(
            CONSENT_TEXT_VERSION
                .bytes()
                .all(|byte| byte.is_ascii_graphic())
        );
    }

    #[test]
    fn a_provider_reached_through_another_family_or_added_by_hand_is_said_to_be_unknown() {
        // A compatible service, reached with the OpenAI family at its own
        // address, named as its entry names it.
        let compatible = Subject {
            name: "DeepSeek",
            adapter: ProviderId::OpenAi,
            native: false,
            terms_url: Some("https://example.org/deepseek-terms"),
            by_hand: false,
        };
        let text = consent_text(&compatible);
        assert!(text.starts_with("What P@x sends to DeepSeek\n"));
        assert!(text.contains("Whether DeepSeek honours that is not known to TUR."));
        assert!(!text.contains("OpenAI"));
        assert!(!text.contains("not reviewed"));
        // Added by hand: TUR says it has not reviewed anything.
        let by_hand = consent_text(&Subject {
            name: "my-gateway",
            by_hand: true,
            terms_url: None,
            ..compatible
        });
        assert!(by_hand.contains("You added my-gateway yourself. TUR has not reviewed its terms"));
        let anthropic_family = consent_text(&Subject {
            adapter: ProviderId::Anthropic,
            ..compatible
        });
        assert!(anthropic_family.contains("Whether DeepSeek keeps it is not known to TUR."));
    }
}
