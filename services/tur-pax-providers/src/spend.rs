// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! Spend accounting for one task.
//!
//! A task is usually several requests: the model answers, calls tools, and is
//! asked again with the results. A [`SpendMeter`] adds up the usage of every
//! request of a task, converts it to a cost with the configured prices, and
//! enforces the task's limit: a stream read through [`SpendMeter::meter`] is
//! stopped as soon as the task goes over (design section 7, "runaway cost").
//!
//! The cost is an estimate for display. It is computed from the provider's
//! reported token counts and the prices in the configuration; the provider's
//! invoice is the authority.

use crate::config::{Prices, ProviderConfig, TaskLimit};
use crate::error::{ErrorKind, ProviderError};
use crate::model::{FinishReason, StreamEvent, Usage};
use crate::provider::EventStream;

const TOKENS_PER_PRICE_UNIT: f64 = 1_000_000.0;

/// Adds up the usage of one task and enforces its limit.
#[derive(Debug, Clone, PartialEq)]
pub struct SpendMeter {
    prices: Option<Prices>,
    limit: TaskLimit,
    spent: Usage,
    requests: u32,
}

impl SpendMeter {
    /// A meter with the given prices and limit, at zero.
    pub fn new(prices: Option<Prices>, limit: TaskLimit) -> Self {
        SpendMeter {
            prices,
            limit,
            spent: Usage::default(),
            requests: 0,
        }
    }

    /// A meter with the prices and the task limit of a configuration.
    pub fn for_config(config: &ProviderConfig) -> Self {
        SpendMeter::new(config.prices.clone(), config.task_limit.unwrap_or_default())
    }

    /// The usage of the task so far.
    pub fn usage(&self) -> Usage {
        self.spent
    }

    /// How many requests have been counted.
    pub fn requests(&self) -> u32 {
        self.requests
    }

    /// The cost of the task so far, if prices are configured.
    pub fn cost(&self) -> Option<f64> {
        self.cost_of(&self.spent)
    }

    /// The currency of the cost, if prices are configured.
    pub fn currency(&self) -> Option<&str> {
        self.prices.as_ref().map(|prices| prices.currency.as_str())
    }

    /// Whether the task is over its limit.
    pub fn is_over_limit(&self) -> bool {
        self.over_limit(&self.spent)
    }

    /// Count the usage of one finished request.
    pub fn record(&mut self, usage: &Usage) {
        self.spent = self.spent.plus(usage);
        self.requests = self.requests.saturating_add(1);
    }

    /// Read a stream through the meter.
    ///
    /// The events pass through unchanged. The usage of the request is counted
    /// when the returned stream finishes or is dropped. If the task is already
    /// over its limit nothing is sent; if it goes over while the response is
    /// streaming, the stream is cut and finishes with a
    /// [`ErrorKind::SpendLimitReached`] error.
    pub fn meter(&mut self, stream: EventStream) -> MeteredStream<'_> {
        MeteredStream {
            meter: self,
            inner: Some(stream),
            current: Usage::default(),
            stop_next: false,
            recorded: false,
        }
    }

    fn cost_of(&self, usage: &Usage) -> Option<f64> {
        let prices = self.prices.as_ref()?;
        let cached = usage.cached_input_tokens.min(usage.input_tokens);
        let fresh = usage.input_tokens - cached;
        let cached_price = prices
            .cached_input_per_million
            .unwrap_or(prices.input_per_million);
        let total = fresh as f64 * prices.input_per_million
            + cached as f64 * cached_price
            + usage.output_tokens as f64 * prices.output_per_million;
        Some(total / TOKENS_PER_PRICE_UNIT)
    }

    fn over_limit(&self, usage: &Usage) -> bool {
        let over_tokens = self
            .limit
            .max_tokens
            .is_some_and(|most| usage.total_tokens() > most);
        let over_cost = match (self.limit.max_cost, self.cost_of(usage)) {
            (Some(most), Some(cost)) => cost > most,
            _ => false,
        };
        over_tokens || over_cost
    }

    fn limit_error(&self) -> ProviderError {
        ProviderError::new(ErrorKind::SpendLimitReached)
            .with_detail("the task was stopped at its configured limit")
    }
}

/// A stream whose usage is counted by a [`SpendMeter`].
pub struct MeteredStream<'a> {
    meter: &'a mut SpendMeter,
    inner: Option<EventStream>,
    /// The usage of this request so far.
    current: Usage,
    /// The limit was passed: the next event is the finish.
    stop_next: bool,
    recorded: bool,
}

impl MeteredStream<'_> {
    /// Count this request and close the connection.
    fn close(&mut self) {
        self.inner = None;
        if !self.recorded {
            self.recorded = true;
            self.meter.record(&self.current);
        }
    }

    fn stop(&mut self) -> StreamEvent {
        let error = self.meter.limit_error();
        self.close();
        StreamEvent::Finish(FinishReason::Error(error))
    }
}

impl Iterator for MeteredStream<'_> {
    type Item = StreamEvent;

    fn next(&mut self) -> Option<StreamEvent> {
        if self.stop_next {
            self.stop_next = false;
            return Some(self.stop());
        }
        let inner = self.inner.as_mut()?;
        if !self.recorded && self.current == Usage::default() && self.meter.is_over_limit() {
            // The task is already over: nothing is sent. The stream has not
            // been polled, so dropping it makes no request.
            self.recorded = true;
            self.inner = None;
            return Some(StreamEvent::Finish(FinishReason::Error(
                self.meter.limit_error(),
            )));
        }
        let event = inner.next();
        match &event {
            Some(StreamEvent::Usage(latest)) => {
                self.current = *latest;
                let total = self.meter.usage().plus(&self.current);
                if self.meter.over_limit(&total) {
                    self.stop_next = true;
                }
            }
            Some(StreamEvent::Finish(_)) | None => self.close(),
            Some(_) => {}
        }
        event
    }
}

impl Drop for MeteredStream<'_> {
    fn drop(&mut self) {
        // A stream abandoned half-way still used tokens.
        self.close();
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::model::ProviderId;

    fn prices() -> Prices {
        Prices {
            currency: "USD".to_owned(),
            input_per_million: 2.0,
            output_per_million: 10.0,
            cached_input_per_million: Some(0.5),
        }
    }

    fn usage(input: u64, output: u64, cached: u64) -> Usage {
        Usage {
            input_tokens: input,
            output_tokens: output,
            cached_input_tokens: cached,
        }
    }

    #[test]
    fn usage_accumulates_over_the_requests_of_a_task() {
        let mut meter = SpendMeter::new(None, TaskLimit::default());
        meter.record(&usage(100, 20, 0));
        meter.record(&usage(150, 30, 50));
        assert_eq!(meter.usage(), usage(250, 50, 50));
        assert_eq!(meter.requests(), 2);
        assert_eq!(meter.cost(), None);
        assert_eq!(meter.currency(), None);
        assert!(!meter.is_over_limit());
    }

    #[test]
    fn cost_uses_the_configured_prices() {
        let mut meter = SpendMeter::new(Some(prices()), TaskLimit::default());
        // 1,000,000 fresh input at 2.0, 1,000,000 cached at 0.5, 500,000 output at 10.0.
        meter.record(&usage(2_000_000, 500_000, 1_000_000));
        assert_eq!(meter.cost(), Some(7.5));
        assert_eq!(meter.currency(), Some("USD"));

        // Without a cached price, cached input is priced as ordinary input.
        let mut plain = prices();
        plain.cached_input_per_million = None;
        let mut meter = SpendMeter::new(Some(plain), TaskLimit::default());
        meter.record(&usage(2_000_000, 500_000, 1_000_000));
        assert_eq!(meter.cost(), Some(9.0));

        // A cached count above the input count cannot make the cost negative.
        let mut meter = SpendMeter::new(Some(prices()), TaskLimit::default());
        meter.record(&usage(10, 0, 1_000));
        assert!(meter.cost().is_some_and(|cost| cost > 0.0));
    }

    #[test]
    fn the_limit_is_passed_only_when_exceeded() {
        let limit = TaskLimit {
            max_cost: Some(1.0),
            max_tokens: Some(1_000),
        };
        let mut meter = SpendMeter::new(Some(prices()), limit);
        meter.record(&usage(900, 100, 0));
        assert!(!meter.is_over_limit(), "exactly at the token limit");
        meter.record(&usage(1, 0, 0));
        assert!(meter.is_over_limit());

        let mut meter = SpendMeter::new(
            Some(prices()),
            TaskLimit {
                max_cost: Some(1.0),
                max_tokens: None,
            },
        );
        meter.record(&usage(0, 100_000, 0));
        assert!(!meter.is_over_limit(), "exactly at the cost limit");
        meter.record(&usage(0, 1, 0));
        assert!(meter.is_over_limit());
    }

    #[test]
    fn a_meter_takes_prices_and_limit_from_the_configuration() {
        let config = ProviderConfig::from_json(
            &json!({
                "schema_version": 1,
                "provider": "google",
                "model": "example-model",
                "max_output_tokens": 256,
                "prices": { "currency": "EUR", "input_per_million": 1.0, "output_per_million": 4.0 },
                "task_limit": { "max_tokens": 10 }
            })
            .to_string(),
        )
        .unwrap();
        let mut meter = SpendMeter::for_config(&config);
        assert_eq!(meter.currency(), Some("EUR"));
        meter.record(&usage(6, 5, 0));
        assert!(meter.is_over_limit());
    }

    #[test]
    fn a_task_already_over_its_limit_sends_nothing() {
        let mut meter = SpendMeter::new(
            None,
            TaskLimit {
                max_cost: None,
                max_tokens: Some(10),
            },
        );
        meter.record(&usage(8, 8, 0));
        // This stream would report a different error if it were polled.
        let stream = EventStream::failed(
            ProviderId::Anthropic,
            ProviderError::new(ErrorKind::Network),
        );
        let events: Vec<StreamEvent> = meter.meter(stream).collect();
        assert_eq!(events.len(), 1);
        match &events[0] {
            StreamEvent::Finish(FinishReason::Error(error)) => {
                assert_eq!(error.kind(), ErrorKind::SpendLimitReached);
                assert_eq!(error.code(), "TUR-AI-PROVIDER-010");
            }
            other => panic!("unexpected event: {other:?}"),
        }
        assert_eq!(meter.requests(), 1, "the refused request is not counted");
    }

    #[test]
    fn a_stream_that_only_fails_is_passed_through_and_counted() {
        let mut meter = SpendMeter::new(None, TaskLimit::default());
        let stream = EventStream::failed(
            ProviderId::OpenAi,
            ProviderError::new(ErrorKind::RateLimited),
        );
        let events: Vec<StreamEvent> = meter.meter(stream).collect();
        assert_eq!(events.len(), 1);
        assert!(matches!(
            &events[0],
            StreamEvent::Finish(FinishReason::Error(error)) if error.kind() == ErrorKind::RateLimited
        ));
        assert_eq!(meter.requests(), 1);
        assert_eq!(meter.usage(), Usage::default());
    }
}
