use crate::model;
use std::env;

#[derive(Clone)]
pub struct Config {
    pub public_url: String,
    pub mailcow_url: String,
    pub api_key: String,
    pub imap_host: String,
    pub imap_port: u16,
    pub admin: String,
    pub database: String,
    pub frontend: String,
    pub secure: bool,
    pub bind: String,
    pub session_hours: i64,
    pub ca_cert: Option<String>,
}
impl Config {
    pub fn from_env() -> Result<Self, String> {
        let required = |key: &str| env::var(key).map_err(|_| format!("{key} is required"));
        let public_url = required("BUCKET_PUBLIC_URL")?
            .trim_end_matches('/')
            .to_owned();
        let mailcow_url = required("BUCKET_MAILCOW_URL")?
            .trim_end_matches('/')
            .to_owned();
        for (name, value, dev) in [
            ("BUCKET_PUBLIC_URL", &public_url, true),
            ("BUCKET_MAILCOW_URL", &mailcow_url, false),
        ] {
            let url =
                url::Url::parse(value).map_err(|_| format!("{name} must be an HTTPS origin"))?;
            let localhost = matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"));
            if url.host_str().is_none()
                || url.path() != "/"
                || url.query().is_some()
                || url.fragment().is_some()
                || !url.username().is_empty()
                || url.password().is_some()
                || (url.scheme() != "https" && !(dev && localhost && url.scheme() == "http"))
            {
                return Err(format!(
                    "{name} must be an HTTPS origin (localhost HTTP is allowed for development)"
                ));
            }
        }
        let api_key = required("BUCKET_MAILCOW_API_KEY")?;
        if api_key.len() < 10 {
            return Err("BUCKET_MAILCOW_API_KEY is invalid".into());
        }
        let admin = model::email(&required("BUCKET_ADMIN_EMAIL")?)
            .map_err(|_| "BUCKET_ADMIN_EMAIL is invalid")?;
        let session_hours = env::var("BUCKET_SESSION_HOURS")
            .unwrap_or("12".into())
            .parse::<i64>()
            .map_err(|_| "Invalid session duration")?;
        if !(1..=168).contains(&session_hours) {
            return Err("Session duration must be 1–168 hours".into());
        }
        let imap_port = env::var("BUCKET_IMAP_PORT")
            .unwrap_or("993".into())
            .parse()
            .map_err(|_| "Invalid IMAP port")?;
        Ok(Self {
            secure: public_url.starts_with("https:"),
            public_url,
            mailcow_url,
            api_key,
            admin,
            session_hours,
            imap_port,
            ca_cert: env::var("BUCKET_CA_CERT").ok(),
            imap_host: required("BUCKET_IMAP_HOST")?,
            database: env::var("BUCKET_DATABASE").unwrap_or("/data/bucket.db".into()),
            frontend: env::var("BUCKET_FRONTEND_DIR").unwrap_or("/app/static".into()),
            bind: env::var("BUCKET_BIND").unwrap_or("0.0.0.0:8080".into()),
        })
    }
    pub fn cookie(&self) -> &str {
        if self.secure {
            "__Host-bucket"
        } else {
            "bucket-dev"
        }
    }
}
