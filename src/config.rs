use std::time::Duration;

use serde::Deserialize;

use crate::sinks::meross::Region;
use crate::nodes::{And, Delay, Not, Or, Xor};
use crate::sinks::{LogSink, MerossSink};
use crate::sources::{CronSource, UrlSource};
use crate::{Alarm, Bus, Sink, Source};

type Error =
    Box<dyn std::error::Error + Send + Sync + 'static>;
type Result<T> = std::result::Result<T, Error>;

// ── top-level ────────────────────────────────────────

/// Parsed representation of a config file.
#[derive(Deserialize)]
pub struct Config {
    #[serde(default)]
    source: Vec<SourceConfig>,
    #[serde(default)]
    node: Vec<NodeConfig>,
    #[serde(default)]
    sink: Vec<SinkConfig>,
}

impl Config {
    pub fn from_str(s: &str) -> Result<Self> {
        toml::from_str(s).map_err(Into::into)
    }

    /// Construct and wire all objects into a [`Bus`].
    pub fn build(self) -> Result<Bus> {
        let mut bus = Bus::new();

        for cfg in self.source {
            bus.add_source(build_source(cfg)?);
        }
        for cfg in self.node {
            let (sink, source) = build_node(cfg)?;
            bus.add_sink(sink);
            bus.add_source(source);
        }
        for cfg in self.sink {
            let (sink, alarm) = build_sink(cfg)?;
            bus.add_sink(sink);
            if let Some(a) = alarm {
                bus.add_alarm(a);
            }
        }

        Ok(bus)
    }
}

// ── source configs ───────────────────────────────────

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
enum SourceConfig {
    Cron {
        name: String,
        /// 6-field cron expression (local time):
        /// `sec min hour day-of-month month day-of-week`
        cron: String,
        /// How long the signal stays `On` after firing.
        duration: String,
        sinks: Vec<String>,
    },
    Url {
        name: String,
        url: String,
        /// Polling interval, e.g. `"60s"` or `"1m30s"`.
        interval: String,
        /// Per-request timeout (default 10 s).
        timeout: Option<String>,
        /// Std deviation for Gaussian interval jitter.
        stddev: Option<String>,
        /// If set, response body must match this regex.
        pattern: Option<String>,
        sinks: Vec<String>,
    },
}

fn build_source(
    cfg: SourceConfig,
) -> Result<Box<dyn Source>> {
    match cfg {
        SourceConfig::Cron {
            name,
            cron,
            duration,
            sinks,
        } => {
            let src = CronSource::new(
                name,
                &cron,
                parse_dur(&duration)?,
                sinks,
            )
            .map_err(|e| -> Error { e.into() })?;
            Ok(Box::new(src))
        }
        SourceConfig::Url {
            name,
            url,
            interval,
            timeout,
            stddev,
            pattern,
            sinks,
        } => {
            let mut src = UrlSource::new(
                name,
                url,
                parse_dur(&interval)?,
                sinks,
            );
            if let Some(t) = timeout {
                src = src.with_timeout(parse_dur(&t)?);
            }
            if let Some(sd) = stddev {
                src = src.with_stddev(parse_dur(&sd)?);
            }
            if let Some(p) = pattern {
                src = src.with_pattern(
                    regex::Regex::new(&p)
                        .map_err(|e| e.to_string())?,
                );
            }
            Ok(Box::new(src))
        }
    }
}

// ── node configs ─────────────────────────────────────

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
enum NodeConfig {
    And { name: String, sinks: Vec<String> },
    Delay {
        name: String,
        /// Turn-on delay, e.g. `"30s"`.
        delay: String,
        sinks: Vec<String>,
    },
    Not { name: String, sinks: Vec<String> },
    Or { name: String, sinks: Vec<String> },
    Xor { name: String, sinks: Vec<String> },
}

fn build_node(
    cfg: NodeConfig,
) -> Result<(Box<dyn Sink>, Box<dyn Source>)> {
    match cfg {
        NodeConfig::And { name, sinks } => {
            let (sink, src) =
                And::new(name, sinks).split();
            Ok((Box::new(sink), Box::new(src)))
        }
        NodeConfig::Delay { name, delay, sinks } => {
            let (sink, src) = Delay::new(
                name,
                parse_dur(&delay)?,
                sinks,
            )
            .split();
            Ok((Box::new(sink), Box::new(src)))
        }
        NodeConfig::Not { name, sinks } => {
            let (sink, src) =
                Not::new(name, sinks).split();
            Ok((Box::new(sink), Box::new(src)))
        }
        NodeConfig::Or { name, sinks } => {
            let (sink, src) =
                Or::new(name, sinks).split();
            Ok((Box::new(sink), Box::new(src)))
        }
        NodeConfig::Xor { name, sinks } => {
            let (sink, src) =
                Xor::new(name, sinks).split();
            Ok((Box::new(sink), Box::new(src)))
        }
    }
}

// ── sink configs ─────────────────────────────────────

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
enum SinkConfig {
    Log {
        name: String,
    },
    Meross {
        name: String,
        /// Device name as shown in the Meross app.
        device: String,
        email: String,
        password: String,
        /// `"eu"`, `"us"`, `"ap"`, or a full HTTPS URL.
        #[serde(default = "default_region")]
        region: String,
        /// Relay channel (default 0 = main outlet).
        #[serde(default)]
        channel: Option<u8>,
    },
}

fn default_region() -> String {
    "us".to_owned()
}

fn build_sink(
    cfg: SinkConfig,
) -> Result<(Box<dyn Sink>, Option<Box<dyn Alarm>>)> {
    match cfg {
        SinkConfig::Log { name } => {
            Ok((Box::new(LogSink::new(name)), None))
        }
        SinkConfig::Meross {
            name,
            device,
            email,
            password,
            region,
            channel,
        } => {
            let r = match region.as_str() {
                "eu" => Region::Eu,
                "us" => Region::Us,
                "ap" => Region::Ap,
                url => Region::Custom(url.to_owned()),
            };
            let mut sink = MerossSink::new(
                name, device, email, password, r,
            );
            if let Some(ch) = channel {
                sink = sink.with_channel(ch);
            }
            let alarm = Box::new(sink.clone())
                as Box<dyn Alarm>;
            Ok((Box::new(sink), Some(alarm)))
        }
    }
}

// ── helpers ──────────────────────────────────────────

fn parse_dur(s: &str) -> Result<Duration> {
    humantime::parse_duration(s).map_err(Into::into)
}
