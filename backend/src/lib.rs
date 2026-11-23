pub mod config;
pub mod db;
pub mod error;
pub mod inbox;
mod mail_actions;
pub mod mailcow;
pub mod model;
mod routes;
pub mod sending;
mod service;
use rand::RngCore;
pub use routes::router;
use sha2::{Digest, Sha256};
use std::sync::Arc;

#[derive(Clone)]
pub struct App {
    pub database_lock: Arc<std::fs::File>,
    pub config: Arc<config::Config>,
    pub db: Arc<db::Db>,
    pub mailcow: mailcow::Mailcow,
    pub auth: Arc<dyn mailcow::Authenticator>,
    pub writes: Arc<tokio::sync::Mutex<()>>,
    pub inbox: Arc<inbox::Connections>,
    pub sending: Arc<sending::Sending>,
}
impl App {
    pub fn new(config: config::Config) -> error::Result<Self> {
        use fs2::FileExt;
        let mut options = std::fs::OpenOptions::new();
        options.create(true).read(true).write(true).truncate(false);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let database_lock = options
            .open(format!("{}.lock", config.database))
            .map_err(|_| error::Error::internal())?;
        database_lock.try_lock_exclusive().map_err(|_| {
            error::Error::conflict("Another Bucket instance is using this database.")
        })?;
        let db = db::Db::open(&config.database)?;
        if db.user(&config.admin)?.is_none() {
            db.save_user(&model::User {
                email: config.admin.clone(),
                enabled: true,
                policy: None,
            })?;
        }
        let mailcow = mailcow::Mailcow::new(&config)?;
        let config = Arc::new(config);
        Ok(Self {
            database_lock: Arc::new(database_lock),
            db: Arc::new(db),
            mailcow,
            auth: Arc::new(mailcow::ImapAuth(config.clone())),
            config,
            writes: Arc::new(tokio::sync::Mutex::new(())),
            inbox: Arc::new(inbox::Connections::default()),
            sending: Arc::new(sending::Sending::default()),
        })
    }
}
pub fn token() -> String {
    let mut b = [0u8; 32];
    rand::rng().fill_bytes(&mut b);
    hex::encode(b)
}
pub fn hash(value: &str) -> String {
    hex::encode(Sha256::digest(value.as_bytes()))
}
