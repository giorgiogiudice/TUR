// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The command-line interface.
//!
//! ```text
//! tur-pax-providers check --config <file> [--ping] [--probe-tools]
//!                         [--key-env <VARIABLE> | --no-key]
//! ```
//!
//! `check` validates a configuration file and prints what the adapter it names
//! declares. It makes no network call unless `--ping` or `--probe-tools` is
//! given. With `--ping` it reads the API key from an environment variable
//! (`TUR_PAX_API_KEY`, or the one named with `--key-env`) and sends one short
//! request to the configured endpoint. With `--probe-tools` it sends one small
//! request that offers a harmless tool and reports whether the model called
//! it ([`crate::probe`]). Each request is billed by the provider like any
//! other. `--no-key` is for a server that takes no key (the openai-chat
//! family only, such as a model server on this computer).
//!
//! The key is never printed.

use std::fs;

use crate::config::ProviderConfig;
use crate::error::{ErrorKind, ProviderError};
use crate::model::{FinishReason, Message, ProviderId, Turn};
use crate::probe::{ToolProbe, probe_tool_calling};
use crate::provider::{Provider, capabilities_of, connect, connect_without_key, endpoint_path};
use crate::secret::{EnvKeySource, KeySource};
use crate::{anthropic, google};

/// Exit status: the configuration is valid, and the ping succeeded if one was asked for.
pub const EXIT_OK: u8 = 0;
/// Exit status: the configuration file could not be read or is not valid.
pub const EXIT_INVALID: u8 = 1;
/// Exit status: the command line was not understood.
pub const EXIT_USAGE: u8 = 2;
/// Exit status: the configuration is valid but the ping failed.
pub const EXIT_PING_FAILED: u8 = 3;
/// Exit status: the configuration is valid but the tool probe did not show
/// that the model calls tools.
pub const EXIT_PROBE_NOT_VERIFIED: u8 = 4;

/// The environment variable `--ping` reads the key from by default.
pub const DEFAULT_KEY_VARIABLE: &str = "TUR_PAX_API_KEY";

/// The most output tokens a ping asks for.
pub const PING_OUTPUT_TOKENS: u32 = 64;

const USAGE: &str = "\
Usage: tur-pax-providers check --config <file> [--ping] [--probe-tools]
                               [--key-env <VARIABLE> | --no-key]

Validates a provider configuration and prints the capabilities its adapter declares.

Options:
  --config <file>        The provider configuration, as JSON.
  --ping                 Also send one short request to the configured endpoint.
                         The provider bills it like any other request.
  --probe-tools          Also send one small request offering a harmless tool, and
                         report whether the model called it. Billed like any request.
  --key-env <VARIABLE>   The environment variable the API key is read from.
                         Default: TUR_PAX_API_KEY.
  --no-key               Send no key: for a server that takes none (openai-chat only).
  -h, --help             Print this help.
  -V, --version          Print the version.

Without --ping or --probe-tools no network call is made.
";

/// What a run of the command produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    /// The exit status.
    pub status: u8,
    /// The text for standard output.
    pub stdout: String,
    /// The text for standard error.
    pub stderr: String,
}

enum Command {
    Help,
    Version,
    Check {
        config: String,
        ping: bool,
        probe: bool,
        /// `None` for `--no-key`.
        key_variable: Option<String>,
    },
}

/// Run the command line. `arguments` excludes the program name.
pub fn run(arguments: &[String]) -> Outcome {
    match parse(arguments) {
        Ok(Command::Help) => Outcome {
            status: EXIT_OK,
            stdout: USAGE.to_owned(),
            stderr: String::new(),
        },
        Ok(Command::Version) => Outcome {
            status: EXIT_OK,
            stdout: format!("tur-pax-providers {}\n", env!("CARGO_PKG_VERSION")),
            stderr: String::new(),
        },
        Ok(Command::Check {
            config,
            ping,
            probe,
            key_variable,
        }) => check(&config, ping, probe, key_variable.as_deref()),
        Err(problem) => Outcome {
            status: EXIT_USAGE,
            stdout: String::new(),
            stderr: format!("{problem}\n\n{USAGE}"),
        },
    }
}

fn parse(arguments: &[String]) -> Result<Command, String> {
    let mut rest = arguments.iter().map(String::as_str);
    match rest.next() {
        Some("-h" | "--help") => return Ok(Command::Help),
        Some("-V" | "--version") => return Ok(Command::Version),
        Some("check") => {}
        Some(other) => return Err(format!("unknown command: {other}")),
        None => return Err("a command is needed".to_owned()),
    }
    let mut config = None;
    let mut ping = false;
    let mut probe = false;
    let mut no_key = false;
    let mut key_variable = None;
    while let Some(argument) = rest.next() {
        match argument {
            "-h" | "--help" => return Ok(Command::Help),
            "--ping" => ping = true,
            "--probe-tools" => probe = true,
            "--no-key" => no_key = true,
            "--config" => {
                config = Some(rest.next().ok_or("--config needs a file")?.to_owned());
            }
            "--key-env" => {
                key_variable = Some(
                    rest.next()
                        .ok_or("--key-env needs a variable name")?
                        .to_owned(),
                );
            }
            other => return Err(format!("unknown option: {other}")),
        }
    }
    let config = config.ok_or("--config <file> is needed")?;
    if no_key && key_variable.is_some() {
        return Err("--no-key and --key-env cannot be given together".to_owned());
    }
    let key_variable = if no_key {
        None
    } else {
        Some(key_variable.unwrap_or_else(|| DEFAULT_KEY_VARIABLE.to_owned()))
    };
    Ok(Command::Check {
        config,
        ping,
        probe,
        key_variable,
    })
}

fn check(path: &str, ping: bool, probe: bool, key_variable: Option<&str>) -> Outcome {
    let loaded = fs::read_to_string(path)
        .map_err(|error| {
            ProviderError::new(ErrorKind::ConfigInvalid)
                .with_detail(format!("the file could not be read: {:?}", error.kind()))
        })
        .and_then(|text| ProviderConfig::from_json(&text));
    let config = match loaded {
        Ok(config) => config,
        Err(error) => {
            return Outcome {
                status: EXIT_INVALID,
                stdout: String::new(),
                stderr: format!("{error}\n"),
            };
        }
    };

    let mut stdout = report(&config);
    if !ping && !probe {
        line(&mut stdout, "ping: not requested; no network call was made");
        return Outcome {
            status: EXIT_OK,
            stdout,
            stderr: String::new(),
        };
    }
    let provider = match adapter(&config, key_variable) {
        Ok(provider) => provider,
        Err(error) => {
            line(&mut stdout, "no request was sent");
            return Outcome {
                status: EXIT_PING_FAILED,
                stdout,
                stderr: format!("{error}\n"),
            };
        }
    };
    if ping {
        match send_ping(&config, &*provider) {
            Ok(turn) => {
                line(
                    &mut stdout,
                    &format!(
                        "ping: ok (finish: {}; input tokens: {}; output tokens: {}{})",
                        turn.finish.name(),
                        turn.usage.input_tokens,
                        turn.usage.output_tokens,
                        if turn.usage.estimated {
                            "; estimated: the service reported no usage"
                        } else {
                            ""
                        }
                    ),
                );
            }
            Err(error) => {
                line(&mut stdout, "ping: failed");
                return Outcome {
                    status: EXIT_PING_FAILED,
                    stdout,
                    stderr: format!("{error}\n"),
                };
            }
        }
    }
    if probe {
        let verdict = probe_tool_calling(&*provider, &config.model);
        let (text, status, stderr) = match &verdict {
            ToolProbe::Verified { .. } => (
                "tool probe: verified (the model called the tool)".to_owned(),
                EXIT_OK,
                String::new(),
            ),
            ToolProbe::NotCalled { .. } => (
                "tool probe: not verified (the model answered without calling the tool)".to_owned(),
                EXIT_PROBE_NOT_VERIFIED,
                String::new(),
            ),
            ToolProbe::Inconclusive { reason, .. } => (
                format!("tool probe: inconclusive (finish: {})", reason.name()),
                EXIT_PROBE_NOT_VERIFIED,
                reason
                    .error()
                    .map(|error| format!("{error}\n"))
                    .unwrap_or_default(),
            ),
        };
        line(&mut stdout, &text);
        return Outcome {
            status,
            stdout,
            stderr,
        };
    }
    Outcome {
        status: EXIT_OK,
        stdout,
        stderr: String::new(),
    }
}

/// The adapter of a configuration, with the key from the environment, or
/// without one for `--no-key`.
fn adapter(
    config: &ProviderConfig,
    key_variable: Option<&str>,
) -> Result<Box<dyn Provider>, ProviderError> {
    match key_variable {
        Some(variable) => {
            let key = EnvKeySource::new(variable).api_key(config.provider)?;
            connect(config, key)
        }
        None => connect_without_key(config),
    }
}

fn line(text: &mut String, content: &str) {
    text.push_str(content);
    text.push('\n');
}

fn yes_no(value: bool) -> &'static str {
    if value { "yes" } else { "no" }
}

/// The description of a valid configuration and of what its adapter declares.
fn report(config: &ProviderConfig) -> String {
    let provider = config.provider;
    let capabilities = capabilities_of(provider);
    let mut text = String::new();
    line(&mut text, "config: valid");
    line(&mut text, &format!("provider: {provider}"));
    line(
        &mut text,
        &format!(
            "endpoint: {}{}",
            config.effective_base_url(),
            endpoint_path(provider)
        ),
    );
    line(&mut text, &format!("api: {}", api_description(provider)));
    line(&mut text, &format!("model: {}", config.model));
    line(
        &mut text,
        &format!("max output tokens: {}", config.max_output_tokens),
    );
    line(
        &mut text,
        &format!(
            "timeouts: connect {} ms, response {} ms, idle {} ms",
            config.timeouts.connect_ms, config.timeouts.response_ms, config.timeouts.idle_ms
        ),
    );
    line(
        &mut text,
        &format!(
            "retry: up to {} attempts, delay {} to {} ms",
            config.retry.max_attempts, config.retry.initial_delay_ms, config.retry.max_delay_ms
        ),
    );
    match &config.prices {
        Some(prices) => line(
            &mut text,
            &format!(
                "prices per million tokens: input {} {currency}, output {} {currency}",
                prices.input_per_million,
                prices.output_per_million,
                currency = prices.currency
            ),
        ),
        None => line(&mut text, "prices: not configured; only tokens are counted"),
    }
    match &config.task_limit {
        Some(limit) => {
            let cost = limit
                .max_cost
                .map_or_else(|| "none".to_owned(), |most| most.to_string());
            let tokens = limit
                .max_tokens
                .map_or_else(|| "none".to_owned(), |most| most.to_string());
            line(
                &mut text,
                &format!("task limit: cost {cost}, tokens {tokens}"),
            );
        }
        None => line(&mut text, "task limit: not configured"),
    }
    line(&mut text, "capabilities:");
    line(
        &mut text,
        &format!("  streaming: {}", yes_no(capabilities.streaming)),
    );
    line(
        &mut text,
        &format!("  tool calling: {}", yes_no(capabilities.tool_calling)),
    );
    line(
        &mut text,
        &format!(
            "  parallel tool calls: {}",
            yes_no(capabilities.parallel_tool_calls)
        ),
    );
    line(
        &mut text,
        &format!("  image input: {}", yes_no(capabilities.image_input)),
    );
    line(
        &mut text,
        &format!(
            "  images in tool results: {}",
            yes_no(capabilities.image_in_tool_results)
        ),
    );
    let control = capabilities.native_computer_control;
    let offered = match control.provider_tool {
        Some(tool) => format!("offered by the provider as \"{tool}\""),
        None => "not offered by the provider".to_owned(),
    };
    let implemented = if control.implemented {
        "implemented by this adapter"
    } else {
        "not implemented by this adapter"
    };
    line(
        &mut text,
        &format!("  native computer control: {offered}; {implemented}"),
    );
    line(
        &mut text,
        &format!(
            "  model discovery: {}",
            yes_no(capabilities.model_discovery)
        ),
    );
    line(
        &mut text,
        &format!(
            "  speech services: {}",
            yes_no(capabilities.speech_services)
        ),
    );
    text
}

fn api_description(provider: ProviderId) -> String {
    match provider {
        ProviderId::Anthropic => {
            format!("Messages API, version {}", anthropic::API_VERSION)
        }
        ProviderId::OpenAi => "Responses API, stateless (store: false)".to_owned(),
        ProviderId::Google => format!(
            "Interactions API {}, stateless (store: false)",
            google::API_VERSION
        ),
        ProviderId::OpenAiChat => {
            "OpenAI-compatible Chat Completions, usage asked for in the stream".to_owned()
        }
    }
}

/// Send one short request and return the response, or the reason it failed.
fn send_ping(config: &ProviderConfig, provider: &dyn Provider) -> Result<Turn, ProviderError> {
    let mut request = config.request(None, vec![Message::user_text("ping")], Vec::new());
    request.max_output_tokens = request.max_output_tokens.min(PING_OUTPUT_TOKENS);
    let turn = Turn::collect(provider.stream(&request));
    match turn.finish {
        FinishReason::Refusal(error) | FinishReason::Error(error) => Err(error),
        FinishReason::EndTurn | FinishReason::ToolUse | FinishReason::OutputLimit => Ok(turn),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn arguments(list: &[&str]) -> Vec<String> {
        list.iter().map(|argument| (*argument).to_owned()).collect()
    }

    #[test]
    fn help_and_version_need_no_configuration() {
        for flag in ["--help", "-h"] {
            let outcome = run(&arguments(&[flag]));
            assert_eq!(outcome.status, EXIT_OK);
            assert!(outcome.stdout.contains("--ping"));
        }
        let outcome = run(&arguments(&["--version"]));
        assert_eq!(outcome.status, EXIT_OK);
        assert!(outcome.stdout.starts_with("tur-pax-providers "));
        let outcome = run(&arguments(&["check", "--help"]));
        assert_eq!(outcome.status, EXIT_OK);
    }

    #[test]
    fn a_command_line_that_is_not_understood_is_a_usage_error() {
        let lists: [&[&str]; 6] = [
            &[],
            &["frobnicate"],
            &["check"],
            &["check", "--config"],
            &["check", "--config", "file.json", "--key-env"],
            &["check", "--config", "file.json", "--verbose"],
        ];
        for list in lists {
            let outcome = run(&arguments(list));
            assert_eq!(outcome.status, EXIT_USAGE, "{list:?}");
            assert!(outcome.stdout.is_empty());
            assert!(outcome.stderr.contains("Usage:"));
        }
    }

    #[test]
    fn a_missing_file_is_an_invalid_configuration() {
        let outcome = run(&arguments(&[
            "check",
            "--config",
            "/nonexistent/tur-pax-providers/provider.json",
        ]));
        assert_eq!(outcome.status, EXIT_INVALID);
        assert!(outcome.stderr.starts_with("TUR-AI-PROVIDER-011"));
    }

    #[test]
    fn the_report_names_every_capability_row() {
        for provider in ProviderId::ALL {
            let config = ProviderConfig::from_json(&format!(
                r#"{{"schema_version": 1, "provider": "{provider}", "base_url": "https://api.example.org/v1", "model": "example-model", "max_output_tokens": 256}}"#
            ))
            .unwrap();
            let text = report(&config);
            let control = if provider.is_company_interface() {
                "native computer control: offered by the provider as"
            } else {
                "native computer control: not offered by the provider"
            };
            for expected in [
                "config: valid",
                "model: example-model",
                "streaming: yes",
                "tool calling: yes",
                "image input: yes",
                control,
                "not implemented by this adapter",
                "model discovery: yes",
                "speech services: no",
            ] {
                assert!(text.contains(expected), "{provider}: {expected}\n{text}");
            }
            assert!(text.contains(&format!("provider: {provider}")));
            assert!(text.contains("endpoint: https://api.example.org/v1/"));
        }
    }

    #[test]
    fn no_key_and_a_key_variable_cannot_be_given_together() {
        let outcome = run(&arguments(&[
            "check",
            "--config",
            "file.json",
            "--no-key",
            "--key-env",
            "X",
        ]));
        assert_eq!(outcome.status, EXIT_USAGE);
    }
}
