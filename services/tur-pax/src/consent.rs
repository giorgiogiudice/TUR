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

/// The name of a provider, as a person reads it.
pub const fn provider_name(provider: ProviderId) -> &'static str {
    match provider {
        ProviderId::Anthropic => "Anthropic",
        ProviderId::OpenAi => "OpenAI",
        ProviderId::Google => "Google",
    }
}

/// What P@x asks of this provider about storing conversations. A statement
/// about what the adapter sends (`docs/architecture/pax-providers.md`,
/// section 9), not about the provider's terms.
const fn storing(provider: ProviderId) -> &'static str {
    match provider {
        ProviderId::Anthropic => {
            "P@x sends Anthropic the whole conversation with each request. The interface P@x \
             uses has no setting for storing a conversation, so there is nothing for P@x to \
             switch off."
        }
        ProviderId::OpenAi => {
            "OpenAI's interface stores what it is sent unless it is asked not to. P@x asks it \
             not to, with every request."
        }
        ProviderId::Google => {
            "Google's interface stores what it is sent unless it is asked not to. P@x asks it \
             not to, with every request."
        }
    }
}

/// The consent text for a provider.
///
/// `terms_url` is where the provider says how it handles data sent to its
/// interface, from the model list. The address is configuration; none is
/// compiled in.
pub fn consent_text(provider: ProviderId, terms_url: Option<&str>) -> String {
    let name = provider_name(provider);
    let terms = match terms_url {
        Some(url) => format!("{name}'s data terms: {url}"),
        None => format!("{name}'s data terms are on its website."),
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
        storing = storing(provider),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_text_names_the_provider_and_says_what_the_design_requires() {
        for provider in ProviderId::ALL {
            let name = provider_name(provider);
            let text = consent_text(provider, Some("https://example.org/data-terms"));
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
                if other != provider {
                    assert!(!text.contains(provider_name(other)), "{name}");
                }
            }
            // Without an address the text still says where the terms are.
            let without = consent_text(provider, None);
            assert!(without.contains(&format!("{name}'s data terms are on its website.")));
            assert!(!without.contains("https://"));
        }
    }

    #[test]
    fn the_paragraph_on_storing_differs_by_provider() {
        let anthropic = consent_text(ProviderId::Anthropic, None);
        let openai = consent_text(ProviderId::OpenAi, None);
        let google = consent_text(ProviderId::Google, None);
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
}
