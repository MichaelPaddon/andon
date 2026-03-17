use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;

use serde_json::json;
use tokio::{sync::mpsc, task::JoinHandle};
use tracing::{debug, error, info};

use super::{MerossClient, Region};
use crate::{Alarm, Message, Signal, Sink};

// ── shared connect helper ─────────────────────────────

/// Log in and return the client together with the
/// resolved UUIDs for `device_name`.  Logs all errors
/// and returns `None` on any failure.
async fn connect(
    name: &str,
    email: &str,
    password: &str,
    region: &Region,
    device_name: &str,
) -> Option<(MerossClient, Vec<String>)> {
    let client =
        match MerossClient::login(email, password, region)
            .await
        {
            Ok(c) => c,
            Err(e) => {
                error!(
                    sink  = %name,
                    error = %e,
                    "login failed",
                );
                return None;
            }
        };

    let uuids =
        match client.device_uuids(device_name).await {
            Ok(u) => u,
            Err(e) => {
                error!(
                    sink  = %name,
                    error = %e,
                    "devList failed",
                );
                return None;
            }
        };

    if uuids.is_empty() {
        error!(
            sink   = %name,
            device = %device_name,
            "no matching device found",
        );
        return None;
    }

    Some((client, uuids))
}

// ── sink ──────────────────────────────────────────────

/// Controls a Meross smart plug via the cloud API.
///
/// Turns the plug **on** when any connected source is
/// `On`; turns it **off** when all are `Off`.
/// Only sends a cloud command when the state changes.
///
/// Also implements [`Alarm`]: calling [`clear`] turns
/// the plug off unconditionally, independent of the
/// message loop.
///
/// [`clear`]: Alarm::clear
pub struct MerossSink {
    name: String,
    device_name: String,
    email: String,
    password: String,
    region: Region,
    channel: u8,
    /// Populated by [`Sink::init`] before the message
    /// loop starts.  `None` if init failed or was not
    /// called (e.g. on the cloned alarm handle).
    connection: Option<(MerossClient, Vec<String>)>,
}

/// Clone carries credentials only; the live connection
/// is not copied.
impl Clone for MerossSink {
    fn clone(&self) -> Self {
        Self {
            name: self.name.clone(),
            device_name: self.device_name.clone(),
            email: self.email.clone(),
            password: self.password.clone(),
            region: self.region.clone(),
            channel: self.channel,
            connection: None,
        }
    }
}

impl MerossSink {
    pub fn new(
        name: impl Into<String>,
        device_name: impl Into<String>,
        email: impl Into<String>,
        password: impl Into<String>,
        region: Region,
    ) -> Self {
        Self {
            name: name.into(),
            device_name: device_name.into(),
            email: email.into(),
            password: password.into(),
            region,
            channel: 0,
            connection: None,
        }
    }

    /// Override the relay channel (default 0 = main).
    pub fn with_channel(mut self, ch: u8) -> Self {
        self.channel = ch;
        self
    }
}

impl Sink for MerossSink {
    fn name(&self) -> &str {
        &self.name
    }

    fn init(
        &mut self,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + '_>>
    {
        Box::pin(async move {
            debug!(sink = %self.name, "connecting");
            self.connection = connect(
                &self.name,
                &self.email,
                &self.password,
                &self.region,
                &self.device_name,
            )
            .await;
            if self.connection.is_some() {
                debug!(sink = %self.name, "connected");
            }
        })
    }

    fn start(
        self: Box<Self>,
        mut rx: mpsc::Receiver<Message>,
    ) -> JoinHandle<()> {
        tokio::spawn(async move {
            let Some((client, uuids)) =
                self.connection
            else {
                return;
            };

            let mut states: HashMap<String, Signal> =
                HashMap::new();
            let mut plug_on = false;

            while let Some(msg) = rx.recv().await {
                states.insert(
                    msg.source.clone(),
                    msg.signal,
                );

                let want_on = states
                    .values()
                    .any(|&s| s == Signal::On);

                if want_on != plug_on {
                    plug_on = want_on;
                    let payload = json!({
                        "togglex": {
                            "channel": self.channel,
                            "onoff":   u8::from(want_on),
                        }
                    });
                    if let Err(e) = client
                        .publish(
                            &uuids,
                            "Appliance.Control.ToggleX",
                            "SET",
                            payload,
                        )
                        .await
                    {
                        error!(
                            sink  = %self.name,
                            error = %e,
                            "toggle failed",
                        );
                    }
                }
            }
        })
    }
}

impl Alarm for MerossSink {
    fn clear(
        &self,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + '_>>
    {
        Box::pin(async move {
            debug!(sink = %self.name, "clearing alarm");

            let Some((client, uuids)) = connect(
                &self.name,
                &self.email,
                &self.password,
                &self.region,
                &self.device_name,
            )
            .await
            else {
                return;
            };

            let payload = json!({
                "togglex": {
                    "channel": self.channel,
                    "onoff":   0u8,
                }
            });

            match client
                .publish(
                    &uuids,
                    "Appliance.Control.ToggleX",
                    "SET",
                    payload,
                )
                .await
            {
                Ok(()) => {
                    info!(
                        sink = %self.name,
                        "alarm cleared",
                    );
                }
                Err(e) => {
                    error!(
                        sink  = %self.name,
                        error = %e,
                        "alarm clear failed",
                    );
                }
            }
        })
    }
}
