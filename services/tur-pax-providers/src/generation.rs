// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! Generation settings: temperature, top-p, top-k, stop sequences and how
//! much the model reasons, and how each adapter family writes them.
//!
//! A [`Generation`] is part of a configuration and of every request made
//! from it. Nothing in it is required, and nothing is sent that was not
//! set: an empty one leaves every default to the provider, which is what
//! every request sent before this module did.
//!
//! **Two checks.** This module knows what each family's interface has a
//! place for, and refuses the rest ([`Generation::check`], called by
//! [`crate::ProviderConfig::validate`]): a setting is never dropped in
//! silence. What one *model* accepts, and in which ranges, is the model
//! list's to declare; the session service holds a person's choice to that
//! declaration before it reaches this layer.
//!
//! **What each family is sent**, from the providers' documentation read on
//! 2026-11-13 (`docs/architecture/pax-providers.md`, section 15):
//!
//! | Setting | Anthropic Messages | OpenAI Responses | Google Interactions | Chat Completions |
//! |---|---|---|---|---|
//! | temperature | refused | `temperature` | refused | `temperature` |
//! | top-p | refused | `top_p` | refused | `top_p` |
//! | top-k | refused | refused | refused | `top_k` |
//! | stop | `stop_sequences` | refused | `generation_config.stop_sequences` | `stop` |
//! | effort `none` | `thinking: {type: disabled}` | `reasoning.effort` | refused | `reasoning_effort`, or `chat_template_kwargs.enable_thinking: false` |
//! | effort `minimal` | refused | `reasoning.effort` | `generation_config.thinking_level` | `reasoning_effort`, or `enable_thinking: true` |
//! | effort `low` … `xhigh` | `output_config.effort` | `reasoning.effort` | `generation_config.thinking_level` | as `minimal` |
//! | effort `max` | `output_config.effort` | `reasoning.effort` | refused | as `minimal` |
//! | thinking budget | refused | refused | refused | `thinking_budget_tokens`, with `enable_thinking` only |

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::model::ProviderId;

/// The most stop sequences a configuration may hold. A model's own limit,
/// which can be lower (four for OpenAI's Chat Completions), is declared in
/// the model list.
pub const STOP_MAX: usize = 16;

/// The longest stop sequence, in characters.
pub const STOP_LENGTH_MAX: usize = 200;

/// The largest top-k this layer accepts.
pub const TOP_K_MAX: u32 = 100_000;

/// The largest thinking budget this layer accepts, in tokens.
pub const THINKING_BUDGET_MAX: u32 = 10_000_000;

/// How much the model reasons before it answers.
///
/// The names are the union of the levels the providers document; no
/// family takes all of them, and a model takes fewer still.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Effort {
    /// No reasoning: thinking switched off.
    None,
    /// The least reasoning short of none.
    Minimal,
    /// Little reasoning.
    Low,
    /// Some reasoning.
    Medium,
    /// Much reasoning.
    High,
    /// More than high.
    Xhigh,
    /// As much as the model can.
    Max,
}

impl Effort {
    /// Every level, from none to max.
    pub const ALL: [Effort; 7] = [
        Effort::None,
        Effort::Minimal,
        Effort::Low,
        Effort::Medium,
        Effort::High,
        Effort::Xhigh,
        Effort::Max,
    ];

    /// The level as the interfaces and the configuration write it.
    pub const fn as_str(self) -> &'static str {
        match self {
            Effort::None => "none",
            Effort::Minimal => "minimal",
            Effort::Low => "low",
            Effort::Medium => "medium",
            Effort::High => "high",
            Effort::Xhigh => "xhigh",
            Effort::Max => "max",
        }
    }

    /// The level with this name.
    pub fn from_text(text: &str) -> Option<Effort> {
        Effort::ALL
            .into_iter()
            .find(|effort| effort.as_str() == text)
    }
}

impl std::fmt::Display for Effort {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// How a service of the OpenAI-compatible family is told how much to
/// reason. Only that family reads it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReasoningStyle {
    /// `reasoning_effort`, the field of the Chat Completions reference,
    /// which DeepSeek, Mistral, xAI and Groq document too.
    #[default]
    ReasoningEffort,
    /// llama.cpp's `llama-server`: the chat template's switch,
    /// `chat_template_kwargs: {"enable_thinking": …}`, which its README
    /// documents; and, with a budget, `thinking_budget_tokens`.
    EnableThinking,
}

impl ReasoningStyle {
    /// Whether this is the default style.
    pub fn is_default(&self) -> bool {
        *self == ReasoningStyle::default()
    }
}

/// The generation settings of a configuration. Each one that is set is
/// sent, in the form its family takes; one that is not set is not sent.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Generation {
    /// Sampling temperature.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    /// Nucleus sampling: the share of probability mass sampled from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f64>,
    /// Sample from this many most likely tokens.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top_k: Option<u32>,
    /// Text that ends the response where it appears.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub stop: Vec<String>,
    /// How much the model reasons before it answers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<Effort>,
    /// The most tokens the model may reason for, where the interface takes
    /// a number rather than a level.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thinking_budget: Option<u32>,
    /// How an OpenAI-compatible service is told how much to reason.
    #[serde(default, skip_serializing_if = "ReasoningStyle::is_default")]
    pub reasoning_style: ReasoningStyle,
}

fn refuse(family: ProviderId, what: &str, why: &str) -> Result<(), String> {
    Err(format!("the {family} family cannot be sent {what}: {why}"))
}

impl Generation {
    /// Whether nothing is set: every default is the provider's.
    pub fn is_empty(&self) -> bool {
        *self == Generation::default()
    }

    /// The names of the settings that are set.
    pub fn set_names(&self) -> Vec<&'static str> {
        let mut names = Vec::new();
        if self.temperature.is_some() {
            names.push("temperature");
        }
        if self.top_p.is_some() {
            names.push("top_p");
        }
        if self.top_k.is_some() {
            names.push("top_k");
        }
        if !self.stop.is_empty() {
            names.push("stop");
        }
        if self.effort.is_some() {
            names.push("effort");
        }
        if self.thinking_budget.is_some() {
            names.push("thinking_budget");
        }
        names
    }

    /// Check the settings for a family: each value in its form, and only
    /// settings the family's interface has a place for. The reason names
    /// the setting and quotes no value.
    ///
    /// # Errors
    ///
    /// A sentence saying what is wrong.
    pub fn check(&self, family: ProviderId) -> Result<(), String> {
        if let Some(temperature) = self.temperature
            && !(temperature.is_finite() && (0.0..=2.0).contains(&temperature))
        {
            return Err("temperature must be a number from 0 to 2".to_owned());
        }
        if let Some(top_p) = self.top_p
            && !(top_p.is_finite() && (0.0..=1.0).contains(&top_p))
        {
            return Err("top_p must be a number from 0 to 1".to_owned());
        }
        if self.top_k.is_some_and(|top_k| top_k > TOP_K_MAX) {
            return Err(format!("top_k must be at most {TOP_K_MAX}"));
        }
        if self.stop.len() > STOP_MAX {
            return Err(format!("at most {STOP_MAX} stop sequences"));
        }
        for stop in &self.stop {
            let length = stop.chars().count();
            if length == 0 || length > STOP_LENGTH_MAX || stop.contains('\0') {
                return Err(format!(
                    "a stop sequence must be 1 to {STOP_LENGTH_MAX} characters"
                ));
            }
        }
        if self
            .thinking_budget
            .is_some_and(|budget| budget > THINKING_BUDGET_MAX)
        {
            return Err(format!(
                "thinking_budget must be at most {THINKING_BUDGET_MAX}"
            ));
        }
        if !self.reasoning_style.is_default() && family != ProviderId::OpenAiChat {
            return Err("reasoning_style is read by the openai-chat family only".to_owned());
        }
        let sampling = self.temperature.is_some() || self.top_p.is_some() || self.top_k.is_some();
        match family {
            ProviderId::Anthropic => {
                if sampling {
                    return refuse(
                        family,
                        "temperature, top_p or top_k",
                        "Messages API version 2026-10-21 rejects them on every model",
                    );
                }
                if self.effort == Some(Effort::Minimal) {
                    return refuse(family, "the effort minimal", "the API has no such level");
                }
                if self.thinking_budget.is_some() {
                    return refuse(
                        family,
                        "a thinking budget",
                        "budget_tokens is removed on current models; effort is the control",
                    );
                }
            }
            ProviderId::OpenAi => {
                if self.top_k.is_some() {
                    return refuse(family, "top_k", "the Responses API has no such field");
                }
                if !self.stop.is_empty() {
                    return refuse(
                        family,
                        "stop sequences",
                        "the Responses API has no such field",
                    );
                }
                if self.thinking_budget.is_some() {
                    return refuse(
                        family,
                        "a thinking budget",
                        "the Responses API takes reasoning.effort",
                    );
                }
            }
            ProviderId::Google => {
                if sampling {
                    return refuse(
                        family,
                        "temperature, top_p or top_k",
                        "the Interactions API's generation_config has no such fields; Gemini \
                         3.6 and later ignore them",
                    );
                }
                if matches!(self.effort, Some(Effort::None | Effort::Max)) {
                    return refuse(
                        family,
                        "the effort none or max",
                        "thinking_level has no such level",
                    );
                }
                if self.thinking_budget.is_some() {
                    return refuse(
                        family,
                        "a thinking budget",
                        "the Interactions API takes thinking_level, and a request with both is \
                         rejected",
                    );
                }
            }
            ProviderId::OpenAiChat => {
                if self.thinking_budget.is_some()
                    && self.reasoning_style != ReasoningStyle::EnableThinking
                {
                    return refuse(
                        family,
                        "a thinking budget",
                        "only a server that switches thinking in its chat template \
                         (reasoning_style enable_thinking) takes one",
                    );
                }
            }
        }
        Ok(())
    }
}

/// Write the settings into an Anthropic Messages request.
pub(crate) fn apply_anthropic(generation: &Generation, body: &mut Value) {
    if !generation.stop.is_empty() {
        body["stop_sequences"] = json!(generation.stop);
    }
    match generation.effort {
        None => {}
        Some(Effort::None) => body["thinking"] = json!({ "type": "disabled" }),
        Some(effort) => body["output_config"] = json!({ "effort": effort.as_str() }),
    }
}

/// Write the settings into an OpenAI Responses request.
pub(crate) fn apply_openai(generation: &Generation, body: &mut Value) {
    if let Some(temperature) = generation.temperature {
        body["temperature"] = json!(temperature);
    }
    if let Some(top_p) = generation.top_p {
        body["top_p"] = json!(top_p);
    }
    if let Some(effort) = generation.effort {
        body["reasoning"] = json!({ "effort": effort.as_str() });
    }
}

/// Write the settings into a Google Interactions request, whose
/// `generation_config` is already there.
pub(crate) fn apply_google(generation: &Generation, body: &mut Value) {
    if !generation.stop.is_empty() {
        body["generation_config"]["stop_sequences"] = json!(generation.stop);
    }
    if let Some(effort) = generation.effort {
        body["generation_config"]["thinking_level"] = json!(effort.as_str());
    }
}

/// Write the settings into a Chat Completions request.
pub(crate) fn apply_openai_chat(generation: &Generation, body: &mut Value) {
    if let Some(temperature) = generation.temperature {
        body["temperature"] = json!(temperature);
    }
    if let Some(top_p) = generation.top_p {
        body["top_p"] = json!(top_p);
    }
    if let Some(top_k) = generation.top_k {
        body["top_k"] = json!(top_k);
    }
    if !generation.stop.is_empty() {
        body["stop"] = json!(generation.stop);
    }
    match generation.reasoning_style {
        ReasoningStyle::ReasoningEffort => {
            if let Some(effort) = generation.effort {
                body["reasoning_effort"] = json!(effort.as_str());
            }
        }
        ReasoningStyle::EnableThinking => {
            if let Some(effort) = generation.effort {
                body["chat_template_kwargs"] = json!({ "enable_thinking": effort != Effort::None });
            }
            if let Some(budget) = generation.thinking_budget {
                body["thinking_budget_tokens"] = json!(budget);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all_set() -> Generation {
        Generation {
            temperature: Some(0.5),
            top_p: Some(0.9),
            top_k: Some(40),
            stop: vec!["END".to_owned()],
            effort: Some(Effort::High),
            thinking_budget: Some(1024),
            reasoning_style: ReasoningStyle::default(),
        }
    }

    #[test]
    fn levels_have_one_name_each() {
        for effort in Effort::ALL {
            assert_eq!(Effort::from_text(effort.as_str()), Some(effort));
            let text = serde_json::to_string(&effort).unwrap();
            assert_eq!(text, format!("\"{}\"", effort.as_str()));
        }
        assert_eq!(Effort::from_text("ultra"), None);
    }

    #[test]
    fn nothing_set_is_empty_and_writes_nothing() {
        let empty = Generation::default();
        assert!(empty.is_empty());
        assert!(empty.set_names().is_empty());
        for family in ProviderId::ALL {
            assert!(empty.check(family).is_ok());
        }
        let mut body = json!({ "generation_config": { "max_output_tokens": 1 } });
        let before = body.clone();
        apply_anthropic(&empty, &mut body);
        apply_openai(&empty, &mut body);
        apply_google(&empty, &mut body);
        apply_openai_chat(&empty, &mut body);
        assert_eq!(body, before);
        assert_eq!(serde_json::to_string(&empty).unwrap(), "{}");
    }

    #[test]
    fn values_out_of_their_form_are_refused_for_every_family() {
        for family in ProviderId::ALL {
            let bad = [
                Generation {
                    temperature: Some(2.5),
                    ..Generation::default()
                },
                Generation {
                    temperature: Some(f64::NAN),
                    ..Generation::default()
                },
                Generation {
                    top_p: Some(1.5),
                    ..Generation::default()
                },
                Generation {
                    top_k: Some(TOP_K_MAX + 1),
                    ..Generation::default()
                },
                Generation {
                    stop: vec![String::new()],
                    ..Generation::default()
                },
                Generation {
                    stop: vec!["x".to_owned(); STOP_MAX + 1],
                    ..Generation::default()
                },
            ];
            for generation in bad {
                assert!(
                    generation.check(family).is_err(),
                    "{family}: {generation:?}"
                );
            }
        }
    }

    #[test]
    fn each_family_refuses_what_its_interface_has_no_place_for() {
        let only = |change: fn(&mut Generation)| {
            let mut generation = Generation::default();
            change(&mut generation);
            generation
        };
        let temperature = only(|g| g.temperature = Some(0.3));
        let top_k = only(|g| g.top_k = Some(20));
        let stop = only(|g| g.stop = vec!["\n\n".to_owned()]);
        let minimal = only(|g| g.effort = Some(Effort::Minimal));
        let none = only(|g| g.effort = Some(Effort::None));
        let max = only(|g| g.effort = Some(Effort::Max));
        let budget = only(|g| g.thinking_budget = Some(2048));

        assert!(temperature.check(ProviderId::Anthropic).is_err());
        assert!(top_k.check(ProviderId::Anthropic).is_err());
        assert!(minimal.check(ProviderId::Anthropic).is_err());
        assert!(budget.check(ProviderId::Anthropic).is_err());
        assert!(stop.check(ProviderId::Anthropic).is_ok());
        assert!(none.check(ProviderId::Anthropic).is_ok());
        assert!(max.check(ProviderId::Anthropic).is_ok());

        assert!(temperature.check(ProviderId::OpenAi).is_ok());
        assert!(top_k.check(ProviderId::OpenAi).is_err());
        assert!(stop.check(ProviderId::OpenAi).is_err());
        assert!(budget.check(ProviderId::OpenAi).is_err());
        assert!(none.check(ProviderId::OpenAi).is_ok());
        assert!(max.check(ProviderId::OpenAi).is_ok());

        assert!(temperature.check(ProviderId::Google).is_err());
        assert!(top_k.check(ProviderId::Google).is_err());
        assert!(none.check(ProviderId::Google).is_err());
        assert!(max.check(ProviderId::Google).is_err());
        assert!(budget.check(ProviderId::Google).is_err());
        assert!(minimal.check(ProviderId::Google).is_ok());
        assert!(stop.check(ProviderId::Google).is_ok());

        assert!(temperature.check(ProviderId::OpenAiChat).is_ok());
        assert!(top_k.check(ProviderId::OpenAiChat).is_ok());
        assert!(stop.check(ProviderId::OpenAiChat).is_ok());
        assert!(budget.check(ProviderId::OpenAiChat).is_err());
        let mut budget_with_switch = budget.clone();
        budget_with_switch.reasoning_style = ReasoningStyle::EnableThinking;
        assert!(budget_with_switch.check(ProviderId::OpenAiChat).is_ok());
        assert!(budget_with_switch.check(ProviderId::OpenAi).is_err());
    }

    #[test]
    fn the_wire_fields_of_each_family() {
        let mut generation = all_set();
        generation.top_k = None;
        generation.thinking_budget = None;
        let mut anthropic = json!({});
        let mut claude = generation.clone();
        claude.temperature = None;
        claude.top_p = None;
        apply_anthropic(&claude, &mut anthropic);
        assert_eq!(
            anthropic,
            json!({ "stop_sequences": ["END"], "output_config": { "effort": "high" } })
        );
        claude.effort = Some(Effort::None);
        let mut off = json!({});
        apply_anthropic(&claude, &mut off);
        assert_eq!(
            off,
            json!({ "stop_sequences": ["END"], "thinking": { "type": "disabled" } })
        );

        let mut openai = json!({});
        apply_openai(&generation, &mut openai);
        assert_eq!(
            openai,
            json!({ "temperature": 0.5, "top_p": 0.9, "reasoning": { "effort": "high" } })
        );

        let mut google = json!({ "generation_config": { "max_output_tokens": 8 } });
        apply_google(&generation, &mut google);
        assert_eq!(
            google,
            json!({ "generation_config": {
                "max_output_tokens": 8, "stop_sequences": ["END"], "thinking_level": "high"
            } })
        );

        let mut chat = json!({});
        apply_openai_chat(&all_set(), &mut chat);
        assert_eq!(
            chat,
            json!({
                "temperature": 0.5, "top_p": 0.9, "top_k": 40, "stop": ["END"],
                "reasoning_effort": "high"
            })
        );
        let mut llama = all_set();
        llama.reasoning_style = ReasoningStyle::EnableThinking;
        let mut server = json!({});
        apply_openai_chat(&llama, &mut server);
        assert_eq!(
            server["chat_template_kwargs"],
            json!({ "enable_thinking": true })
        );
        assert_eq!(server["thinking_budget_tokens"], json!(1024));
        assert!(server.get("reasoning_effort").is_none());
        llama.effort = Some(Effort::None);
        llama.thinking_budget = None;
        let mut quiet = json!({});
        apply_openai_chat(&llama, &mut quiet);
        assert_eq!(
            quiet["chat_template_kwargs"],
            json!({ "enable_thinking": false })
        );
        assert!(quiet.get("thinking_budget_tokens").is_none());
    }

    #[test]
    fn the_style_is_written_only_when_it_is_not_the_default() {
        let mut generation = Generation {
            effort: Some(Effort::Low),
            ..Generation::default()
        };
        assert_eq!(
            serde_json::to_value(&generation).unwrap(),
            json!({ "effort": "low" })
        );
        generation.reasoning_style = ReasoningStyle::EnableThinking;
        assert_eq!(
            serde_json::to_value(&generation).unwrap(),
            json!({ "effort": "low", "reasoning_style": "enable_thinking" })
        );
        let back: Generation = serde_json::from_value(
            json!({ "effort": "low", "reasoning_style": "enable_thinking" }),
        )
        .unwrap();
        assert_eq!(back, generation);
        assert!(serde_json::from_value::<Generation>(json!({ "seed": 1 })).is_err());
    }
}
