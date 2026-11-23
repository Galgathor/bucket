use crate::error::{Error, Result};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::LazyLock;

static LOCAL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[a-z0-9](?:[a-z0-9._-]{0,62}[a-z0-9])?$").unwrap());
static DOMAIN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(?:[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?\.)+[a-z]{2,63}$").unwrap()
});
pub fn local(s: &str) -> bool {
    LOCAL.is_match(s) && !s.contains("..")
}
pub fn domain(s: &str) -> bool {
    s.len() <= 253 && DOMAIN.is_match(s)
}
pub fn email(s: &str) -> Result<String> {
    let s = s.trim().to_lowercase();
    let parts: Vec<_> = s.split('@').collect();
    if s.len() > 254 || parts.len() != 2 || !local(parts[0]) || !domain(parts[1]) {
        return Err(Error::bad("Enter a valid email address."));
    }
    Ok(s)
}
pub fn password(s: &str) -> Result<()> {
    if s.len() < 12 || s.len() > 128 || s.chars().any(char::is_control) {
        return Err(Error::bad("Use a password with 12 to 128 characters."));
    }
    Ok(())
}
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Policy {
    pub mailboxes: bool,
    pub aliases: bool,
    pub max_mailboxes: u32,
    pub max_aliases: u32,
    pub max_storage: u64,
    pub default_storage: u64,
    pub total_storage: u64,
}
impl Default for Policy {
    fn default() -> Self {
        Self {
            mailboxes: true,
            aliases: true,
            max_mailboxes: 3,
            max_aliases: 50,
            max_storage: 5120,
            default_storage: 1024,
            total_storage: 10240,
        }
    }
}
impl Policy {
    pub fn validate(&self) -> Result<()> {
        if self.max_mailboxes > 1000
            || self.max_aliases > 10000
            || self.default_storage < 128
            || self.default_storage > self.max_storage
            || self.max_storage > 1048576
            || self.total_storage > 104857600
            || self.max_storage > self.total_storage
        {
            return Err(Error::bad(
                "Default storage must fit within mailbox and total storage limits.",
            ));
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(deny_unknown_fields)]
pub struct Configuration {
    pub domains: Vec<String>,
    pub policy: Policy,
}
impl Configuration {
    pub fn validate(&mut self) -> Result<()> {
        self.policy.validate()?;
        for d in &mut self.domains {
            *d = d.trim().to_lowercase();
        }
        self.domains.sort();
        self.domains.dedup();
        if self.domains.len() > 100 || self.domains.iter().any(|d| !domain(d)) {
            return Err(Error::bad("Enter valid domain names."));
        }
        Ok(())
    }
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct User {
    pub email: String,
    pub enabled: bool,
    pub policy: Option<Policy>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Login {
    pub email: String,
    pub password: String,
}
#[derive(Clone, Deserialize, Serialize, PartialEq, Eq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Mailbox,
    Alias,
}
impl Kind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Mailbox => "mailbox",
            Self::Alias => "alias",
        }
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Create {
    pub kind: Kind,
    pub local: String,
    pub domain: String,
    #[serde(default)]
    pub name: String,
    pub password: Option<String>,
    pub storage: Option<u64>,
    pub destination: Option<String>,
}
impl Create {
    pub fn validate(&mut self) -> Result<()> {
        self.local = self.local.trim().to_lowercase();
        self.domain = self.domain.trim().to_lowercase();
        if !local(&self.local)
            || !domain(&self.domain)
            || self.name.len() > 100
            || self.local.len() + self.domain.len() + 1 > 254
        {
            return Err(Error::bad("Check the address and name."));
        }
        if self.kind == Kind::Mailbox {
            password(self.password.as_deref().unwrap_or(""))?;
        } else if self.password.is_some() || self.storage.is_some() {
            return Err(Error::bad("Aliases do not have passwords or storage."));
        }
        Ok(())
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Edit {
    #[serde(default)]
    pub name: String,
    pub active: bool,
    pub password: Option<String>,
    pub storage: Option<u64>,
    pub destination: Option<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Delete {
    pub confirmation: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Resolve {
    pub action: String,
    pub confirmation: String,
    pub owner: Option<String>,
}
#[derive(Clone, Serialize)]
pub struct Resource {
    pub id: i64,
    pub address: String,
    pub kind: Kind,
    pub owner: String,
    pub name: String,
    #[serde(skip_serializing)]
    pub marker: String,
    pub state: String,
    pub remote_id: Option<String>,
    #[serde(skip_serializing)]
    pub identity: Option<String>,
    pub operation: Option<String>,
    pub created: i64,
}
#[derive(Clone)]
pub struct Session {
    pub token: String,
    pub email: String,
    pub csrf: String,
    pub expires: i64,
    pub identity: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Preferences {
    pub alias_suffix: bool,
}
