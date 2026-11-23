use crate::{
    config::Config,
    error::{Error, Result},
    model::Kind,
};
use serde_json::{Value, json};
use std::{sync::Arc, time::Duration};

#[derive(Clone)]
pub struct Mailcow {
    client: reqwest::Client,
    base: String,
}
impl Mailcow {
    pub fn new(config: &Config) -> Result<Self> {
        let mut headers = reqwest::header::HeaderMap::new();
        let mut key = reqwest::header::HeaderValue::from_str(&config.api_key)
            .map_err(|_| Error::internal())?;
        key.set_sensitive(true);
        headers.insert("X-API-Key", key);
        let mut builder = reqwest::Client::builder()
            .default_headers(headers)
            .timeout(Duration::from_secs(20))
            .connect_timeout(Duration::from_secs(8))
            .redirect(reqwest::redirect::Policy::none());
        if let Some(path) = &config.ca_cert {
            let pem = std::fs::read(path)
                .map_err(|_| Error::bad("Could not read the configured CA certificate."))?;
            let cert = reqwest::Certificate::from_pem(&pem)
                .map_err(|_| Error::bad("The configured CA certificate is invalid."))?;
            builder = builder.add_root_certificate(cert);
        }
        let client = builder.build().map_err(|_| Error::internal())?;
        Ok(Self {
            client,
            base: format!("{}/api/v1/", config.mailcow_url),
        })
    }
    pub async fn request(&self, path: &str, body: Option<Value>) -> Result<Value> {
        let write = body.is_some();
        let req = if let Some(b) = body {
            self.client.post(format!("{}{path}", self.base)).json(&b)
        } else {
            self.client.get(format!("{}{path}", self.base))
        };
        let response = req.send().await.map_err(|_| Error::upstream(write))?;
        if !response.status().is_success() {
            return Err(Error::upstream(write));
        }
        let data: Value = response.json().await.map_err(|_| Error::upstream(write))?;
        parse_response(data, write)
    }
    pub async fn get(&self, kind: &Kind, id: &str) -> Result<Option<Value>> {
        // Encode identifiers as one URL segment, including @ and dots.
        let encoded: String = url::form_urlencoded::byte_serialize(id.as_bytes()).collect();
        let value = self
            .request(&format!("get/{}/{encoded}", kind.as_str()), None)
            .await?;
        if value.is_null()
            || value.as_object().is_some_and(|o| o.is_empty())
            || value.as_array().is_some_and(|a| a.is_empty())
        {
            return Ok(None);
        }
        if !value.is_object() {
            return Err(Error::upstream(false));
        }
        Ok(Some(value))
    }
    pub async fn domains(&self) -> Result<Vec<String>> {
        let value = self.request("get/domain/all", None).await?;
        let rows = value.as_array().ok_or_else(|| Error::upstream(false))?;
        Ok(rows
            .iter()
            .filter(|r| number(&r["active_int"]) == 1 || number(&r["active"]) == 1)
            .filter_map(|r| r["domain_name"].as_str().map(str::to_owned))
            .collect())
    }
}
pub fn parse_response(data: Value, write: bool) -> Result<Value> {
    let rows = if let Some(a) = data.as_array() {
        a.clone()
    } else {
        vec![data.clone()]
    };
    let messages: Vec<_> = rows.iter().filter(|m| m.get("type").is_some()).collect();
    if messages
        .iter()
        .any(|m| matches!(m["type"].as_str(), Some("danger" | "error" | "warning")))
    {
        let text = messages
            .iter()
            .map(|m| m["msg"].to_string())
            .collect::<String>()
            .to_lowercase();
        let mut e = if text.contains("is_alias_or_mailbox") || text.contains("mailbox_exists") {
            Error::conflict("This email address is already in use.")
        } else if text.contains("password") {
            Error::bad("This password does not meet the mail server’s password rules.")
        } else if text.contains("quota") || text.contains("max_") {
            Error::conflict("The mail server’s account or storage limit has been reached.")
        } else {
            Error::conflict(
                "The mail server could not save this change. Ask an administrator to check it.",
            )
        };
        // Even a rejected write can have side effects. Never discard the journal here.
        e.uncertain = write;
        return Err(e);
    }
    if write && (messages.is_empty() || !messages.iter().any(|m| m["type"] == "success")) {
        return Err(Error::upstream(true));
    }
    if !write && !messages.is_empty() {
        return Err(Error::upstream(false));
    }
    Ok(data)
}
pub fn number(value: &Value) -> i64 {
    value
        .as_i64()
        .or_else(|| value.as_str().and_then(|s| s.parse().ok()))
        .unwrap_or(0)
}
pub fn text(value: &Value) -> String {
    value.as_str().map(str::to_owned).unwrap_or_else(|| {
        if value.is_number() {
            value.to_string()
        } else {
            String::new()
        }
    })
}
pub fn active(v: &Value) -> i64 {
    if v.get("active_int").is_some() {
        number(&v["active_int"])
    } else {
        number(&v["active"])
    }
}
pub fn identity(v: &Value) -> String {
    text(&v["created"])
}
pub fn payload_id(kind: &Kind, v: &Value, address: &str) -> String {
    if *kind == Kind::Alias {
        text(&v["id"])
    } else {
        address.into()
    }
}
pub fn matches(row: &crate::model::Resource, v: &Value) -> bool {
    let address = if row.kind == Kind::Mailbox {
        text(&v["username"])
    } else {
        text(&v["address"])
    };
    let marker = if row.kind == Kind::Mailbox {
        v["tags"]
            .as_array()
            .is_some_and(|a| a.contains(&json!(row.marker)))
    } else {
        v["private_comment"] == row.marker
    };
    address == row.address && marker && row.identity.as_ref().is_none_or(|id| id == &identity(v))
}

pub type ImapSession = imap::Session<native_tls::TlsStream<std::net::TcpStream>>;

#[async_trait::async_trait]
pub trait Authenticator: Send + Sync {
    async fn authenticate(&self, email: String, password: String) -> Result<bool>;
    async fn connect(&self, _email: String, _password: String) -> Result<Option<ImapSession>> {
        Ok(None)
    }
}
pub struct ImapAuth(pub Arc<Config>);
#[async_trait::async_trait]
impl Authenticator for ImapAuth {
    async fn connect(&self, email: String, password: String) -> Result<Option<ImapSession>> {
        let config = self.0.clone();
        tokio::task::spawn_blocking(move|| {
            let tls=native_tls::TlsConnector::builder().min_protocol_version(Some(native_tls::Protocol::Tlsv12)).build().map_err(|_|Error::internal())?;
            imap_connect(&config,&tls,&email,&password)?.map(Some).ok_or_else(||Error::new(axum::http::StatusCode::UNAUTHORIZED,"The mail server did not accept this email and password. Use an IMAP-enabled password."))
        }).await.map_err(|_|Error::internal())?
    }
    async fn authenticate(&self, email: String, password: String) -> Result<bool> {
        let config = self.0.clone();
        tokio::task::spawn_blocking(move || {
            let tls = native_tls::TlsConnector::builder()
                .min_protocol_version(Some(native_tls::Protocol::Tlsv12))
                .build()
                .map_err(|_| Error::internal())?;
            imap_login(&config, &tls, &email, &password)
        })
        .await
        .map_err(|_| Error::internal())?
    }
}

fn imap_failure(reason: &str, message: &str) -> Error {
    tracing::warn!(reason, "IMAP sign-in failed");
    Error::new(axum::http::StatusCode::SERVICE_UNAVAILABLE, message)
}

fn imap_login(
    config: &Config,
    tls: &native_tls::TlsConnector,
    email: &str,
    password: &str,
) -> Result<bool> {
    if let Some(mut session) = imap_connect(config, tls, email, password)? {
        let _ = session.logout();
        Ok(true)
    } else {
        Ok(false)
    }
}
fn imap_connect(
    config: &Config,
    tls: &native_tls::TlsConnector,
    email: &str,
    password: &str,
) -> Result<Option<ImapSession>> {
    use std::net::{TcpStream, ToSocketAddrs};
    let addresses: Vec<_> = (config.imap_host.as_str(), config.imap_port)
        .to_socket_addrs()
        .map_err(|_| imap_failure("dns_failed","Bucket could not find the mail server. Ask the administrator to check BUCKET_IMAP_HOST."))?
        .collect();
    let mut connected = None;
    for addr in addresses.into_iter().take(4) {
        if let Ok(stream) = TcpStream::connect_timeout(&addr, Duration::from_secs(3)) {
            connected = Some(stream);
            break;
        }
    }
    let stream = connected.ok_or_else(|| imap_failure("connection_failed","Bucket could not connect to the mail server. Ask the administrator to check the IMAP hostname, port, and firewall."))?;
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .map_err(|_| Error::internal())?;
    stream
        .set_write_timeout(Some(Duration::from_secs(10)))
        .map_err(|_| Error::internal())?;
    let stream = tls
        .connect(&config.imap_host, stream)
        .map_err(|_| imap_failure("tls_failed","Bucket could not establish a secure connection to the mail server. Ask the administrator to check the IMAP hostname and TLS certificate."))?;
    let mut client = imap::Client::new(stream);
    client.read_greeting().map_err(|_| imap_failure("greeting_failed","The mail server did not send a valid IMAP greeting. Ask the administrator to check the IMAP port and service."))?;
    match client.login(email, password) {
        Ok(session) => Ok(Some(session)),
        Err((imap::error::Error::No(_), _)) | Err((imap::error::Error::Bad(_), _)) => Ok(None),
        Err(_) => Err(imap_failure(
            "protocol_failed",
            "The mail server interrupted sign-in. Try again or ask the administrator to check the IMAP service.",
        )),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{BufRead, BufReader, Write},
        net::TcpListener,
        thread,
    };
    fn server(accept: bool) -> (u16, thread::JoinHandle<()>) {
        let cert = include_bytes!("../tests/fixtures/imap-cert.pem");
        let key = include_bytes!("../tests/fixtures/imap-key.pem");
        let identity = native_tls::Identity::from_pkcs8(cert, key).unwrap();
        let tls = native_tls::TlsAcceptor::new(identity).unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let handle = thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let Ok(mut stream) = tls.accept(stream) else {
                return;
            };
            stream.write_all(b"* OK test server ready\r\n").unwrap();
            let mut reader = BufReader::new(stream);
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            assert!(line.contains("LOGIN"));
            let tag = line.split_whitespace().next().unwrap();
            if !accept {
                let _ = write!(reader.get_mut(), "{tag} NO authentication failed\r\n");
                return;
            }
            write!(reader.get_mut(), "{tag} OK LOGIN completed\r\n").unwrap();
            line.clear();
            reader.read_line(&mut line).unwrap();
            let tag = line.split_whitespace().next().unwrap();
            write!(
                reader.get_mut(),
                "* BYE logging out\r\n{tag} OK LOGOUT completed\r\n"
            )
            .unwrap();
        });
        (port, handle)
    }
    fn config(port: u16) -> Config {
        Config {
            public_url: "https://bucket.example.com".into(),
            mailcow_url: "https://mail.example.com".into(),
            api_key: "test-only-key".into(),
            imap_host: "localhost".into(),
            imap_port: port,
            smtp_host: "localhost".into(),
            smtp_port: 1,
            smtp_starttls: false,
            admin: "admin@example.com".into(),
            database: "unused".into(),
            frontend: "unused".into(),
            secure: true,
            bind: "127.0.0.1:0".into(),
            session_hours: 12,
            ca_cert: None,
        }
    }
    #[test]
    fn authenticates_and_rejects_over_actual_tls() {
        let cert =
            native_tls::Certificate::from_pem(include_bytes!("../tests/fixtures/imap-cert.pem"))
                .unwrap();
        let tls = native_tls::TlsConnector::builder()
            .add_root_certificate(cert)
            .build()
            .unwrap();
        for accepted in [true, false] {
            let (port, handle) = server(accepted);
            assert_eq!(
                imap_login(
                    &config(port),
                    &tls,
                    "alice@example.com",
                    "test-only-password"
                )
                .unwrap(),
                accepted
            );
            handle.join().unwrap();
        }
    }
    #[test]
    fn rejects_an_untrusted_imap_certificate() {
        let (port, handle) = server(true);
        let tls = native_tls::TlsConnector::new().unwrap();
        assert!(
            imap_login(
                &config(port),
                &tls,
                "alice@example.com",
                "test-only-password"
            )
            .is_err()
        );
        handle.join().unwrap();
    }
}
