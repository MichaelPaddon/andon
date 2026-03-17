use std::time::Duration;

use regex::Regex;
use reqwest::Client;
use tracing::{debug, warn};

use crate::probe::{Probe, ProbeSchedule};
use crate::Signal;

/// Polls a URL on a fixed interval and emits `On` when
/// the request fails or, if a pattern is set, when the
/// response body does not match the pattern.
/// Emits `Off` when the check passes.
pub struct UrlSource {
    name: String,
    url: String,
    schedule: ProbeSchedule,
    timeout: Duration,
    pattern: Option<Regex>,
    sinks: Vec<String>,
    // Lazy: built on first check so that with_timeout()
    // can be called freely after new().
    client: Option<Client>,
}

impl UrlSource {
    pub fn new(
        name: impl Into<String>,
        url: impl Into<String>,
        interval: Duration,
        sinks: Vec<String>,
    ) -> Self {
        Self {
            name: name.into(),
            url: url.into(),
            schedule: ProbeSchedule::new(interval),
            timeout: Duration::from_secs(10),
            pattern: None,
            sinks,
            client: None,
        }
    }

    pub fn with_timeout(mut self, t: Duration) -> Self {
        self.timeout = t;
        self
    }

    pub fn with_stddev(mut self, sd: Duration) -> Self {
        self.schedule = self.schedule.with_stddev(sd);
        self
    }

    pub fn with_pattern(mut self, pat: Regex) -> Self {
        self.pattern = Some(pat);
        self
    }
}

impl Probe for UrlSource {
    fn name(&self) -> &str {
        &self.name
    }

    fn sink_names(&self) -> &[String] {
        &self.sinks
    }

    fn schedule(&self) -> &ProbeSchedule {
        &self.schedule
    }

    async fn check(&mut self) -> Signal {
        let client = self.client.get_or_insert_with(|| {
            Client::builder()
                .timeout(self.timeout)
                .build()
                .expect("failed to build HTTP client")
        });

        debug!(
            source = %self.name,
            url    = %self.url,
            "checking"
        );

        let sig = run_check(
            &self.name,
            client,
            &self.url,
            self.pattern.as_ref(),
        )
        .await;

        debug!(
            source = %self.name,
            signal = ?sig,
            "check complete"
        );

        sig
    }
}

async fn run_check(
    name: &str,
    client: &Client,
    url: &str,
    pattern: Option<&Regex>,
) -> Signal {
    let resp = match client.get(url).send().await {
        Err(e) => {
            warn!(source = name, error = %e, "request failed");
            return Signal::On;
        }
        Ok(r) => r,
    };

    let status = resp.status();
    if !status.is_success() {
        warn!(source = name, %status, "unexpected status");
        return Signal::On;
    }

    match pattern {
        None => Signal::Off,
        Some(pat) => match resp.text().await {
            Err(e) => {
                warn!(
                    source  = name,
                    error   = %e,
                    "failed to read body",
                );
                Signal::On
            }
            Ok(body) => {
                if pat.is_match(&body) {
                    Signal::Off
                } else {
                    warn!(
                        source  = name,
                        pattern = %pat,
                        "body did not match pattern",
                    );
                    Signal::On
                }
            }
        },
    }
}
