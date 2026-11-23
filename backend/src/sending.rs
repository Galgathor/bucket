use crate::{
    App,
    config::Config,
    db::now,
    error::{Error, Result},
    inbox::{Folder, MailQuery, matches_folder, resolve_folder},
    mailcow::number,
    model::{Kind, Session, User, email},
};
use axum::http::StatusCode;
use lettre::{
    Message as Email,
    message::{Mailbox, header::ContentType},
    transport::smtp::{
        authentication::{Credentials, Mechanism},
        client::{Certificate, SmtpConnection, TlsParameters},
        extension::ClientId,
    },
};
use mailparse::MailHeaderMap;
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};

struct Connection {
    client: Mutex<SmtpConnection>,
    identity: String,
    expires: i64,
}
#[derive(Default)]
pub struct Sending(Mutex<HashMap<(String, String), Arc<Connection>>>);
impl Sending {
    pub fn insert(
        &self,
        s: &Session,
        mailbox: &str,
        identity: &str,
        client: SmtpConnection,
    ) -> Result<()> {
        let mut map = self.0.lock().map_err(|_| Error::internal())?;
        map.retain(|_, c| c.expires > now());
        if !map.contains_key(&(s.token.clone(), mailbox.into()))
            && (map.len() >= 128 || map.keys().filter(|(t, _)| t == &s.token).count() >= 8)
        {
            return Err(Error::conflict(
                "Too many sending connections are open. Sign out and try again.",
            ));
        }
        map.insert(
            (s.token.clone(), mailbox.into()),
            Arc::new(Connection {
                client: Mutex::new(client),
                identity: identity.into(),
                expires: s.expires,
            }),
        );
        Ok(())
    }
    fn get(&self, s: &Session, mailbox: &str, identity: &str) -> Result<Arc<Connection>> {
        let c = self
            .0
            .lock()
            .map_err(|_| Error::internal())?
            .get(&(s.token.clone(), mailbox.into()))
            .cloned()
            .ok_or_else(sending_locked)?;
        if c.identity != identity || c.expires <= now() {
            return Err(sending_locked());
        }
        Ok(c)
    }
    pub fn opened(&self, s: &Session, mailbox: &str, identity: &str) -> bool {
        self.get(s, mailbox, identity).is_ok()
    }
    pub fn remove(&self, token: &str) {
        if let Ok(mut map) = self.0.lock() {
            map.retain(|(t, _), _| t != token);
        }
    }
    pub fn cleanup(&self, db: &crate::db::Db) {
        if let Ok(mut map) = self.0.lock() {
            map.retain(|(t, _), _| db.session(t).ok().flatten().is_some());
        }
    }
    fn remove_mailbox(&self, token: &str, mailbox: &str) {
        if let Ok(mut map) = self.0.lock() {
            map.remove(&(token.into(), mailbox.into()));
        }
    }
}
fn sending_locked() -> Error {
    Error::new(
        StatusCode::LOCKED,
        "Enter the destination mailbox’s password to enable sending.",
    )
}
fn uncertain() -> Error {
    Error::conflict(
        "The mail server did not confirm delivery. Check Sent before trying again; this message will not be sent again automatically.",
    )
}
fn connect(config: &Config, mailbox: &str, password: String) -> Result<SmtpConnection> {
    let mut tls = TlsParameters::builder(config.smtp_host.clone());
    if let Some(path) = &config.ca_cert {
        let pem = std::fs::read(path).map_err(|_| Error::internal())?;
        tls = tls.add_root_certificate(Certificate::from_pem(&pem).map_err(|_| Error::internal())?);
    }
    let tls = tls.build().map_err(|_| Error::internal())?;
    let hello = ClientId::Domain(
        url::Url::parse(&config.public_url)
            .map_err(|_| Error::internal())?
            .host_str()
            .unwrap_or("localhost")
            .into(),
    );
    let connection_error = || {
        Error::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "Bucket could not connect securely to the outgoing mail server. Ask your administrator to check the SMTP settings.",
        )
    };
    let mut c = SmtpConnection::connect(
        (config.smtp_host.as_str(), config.smtp_port),
        Some(Duration::from_secs(8)),
        &hello,
        if config.smtp_starttls {
            None
        } else {
            Some(&tls)
        },
        None,
    )
    .map_err(|_| connection_error())?;
    if config.smtp_starttls {
        c.starttls(&tls, &hello).map_err(|_| connection_error())?;
    }
    if !c.is_encrypted() {
        return Err(connection_error());
    }
    c.auth(
        &[Mechanism::Plain, Mechanism::Login],
        &Credentials::new(mailbox.into(), password),
    ).map_err(|_| Error::bad("The outgoing mail server did not accept this mailbox password. Check that the account or app password allows SMTP."))?;
    Ok(c)
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SendDraft {
    pub id: String,
    pub address: String,
    pub to: String,
    pub subject: String,
    pub text: String,
    pub password: Option<String>,
    pub reply: Option<Reply>,
}
#[derive(Clone, Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct Reply {
    pub uid: u32,
    pub validity: u32,
    #[serde(default)]
    pub folder: Folder,
}
impl App {
    pub async fn open_sending(
        &self,
        s: &Session,
        mailbox: &str,
        identity: &str,
        password: String,
    ) -> Result<()> {
        let config = self.config.clone();
        let destination = mailbox.to_owned();
        let c = tokio::task::spawn_blocking(move || connect(&config, &destination, password))
            .await
            .map_err(|_| Error::internal())??;
        self.sending.insert(s, mailbox, identity, c)
    }
    pub async fn mail_can_send(&self, user: &User, s: &Session, address: &str) -> Result<bool> {
        self.mail_target(user, s, address).await?;
        if address == user.email {
            return Ok(true);
        }
        let row = self
            .db
            .resources(Some(&user.email))?
            .into_iter()
            .find(|r| r.address == address)
            .ok_or_else(|| Error::forbidden("You cannot send from this address."))?;
        if row.kind == Kind::Mailbox {
            return Ok(true);
        }
        let remote = self.checked(&row).await?;
        Ok(number(&remote["sender_allowed"]) == 1 || number(&remote["sender_allowed_int"]) == 1)
    }
    pub async fn mail_send(&self, user: &User, s: &Session, mut input: SendDraft) -> Result<Value> {
        input.address = email(&input.address)?;
        if !(32..=64).contains(&input.id.len())
            || !input.id.bytes().all(|b| b.is_ascii_hexdigit())
            || input.subject.len() > 998
            || input.subject.chars().any(char::is_control)
            || input.to.len() > 2048
            || input.to.chars().any(char::is_control)
            || input.text.len() > 64 * 1024
            || input.text.trim().is_empty()
        {
            return Err(Error::bad(
                "Check the recipients, subject, and message. Messages may contain up to 64 KB of text.",
            ));
        }
        let recipients = parse_recipients(&input.to)?;
        let target = self.mail_target(user, s, &input.address).await?;
        if !self.mail_can_send(user, s, &input.address).await? {
            return Err(Error::forbidden(
                "Sending from this alias is not allowed by your mail server.",
            ));
        }
        let digest = crate::hash(&serde_json::to_string(
            &json!({"address":input.address,"to":input.to,"subject":input.subject,"text":input.text,"reply":input.reply}),
        )?);
        if let Some(old) = self.db.outgoing(&input.id, &user.email)? {
            if old["digest"] != digest {
                return Err(Error::conflict(
                    "This send request has changed. Open a new draft before sending.",
                ));
            }
            return if old["status"] == "sent" {
                Ok(json!({"sent":true,"copy_saved":old["copy_saved"]}))
            } else {
                Err(uncertain())
            };
        }
        if !self
            .db
            .limit(&format!("send:{}", crate::hash(&user.email)), 20, 300)?
        {
            return Err(Error::new(
                StatusCode::TOO_MANY_REQUESTS,
                "You have sent too many messages. Try again in a few minutes.",
            ));
        }
        if let Some(password) = input.password.take() {
            if password.is_empty() || password.len() > 1024 {
                return Err(Error::bad("Enter a valid mailbox password."));
            }
            self.open_sending(s, &target.mailbox, &target.identity, password)
                .await?;
        }
        let smtp = self.sending.get(s, &target.mailbox, &target.identity)?;
        let imap = self.inbox.get(s, &target.mailbox, &target.identity)?;
        // Resolve a reply only through the authorized address view, never trusting client headers.
        let reply_header = if let Some(reply) = &input.reply {
            let q = MailQuery {
                address: input.address.clone(),
                uid: Some(reply.uid),
                validity: Some(reply.validity),
                folder: reply.folder.clone(),
                before: None,
                part: None,
            };
            self.mail_body(user, s, q, None).await?;
            let c = imap.clone();
            let reply = reply.clone();
            let address = input.address.clone();
            tokio::task::spawn_blocking(move || {
                let mut c = c.client.lock().map_err(|_| Error::internal())?;
                let folder = resolve_folder(&mut c, &reply.folder, false)?
                    .ok_or_else(|| Error::bad("The original message is no longer available."))?;
                let selected = c.select(folder).map_err(|_| crate::inbox::unavailable())?;
                if selected.uid_validity != Some(reply.validity) {
                    return Err(Error::conflict(
                        "The original message changed. Reopen it before replying.",
                    ));
                }
                let messages = c
                    .uid_fetch(reply.uid.to_string(), "(UID BODY.PEEK[HEADER]<0.65536>)")
                    .map_err(|_| crate::inbox::unavailable())?;
                let header = messages
                    .iter()
                    .find(|f| f.uid == Some(reply.uid))
                    .and_then(|f| f.header())
                    .ok_or_else(|| Error::bad("The original message is no longer available."))?;
                if !matches_folder(header, &address, &reply.folder) {
                    return Err(Error::forbidden("You cannot reply to this message."));
                }
                let headers = mailparse::parse_headers(header)
                    .map_err(|_| Error::bad("The original message could not be read."))?
                    .0;
                Ok(headers.get_first_value("Message-ID").filter(|id| {
                    id.len() < 900
                        && id.starts_with('<')
                        && id.ends_with('>')
                        && !id.chars().any(char::is_control)
                }))
            })
            .await
            .map_err(|_| Error::internal())??
        } else {
            None
        };
        let message_id = format!(
            "<{}.bucket@{}>",
            crate::token(),
            input.address.split('@').nth(1).unwrap()
        );
        let mut builder = Email::builder()
            .from(
                input
                    .address
                    .parse::<Mailbox>()
                    .map_err(|_| Error::bad("Choose a valid sender."))?,
            )
            .subject(&input.subject)
            .message_id(Some(message_id.clone()))
            .header(ContentType::TEXT_PLAIN);
        for recipient in recipients {
            builder = builder.to(recipient)
        }
        if let Some(id) = reply_header {
            builder = builder.in_reply_to(id.clone()).references(id)
        }
        let message = builder
            .body(input.text)
            .map_err(|_| Error::bad("This message could not be prepared."))?;
        let raw = message.formatted();
        let envelope = message.envelope().clone();
        let actor = user.email.clone();
        let address = input.address.clone();
        let id = input.id;
        let db = self.db.clone();
        let sending = self.sending.clone();
        let token = s.token.clone();
        let mailbox = target.mailbox;
        let permit = self.inbox.slots.clone().try_acquire_owned().map_err(|_| {
            Error::new(
                StatusCode::TOO_MANY_REQUESTS,
                "The mail service is busy. Try again shortly.",
            )
        })?;
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            let mut smtp_client = smtp.client.lock().map_err(|_| Error::internal())?;
            if smtp.expires <= now() || !smtp_client.test_connected() {
                drop(smtp_client);
                sending.remove_mailbox(&token, &mailbox);
                return Err(sending_locked());
            }
            // Serialize duplicate IDs on this connection before any SMTP submission.
            if let Some(old) = db.outgoing(&id, &actor)? {
                return if old["digest"] == digest && old["status"] == "sent" {
                    Ok(json!({"sent": true, "copy_saved": old["copy_saved"]}))
                } else {
                    Err(uncertain())
                };
            }
            db.start_outgoing(&id, &actor, &address, &digest, &message_id)?;
            if let Err(error) = smtp_client.send(&envelope, &raw) {
                let rejected = error.is_permanent() || error.is_transient();
                drop(smtp_client);
                sending.remove_mailbox(&token, &mailbox);
                db.finish_outgoing(&id, if rejected { "rejected" } else { "pending" }, false)?;
                db.audit(&actor, "mail.send.failed", &address, if rejected { "failed" } else { "review" })?;
                return Err(if rejected {
                    Error::bad("The mail server rejected this message. Check the sender and recipients before trying again.")
                } else {
                    uncertain()
                });
            }
            // Record delivery before appending a copy: copy failures must never invite a resend.
            db.finish_outgoing(&id, "sent", false)?;
            drop(smtp_client);
            let copy_saved = (|| -> Result<()> {
                let mut client = imap.client.lock().map_err(|_| Error::internal())?;
                let folder = resolve_folder(&mut client, &Folder::Sent, true)?
                    .ok_or_else(Error::internal)?;
                client.append(&folder, &raw).flag(imap::types::Flag::Seen).finish()
                    .map_err(|_| crate::inbox::unavailable())?;
                Ok(())
            })().is_ok();
            db.finish_outgoing(&id, "sent", copy_saved)?;
            db.audit(&actor, "mail.send", &address, "success")?;
            Ok(json!({"sent": true, "copy_saved": copy_saved}))
        }).await.map_err(|_| Error::internal())?
    }
}
fn parse_recipients(value: &str) -> Result<Vec<Mailbox>> {
    let parsed = mailparse::addrparse(value)
        .map_err(|_| Error::bad("Enter valid recipient email addresses."))?;
    let mut recipients = vec![];
    for item in parsed.iter() {
        match item {
            mailparse::MailAddr::Single(s) => recipients.push(
                s.addr
                    .parse::<Mailbox>()
                    .map_err(|_| Error::bad("Enter valid recipient email addresses."))?,
            ),
            mailparse::MailAddr::Group(_) => {
                return Err(Error::bad(
                    "Enter individual recipient email addresses, separated by commas.",
                ));
            }
        }
    }
    if recipients.is_empty() || recipients.len() > 20 {
        return Err(Error::bad("Choose between 1 and 20 recipients."));
    }
    Ok(recipients)
}
