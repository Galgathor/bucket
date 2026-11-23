use crate::{
    error::{Error, Result},
    model::*,
};
use rusqlite::{Connection, OptionalExtension, params};
use std::{
    sync::Mutex,
    time::{SystemTime, UNIX_EPOCH},
};

pub fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}
pub struct Db(Mutex<Connection>);
impl Db {
    pub fn open(path: &str) -> Result<Self> {
        let conn = Connection::open(path)?;
        conn.busy_timeout(std::time::Duration::from_secs(15))?;
        conn.execute_batch(
            "PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL; PRAGMA foreign_keys=ON;",
        )?;
        let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if version > 3 {
            return Err(Error::internal());
        }
        conn.execute_batch("BEGIN IMMEDIATE;
CREATE TABLE IF NOT EXISTS settings(id INTEGER PRIMARY KEY CHECK(id=1),value TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS users(email TEXT PRIMARY KEY,enabled INTEGER NOT NULL,policy TEXT,identity TEXT);
CREATE TABLE IF NOT EXISTS sessions(token TEXT PRIMARY KEY,email TEXT NOT NULL,csrf TEXT NOT NULL,expires INTEGER NOT NULL,identity TEXT NOT NULL);
CREATE INDEX IF NOT EXISTS session_expiry ON sessions(expires);
CREATE TABLE IF NOT EXISTS resources(id INTEGER PRIMARY KEY,address TEXT NOT NULL UNIQUE,kind TEXT NOT NULL,owner TEXT NOT NULL,name TEXT NOT NULL,marker TEXT NOT NULL UNIQUE,state TEXT NOT NULL,remote_id TEXT,identity TEXT,operation TEXT,created INTEGER NOT NULL);
CREATE INDEX IF NOT EXISTS resource_owner ON resources(owner);
CREATE TABLE IF NOT EXISTS audit(id INTEGER PRIMARY KEY,at INTEGER NOT NULL,actor TEXT NOT NULL,action TEXT NOT NULL,address TEXT NOT NULL,outcome TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS attempts(key TEXT PRIMARY KEY,count INTEGER NOT NULL,reset INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS preferences(email TEXT PRIMARY KEY,alias_suffix INTEGER NOT NULL DEFAULT 1 CHECK(alias_suffix IN (0,1)));
CREATE TABLE IF NOT EXISTS outgoing(id TEXT PRIMARY KEY,owner TEXT NOT NULL,address TEXT NOT NULL,digest TEXT NOT NULL,status TEXT NOT NULL,message_id TEXT NOT NULL,copy_saved INTEGER NOT NULL DEFAULT 0,created INTEGER NOT NULL);
PRAGMA user_version=3; COMMIT;")?;
        Ok(Self(Mutex::new(conn)))
    }
    pub fn with<T>(&self, f: impl FnOnce(&mut Connection) -> Result<T>) -> Result<T> {
        {
            let mut guard = self.0.lock().map_err(|_| Error::internal())?;
            f(&mut guard)
        }
    }
    pub fn outgoing(&self, id: &str, owner: &str) -> Result<Option<serde_json::Value>> {
        self.with(|c| {
            Ok(c.query_row(
                "SELECT digest,status,message_id,copy_saved FROM outgoing WHERE id=? AND owner=?",
                params![id, owner],
                |r| {
                    Ok(serde_json::json!({
                        "digest": r.get::<_, String>(0)?,
                        "status": r.get::<_, String>(1)?,
                        "message_id": r.get::<_, String>(2)?,
                        "copy_saved": r.get::<_, bool>(3)?,
                    }))
                },
            )
            .optional()?)
        })
    }
    pub fn start_outgoing(
        &self,
        id: &str,
        owner: &str,
        address: &str,
        digest: &str,
        message_id: &str,
    ) -> Result<()> {
        self.with(|c| {
            c.execute(
                "INSERT INTO outgoing(id,owner,address,digest,status,message_id,created) VALUES(?,?,?,?,'pending',?,?)",
                params![id, owner, address, digest, message_id, now()],
            )?;
            Ok(())
        })
    }
    pub fn finish_outgoing(&self, id: &str, status: &str, copy_saved: bool) -> Result<()> {
        self.with(|c| {
            c.execute(
                "UPDATE outgoing SET status=?,copy_saved=? WHERE id=?",
                params![status, copy_saved, id],
            )?;
            Ok(())
        })
    }
    pub fn alias_suffix(&self, email: &str) -> Result<bool> {
        self.with(|c| {
            Ok(c.query_row(
                "SELECT alias_suffix FROM preferences WHERE email=?",
                [email],
                |r| r.get(0),
            )
            .optional()?
            .unwrap_or(true))
        })
    }
    pub fn save_alias_suffix(&self, email: &str, value: bool) -> Result<()> {
        self.with(|c|{c.execute("INSERT INTO preferences(email,alias_suffix) VALUES(?,?) ON CONFLICT(email) DO UPDATE SET alias_suffix=excluded.alias_suffix",params![email,value])?;Ok(())})
    }
    pub fn config(&self) -> Result<Configuration> {
        self.with(|c| {
            let value: Option<String> = c
                .query_row("SELECT value FROM settings WHERE id=1", [], |r| r.get(0))
                .optional()?;
            Ok(match value {
                Some(v) => serde_json::from_str(&v)?,
                None => Configuration::default(),
            })
        })
    }
    pub fn save_config(&self, config: &Configuration) -> Result<()> {
        self.with(|c|{c.execute("INSERT INTO settings(id,value) VALUES(1,?) ON CONFLICT(id) DO UPDATE SET value=excluded.value",[serde_json::to_string(config)?])?; Ok(())})
    }
    pub fn user(&self, email: &str) -> Result<Option<User>> {
        self.with(|c| {
            let row: Option<(String, bool, Option<String>)> = c
                .query_row(
                    "SELECT email,enabled,policy FROM users WHERE email=?",
                    [email],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .optional()?;
            row.map(|(email, enabled, policy)| {
                Ok(User {
                    email,
                    enabled,
                    policy: policy.map(|p| serde_json::from_str(&p)).transpose()?,
                })
            })
            .transpose()
        })
    }
    pub fn users(&self) -> Result<Vec<User>> {
        self.with(|c| {
            let mut q = c.prepare("SELECT email,enabled,policy FROM users ORDER BY email")?;
            let rows = q.query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, bool>(1)?,
                    r.get::<_, Option<String>>(2)?,
                ))
            })?;
            rows.map(|r| {
                let (email, enabled, policy) = r?;
                Ok(User {
                    email,
                    enabled,
                    policy: policy.map(|p| serde_json::from_str(&p)).transpose()?,
                })
            })
            .collect()
        })
    }
    pub fn save_user(&self, user: &User) -> Result<()> {
        self.save_user_identity(user, None)
    }
    pub fn save_user_identity(&self, user: &User, identity: Option<&str>) -> Result<()> {
        self.with(|c| {
            let t=c.transaction()?;
            t.execute("INSERT INTO users(email,enabled,policy,identity) VALUES(?,?,?,?) ON CONFLICT(email) DO UPDATE SET enabled=excluded.enabled,policy=excluded.policy,identity=COALESCE(excluded.identity,users.identity)",params![user.email,user.enabled,user.policy.as_ref().map(serde_json::to_string).transpose()?,identity])?;
            t.execute("DELETE FROM sessions WHERE email=?",[&user.email])?;
            t.commit()?;Ok(())
        })
    }
    pub fn user_identity(&self, email: &str) -> Result<Option<String>> {
        self.with(|c| {
            Ok(
                c.query_row("SELECT identity FROM users WHERE email=?", [email], |r| {
                    r.get::<_, Option<String>>(0)
                })
                .optional()?
                .flatten(),
            )
        })
    }
    pub fn bind_identity(&self, email: &str, identity: &str) -> Result<()> {
        self.with(|c| {
            c.execute(
                "UPDATE users SET identity=? WHERE email=? AND identity IS NULL",
                params![identity, email],
            )?;
            Ok(())
        })
    }
    pub fn delete_user(&self, email: &str) -> Result<()> {
        self.with(|c| {
            let t = c.transaction()?;
            t.execute("DELETE FROM users WHERE email=?", [email])?;
            t.execute("DELETE FROM sessions WHERE email=?", [email])?;
            t.commit()?;
            Ok(())
        })
    }
    pub fn session(&self, token: &str) -> Result<Option<Session>> {
        self.with(|c| {
        Ok(c.query_row("SELECT token,email,csrf,expires,identity FROM sessions WHERE token=? AND expires>?",params![token,now()],|r|Ok(Session{token:r.get(0)?,email:r.get(1)?,csrf:r.get(2)?,expires:r.get(3)?,identity:r.get(4)?})).optional()?)
    })
    }
    pub fn save_session(&self, s: &Session) -> Result<()> {
        self.with(|c|{
        c.execute("DELETE FROM sessions WHERE expires<=?",[now()])?;
        // Bound the number of active sessions per account.
        c.execute("DELETE FROM sessions WHERE email=? AND token NOT IN (SELECT token FROM sessions WHERE email=? ORDER BY expires DESC LIMIT 9)",params![s.email,s.email])?;
        c.execute("INSERT INTO sessions(token,email,csrf,expires,identity) VALUES(?,?,?,?,?)",params![s.token,s.email,s.csrf,s.expires,s.identity])?; Ok(())
    })
    }
    pub fn delete_session(&self, token: &str) -> Result<()> {
        self.with(|c| {
            c.execute("DELETE FROM sessions WHERE token=?", [token])?;
            Ok(())
        })
    }
    pub fn limit(&self, key: &str, max: i64, seconds: i64) -> Result<bool> {
        self.with(|c| {
        let t=c.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        t.execute("DELETE FROM attempts WHERE reset<?",[now()])?;
        t.execute("INSERT INTO attempts(key,count,reset) VALUES(?,1,?) ON CONFLICT(key) DO UPDATE SET count=count+1",params![key,now()+seconds])?;
        let count:i64=t.query_row("SELECT count FROM attempts WHERE key=?",[key],|r|r.get(0))?;
        t.commit()?; Ok(count<=max)
    })
    }
    pub fn audit(&self, actor: &str, action: &str, address: &str, outcome: &str) -> Result<()> {
        self.with(|c| {
            c.execute(
                "INSERT INTO audit(at,actor,action,address,outcome) VALUES(?,?,?,?,?)",
                params![now(), actor, action, address, outcome],
            )?;
            tracing::info!(actor, action, address, outcome, "audit");
            Ok(())
        })
    }
    pub fn audits(&self) -> Result<Vec<serde_json::Value>> {
        self.with(|c| {
        let mut q=c.prepare("SELECT at,actor,action,address,outcome FROM audit ORDER BY id DESC LIMIT 200")?;
        let rows=q.query_map([],|r|Ok(serde_json::json!({"at":r.get::<_,i64>(0)?,"actor":r.get::<_,String>(1)?,"action":r.get::<_,String>(2)?,"address":r.get::<_,String>(3)?,"outcome":r.get::<_,String>(4)?})))?;
        rows.collect::<std::result::Result<Vec<_>,_>>().map_err(Into::into)
    })
    }
    pub fn resources(&self, owner: Option<&str>) -> Result<Vec<Resource>> {
        self.with(|c| {
        let mut q=c.prepare("SELECT id,address,kind,owner,name,marker,state,remote_id,identity,operation,created FROM resources WHERE (?1 IS NULL OR owner=?1) ORDER BY created DESC,id DESC")?;
        let rows=q.query_map([owner],resource)?;
        rows.collect::<std::result::Result<Vec<_>,_>>().map_err(Into::into)
    })
    }
    pub fn resource(&self, id: i64) -> Result<Resource> {
        self.with(|c| {
        c.query_row("SELECT id,address,kind,owner,name,marker,state,remote_id,identity,operation,created FROM resources WHERE id=?",[id],resource).optional()?.ok_or_else(||Error::new(axum::http::StatusCode::NOT_FOUND,"Address not found."))
    })
    }
    pub fn insert_resource(
        &self,
        address: &str,
        kind: &Kind,
        owner: &str,
        name: &str,
        marker: &str,
    ) -> Result<i64> {
        self.with(|c| {
        c.execute("INSERT INTO resources(address,kind,owner,name,marker,state,operation,created) VALUES(?,?,?,?,?,'pending','create',?)",params![address,kind.as_str(),owner,name,marker,now()])?; Ok(c.last_insert_rowid())
    })
    }
    pub fn pending(&self, id: i64, operation: &str) -> Result<()> {
        self.with(|c| {
            c.execute(
                "UPDATE resources SET state='pending',operation=? WHERE id=?",
                params![operation, id],
            )?;
            Ok(())
        })
    }
    pub fn review(&self, id: i64) -> Result<()> {
        self.with(|c| {
            c.execute("UPDATE resources SET state='review' WHERE id=?", [id])?;
            Ok(())
        })
    }
    pub fn confirm(&self, id: i64, remote_id: &str, identity: &str, name: &str) -> Result<()> {
        self.with(|c| {
        c.execute("UPDATE resources SET state='managed',operation=NULL,remote_id=?,identity=?,name=? WHERE id=?",params![remote_id,identity,name,id])?; Ok(())
    })
    }
    pub fn forget(&self, id: i64) -> Result<()> {
        self.with(|c| {
            c.execute("DELETE FROM resources WHERE id=?", [id])?;
            Ok(())
        })
    }
    pub fn transfer(&self, id: i64, owner: &str) -> Result<()> {
        self.with(|c| {
            c.execute(
                "UPDATE resources SET owner=? WHERE id=?",
                params![owner, id],
            )?;
            Ok(())
        })
    }
}
fn resource(r: &rusqlite::Row<'_>) -> rusqlite::Result<Resource> {
    let kind: String = r.get(2)?;
    Ok(Resource {
        id: r.get(0)?,
        address: r.get(1)?,
        kind: if kind == "mailbox" {
            Kind::Mailbox
        } else {
            Kind::Alias
        },
        owner: r.get(3)?,
        name: r.get(4)?,
        marker: r.get(5)?,
        state: r.get(6)?,
        remote_id: r.get(7)?,
        identity: r.get(8)?,
        operation: r.get(9)?,
        created: r.get(10)?,
    })
}
