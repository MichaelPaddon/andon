use base64::{engine::general_purpose::STANDARD, Engine};
use serde_json::{json, Value};

use super::util::{
    md5_hex, random_alnum_upper, random_hex, unix_millis,
};
use super::Session;

type Error =
    Box<dyn std::error::Error + Send + Sync + 'static>;
type Result<T> = std::result::Result<T, Error>;

const SECRET: &str = "23x17ahWarFH6w29";

const HEADERS: &[(&str, &str)] = &[
    ("AppType", "MMS"),
    ("AppVersion", "3.39.0"),
    ("AppLanguage", "EN"),
    ("vender", "meross"),
    ("User-Agent", "MerossIOT/3.39.0"),
];

fn envelope(params: &Value) -> Value {
    let params_b64 =
        STANDARD.encode(params.to_string().as_bytes());
    let nonce = random_alnum_upper(16);
    let ts = unix_millis();
    let sign = md5_hex(&format!(
        "{}{}{}{}",
        SECRET, ts, nonce, params_b64
    ));
    json!({
        "params":    params_b64,
        "sign":      sign,
        "timestamp": ts,
        "nonce":     nonce,
    })
}

async fn post(
    client: &reqwest::Client,
    url: &str,
    token: Option<&str>,
    params: &Value,
) -> Result<Value> {
    let body = envelope(params);
    let mut req = client.post(url).json(&body);
    for (k, v) in HEADERS {
        req = req.header(*k, *v);
    }
    let auth = match token {
        None => "Basic".to_owned(),
        Some(t) => format!("Basic {}", t),
    };
    req = req.header("Authorization", auth);
    let resp: Value = req.send().await?.json().await?;
    Ok(resp)
}

fn str_field(v: &Value, key: &str) -> Result<String> {
    v[key]
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| {
            format!("missing field `{}`", key).into()
        })
}

/// Authenticate against `api_base`, following a single
/// wrong-region redirect (status 1030) if needed.
pub async fn login(
    client: &reqwest::Client,
    email: &str,
    password: &str,
    api_base: &str,
) -> Result<Session> {
    let password_md5 = md5_hex(password);
    let mut base = api_base.to_owned();

    let resp = loop {
        let params = json!({
            "email":    email,
            "password": password_md5,
            "accountCountryCode": "us",
            "encryption": 1,
            "agree":      1,
            "mobileInfo": {
                "deviceModel":     "Linux",
                "mobileOsVersion": "1.0",
                "mobileOs":        "Linux",
                "uuid":            random_hex(30),
                "carrier":         "",
            },
        });
        let url = format!("{}/v1/Auth/signIn", base);
        let resp =
            post(client, &url, None, &params).await?;
        let status =
            resp["apiStatus"].as_i64().unwrap_or(-1);
        if status == 1030 {
            match resp["data"]["domain"].as_str() {
                Some(d) => {
                    base = d.to_owned();
                    continue;
                }
                None => {
                    return Err(
                        "region redirect with no domain"
                            .into(),
                    )
                }
            }
        }
        if status != 0 {
            return Err(format!(
                "login failed (apiStatus {})",
                status
            )
            .into());
        }
        break resp;
    };

    let d = &resp["data"];
    let app_id =
        md5_hex(&format!("API{}", random_hex(32)));

    Ok(Session {
        user_id: str_field(d, "userid")?,
        key: str_field(d, "key")?,
        token: str_field(d, "token")?,
        mqtt_domain: str_field(d, "mqttDomain")?,
        app_id,
        api_base: base,
    })
}

/// Return the UUIDs of every device matching `name`.
pub async fn device_uuids(
    client: &reqwest::Client,
    session: &Session,
    device_name: &str,
) -> Result<Vec<String>> {
    let url = format!(
        "{}/v1/Device/devList",
        session.api_base
    );
    let resp =
        post(client, &url, Some(&session.token), &json!({}))
            .await?;

    if resp["apiStatus"].as_i64().unwrap_or(-1) != 0 {
        return Err("devList failed".into());
    }

    let uuids = resp["data"]
        .as_array()
        .ok_or("devList data is not an array")?
        .iter()
        .filter(|d| {
            d["devName"].as_str() == Some(device_name)
        })
        .filter_map(|d| {
            d["uuid"].as_str().map(str::to_owned)
        })
        .collect();

    Ok(uuids)
}
