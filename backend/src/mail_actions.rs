use crate::{
    App,
    error::{Error, Result},
    inbox::{Folder, matches_folder, resolve_folder, unavailable},
    model::{Session, User, email},
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::HashMap;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Move {
    pub address: String,
    pub uid: u32,
    pub validity: u32,
    #[serde(default)]
    pub folder: Folder,
    pub destination: Folder,
}
impl App {
    pub async fn mail_move(&self, user: &User, s: &Session, input: Move) -> Result<()> {
        let address = email(&input.address)?;
        if input.uid == 0 || input.folder == input.destination || input.destination == Folder::Sent
        {
            return Err(Error::bad(
                "Choose Inbox, Spam, or Trash as the destination.",
            ));
        }
        let target = self.mail_target(user, s, &address).await?;
        let conn = self.inbox.get(s, &target.mailbox, &target.identity)?;
        let permit = self
            .inbox
            .slots
            .clone()
            .try_acquire_owned()
            .map_err(|_| Error::conflict("The inbox is busy. Try again shortly."))?;
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            let mut client = conn.client.lock().map_err(|_| Error::internal())?;
            let source = resolve_folder(&mut client, &input.folder, false)?
                .ok_or_else(|| Error::bad("The original folder is no longer available."))?;
            let selected = client.select(source).map_err(|_| unavailable())?;
            if selected.uid_validity != Some(input.validity) {
                return Err(Error::conflict("The folder changed. Refresh it before moving this message."));
            }
            let uid = input.uid.to_string();
            let headers = client.uid_fetch(&uid, "(UID BODY.PEEK[HEADER]<0.65536>)")
                .map_err(|_| unavailable())?;
            let header = headers.iter().find(|f| f.uid == Some(input.uid))
                .and_then(|f| f.header())
                .ok_or_else(|| Error::new(axum::http::StatusCode::NOT_FOUND, "Message not found."))?;
            if !matches_folder(header, &address, &input.folder) {
                return Err(Error::new(axum::http::StatusCode::NOT_FOUND, "Message not found."));
            }
            drop(headers);
            let destination = resolve_folder(&mut client, &input.destination, true)?
                .ok_or_else(Error::internal)?;
            client.uid_mv(uid, destination).map_err(|_| Error::conflict(
                "The move could not be confirmed. Refresh the folder before trying again. Your mail server must support IMAP MOVE."
            ))?;
            Ok(())
        }).await.map_err(|_| Error::internal())?
    }
    pub async fn mail_status(&self, user: &User, s: &Session) -> Result<Value> {
        let addresses = self.mail_addresses(user, s).await?;
        let mut rows = addresses
            .iter()
            .map(|a| {
                let mut row = serde_json::to_value(a).unwrap();
                row["unread"] = Value::Null;
                row["unread_limited"] = json!(false);
                row
            })
            .collect::<Vec<_>>();
        let mut groups: HashMap<String, Vec<usize>> = HashMap::new();
        for (i, a) in addresses.iter().enumerate() {
            if a.unlocked {
                groups.entry(a.mailbox.clone()).or_default().push(i);
            }
        }
        for (mailbox, indices) in groups {
            let target = self
                .mail_target(user, s, &addresses[indices[0]].address)
                .await?;
            let Ok(conn) = self.inbox.get(s, &mailbox, &target.identity) else {
                continue;
            };
            let Ok(permit) = self.inbox.slots.clone().try_acquire_owned() else {
                continue;
            };
            let names = indices
                .iter()
                .map(|i| addresses[*i].address.clone())
                .collect::<Vec<_>>();
            let result = tokio::task::spawn_blocking(move || -> Result<(Vec<u32>, bool)> {
                let _permit = permit;
                let mut client = conn.client.lock().map_err(|_| Error::internal())?;
                client.select("INBOX").map_err(|_| unavailable())?;
                let mut ids = client
                    .uid_search("UNSEEN")
                    .map_err(|_| unavailable())?
                    .into_iter()
                    .collect::<Vec<_>>();
                ids.sort_unstable_by(|a, b| b.cmp(a));
                let limited = ids.len() > 500;
                ids.truncate(500);
                let mut counts = vec![0; names.len()];
                for chunk in ids.chunks(25) {
                    let set = chunk
                        .iter()
                        .map(u32::to_string)
                        .collect::<Vec<_>>()
                        .join(",");
                    let messages = client
                        .uid_fetch(set, "(UID FLAGS BODY.PEEK[HEADER]<0.65536>)")
                        .map_err(|_| unavailable())?;
                    for message in messages.iter() {
                        if message
                            .flags()
                            .iter()
                            .any(|f| matches!(f, imap::types::Flag::Seen))
                        {
                            continue;
                        }
                        if let Some(header) = message.header() {
                            for (i, address) in names.iter().enumerate() {
                                if crate::inbox::recipients_match(header, address) {
                                    counts[i] += 1;
                                }
                            }
                        }
                    }
                }
                Ok((counts, limited))
            })
            .await
            .map_err(|_| Error::internal())?;
            match result {
                Ok((counts, limited)) => {
                    for (index, count) in indices.into_iter().zip(counts) {
                        rows[index]["unread"] = if count == 0 && limited {
                            Value::Null
                        } else {
                            json!(count)
                        };
                        rows[index]["unread_limited"] = json!(limited);
                    }
                }
                Err(_) => {
                    self.inbox.remove_mailbox(&s.token, &mailbox);
                    for index in indices {
                        rows[index]["unlocked"] = json!(false);
                    }
                }
            }
        }
        Ok(json!({"addresses":rows}))
    }
}
