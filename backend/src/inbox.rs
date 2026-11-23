use crate::{
    App,
    db::now,
    error::{Error, Result},
    mailcow::{ImapSession, active, identity, text},
    model::{Kind, Session, User, email},
};
use mailparse::MailHeaderMap;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex},
};

pub(crate) const MAX_MESSAGE: u32 = 5 * 1024 * 1024;
type ConnectionKey = (String, String);
type ConnectionMap = HashMap<ConnectionKey, Arc<Connected>>;
pub(crate) struct Connected {
    pub(crate) client: Mutex<ImapSession>,
    pub(crate) expires: i64,
    identity: String,
}
pub struct Connections {
    connections: Mutex<ConnectionMap>,
    pub(crate) slots: Arc<tokio::sync::Semaphore>,
}
impl Default for Connections {
    fn default() -> Self {
        Self {
            connections: Mutex::new(HashMap::new()),
            slots: Arc::new(tokio::sync::Semaphore::new(4)),
        }
    }
}
impl Connections {
    pub fn insert(
        &self,
        session: &Session,
        mailbox: &str,
        identity: &str,
        client: ImapSession,
    ) -> Result<()> {
        let mut map = self.connections.lock().map_err(|_| Error::internal())?;
        map.retain(|_, v| v.expires > now());
        if !map.contains_key(&(session.token.clone(), mailbox.into()))
            && (map.len() >= 128 || map.keys().filter(|(s, _)| s == &session.token).count() >= 8)
        {
            return Err(Error::conflict(
                "Too many mailboxes are open. Sign out and try again.",
            ));
        }
        map.insert(
            (session.token.clone(), mailbox.into()),
            Arc::new(Connected {
                client: Mutex::new(client),
                expires: session.expires,
                identity: identity.into(),
            }),
        );
        Ok(())
    }
    pub fn remove(&self, token: &str) {
        if let Ok(mut m) = self.connections.lock() {
            m.retain(|(t, _), _| t != token);
        }
    }
    pub fn remove_mailbox(&self, token: &str, mailbox: &str) {
        if let Ok(mut map) = self.connections.lock() {
            map.remove(&(token.into(), mailbox.into()));
        }
    }
    pub fn cleanup(&self, db: &crate::db::Db) {
        if let Ok(mut m) = self.connections.lock() {
            m.retain(|(token, _), _| db.session(token).ok().flatten().is_some());
        }
    }
    pub fn opened(&self, session: &Session, mailbox: &str, identity: &str) -> bool {
        self.get(session, mailbox, identity).is_ok()
    }
    pub(crate) fn get(&self, s: &Session, mailbox: &str, identity: &str) -> Result<Arc<Connected>> {
        let map = self.connections.lock().map_err(|_| Error::internal())?;
        let conn = map
            .get(&(s.token.clone(), mailbox.into()))
            .ok_or_else(locked)?
            .clone();
        drop(map);
        if conn.expires <= now() || conn.identity != identity {
            return Err(locked());
        }
        Ok(conn)
    }
}
fn locked() -> Error {
    Error::new(
        axum::http::StatusCode::LOCKED,
        "Enter this mailbox’s password to open its inbox.",
    )
}
pub(crate) fn unavailable() -> Error {
    Error::new(
        axum::http::StatusCode::SERVICE_UNAVAILABLE,
        "The inbox connection was interrupted. Open the mailbox again to reconnect.",
    )
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MailQuery {
    pub address: String,
    pub before: Option<u32>,
    pub uid: Option<u32>,
    pub validity: Option<u32>,
    pub part: Option<usize>,
    #[serde(default)]
    pub folder: Folder,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Unlock {
    pub address: String,
    pub password: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Read {
    pub address: String,
    pub uid: u32,
    pub validity: u32,
    pub read: bool,
    #[serde(default)]
    pub folder: Folder,
}
#[derive(Serialize, Clone)]
pub struct MailAddress {
    pub address: String,
    pub mailbox: String,
    pub kind: String,
    pub name: String,
    pub unlocked: bool,
    pub sending_unlocked: bool,
    pub can_send: bool,
}
#[derive(Serialize)]
pub struct Message {
    pub uid: u32,
    pub subject: String,
    pub from: String,
    pub to: String,
    pub date: String,
    pub read: bool,
    pub size: u32,
    pub reply_to: String,
}
#[derive(Serialize)]
pub struct Attachment {
    pub part: usize,
    pub name: String,
    pub size: usize,
}
#[derive(Serialize)]
pub struct Body {
    pub message: Message,
    pub text: String,
    pub html: String,
    pub attachments: Vec<Attachment>,
}
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Folder {
    #[default]
    Inbox,
    Spam,
    Trash,
    Sent,
}
impl Folder {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Inbox => "Inbox",
            Self::Spam => "Spam",
            Self::Trash => "Trash",
            Self::Sent => "Sent",
        }
    }
}

pub struct Target {
    pub mailbox: String,
    pub identity: String,
}
impl App {
    pub async fn mail_target(&self, user: &User, s: &Session, address: &str) -> Result<Target> {
        let address = email(address)?;
        let mailbox = if address == user.email {
            address.clone()
        } else {
            let row = self
                .db
                .resources(Some(&user.email))?
                .into_iter()
                .find(|r| r.address == address)
                .ok_or_else(|| Error::forbidden("You cannot open this address."))?;
            let remote = self.checked(&row).await?;
            if active(&remote) != 1 {
                return Err(Error::conflict("This address is turned off."));
            }
            if row.kind == Kind::Mailbox {
                row.address
            } else {
                let destination = text(&remote["goto"]);
                self.destination(&user.email, &destination).await?;
                destination
            }
        };
        let stamp = if mailbox == user.email {
            s.identity.clone()
        } else {
            let row = self
                .db
                .resources(Some(&user.email))?
                .into_iter()
                .find(|r| r.kind == Kind::Mailbox && r.address == mailbox)
                .ok_or_else(|| Error::forbidden("You cannot open this mailbox."))?;
            let remote = self.checked(&row).await?;
            if active(&remote) != 1 {
                return Err(Error::conflict("This mailbox is turned off."));
            }
            identity(&remote)
        };
        Ok(Target {
            mailbox,
            identity: stamp,
        })
    }
    pub async fn mail_addresses(&self, user: &User, s: &Session) -> Result<Vec<MailAddress>> {
        let mut addresses = vec![(
            user.email.clone(),
            "mailbox".into(),
            "Primary mailbox".into(),
        )];
        addresses.extend(
            self.list(&user.email)
                .await?
                .into_iter()
                .filter(|r| r["state"] == "managed" && r["active"] == 1)
                .map(|r| (text(&r["address"]), text(&r["kind"]), text(&r["name"]))),
        );
        let mut result = vec![];
        for (address, kind, name) in addresses {
            if let Ok(target) = self.mail_target(user, s, &address).await {
                result.push(MailAddress {
                    unlocked: self.inbox.opened(s, &target.mailbox, &target.identity),
                    sending_unlocked: self.sending.opened(s, &target.mailbox, &target.identity),
                    can_send: self.mail_can_send(user, s, &address).await?,
                    address,
                    mailbox: target.mailbox,
                    kind,
                    name,
                });
            }
        }
        Ok(result)
    }
    pub async fn mail_unlock(&self, user: &User, s: &Session, input: Unlock) -> Result<()> {
        if input.password.is_empty() || input.password.len() > 1024 {
            return Err(Error::bad("Enter the mailbox password."));
        }
        let target = self.mail_target(user, s, &input.address).await?;
        if !self.db.limit(
            &format!("mail-unlock:{}", crate::hash(&user.email)),
            10,
            900,
        )? {
            return Err(Error::new(
                axum::http::StatusCode::TOO_MANY_REQUESTS,
                "Too many attempts. Try again in 15 minutes.",
            ));
        }
        let smtp_password = input.password.clone();
        let client = self
            .auth
            .connect(target.mailbox.clone(), input.password)
            .await
            .map_err(|e| {
                if e.status == axum::http::StatusCode::UNAUTHORIZED {
                    Error::bad(
                        "The mailbox password was not accepted. Use an IMAP-enabled password.",
                    )
                } else {
                    e
                }
            })?
            .ok_or_else(|| Error::bad("The mailbox password was not accepted."))?;
        self.inbox
            .insert(s, &target.mailbox, &target.identity, client)?;
        if self
            .open_sending(s, &target.mailbox, &target.identity, smtp_password)
            .await
            .is_err()
        {
            tracing::warn!("outgoing mail connection unavailable; inbox remains available");
        }
        self.db
            .audit(&user.email, "mail.open", &input.address, "success")?;
        Ok(())
    }
    pub async fn mail_list(&self, user: &User, s: &Session, mut q: MailQuery) -> Result<Value> {
        q.address = email(&q.address)?;
        let target = self.mail_target(user, s, &q.address).await?;
        let conn = self.inbox.get(s, &target.mailbox, &target.identity)?;
        let permit = self.inbox.slots.clone().try_acquire_owned().map_err(|_| {
            Error::new(
                axum::http::StatusCode::TOO_MANY_REQUESTS,
                "The inbox is busy. Try again shortly.",
            )
        })?;
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            let mut c = conn.client.lock().map_err(|_| Error::internal())?;
            if conn.expires <= now() {
                return Err(locked());
            }
            let selected = {
                let Some(name) = resolve_folder(&mut c, &q.folder, false)? else {
                    return Ok(json!({"messages":[],"validity":0,"next":null}));
                };
                c.select(&name).map_err(|_| unavailable())?
            };
            let validity = selected.uid_validity.ok_or_else(unavailable)?;
            if q.validity.is_some_and(|old| old != validity) {
                return Err(Error::conflict(
                    "The inbox changed. Refresh the message list.",
                ));
            }
            let end = q
                .before
                .unwrap_or(selected.uid_next.unwrap_or(u32::MAX))
                .saturating_sub(1);
            if end == 0 {
                return Ok(json!({"messages":[],"validity":validity,"next":null}));
            }
            // Limit candidates scanned per page; IMAP HEADER searches are substring searches,
            // so recheck parsed recipient addresses before returning any metadata.
            let start = end.saturating_sub(1999).max(1);
            let address_query = search_address(&q.address, &q.folder);
            let query = format!("UID {start}:{end} {address_query}");
            let mut ids: Vec<_> = c
                .uid_search(query)
                .map_err(|_| unavailable())?
                .into_iter()
                .filter(|id| *id <= end && *id >= start)
                .collect();
            ids.sort_unstable_by(|a, b| b.cmp(a));
            ids.truncate(200);
            let next = ids
                .last()
                .copied()
                .filter(|id| *id > 1)
                .or((start > 1).then_some(start));
            let mut messages = vec![];
            for chunk in ids.chunks(25) {
                let set = chunk
                    .iter()
                    .map(u32::to_string)
                    .collect::<Vec<_>>()
                    .join(",");
                let fetched = c
                    .uid_fetch(set, "(UID FLAGS RFC822.SIZE BODY.PEEK[HEADER]<0.65536>)")
                    .map_err(|_| unavailable())?;
                for f in fetched.iter() {
                    if let Some(header) = f.header()
                        && matches_folder(header, &q.address, &q.folder)
                    {
                        messages.push(summary(f, header));
                    }
                }
                if messages.len() >= 50 {
                    break;
                }
            }
            messages.sort_by_key(|m| std::cmp::Reverse(m.uid));
            messages.truncate(50);
            // Preserve pagination even when a sparse alias has no matches in this window.
            let next = if messages.len() >= 50 {
                messages.last().map(|m| m.uid)
            } else {
                next
            };
            Ok(json!({"messages":messages,"validity":validity,"next":next}))
        })
        .await
        .map_err(|_| Error::internal())?
    }
    pub async fn mail_body(
        &self,
        user: &User,
        s: &Session,
        mut q: MailQuery,
        read: Option<bool>,
    ) -> Result<(Body, Option<(String, Vec<u8>)>)> {
        q.address = email(&q.address)?;
        let target = self.mail_target(user, s, &q.address).await?;
        let conn = self.inbox.get(s, &target.mailbox, &target.identity)?;
        let permit = self.inbox.slots.clone().try_acquire_owned().map_err(|_| {
            Error::new(
                axum::http::StatusCode::TOO_MANY_REQUESTS,
                "The inbox is busy. Try again shortly.",
            )
        })?;
        tokio::task::spawn_blocking(move || {
            let _permit=permit;
            let mut c = conn.client.lock().map_err(|_| Error::internal())?;
            if conn.expires <= now() {
                return Err(locked());
            }
            let name=resolve_folder(&mut c,&q.folder,false)?.ok_or_else(||Error::new(axum::http::StatusCode::NOT_FOUND,"This folder is empty."))?;
            let selected = c.select(&name).map_err(|_| unavailable())?;
            if q.validity.is_none() || selected.uid_validity != q.validity {
                return Err(Error::conflict(
                    "The inbox changed. Refresh the message list.",
                ));
            }
            let uid = q
                .uid
                .filter(|u| *u > 0)
                .ok_or_else(|| Error::bad("Choose a message."))?;
            let set = uid.to_string();
            let headers = c
                .uid_fetch(&set, "(UID FLAGS RFC822.SIZE BODY.PEEK[HEADER]<0.65536>)")
                .map_err(|_| unavailable())?;
            let f = headers.iter().find(|f| f.uid == Some(uid)).ok_or_else(|| {
                Error::new(axum::http::StatusCode::NOT_FOUND, "Message not found.")
            })?;
            let header = f.header().ok_or_else(unavailable)?;
            if !matches_folder(header, &q.address,&q.folder) {
                return Err(Error::new(
                    axum::http::StatusCode::NOT_FOUND,
                    "Message not found.",
                ));
            }
            let mut message = summary(f, header);
            if message.size > MAX_MESSAGE {
                return Err(Error::bad(
                    "This message is larger than 5 MB. Open it in your usual mail client.",
                ));
            }
            drop(headers);
            let content = c
                .uid_fetch(&set, format!("(UID BODY.PEEK[]<0.{}>)", MAX_MESSAGE + 1))
                .map_err(|_| unavailable())?;
            let raw = content
                .iter()
                .find(|f| f.uid == Some(uid))
                .and_then(|f| f.body())
                .ok_or_else(unavailable)?;
            if raw.len() > MAX_MESSAGE as usize {
                return Err(Error::bad("This message is larger than 5 MB."));
            }
            if raw.split(|b|*b==b'\n').filter(|line|line.get(..13).is_some_and(|prefix|prefix.eq_ignore_ascii_case(b"content-type:"))).take(101).count()>100 {
                return Err(Error::bad("This message has too many parts to display. Open it in your usual mail client."));
            }
            let parsed = mailparse::parse_mail(raw)
                .map_err(|_| Error::bad("This message could not be read."))?;
            let mut body = Body {
                message: message.clone_for_body(),
                text: String::new(),
                html: String::new(),
                attachments: vec![],
            };
            let mut parts = vec![];
            collect_parts(&parsed, &mut parts, 0);
            let mut attachment = None;
            for (part, node) in parts.iter().enumerate() {
                let disposition = node.get_content_disposition();
                let is_file = disposition.disposition == mailparse::DispositionType::Attachment
                    || disposition.params.contains_key("filename")
                    || node.ctype.params.contains_key("name");
                if is_file {
                    let data = node
                        .get_body_raw()
                        .map_err(|_| Error::bad("This attachment could not be read."))?;
                    let name = disposition
                        .params
                        .get("filename")
                        .or(node.ctype.params.get("name"))
                        .cloned()
                        .unwrap_or("attachment".into());
                    body.attachments.push(Attachment {
                        part,
                        name: name.clone(),
                        size: data.len(),
                    });
                    if q.part == Some(part) {
                        attachment = Some((name, data));
                    }
                } else if node.ctype.mimetype == "text/plain" && body.text.is_empty() {
                    body.text = node.get_body().unwrap_or_default();
                } else if node.ctype.mimetype == "text/html" && body.html.is_empty() {
                    let source=node.get_body().unwrap_or_default();
                    if source.len()>256*1024 {return Err(Error::bad("This message’s formatted content is too large to display. Open it in your usual mail client."));}
                    body.html = sanitize_html(&source);
                }
            }
            drop(content);
            if let Some(read) = read {
                c
                    .uid_store(
                        &set,
                        if read {
                            "+FLAGS.SILENT (\\Seen)"
                        } else {
                            "-FLAGS.SILENT (\\Seen)"
                        },
                    )
                    .map_err(|_| unavailable())?;
                message.read = read;
                body.message = message;
            }
            Ok((body, attachment))
        })
        .await
        .map_err(|_| Error::internal())?
    }
}
impl Message {
    fn clone_for_body(&self) -> Self {
        Self {
            uid: self.uid,
            subject: self.subject.clone(),
            from: self.from.clone(),
            to: self.to.clone(),
            date: self.date.clone(),
            read: self.read,
            size: self.size,
            reply_to: self.reply_to.clone(),
        }
    }
}
fn summary(f: &imap::types::Fetch<'_>, header: &[u8]) -> Message {
    let headers = mailparse::parse_headers(header)
        .map(|(h, _)| h)
        .unwrap_or_default();
    let get = |name| headers.get_first_value(name).unwrap_or_default();
    Message {
        uid: f.uid.unwrap_or(0),
        subject: get("Subject"),
        from: get("From"),
        to: get("To"),
        date: get("Date"),
        read: f
            .flags()
            .iter()
            .any(|flag| matches!(flag, imap::types::Flag::Seen)),
        size: f.size.unwrap_or(0),
        reply_to: reply_recipient(&headers),
    }
}
pub fn recipients_match(header: &[u8], address: &str) -> bool {
    let Ok((headers, _)) = mailparse::parse_headers(header) else {
        return false;
    };
    let addresses = |names: &[&str]| -> HashSet<String> {
        let mut result = HashSet::new();
        for name in names {
            for value in headers.get_all_values(name) {
                if let Ok(list) = mailparse::addrparse(&value) {
                    for item in list.iter() {
                        match item {
                            mailparse::MailAddr::Single(s) => {
                                result.insert(s.addr.to_lowercase());
                            }
                            mailparse::MailAddr::Group(g) => {
                                for s in &g.addrs {
                                    result.insert(s.addr.to_lowercase());
                                }
                            }
                        }
                    }
                }
            }
        }
        result
    };
    let original = addresses(&["X-Original-To", "Envelope-To"]);
    if !original.is_empty() {
        return original.contains(&address.to_lowercase());
    }
    let explicit = addresses(&["To", "Cc"]);
    if !explicit.is_empty() {
        return explicit.contains(&address.to_lowercase());
    }
    addresses(&["Delivered-To"]).contains(&address.to_lowercase())
}
fn collect_parts<'a, 'b>(
    mail: &'a mailparse::ParsedMail<'b>,
    parts: &mut Vec<&'a mailparse::ParsedMail<'b>>,
    depth: usize,
) {
    if depth > 20 {
        return;
    }
    let disposition = mail.get_content_disposition();
    if mail.subparts.is_empty()
        || disposition.disposition == mailparse::DispositionType::Attachment
        || disposition.params.contains_key("filename")
    {
        parts.push(mail);
    } else {
        for part in &mail.subparts {
            collect_parts(part, parts, depth + 1);
        }
    }
}
pub fn sanitize_html(html: &str) -> String {
    let clean = ammonia::Builder::default()
        .url_schemes(HashSet::new())
        .rm_tag_attributes("a", &["href"])
        .rm_tags(&[
            "img", "style", "link", "iframe", "form", "input", "button", "video", "audio",
            "source", "svg",
        ])
        .clean(html)
        .to_string();
    format!(
        "<!doctype html><html><head><meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; style-src 'unsafe-inline'; img-src 'none'; form-action 'none'; base-uri 'none'\"><meta name=\"color-scheme\" content=\"light\"><style>body{{color:#242633;background:#fff;font:16px/1.6 system-ui;overflow-wrap:anywhere;padding:8px}}pre{{white-space:pre-wrap}}a{{color:inherit}}</style></head><body>{clean}</body></html>"
    )
}

pub(crate) fn resolve_folder(
    client: &mut ImapSession,
    folder: &Folder,
    create: bool,
) -> Result<Option<String>> {
    use imap_proto::NameAttribute;
    if *folder == Folder::Inbox {
        return Ok(Some("INBOX".into()));
    }
    let names = client.list(None, Some("*")).map_err(|_| unavailable())?;
    let special = names.iter().find(|name| {
        name.attributes().iter().any(|a| {
            matches!(
                (folder, a),
                (Folder::Spam, NameAttribute::Junk)
                    | (Folder::Trash, NameAttribute::Trash)
                    | (Folder::Sent, NameAttribute::Sent)
            )
        })
    });
    if let Some(name) = special {
        return Ok(Some(name.name().into()));
    }
    let common = names.iter().find(|name| match folder {
        Folder::Spam => ["junk", "spam"].contains(&name.name().to_ascii_lowercase().as_str()),
        Folder::Trash => name.name().eq_ignore_ascii_case("Trash"),
        Folder::Sent => ["sent", "sent messages", "sent items"]
            .contains(&name.name().to_ascii_lowercase().as_str()),
        Folder::Inbox => false,
    });
    if let Some(name) = common {
        return Ok(Some(name.name().into()));
    }
    drop(names);
    if !create {
        return Ok(None);
    }
    let name = match folder {
        Folder::Spam => "Junk",
        Folder::Trash => "Trash",
        Folder::Sent => "Sent",
        Folder::Inbox => "INBOX",
    };
    client
        .create(name)
        .map_err(|_| Error::conflict("The mail server could not create this folder."))?;
    Ok(Some(name.into()))
}
pub(crate) fn search_address(address: &str, folder: &Folder) -> String {
    let incoming = format!(
        "OR OR OR OR HEADER To \"{address}\" HEADER Cc \"{address}\" HEADER X-Original-To \"{address}\" HEADER Delivered-To \"{address}\" HEADER Envelope-To \"{address}\""
    );
    match folder {
        Folder::Sent => format!("HEADER From \"{address}\""),
        Folder::Trash => format!("OR HEADER From \"{address}\" {incoming}"),
        _ => incoming,
    }
}
pub(crate) fn matches_folder(header: &[u8], address: &str, folder: &Folder) -> bool {
    let sender = || {
        mailparse::parse_headers(header).ok().is_some_and(|(headers, _)| {
            headers.get_first_value("From")
                .and_then(|from| mailparse::addrparse(&from).ok())
                .is_some_and(|list| list.iter().any(|item| {
                    matches!(item, mailparse::MailAddr::Single(sender) if sender.addr.eq_ignore_ascii_case(address))
                }))
        })
    };
    match folder {
        Folder::Sent => sender(),
        Folder::Trash => sender() || recipients_match(header, address),
        _ => recipients_match(header, address),
    }
}
pub(crate) fn reply_recipient(headers: &[mailparse::MailHeader<'_>]) -> String {
    headers
        .get_first_value("Reply-To")
        .or_else(|| headers.get_first_value("From"))
        .and_then(|s| mailparse::addrparse(&s).ok())
        .and_then(|list| {
            list.iter().find_map(|item| {
                if let mailparse::MailAddr::Single(s) = item {
                    Some(s.addr.clone())
                } else {
                    None
                }
            })
        })
        .unwrap_or_default()
}
