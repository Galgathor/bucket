use crate::{
    App,
    error::{Error, Result},
    mailcow::{self, active, number, text},
    model::*,
};
use serde_json::{Value, json};

impl App {
    fn current_user(&self, user: &User) -> Result<User> {
        self.db
            .user(&user.email)?
            .filter(|u| u.enabled)
            .ok_or_else(|| Error::forbidden("Your access has changed. Sign in again."))
    }

    pub async fn remote(&self, row: &Resource) -> Result<Option<Value>> {
        self.mailcow
            .get(
                &row.kind,
                if row.kind == Kind::Alias {
                    row.remote_id.as_deref().unwrap_or(&row.address)
                } else {
                    &row.address
                },
            )
            .await
    }
    pub async fn checked(&self, row: &Resource) -> Result<Value> {
        let remote = self.remote(row).await?;
        if row.state != "managed" || !remote.as_ref().is_some_and(|r| mailcow::matches(row, r)) {
            if row.state == "managed" {
                self.db.review(row.id)?;
            }
            return Err(Error::conflict(
                "This address needs an administrator’s review before it can be changed.",
            ));
        }
        Ok(remote.unwrap())
    }
    pub fn public(row: &Resource, remote: Option<&Value>) -> Value {
        let okay = row.state == "managed" && remote.is_some_and(|r| mailcow::matches(row, r));
        let v = remote.filter(|_| okay).cloned().unwrap_or(json!({}));
        json!({"id":row.id,"address":row.address,"kind":row.kind,"name":row.name,
            "state":if okay{"managed"}else{"review"},"active":active(&v),
            "destination":if row.kind==Kind::Alias{text(&v["goto"])}else{String::new()},
            "storage":number(&v["quota"]),"used":number(&v["quota_used"])})
    }
    pub fn owned(&self, id: i64, email: &str) -> Result<Resource> {
        let row = self.db.resource(id)?;
        if row.owner != email {
            return Err(Error::new(
                axum::http::StatusCode::NOT_FOUND,
                "Address not found.",
            ));
        }
        Ok(row)
    }
    pub fn policy(&self, user: &User) -> Result<Policy> {
        Ok(user.policy.clone().unwrap_or(self.db.config()?.policy))
    }
    pub async fn destinations(&self, email: &str) -> Result<Vec<String>> {
        let mut addresses = vec![email.to_owned()];
        for row in self
            .db
            .resources(Some(email))?
            .into_iter()
            .filter(|r| r.kind == Kind::Mailbox && r.state == "managed")
        {
            let remote = self.remote(&row).await?;
            if remote
                .as_ref()
                .is_some_and(|v| mailcow::matches(&row, v) && active(v) == 1)
            {
                addresses.push(row.address);
            }
        }
        Ok(addresses)
    }
    pub async fn destination(&self, email: &str, destination: &str) -> Result<()> {
        if !self
            .destinations(email)
            .await?
            .iter()
            .any(|s| s == destination)
        {
            return Err(Error::forbidden("Choose one of your available mailboxes."));
        }
        Ok(())
    }
    pub async fn storage(&self, user: &User, amount: u64, exclude: Option<i64>) -> Result<()> {
        let policy = self.policy(user)?;
        if amount < 128 || amount > policy.max_storage {
            return Err(Error::forbidden(
                "Choose storage within your allowed limit.",
            ));
        }
        let mut total = amount;
        for row in self
            .db
            .resources(Some(&user.email))?
            .into_iter()
            .filter(|r| r.kind == Kind::Mailbox && Some(r.id) != exclude)
        {
            let r = self.checked(&row).await?;
            let quota = number(&r["quota"]);
            if quota <= 0 {
                return Err(Error::conflict(
                    "An existing mailbox has unlimited storage. Ask an administrator to review it.",
                ));
            }
            total += (quota as u64).div_ceil(1048576);
        }
        if total > policy.total_storage {
            return Err(Error::forbidden(
                "Your total storage limit would be exceeded.",
            ));
        }
        Ok(())
    }
    pub async fn list(&self, email: &str) -> Result<Vec<Value>> {
        let mut result = vec![];
        for row in self.db.resources(Some(email))? {
            let remote = self.remote(&row).await?;
            if row.state == "managed" && !remote.as_ref().is_some_and(|r| mailcow::matches(&row, r))
            {
                self.db.review(row.id)?;
            }
            result.push(Self::public(&row, remote.as_ref()));
        }
        Ok(result)
    }
    pub async fn create(&self, user: &User, mut data: Create) -> Result<Value> {
        data.validate()?;
        let _guard = self.writes.lock().await;
        let user = self.current_user(user)?;
        let user = &user;
        let config = self.db.config()?;
        let policy = self.policy(user)?;
        if !config.domains.contains(&data.domain) {
            return Err(Error::forbidden("This domain is not available."));
        }
        if !self.mailcow.domains().await?.contains(&data.domain) {
            return Err(Error::conflict(
                "This domain is currently unavailable on the mail server.",
            ));
        }
        let (enabled, max) = if data.kind == Kind::Mailbox {
            (policy.mailboxes, policy.max_mailboxes)
        } else {
            (policy.aliases, policy.max_aliases)
        };
        let rows = self.db.resources(Some(&user.email))?;
        if !enabled || rows.iter().filter(|r| r.kind == data.kind).count() >= max as usize {
            return Err(Error::forbidden(
                "Your limit for this address type has been reached.",
            ));
        }
        let address = format!("{}@{}", data.local, data.domain);
        if self
            .db
            .resources(None)?
            .iter()
            .any(|r| r.address == address)
            || self.mailcow.get(&Kind::Mailbox, &address).await?.is_some()
            || self.mailcow.get(&Kind::Alias, &address).await?.is_some()
        {
            return Err(Error::conflict("This email address is already in use."));
        }
        let marker = format!("bucket:{}", crate::token());
        let body = if data.kind == Kind::Mailbox {
            let amount = data.storage.unwrap_or(policy.default_storage);
            self.storage(user, amount, None).await?;
            json!({"local_part":data.local,"domain":data.domain,"name":data.name,"password":data.password,"password2":data.password,"quota":amount,"active":"1","tags":[marker],"authsource":"mailcow"})
        } else {
            let destination = data.destination.as_deref().unwrap_or(&user.email);
            self.destination(&user.email, destination).await?;
            json!({"address":address,"goto":destination,"active":"1","sogo_visible":"1","sender_allowed":"1","private_comment":marker})
        };
        let id = self
            .db
            .insert_resource(&address, &data.kind, &user.email, &data.name, &marker)?;
        self.db
            .audit(&user.email, "create.started", &address, "pending")?;
        let outcome: Result<Value> = async {
            self.mailcow
                .request(&format!("add/{}", data.kind.as_str()), Some(body))
                .await?;
            let row = self.db.resource(id)?;
            let remote = self
                .remote(&row)
                .await?
                .ok_or_else(|| Error::upstream(true))?;
            if !mailcow::matches(&row, &remote) {
                return Err(Error::upstream(true));
            }
            let remote_id = mailcow::payload_id(&row.kind, &remote, &address);
            if remote_id.is_empty() {
                return Err(Error::upstream(true));
            }
            self.db
                .confirm(id, &remote_id, &mailcow::identity(&remote), &data.name)?;
            self.db.audit(&user.email, "create", &address, "success")?;
            Ok(Self::public(&self.db.resource(id)?, Some(&remote)))
        }
        .await;
        if outcome.is_err() {
            self.db
                .audit(&user.email, "create.failed", &address, "review")?;
        }
        outcome
    }
    pub fn protect_login(&self, row: &Resource) -> Result<()> {
        if self.db.user(&row.address)?.is_some() || row.address == self.config.admin {
            return Err(Error::conflict(
                "This address is used to sign in to Bucket. An administrator must remove its access first.",
            ));
        }
        Ok(())
    }
    pub async fn edit(&self, id: i64, user: &User, data: Edit) -> Result<Value> {
        if data.name.len() > 100 {
            return Err(Error::bad("Keep the name under 100 characters."));
        }
        if let Some(p) = &data.password {
            password(p)?;
        }
        let _guard = self.writes.lock().await;
        let user = self.current_user(user)?;
        let user = &user;
        let row = self.owned(id, &user.email)?;
        let remote = self.checked(&row).await?;
        let mut attr = json!({"active":i32::from(data.active)});
        if row.kind == Kind::Mailbox {
            if data.destination.is_some() {
                return Err(Error::bad("Mailboxes do not have forwarding destinations."));
            }
            if !data.active || data.password.is_some() {
                self.protect_login(&row)?;
            }
            attr["name"] = json!(data.name);
            if let Some(amount) = data.storage {
                self.storage(user, amount, Some(id)).await?;
                if amount * 1048576 < (number(&remote["quota_used"]).max(0) as u64) {
                    return Err(Error::conflict(
                        "Storage cannot be smaller than the space currently used.",
                    ));
                }
                attr["quota"] = json!(amount);
            }
            if let Some(p) = data.password {
                attr["password"] = json!(p);
                attr["password2"] = attr["password"].clone();
            }
        } else {
            if data.password.is_some() || data.storage.is_some() {
                return Err(Error::bad("Aliases do not have passwords or storage."));
            }
            let destination = data
                .destination
                .as_deref()
                .ok_or_else(|| Error::bad("Choose a destination mailbox."))?;
            self.destination(&user.email, destination).await?;
            attr["goto"] = json!(destination);
        }
        self.db.pending(id, "edit")?;
        self.db
            .audit(&user.email, "edit.started", &row.address, "pending")?;
        let result: Result<Value> = async {
            let identifier = if row.kind == Kind::Alias {
                row.remote_id.as_deref().unwrap_or(&row.address)
            } else {
                &row.address
            };
            self.mailcow
                .request(
                    &format!("edit/{}", row.kind.as_str()),
                    Some(json!({"items":[identifier],"attr":attr})),
                )
                .await?;
            let updated = self
                .remote(&row)
                .await?
                .ok_or_else(|| Error::upstream(true))?;
            if !mailcow::matches(&row, &updated) || active(&updated) != i64::from(data.active) {
                return Err(Error::upstream(true));
            }
            if row.kind == Kind::Alias
                && text(&updated["goto"]) != data.destination.unwrap_or_default()
            {
                return Err(Error::upstream(true));
            }
            if row.kind == Kind::Mailbox
                && data
                    .storage
                    .is_some_and(|s| number(&updated["quota"]) != s as i64 * 1048576)
            {
                return Err(Error::upstream(true));
            }
            self.db.confirm(
                id,
                row.remote_id.as_deref().unwrap_or(&row.address),
                &mailcow::identity(&updated),
                &data.name,
            )?;
            self.db
                .audit(&user.email, "edit", &row.address, "success")?;
            Ok(Self::public(&self.db.resource(id)?, Some(&updated)))
        }
        .await;
        if result.is_err() {
            self.db
                .audit(&user.email, "edit.failed", &row.address, "review")?;
        }
        result
    }
    pub async fn delete(&self, id: i64, user: &User, confirmation: &str) -> Result<()> {
        let _guard = self.writes.lock().await;
        let user = self.current_user(user)?;
        let user = &user;
        let row = self.owned(id, &user.email)?;
        if confirmation != row.address {
            return Err(Error::bad(
                "Type the full email address to confirm deletion.",
            ));
        }
        self.checked(&row).await?;
        if row.kind == Kind::Mailbox {
            self.protect_login(&row)?;
            let aliases = self.mailcow.request("get/alias/all", None).await?;
            let aliases = aliases.as_array().ok_or_else(|| Error::upstream(false))?;
            if aliases
                .iter()
                .any(|a| text(&a["goto"]).split(',').any(|d| d.trim() == row.address))
            {
                return Err(Error::conflict(
                    "Change or delete aliases that forward to this mailbox first.",
                ));
            }
        }
        self.db.pending(id, "delete")?;
        self.db
            .audit(&user.email, "delete.started", &row.address, "pending")?;
        let result: Result<()> = async {
            let identifier = if row.kind == Kind::Alias {
                row.remote_id.as_deref().unwrap_or(&row.address)
            } else {
                &row.address
            };
            self.mailcow
                .request(
                    &format!("delete/{}", row.kind.as_str()),
                    Some(json!([identifier])),
                )
                .await?;
            if self.remote(&row).await?.is_some() {
                return Err(Error::upstream(true));
            }
            self.db.forget(id)?;
            self.db
                .audit(&user.email, "delete", &row.address, "success")?;
            Ok(())
        }
        .await;
        if result.is_err() {
            self.db
                .audit(&user.email, "delete.failed", &row.address, "review")?;
        }
        result
    }
    pub async fn resolve(&self, id: i64, actor: &str, input: Resolve) -> Result<()> {
        let _guard = self.writes.lock().await;
        let row = self.db.resource(id)?;
        if input.confirmation != row.address {
            return Err(Error::bad("Type the full email address to confirm."));
        }
        match input.action.as_str() {
            "forget" => {
                // Forgetting only releases a reservation when the remote address is gone,
                // or its identity changed. Matching managed addresses must be deleted normally.
                let remote = self.remote(&row).await?;
                if remote.as_ref().is_some_and(|v| mailcow::matches(&row, v)) {
                    return Err(Error::conflict(
                        "This address still belongs to Bucket. Verify it or delete it from its owner’s account.",
                    ));
                }
                self.db.forget(id)?;
            }
            "verify" => {
                let remote = self.remote(&row).await?.ok_or_else(|| {
                    Error::conflict("This address no longer exists. Remove the record instead.")
                })?;
                if !mailcow::matches(&row, &remote) {
                    return Err(Error::conflict(
                        "Ownership could not be verified. Check the address in Mailcow.",
                    ));
                }
                if row.operation.as_deref() == Some("delete") {
                    return Err(Error::conflict(
                        "Deletion was not completed. Confirm the remaining mailbox and clear the operation using transfer or contact the operator.",
                    ));
                }
                self.db.confirm(
                    id,
                    &mailcow::payload_id(&row.kind, &remote, &row.address),
                    &mailcow::identity(&remote),
                    &row.name,
                )?;
            }
            "transfer" => {
                let owner = email(input.owner.as_deref().unwrap_or(""))?;
                if !self.db.user(&owner)?.is_some_and(|u| u.enabled) {
                    return Err(Error::bad("Choose a user with Bucket access."));
                }
                let remote = self
                    .remote(&row)
                    .await?
                    .ok_or_else(|| Error::conflict("This address no longer exists."))?;
                if !mailcow::matches(&row, &remote) {
                    return Err(Error::conflict("Ownership could not be verified."));
                }
                self.db.transfer(id, &owner)?;
                self.db.confirm(
                    id,
                    &mailcow::payload_id(&row.kind, &remote, &row.address),
                    &mailcow::identity(&remote),
                    &row.name,
                )?;
            }
            _ => return Err(Error::bad("Choose a supported action.")),
        }
        self.db.audit(
            actor,
            &format!("ownership.{}", input.action),
            &row.address,
            "success",
        )?;
        Ok(())
    }
}
