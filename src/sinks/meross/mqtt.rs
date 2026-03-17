use std::sync::Arc;
use std::time::Duration;

use rumqttc::{
    AsyncClient, Event, MqttOptions, Packet, QoS,
    TlsConfiguration, Transport,
};
use rustls::{ClientConfig, RootCertStore};
use serde_json::{json, Value};

use super::util::{md5_hex, random_hex, unix_secs};
use super::Session;

type Error =
    Box<dyn std::error::Error + Send + Sync + 'static>;
type Result<T> = std::result::Result<T, Error>;

const PUBACK_TIMEOUT: Duration =
    Duration::from_secs(20);

fn tls() -> Result<TlsConfiguration> {
    let mut roots = RootCertStore::empty();
    for cert in rustls_native_certs::load_native_certs()
        .map_err(|e| format!("load certs: {}", e))?
    {
        let _ = roots.add(cert);
    }
    let cfg = ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();
    Ok(TlsConfiguration::Rustls(Arc::new(cfg)))
}

/// Build and sign a device message envelope.
fn envelope(
    session: &Session,
    namespace: &str,
    method: &str,
    payload: Value,
) -> Value {
    let msg_id = random_hex(32);
    let ts = unix_secs();
    let sign = md5_hex(&format!(
        "{}{}{}",
        msg_id, session.key, ts
    ));
    json!({
        "header": {
            "from": format!(
                "/app/{}-{}/subscribe",
                session.user_id, session.app_id
            ),
            "messageId":      msg_id,
            "method":         method,
            "namespace":      namespace,
            "payloadVersion": 1,
            "sign":           sign,
            "timestamp":      ts,
            "triggerSrc":     "andon",
        },
        "payload": payload,
    })
}

/// Publish a signed message to every UUID and wait for
/// all QoS-1 PUBACKs before returning.
pub async fn publish(
    session: &Session,
    uuids: &[String],
    namespace: &str,
    method: &str,
    payload: Value,
) -> Result<()> {
    if uuids.is_empty() {
        return Ok(());
    }

    let mqtt_pass = md5_hex(&format!(
        "{}{}",
        session.user_id, session.key
    ));
    let client_id = format!("app:{}", session.app_id);

    let mut opts = MqttOptions::new(
        &client_id,
        &session.mqtt_domain,
        443u16,
    );
    opts.set_credentials(&session.user_id, &mqtt_pass);
    opts.set_keep_alive(Duration::from_secs(30));
    opts.set_clean_session(false);
    opts.set_transport(Transport::Tls(tls()?));

    let msg =
        envelope(session, namespace, method, payload)
            .to_string();

    let (client, mut evloop) =
        AsyncClient::new(opts, 10);

    for uuid in uuids {
        let topic =
            format!("/appliance/{}/subscribe", uuid);
        client
            .publish(
                &topic,
                QoS::AtLeastOnce,
                false,
                msg.as_bytes(),
            )
            .await?;
    }

    let n = uuids.len();
    tokio::time::timeout(PUBACK_TIMEOUT, async move {
        let mut remaining = n;
        while remaining > 0 {
            match evloop.poll().await? {
                Event::Incoming(Packet::PubAck(_)) => {
                    remaining -= 1;
                }
                _ => {}
            }
        }
        Ok::<(), rumqttc::ConnectionError>(())
    })
    .await
    .map_err(|_| "MQTT PUBACK timeout")?
    .map_err(|e| Box::new(e) as Error)?;

    Ok(())
}
