// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! Generation settings, as P@x offers them: what a model accepts, what the
//! person chose, and the check that holds the one to the other.
//!
//! * A [`SettingsSpec`] is what the model list declares a model (or every
//!   model of a provider) accepts: temperature, top-p, top-k, stop
//!   sequences, a reasoning effort, a thinking budget, an output limit, and
//!   for a model on this computer the context size; each with its range,
//!   and with a default where P@x sends one when the person chose none.
//! * [`Values`] is what the person chose for one model of one provider,
//!   kept in the settings file (`generation`).
//! * [`check`] refuses a choice the declaration does not allow, with a
//!   sentence for the person; the provider layer then refuses what the
//!   family's interface has no place for
//!   ([`tur_pax_providers::Generation::check`]).
//!
//! A model with no declaration takes no setting but its output limit: TUR
//! has nothing to hold a choice to, and a setting a provider does not take
//! is a request it rejects.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use tur_pax_providers::generation::{STOP_LENGTH_MAX, STOP_MAX};
use tur_pax_providers::{Effort, Generation, ProviderId, ReasoningStyle};

/// A range of numbers, and the value P@x sends when the person chose none.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NumberRange {
    /// The smallest value.
    pub min: f64,
    /// The largest value.
    pub max: f64,
    /// What P@x sends when the person chose none; absent: nothing is sent
    /// and the provider's own default applies.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<f64>,
}

/// A range of whole numbers, and the value P@x sends when the person chose
/// none.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CountRange {
    /// The smallest value.
    pub min: u32,
    /// The largest value.
    pub max: u32,
    /// What P@x uses when the person chose none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<u32>,
}

/// The reasoning efforts a model takes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EffortSpec {
    /// The levels it takes.
    pub values: Vec<Effort>,
    /// What P@x sends when the person chose none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<Effort>,
    /// What the provider applies when nothing is sent, as its documentation
    /// says. Only for the rule below.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_default: Option<Effort>,
    /// Temperature, top-p and top-k are taken only at these levels: the
    /// model refuses or ignores them while it reasons. Empty: at any level.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sampling_only_with: Vec<Effort>,
}

/// How many stop sequences a model takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StopSpec {
    /// The most.
    pub max: u32,
}

/// What a model accepts. A setting that is absent is not accepted.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SettingsSpec {
    /// Sampling temperature.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature: Option<NumberRange>,
    /// Nucleus sampling.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top_p: Option<NumberRange>,
    /// Sampling from the most likely tokens.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top_k: Option<CountRange>,
    /// Stop sequences.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop: Option<StopSpec>,
    /// How much the model reasons.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<EffortSpec>,
    /// The most tokens it may reason for.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thinking_budget: Option<CountRange>,
    /// The output limit of one response.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<CountRange>,
    /// The context a model on this computer is started with, in tokens.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_tokens: Option<CountRange>,
    /// The documentation the declaration was read from.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sources: Vec<String>,
    /// When it was read, `YYYY-MM-DD`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checked: Option<String>,
}

impl SettingsSpec {
    /// These declarations, with what `over` declares in their place: a
    /// model's own over its provider's.
    #[must_use]
    pub fn overlaid(&self, over: Option<&SettingsSpec>) -> SettingsSpec {
        let Some(over) = over else {
            return self.clone();
        };
        SettingsSpec {
            temperature: over.temperature.or(self.temperature),
            top_p: over.top_p.or(self.top_p),
            top_k: over.top_k.or(self.top_k),
            stop: over.stop.or(self.stop),
            effort: over.effort.clone().or_else(|| self.effort.clone()),
            thinking_budget: over.thinking_budget.or(self.thinking_budget),
            max_output_tokens: over.max_output_tokens.or(self.max_output_tokens),
            context_tokens: over.context_tokens.or(self.context_tokens),
            sources: if over.sources.is_empty() {
                self.sources.clone()
            } else {
                over.sources.clone()
            },
            checked: over.checked.clone().or_else(|| self.checked.clone()),
        }
    }

    /// Check the declaration for a family: each range in order, each
    /// default inside its range, each declared setting one the family's
    /// interface has a place for. The reason names what is wrong.
    ///
    /// # Errors
    ///
    /// A sentence.
    pub fn check(&self, family: ProviderId, style: ReasoningStyle) -> Result<(), String> {
        for (name, range) in [("temperature", self.temperature), ("top_p", self.top_p)] {
            if let Some(range) = range {
                let ordered =
                    range.min.is_finite() && range.max.is_finite() && range.min <= range.max;
                let inside = range.default.is_none_or(|value| {
                    value.is_finite() && (range.min..=range.max).contains(&value)
                });
                if !ordered || !inside {
                    return Err(format!("the range of {name} is not in order"));
                }
            }
        }
        for (name, range) in [
            ("top_k", self.top_k),
            ("thinking_budget", self.thinking_budget),
            ("max_output_tokens", self.max_output_tokens),
            ("context_tokens", self.context_tokens),
        ] {
            if let Some(range) = range
                && (range.min > range.max
                    || range
                        .default
                        .is_some_and(|value| !(range.min..=range.max).contains(&value)))
            {
                return Err(format!("the range of {name} is not in order"));
            }
        }
        if self.max_output_tokens.is_some_and(|range| range.min == 0) {
            return Err("an output limit starts at 1".to_owned());
        }
        if let Some(stop) = self.stop
            && (stop.max == 0 || stop.max as usize > STOP_MAX)
        {
            return Err(format!("stop sequences are 1 to {STOP_MAX}"));
        }
        if let Some(effort) = &self.effort {
            let mut seen = Vec::new();
            for value in &effort.values {
                if seen.contains(value) {
                    return Err("a reasoning effort is listed twice".to_owned());
                }
                seen.push(*value);
            }
            if effort.values.is_empty()
                || effort
                    .default
                    .is_some_and(|value| !effort.values.contains(&value))
                || effort
                    .sampling_only_with
                    .iter()
                    .any(|value| !effort.values.contains(value))
            {
                return Err("the reasoning efforts are not consistent".to_owned());
            }
        }
        // Every declared setting must be one the family can be sent.
        let mut probes: Vec<Generation> = Vec::new();
        let mut probe = |change: &dyn Fn(&mut Generation)| {
            let mut generation = Generation {
                reasoning_style: style,
                ..Generation::default()
            };
            change(&mut generation);
            probes.push(generation);
        };
        if let Some(range) = self.temperature {
            probe(&|g: &mut Generation| g.temperature = Some(range.min));
        }
        if let Some(range) = self.top_p {
            probe(&|g: &mut Generation| g.top_p = Some(range.min));
        }
        if let Some(range) = self.top_k {
            probe(&|g: &mut Generation| g.top_k = Some(range.min));
        }
        if self.stop.is_some() {
            probe(&|g: &mut Generation| g.stop = vec!["stop".to_owned()]);
        }
        if let Some(effort) = &self.effort {
            for value in &effort.values {
                let value = *value;
                probe(&move |g: &mut Generation| g.effort = Some(value));
            }
        }
        if let Some(range) = self.thinking_budget {
            probe(&|g: &mut Generation| g.thinking_budget = Some(range.min));
        }
        for generation in probes {
            generation.check(family)?;
        }
        Ok(())
    }
}

/// What the person chose for one model. A setting that is absent is the
/// model's default.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Values {
    /// Sampling temperature.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    /// Nucleus sampling.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f64>,
    /// Sampling from the most likely tokens.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top_k: Option<u32>,
    /// Stop sequences.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub stop: Vec<String>,
    /// How much the model reasons.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<Effort>,
    /// The most tokens it may reason for.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thinking_budget: Option<u32>,
    /// The output limit of one response.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<u32>,
    /// The context of a model on this computer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_tokens: Option<u32>,
}

impl Values {
    /// Whether nothing is chosen.
    pub fn is_empty(&self) -> bool {
        *self == Values::default()
    }

    /// The form of each value, whatever the model: numbers finite and in
    /// the widest range any provider takes, stop sequences short text.
    ///
    /// # Errors
    ///
    /// A sentence.
    pub fn check_form(&self) -> Result<(), String> {
        if self
            .temperature
            .is_some_and(|value| !(value.is_finite() && (0.0..=2.0).contains(&value)))
        {
            return Err("A temperature is a number from 0 to 2.".to_owned());
        }
        if self
            .top_p
            .is_some_and(|value| !(value.is_finite() && (0.0..=1.0).contains(&value)))
        {
            return Err("Top-p is a number from 0 to 1.".to_owned());
        }
        if self.stop.len() > STOP_MAX
            || self.stop.iter().any(|stop| {
                stop.is_empty() || stop.chars().count() > STOP_LENGTH_MAX || stop.contains('\0')
            })
        {
            return Err(format!(
                "Up to {STOP_MAX} stop sequences of 1 to {STOP_LENGTH_MAX} characters."
            ));
        }
        if self.max_output_tokens == Some(0) || self.context_tokens == Some(0) {
            return Err("An output limit and a context size start at 1.".to_owned());
        }
        Ok(())
    }

    /// The settings for the provider layer.
    pub fn generation(&self, style: ReasoningStyle) -> Generation {
        Generation {
            temperature: self.temperature,
            top_p: self.top_p,
            top_k: self.top_k,
            stop: self.stop.clone(),
            effort: self.effort,
            thinking_budget: self.thinking_budget,
            reasoning_style: style,
        }
    }
}

/// What the person chose for one model of one provider, as the settings
/// file keeps it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Choice {
    /// The provider's identifier.
    pub provider: String,
    /// The model's name.
    pub model: String,
    /// What was chosen.
    pub settings: Values,
}

fn list(efforts: &[Effort]) -> String {
    let names: Vec<&str> = efforts.iter().map(|effort| effort.as_str()).collect();
    match names.split_last() {
        None => String::new(),
        Some((last, [])) => (*last).to_owned(),
        Some((last, rest)) => format!("{} or {last}", rest.join(", ")),
    }
}

fn number(value: f64) -> String {
    let text = format!("{value}");
    text.strip_suffix(".0").map_or(text.clone(), str::to_owned)
}

/// The sentence that says a model takes sampling settings only at some
/// efforts, if it does.
pub fn sampling_rule(spec: &SettingsSpec) -> Option<String> {
    let effort = spec.effort.as_ref()?;
    if effort.sampling_only_with.is_empty() {
        return None;
    }
    Some(format!(
        "This model takes temperature, top-p and top-k only when its reasoning effort is {}; \
         while it reasons it refuses or ignores them.",
        list(&effort.sampling_only_with)
    ))
}

/// The values that take effect: the person's, and the declared default of
/// each setting they did not choose. A default for temperature, top-p or
/// top-k is left out at an effort that does not take them.
pub fn effective(spec: Option<&SettingsSpec>, chosen: &Values) -> Values {
    let Some(spec) = spec else {
        return chosen.clone();
    };
    let mut values = chosen.clone();
    let effort_default = spec.effort.as_ref().and_then(|effort| effort.default);
    values.effort = chosen.effort.or(effort_default);
    let sampling_allowed = match &spec.effort {
        Some(effort) if !effort.sampling_only_with.is_empty() => values
            .effort
            .or(effort.provider_default)
            .is_some_and(|value| effort.sampling_only_with.contains(&value)),
        _ => true,
    };
    if sampling_allowed {
        values.temperature = chosen
            .temperature
            .or(spec.temperature.and_then(|range| range.default));
        values.top_p = chosen.top_p.or(spec.top_p.and_then(|range| range.default));
        values.top_k = chosen.top_k.or(spec.top_k.and_then(|range| range.default));
    }
    values.thinking_budget = chosen
        .thinking_budget
        .or(spec.thinking_budget.and_then(|range| range.default));
    values.max_output_tokens = chosen
        .max_output_tokens
        .or(spec.max_output_tokens.and_then(|range| range.default));
    values.context_tokens = chosen
        .context_tokens
        .or(spec.context_tokens.and_then(|range| range.default));
    values
}

/// Check values against what the model declares, then against what the
/// family can be sent.
///
/// # Errors
///
/// A sentence for the person: what the model does not take, and why.
pub fn check(
    spec: Option<&SettingsSpec>,
    values: &Values,
    family: ProviderId,
    style: ReasoningStyle,
) -> Result<(), String> {
    values.check_form()?;
    let Some(spec) = spec else {
        let only_output = Values {
            max_output_tokens: values.max_output_tokens,
            ..Values::default()
        };
        if *values != only_output {
            return Err(
                "TUR has no list of the settings this model accepts, so only its output limit \
                 can be set."
                    .to_owned(),
            );
        }
        return Ok(());
    };
    let refused = |what: &str| Err(format!("This model does not take {what}."));
    let outside = |what: &str, min: String, max: String| {
        Err(format!(
            "{what} must be from {min} to {max} for this model."
        ))
    };
    if let Some(value) = values.temperature {
        match spec.temperature {
            None => return refused("a temperature"),
            Some(range) if !(range.min..=range.max).contains(&value) => {
                return outside("The temperature", number(range.min), number(range.max));
            }
            _ => {}
        }
    }
    if let Some(value) = values.top_p {
        match spec.top_p {
            None => return refused("top-p"),
            Some(range) if !(range.min..=range.max).contains(&value) => {
                return outside("Top-p", number(range.min), number(range.max));
            }
            _ => {}
        }
    }
    for (value, range, what, name) in [
        (values.top_k, spec.top_k, "top-k", "Top-k"),
        (
            values.thinking_budget,
            spec.thinking_budget,
            "a thinking budget",
            "The thinking budget",
        ),
        (
            values.max_output_tokens,
            spec.max_output_tokens,
            "an output limit",
            "The output limit",
        ),
        (
            values.context_tokens,
            spec.context_tokens,
            "a context size",
            "The context size",
        ),
    ] {
        if let Some(value) = value {
            match range {
                // Every model takes an output limit; without a declared
                // range, any.
                None if what == "an output limit" => {}
                None => return refused(what),
                Some(range) if !(range.min..=range.max).contains(&value) => {
                    return outside(name, range.min.to_string(), range.max.to_string());
                }
                _ => {}
            }
        }
    }
    if !values.stop.is_empty() {
        match spec.stop {
            None => return refused("stop sequences"),
            Some(stop) if values.stop.len() > stop.max as usize => {
                return Err(format!(
                    "This model takes at most {} stop sequences.",
                    stop.max
                ));
            }
            _ => {}
        }
    }
    if let Some(value) = values.effort {
        match &spec.effort {
            None => return refused("a reasoning effort"),
            Some(effort) if !effort.values.contains(&value) => {
                return Err(format!(
                    "This model's reasoning effort can be {}.",
                    list(&effort.values)
                ));
            }
            _ => {}
        }
    }
    let sampling = values.temperature.is_some() || values.top_p.is_some() || values.top_k.is_some();
    if sampling
        && let Some(effort) = &spec.effort
        && !effort.sampling_only_with.is_empty()
    {
        let at = values.effort.or(effort.provider_default);
        if !at.is_some_and(|value| effort.sampling_only_with.contains(&value)) {
            return Err(format!(
                "{} Set the reasoning effort to {} first.",
                sampling_rule(spec).unwrap_or_default(),
                list(&effort.sampling_only_with)
            ));
        }
    }
    if values.thinking_budget.is_some() && values.effort == Some(Effort::None) {
        return Err("A thinking budget needs the model to reason: its effort is none.".to_owned());
    }
    values
        .generation(style)
        .check(family)
        .map_err(|detail| format!("The provider's interface does not take this: {detail}."))
}

/// What a model takes, for the person to read: each accepted setting with
/// its range and default.
pub fn accepted(spec: Option<&SettingsSpec>) -> Value {
    let Some(spec) = spec else {
        return json!({});
    };
    let mut out = Map::new();
    let mut put = |name: &str, value: Value| {
        out.insert(name.to_owned(), value);
    };
    if let Some(range) = spec.temperature {
        put("temperature", json!(range));
    }
    if let Some(range) = spec.top_p {
        put("top_p", json!(range));
    }
    if let Some(range) = spec.top_k {
        put("top_k", json!(range));
    }
    if let Some(stop) = spec.stop {
        put("stop", json!(stop));
    }
    if let Some(effort) = &spec.effort {
        put("effort", json!(effort));
    }
    if let Some(range) = spec.thinking_budget {
        put("thinking_budget", json!(range));
    }
    if let Some(range) = spec.max_output_tokens {
        put("max_output_tokens", json!(range));
    }
    if let Some(range) = spec.context_tokens {
        put("context_tokens", json!(range));
    }
    Value::Object(out)
}

/// Read values from the JSON object of `SetGenerationSettings`: each
/// member is a setting, `null` clears it, an absent one is left as it was.
///
/// # Errors
///
/// A sentence naming the member that is not a setting or not of its type.
pub fn merge(current: &Values, given: &Map<String, Value>) -> Result<Values, String> {
    let mut values = current.clone();
    for (name, value) in given {
        let number = || -> Result<Option<f64>, String> {
            match value {
                Value::Null => Ok(None),
                other => other
                    .as_f64()
                    .map(Some)
                    .ok_or_else(|| format!("{name} is not a number")),
            }
        };
        let count = || -> Result<Option<u32>, String> {
            match value {
                Value::Null => Ok(None),
                other => other
                    .as_u64()
                    .and_then(|number| u32::try_from(number).ok())
                    .map(Some)
                    .ok_or_else(|| format!("{name} is not a whole number")),
            }
        };
        match name.as_str() {
            "temperature" => values.temperature = number()?,
            "top_p" => values.top_p = number()?,
            "top_k" => values.top_k = count()?,
            "thinking_budget" => values.thinking_budget = count()?,
            "max_output_tokens" => values.max_output_tokens = count()?,
            "context_tokens" => values.context_tokens = count()?,
            "effort" => {
                values.effort = match value {
                    Value::Null => None,
                    Value::String(text) => Some(
                        Effort::from_text(text)
                            .ok_or_else(|| "effort is not a level P@x knows".to_owned())?,
                    ),
                    _ => return Err("effort is not a level P@x knows".to_owned()),
                };
            }
            "stop" => {
                values.stop = match value {
                    Value::Null => Vec::new(),
                    Value::Array(items) => items
                        .iter()
                        .map(|item| {
                            item.as_str()
                                .map(str::to_owned)
                                .ok_or_else(|| "stop is not a list of text".to_owned())
                        })
                        .collect::<Result<Vec<String>, String>>()?,
                    _ => return Err("stop is not a list of text".to_owned()),
                };
            }
            _ => return Err(format!("{name} is not a generation setting")),
        }
    }
    Ok(values)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(text: &str) -> SettingsSpec {
        serde_json::from_str(text).expect("a declaration")
    }

    fn values(text: &str) -> Values {
        serde_json::from_str(text).expect("values")
    }

    /// Like OpenAI's reasoning models: sampling only without reasoning.
    fn reasoning_model() -> SettingsSpec {
        spec(
            r#"{ "temperature": {"min": 0, "max": 2}, "top_p": {"min": 0, "max": 1},
                 "effort": {"values": ["none", "low", "medium", "high"],
                            "provider_default": "medium", "sampling_only_with": ["none"]},
                 "max_output_tokens": {"min": 1, "max": 128000} }"#,
        )
    }

    #[test]
    fn a_choice_inside_the_declaration_passes_and_one_outside_is_refused_in_words() {
        let model = reasoning_model();
        let ok = values(r#"{"effort": "none", "temperature": 0.3}"#);
        assert_eq!(
            check(
                Some(&model),
                &ok,
                ProviderId::OpenAi,
                ReasoningStyle::default()
            ),
            Ok(())
        );
        let hot = values(r#"{"effort": "none", "temperature": 2.5}"#);
        assert_eq!(
            check(
                Some(&model),
                &hot,
                ProviderId::OpenAi,
                ReasoningStyle::default()
            ),
            Err("A temperature is a number from 0 to 2.".to_owned())
        );
        let level = values(r#"{"effort": "xhigh"}"#);
        assert_eq!(
            check(
                Some(&model),
                &level,
                ProviderId::OpenAi,
                ReasoningStyle::default()
            ),
            Err("This model's reasoning effort can be none, low, medium or high.".to_owned())
        );
        let top_k = values(r#"{"top_k": 5}"#);
        assert_eq!(
            check(
                Some(&model),
                &top_k,
                ProviderId::OpenAi,
                ReasoningStyle::default()
            ),
            Err("This model does not take top-k.".to_owned())
        );
    }

    #[test]
    fn sampling_is_refused_while_a_model_reasons_that_refuses_it() {
        let model = reasoning_model();
        // No effort chosen: the provider's default, medium, reasons.
        let error = check(
            Some(&model),
            &values(r#"{"temperature": 0.3}"#),
            ProviderId::OpenAi,
            ReasoningStyle::default(),
        )
        .expect_err("refused");
        assert!(error.starts_with("This model takes temperature, top-p and top-k only when its reasoning effort is none"), "{error}");
        assert!(
            error.ends_with("Set the reasoning effort to none first."),
            "{error}"
        );
        let error = check(
            Some(&model),
            &values(r#"{"top_p": 0.9, "effort": "high"}"#),
            ProviderId::OpenAi,
            ReasoningStyle::default(),
        )
        .expect_err("refused");
        assert!(
            error.contains("only when its reasoning effort is none"),
            "{error}"
        );
    }

    #[test]
    fn a_model_without_a_declaration_takes_only_an_output_limit() {
        let only = values(r#"{"max_output_tokens": 2048}"#);
        assert_eq!(
            check(
                None,
                &only,
                ProviderId::OpenAiChat,
                ReasoningStyle::default()
            ),
            Ok(())
        );
        let error = check(
            None,
            &values(r#"{"temperature": 0.1}"#),
            ProviderId::OpenAiChat,
            ReasoningStyle::default(),
        )
        .expect_err("refused");
        assert!(error.starts_with("TUR has no list"), "{error}");
    }

    #[test]
    fn defaults_apply_unless_chosen_and_sampling_defaults_give_way_to_reasoning() {
        let local = spec(
            r#"{ "temperature": {"min": 0, "max": 2, "default": 0.7},
                 "top_k": {"min": 0, "max": 1000, "default": 20},
                 "effort": {"values": ["none", "medium"], "default": "none"},
                 "context_tokens": {"min": 2048, "max": 32768, "default": 8192} }"#,
        );
        let none = effective(Some(&local), &Values::default());
        assert_eq!(none.temperature, Some(0.7));
        assert_eq!(none.top_k, Some(20));
        assert_eq!(none.effort, Some(Effort::None));
        assert_eq!(none.context_tokens, Some(8192));
        let chosen = effective(
            Some(&local),
            &values(r#"{"temperature": 0.2, "effort": "medium"}"#),
        );
        assert_eq!(chosen.temperature, Some(0.2));
        assert_eq!(chosen.effort, Some(Effort::Medium));

        let model = reasoning_model();
        let mut with_default = model.clone();
        with_default.temperature = Some(NumberRange {
            min: 0.0,
            max: 2.0,
            default: Some(1.0),
        });
        // Medium reasons, so the default temperature is not sent.
        assert_eq!(
            effective(Some(&with_default), &Values::default()).temperature,
            None
        );
        assert_eq!(
            effective(Some(&with_default), &values(r#"{"effort": "none"}"#)).temperature,
            Some(1.0)
        );
    }

    #[test]
    fn a_declaration_the_family_cannot_be_sent_is_not_valid() {
        let claude = spec(r#"{ "temperature": {"min": 0, "max": 1} }"#);
        assert!(
            claude
                .check(ProviderId::Anthropic, ReasoningStyle::default())
                .is_err()
        );
        let gemini = spec(r#"{ "effort": {"values": ["none", "low"]} }"#);
        assert!(
            gemini
                .check(ProviderId::Google, ReasoningStyle::default())
                .is_err()
        );
        let fine = spec(
            r#"{ "effort": {"values": ["low", "high"], "default": "low"}, "stop": {"max": 4} }"#,
        );
        assert!(
            fine.check(ProviderId::Anthropic, ReasoningStyle::default())
                .is_ok()
        );
        let backwards = spec(r#"{ "top_k": {"min": 10, "max": 1} }"#);
        assert!(
            backwards
                .check(ProviderId::OpenAiChat, ReasoningStyle::default())
                .is_err()
        );
        let outside = spec(r#"{ "effort": {"values": ["low"], "default": "high"} }"#);
        assert!(
            outside
                .check(ProviderId::OpenAi, ReasoningStyle::default())
                .is_err()
        );
    }

    #[test]
    fn a_model_s_declaration_overlays_its_provider_s() {
        let provider = spec(
            r#"{ "stop": {"max": 4}, "temperature": {"min": 0, "max": 2}, "sources": ["https://example.org/a"] }"#,
        );
        let model = spec(r#"{ "temperature": {"min": 0, "max": 1} }"#);
        let both = provider.overlaid(Some(&model));
        assert_eq!(both.temperature.map(|range| range.max), Some(1.0));
        assert_eq!(both.stop, Some(StopSpec { max: 4 }));
        assert_eq!(both.sources, vec!["https://example.org/a".to_owned()]);
    }

    #[test]
    fn a_change_is_read_member_by_member() {
        let current = values(r#"{"temperature": 0.5, "stop": ["x"]}"#);
        let given: Map<String, Value> =
            serde_json::from_str(r#"{"temperature": null, "effort": "high", "top_k": 7}"#).unwrap();
        let merged = merge(&current, &given).expect("merged");
        assert_eq!(merged.temperature, None);
        assert_eq!(merged.effort, Some(Effort::High));
        assert_eq!(merged.top_k, Some(7));
        assert_eq!(merged.stop, vec!["x".to_owned()]);
        for bad in [
            r#"{"seed": 1}"#,
            r#"{"effort": "ultra"}"#,
            r#"{"top_k": -1}"#,
            r#"{"stop": "x"}"#,
        ] {
            let given: Map<String, Value> = serde_json::from_str(bad).unwrap();
            assert!(merge(&current, &given).is_err(), "{bad}");
        }
    }
}
