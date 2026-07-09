use std::time::Duration;

use regex::Regex;
use reqwest::Client;
use tracing::{debug, warn};

use crate::logging::ErrorChain;
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
            // ErrorChain surfaces the root cause (DNS,
            // refused, timeout), which reqwest's Display
            // alone omits.
            warn!(
                source = name,
                url,
                error  = %ErrorChain(&e),
                "request failed",
            );
            return Signal::On;
        }
        Ok(r) => r,
    };

    let status = resp.status();
    if !status.is_success() {
        warn!(
            source = name,
            url,
            %status,
            "unexpected status",
        );
        return Signal::On;
    }

    match pattern {
        None => Signal::Off,
        Some(pat) => match resp.text().await {
            Err(e) => {
                warn!(
                    source = name,
                    url,
                    error  = %ErrorChain(&e),
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
                        url,
                        pattern = %pat,
                        "body did not match pattern",
                    );
                    Signal::On
                }
            }
        },
    }
}

#[cfg(test)]
mod tests {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    use super::*;

    /// Serve one canned HTTP response on an ephemeral
    /// localhost port and return the URL to fetch.
    async fn serve_once(response: &'static str) -> String {
        let listener =
            TcpListener::bind("127.0.0.1:0")
                .await
                .expect("bind ephemeral port");
        let addr = listener
            .local_addr()
            .expect("local addr");

        tokio::spawn(async move {
            let (mut sock, _) = listener
                .accept()
                .await
                .expect("accept");
            // Drain the request head; enough for a
            // canned exchange.
            let mut buf = [0u8; 1024];
            let _ = sock.read(&mut buf).await;
            let _ = sock
                .write_all(response.as_bytes())
                .await;
        });

        format!("http://{addr}/")
    }

    fn client() -> Client {
        Client::builder()
            .timeout(Duration::from_secs(5))
            .build()
            .expect("build client")
    }

    fn http_ok(body: &str) -> String {
        format!(
            "HTTP/1.1 200 OK\r\n\
             content-length: {}\r\n\
             connection: close\r\n\r\n{}",
            body.len(),
            body,
        )
    }

    #[tokio::test]
    async fn success_yields_off() {
        let resp = http_ok("hello").leak();
        let url = serve_once(resp).await;
        let sig =
            run_check("t", &client(), &url, None).await;
        assert_eq!(sig, Signal::Off);
    }

    #[tokio::test]
    async fn error_status_yields_on() {
        let url = serve_once(
            "HTTP/1.1 500 Internal Server Error\r\n\
             content-length: 0\r\n\
             connection: close\r\n\r\n",
        )
        .await;
        let sig =
            run_check("t", &client(), &url, None).await;
        assert_eq!(sig, Signal::On);
    }

    #[tokio::test]
    async fn connection_failure_yields_on() {
        // Bind then drop, so the port is known-dead.
        let listener =
            TcpListener::bind("127.0.0.1:0")
                .await
                .expect("bind ephemeral port");
        let url = format!(
            "http://{}/",
            listener.local_addr().expect("local addr")
        );
        drop(listener);

        let sig =
            run_check("t", &client(), &url, None).await;
        assert_eq!(sig, Signal::On);
    }

    #[tokio::test]
    async fn matching_pattern_yields_off() {
        let resp = http_ok("status: healthy").leak();
        let url = serve_once(resp).await;
        let pat =
            Regex::new("healthy").expect("valid regex");
        let sig = run_check(
            "t",
            &client(),
            &url,
            Some(&pat),
        )
        .await;
        assert_eq!(sig, Signal::Off);
    }

    #[tokio::test]
    async fn non_matching_pattern_yields_on() {
        let resp = http_ok("status: degraded").leak();
        let url = serve_once(resp).await;
        let pat =
            Regex::new("healthy").expect("valid regex");
        let sig = run_check(
            "t",
            &client(),
            &url,
            Some(&pat),
        )
        .await;
        assert_eq!(sig, Signal::On);
    }
}
