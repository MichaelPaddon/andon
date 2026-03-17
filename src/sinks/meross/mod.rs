mod http;
mod mqtt;
mod plug;
mod util;

type Error =
    Box<dyn std::error::Error + Send + Sync + 'static>;
type Result<T> = std::result::Result<T, Error>;

pub use plug::MerossSink;

// ── public types ─────────────────────────────────────

/// Meross cloud region.
#[derive(Clone)]
pub enum Region {
    Eu,
    Us,
    Ap,
    /// Supply a full HTTPS base URL.
    Custom(String),
}

impl Region {
    pub fn api_base(&self) -> &str {
        match self {
            Region::Eu => "https://iotx-eu.meross.com",
            Region::Us => "https://iotx-us.meross.com",
            Region::Ap => "https://iotx-ap.meross.com",
            Region::Custom(url) => url.as_str(),
        }
    }
}

/// Credentials and endpoints returned by a successful
/// login.  Exposed so callers can inspect connection
/// details if needed.
pub struct Session {
    pub user_id: String,
    pub key: String,
    pub token: String,
    pub mqtt_domain: String,
    /// Derived app identifier used in MQTT client-id
    /// and message `from` field.
    pub(crate) app_id: String,
    /// Actual API base URL after any region redirect.
    pub(crate) api_base: String,
}

// ── client ───────────────────────────────────────────

/// Authenticated Meross cloud client.
///
/// Handles login, device discovery, and generic MQTT
/// message publishing.  Device-specific logic (payload
/// structure, namespace) lives in higher layers.
pub struct MerossClient {
    http: reqwest::Client,
    session: Session,
}

impl MerossClient {
    /// Log in and return a ready client.
    ///
    /// Follows a single wrong-region redirect
    /// (API status 1030) automatically.
    pub async fn login(
        email: &str,
        password: &str,
        region: &Region,
    ) -> Result<Self> {
        let http = reqwest::Client::new();
        let session = http::login(
            &http,
            email,
            password,
            region.api_base(),
        )
        .await?;
        Ok(Self { http, session })
    }

    /// Return the UUIDs of every device whose name
    /// matches `device_name` exactly.
    pub async fn device_uuids(
        &self,
        device_name: &str,
    ) -> Result<Vec<String>> {
        http::device_uuids(
            &self.http,
            &self.session,
            device_name,
        )
        .await
    }

    /// Publish a signed message to each UUID and wait
    /// for all QoS-1 PUBACKs.
    ///
    /// `namespace` and `method` select the operation
    /// (e.g. `"Appliance.Control.ToggleX"` / `"SET"`).
    /// `payload` is the device-specific JSON body placed
    /// verbatim in the message `"payload"` field.
    pub async fn publish(
        &self,
        uuids: &[String],
        namespace: &str,
        method: &str,
        payload: serde_json::Value,
    ) -> Result<()> {
        mqtt::publish(
            &self.session,
            uuids,
            namespace,
            method,
            payload,
        )
        .await
    }
}
