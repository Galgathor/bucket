use axum::{
    Json, Router,
    body::Body,
    extract::{ConnectInfo, Path, State},
    http::{Method, Request, StatusCode},
};
use bucket::{
    App, config::Config, db::now, error::Result, hash, mailcow::Authenticator, model::*, token,
};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};
use tower::ServiceExt;

#[derive(Default)]
struct Remote {
    mailboxes: HashMap<String, Value>,
    aliases: HashMap<String, Value>,
    writes: Vec<(String, Value)>,
    failure: Option<&'static str>,
}
#[derive(Clone)]
struct Mock(Arc<Mutex<Remote>>);
async fn mailcow(
    State(mock): State<Mock>,
    Path(path): Path<String>,
    method: Method,
    body: Option<Json<Value>>,
) -> (StatusCode, Json<Value>) {
    let mut r = mock.0.lock().unwrap();
    let parts: Vec<_> = path.split('/').collect();
    if method == Method::GET {
        let value = match parts.as_slice() {
            ["get", "domain", "all"] => json!([{"domain_name":"example.com","active":1}]),
            ["get", "mailbox", id] => r.mailboxes.get(*id).cloned().unwrap_or(json!({})),
            ["get", "alias", "all"] => json!(r.aliases.values().collect::<Vec<_>>()),
            ["get", "alias", id] => r
                .aliases
                .get(*id)
                .or_else(|| {
                    r.aliases
                        .values()
                        .find(|v| v["id"].to_string().trim_matches('"') == *id)
                })
                .cloned()
                .unwrap_or(json!({})),
            _ => return (StatusCode::NOT_FOUND, Json(json!({}))),
        };
        return (StatusCode::OK, Json(value));
    }
    let body = body.unwrap().0;
    r.writes.push((path.clone(), body.clone()));
    let failure = r.failure.take();
    if failure == Some("reject") {
        return (
            StatusCode::OK,
            Json(json!([{"type":"danger","msg":"max_mailbox_exceeded"}])),
        );
    }
    match parts.as_slice() {
        ["add", "mailbox"] => {
            let address = format!(
                "{}@{}",
                body["local_part"].as_str().unwrap(),
                body["domain"].as_str().unwrap()
            );
            r.mailboxes.insert(address.clone(),json!({"username":address,"active":1,"quota":body["quota"].as_u64().unwrap()*1048576,"quota_used":0,"tags":body["tags"],"created":"2026-11-22 12:00:00","name":body["name"],"authsource":"mailcow"}));
        }
        ["add", "alias"] => {
            let id = r.aliases.len() + 1;
            r.aliases.insert(body["address"].as_str().unwrap().into(),json!({"address":body["address"],"id":id,"goto":body["goto"],"private_comment":body["private_comment"],"active":1,"created":"2026-11-22 12:00:00"}));
        }
        ["edit", kind] => {
            let id = body["items"][0].as_str().unwrap();
            let row = if *kind == "mailbox" {
                r.mailboxes.get_mut(id)
            } else {
                r.aliases
                    .values_mut()
                    .find(|v| v["id"].to_string().trim_matches('"') == id)
            }
            .unwrap();
            for (key, value) in body["attr"].as_object().unwrap() {
                if key == "quota" {
                    row[key] = json!(value.as_u64().unwrap() * 1048576)
                } else if key != "password" && key != "password2" {
                    row[key] = value.clone()
                }
            }
        }
        ["delete", kind] => {
            let id = body[0].as_str().unwrap();
            if *kind == "mailbox" {
                r.mailboxes.remove(id);
            } else {
                let key = r
                    .aliases
                    .iter()
                    .find(|(_, v)| v["id"].to_string().trim_matches('"') == id)
                    .map(|(k, _)| k.clone())
                    .unwrap();
                r.aliases.remove(&key);
            }
        }
        _ => return (StatusCode::NOT_FOUND, Json(json!({}))),
    }
    if failure == Some("partial") {
        return (StatusCode::BAD_GATEWAY, Json(json!({})));
    }
    (
        StatusCode::OK,
        Json(json!([{"type":"success","msg":"saved"}])),
    )
}
struct FakeAuth {
    okay: bool,
    calls: AtomicUsize,
}
#[async_trait::async_trait]
impl Authenticator for FakeAuth {
    async fn authenticate(&self, _: String, _: String) -> Result<bool> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(self.okay)
    }
}
struct Fixture {
    _dir: tempfile::TempDir,
    app: App,
    mock: Mock,
    server: tokio::task::JoinHandle<()>,
    auth: Arc<FakeAuth>,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.server.abort();
    }
}
impl Fixture {
    async fn new() -> Self {
        let mock = Mock(Arc::new(Mutex::new(Remote::default())));
        for email in ["admin@example.com", "alice@example.com", "bob@example.com"] {
            mock.0.lock().unwrap().mailboxes.insert(email.into(),json!({"username":email,"active":1,"authsource":"mailcow","quota":1073741824,"quota_used":0,"created":"2026-01-01 00:00:00"}));
        }
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let upstream = Router::new()
            .route("/api/v1/{*path}", axum::routing::any(mailcow))
            .with_state(mock.clone());
        let server = tokio::spawn(async move { axum::serve(listener, upstream).await.unwrap() });
        let dir = tempfile::tempdir().unwrap();
        let config = Config {
            public_url: "https://bucket.example.com".into(),
            mailcow_url: format!("http://{addr}"),
            api_key: "test-api-key-only".into(),
            imap_host: "localhost".into(),
            imap_port: 993,
            admin: "admin@example.com".into(),
            database: dir.path().join("bucket.db").to_string_lossy().into(),
            frontend: dir.path().to_string_lossy().into(),
            secure: true,
            bind: "127.0.0.1:0".into(),
            session_hours: 12,
            ca_cert: None,
        };
        let mut app = App::new(config).unwrap();
        let auth = Arc::new(FakeAuth {
            okay: true,
            calls: AtomicUsize::new(0),
        });
        app.auth = auth.clone();
        for email in ["alice@example.com", "bob@example.com"] {
            app.db
                .save_user(&User {
                    email: email.into(),
                    enabled: true,
                    policy: None,
                })
                .unwrap();
        }
        app.db
            .save_config(&Configuration {
                domains: vec!["example.com".into()],
                policy: Policy::default(),
            })
            .unwrap();
        Self {
            _dir: dir,
            app,
            mock,
            server,
            auth,
        }
    }
    fn session(&self, email: &str) -> (String, String) {
        let value = token();
        let csrf = token();
        self.app
            .db
            .save_session(&Session {
                token: hash(&value),
                email: email.into(),
                csrf: csrf.clone(),
                expires: now() + 3600,
                identity: "2026-01-01 00:00:00".into(),
            })
            .unwrap();
        (value, csrf)
    }
    async fn call(
        &self,
        method: &str,
        path: &str,
        session: Option<&(String, String)>,
        body: Option<Value>,
    ) -> (StatusCode, Value, axum::http::HeaderMap) {
        let mut req = Request::builder()
            .method(method)
            .uri(path)
            .header("origin", "https://bucket.example.com");
        if let Some((cookie, csrf)) = session {
            req = req
                .header("cookie", format!("__Host-bucket={cookie}"))
                .header("x-csrf-token", csrf);
        }
        if body.is_some() {
            req = req.header("content-type", "application/json");
        }
        let mut req = req
            .body(body.map_or(Body::empty(), |b| Body::from(b.to_string())))
            .unwrap();
        req.extensions_mut()
            .insert(ConnectInfo("127.0.0.1:9000".parse::<SocketAddr>().unwrap()));
        let response = bucket::router(self.app.clone()).oneshot(req).await.unwrap();
        let status = response.status();
        let headers = response.headers().clone();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let value = serde_json::from_slice(&bytes).unwrap_or(json!(null));
        (status, value, headers)
    }
    async fn alias(&self, session: &(String, String), local: &str) -> Value {
        let (status, value, _) = self
            .call(
                "POST",
                "/api/addresses",
                Some(session),
                Some(json!({"kind":"alias","local":local,"domain":"example.com"})),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{value}");
        value
    }
    async fn mailbox(&self, session: &(String, String), local: &str) -> Value {
        let(status,value,_)=self.call("POST","/api/addresses",Some(session),Some(json!({"kind":"mailbox","local":local,"domain":"example.com","password":"A-very-good-password123!","storage":512}))).await;
        assert_eq!(status, StatusCode::OK, "{value}");
        value
    }
}
#[tokio::test]
async fn login_requires_enrollment_and_imap_and_sets_secure_session() {
    let f = Fixture::new().await;
    let (s, _, _) = f
        .call(
            "POST",
            "/api/login",
            None,
            Some(json!({"email":"unknown@example.com","password":"anything"})),
        )
        .await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    assert_eq!(f.auth.calls.load(Ordering::SeqCst), 0);
    let (s, v, h) = f
        .call(
            "POST",
            "/api/login",
            None,
            Some(json!({"email":"alice@example.com","password":"test-only-password"})),
        )
        .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(f.auth.calls.load(Ordering::SeqCst), 1);
    assert_eq!(v["admin"], false);
    let cookie = h["set-cookie"].to_str().unwrap();
    for attribute in [
        "__Host-bucket=",
        "HttpOnly",
        "Secure",
        "SameSite=Strict",
        "Path=/",
    ] {
        assert!(cookie.contains(attribute));
    }
    let raw = cookie.split(';').next().unwrap().split('=').nth(1).unwrap();
    let session = f.app.db.session(&hash(raw)).unwrap().unwrap();
    assert_ne!(session.token, raw);
    assert_eq!(session.csrf, v["csrf"]);
    let audits = serde_json::to_string(&f.app.db.audits().unwrap()).unwrap();
    assert!(!audits.contains("test-only-password"));
}
#[tokio::test]
async fn imap_rejection_and_rate_limits_are_enforced() {
    let mut f = Fixture::new().await;
    f.app.auth = Arc::new(FakeAuth {
        okay: false,
        calls: AtomicUsize::new(0),
    });
    for _ in 0..10 {
        assert_eq!(
            f.call(
                "POST",
                "/api/login",
                None,
                Some(json!({"email":"alice@example.com","password":"bad"}))
            )
            .await
            .0,
            StatusCode::UNAUTHORIZED
        );
    }
    assert_eq!(
        f.call(
            "POST",
            "/api/login",
            None,
            Some(json!({"email":"alice@example.com","password":"bad"}))
        )
        .await
        .0,
        StatusCode::TOO_MANY_REQUESTS
    );
    assert!(f.app.db.session(&hash("bad")).unwrap().is_none());
}
#[tokio::test]
async fn rejects_external_identity_sources() {
    let f = Fixture::new().await;
    f.mock
        .0
        .lock()
        .unwrap()
        .mailboxes
        .get_mut("alice@example.com")
        .unwrap()["authsource"] = json!("generic-oidc");
    assert_eq!(
        f.call(
            "POST",
            "/api/login",
            None,
            Some(json!({"email":"alice@example.com","password":"test"}))
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(f.auth.calls.load(Ordering::SeqCst), 0);
}
#[tokio::test]
async fn csrf_and_admin_boundaries() {
    let f = Fixture::new().await;
    let user = f.session("alice@example.com");
    let mut bad = user.clone();
    bad.1 = "invalid".into();
    assert_eq!(
        f.call(
            "POST",
            "/api/addresses",
            Some(&bad),
            Some(json!({"kind":"alias","local":"shop","domain":"example.com"}))
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    for path in [
        "/api/admin/config",
        "/api/admin/users",
        "/api/admin/ownership",
        "/api/admin/audit",
    ] {
        assert_eq!(
            f.call("GET", path, Some(&user), None).await.0,
            StatusCode::FORBIDDEN
        );
    }
    let req = Request::builder()
        .method("POST")
        .uri("/api/login")
        .header("origin", "https://evil.example")
        .header("content-type", "application/json")
        .body(Body::from("{}"))
        .unwrap();
    assert_eq!(
        bucket::router(f.app.clone())
            .oneshot(req)
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
    let (s, _, h) = f.call("GET", "/api/addresses", None, None).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    assert_eq!(h["cache-control"], "no-store");
    assert_eq!(h["x-frame-options"], "DENY");
    assert!(h.contains_key("content-security-policy"));
}
#[tokio::test]
async fn alias_lifecycle_uses_official_payloads() {
    let f = Fixture::new().await;
    let user = f.session("alice@example.com");
    let a = f.alias(&user, "shopping").await;
    assert_eq!(a["destination"], "alice@example.com");
    let id = a["id"].as_i64().unwrap();
    assert_eq!(
        f.call(
            "PUT",
            &format!("/api/addresses/{id}"),
            Some(&user),
            Some(json!({"name":"Shopping","active":false,"destination":"alice@example.com"}))
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        f.call(
            "DELETE",
            &format!("/api/addresses/{id}"),
            Some(&user),
            Some(json!({"confirmation":"wrong"}))
        )
        .await
        .0,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(
        f.call(
            "DELETE",
            &format!("/api/addresses/{id}"),
            Some(&user),
            Some(json!({"confirmation":"shopping@example.com"}))
        )
        .await
        .0,
        StatusCode::OK
    );
    assert!(f.app.db.resources(None).unwrap().is_empty());
    let remote = f.mock.0.lock().unwrap();
    assert_eq!(remote.writes[0].0, "add/alias");
    assert_eq!(remote.writes[0].1["active"], "1");
    assert_eq!(remote.writes[1].1["items"], json!(["1"]));
    assert_eq!(remote.writes[2].1, json!(["1"]));
}
#[tokio::test]
async fn mailbox_lifecycle_and_storage_authority() {
    let f = Fixture::new().await;
    let user = f.session("alice@example.com");
    let a = f.mailbox(&user, "work").await;
    assert_eq!(a["storage"], 512 * 1048576);
    let id = a["id"].as_i64().unwrap();
    let(s,_,_)=f.call("PUT",&format!("/api/addresses/{id}"),Some(&user),Some(json!({"name":"Work","active":true,"storage":1024,"password":"changed-passWord123!"}))).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(
        f.call(
            "PUT",
            &format!("/api/addresses/{id}"),
            Some(&user),
            Some(json!({"name":"Work","active":false}))
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        f.call(
            "DELETE",
            &format!("/api/addresses/{id}"),
            Some(&user),
            Some(json!({"confirmation":"work@example.com"}))
        )
        .await
        .0,
        StatusCode::OK
    );
    let r = f.mock.0.lock().unwrap();
    assert_eq!(r.writes[0].1["password"], r.writes[0].1["password2"]);
    assert_eq!(r.writes[1].1["attr"]["quota"], 1024);
    assert!(
        !f.app
            .db
            .resources(None)
            .unwrap()
            .iter()
            .any(|r| r.name.contains("passWord"))
    );
}
#[tokio::test]
async fn ownership_idor_and_unrestricted_forwarding_are_blocked() {
    let f = Fixture::new().await;
    let alice = f.session("alice@example.com");
    let bob = f.session("bob@example.com");
    let a = f.alias(&alice, "news").await;
    let id = a["id"].as_i64().unwrap();
    assert_eq!(
        f.call(
            "PUT",
            &format!("/api/addresses/{id}"),
            Some(&bob),
            Some(json!({"active":true,"destination":"bob@example.com"}))
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        f.call(
            "DELETE",
            &format!("/api/addresses/{id}"),
            Some(&bob),
            Some(json!({"confirmation":"news@example.com"}))
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        f.call(
            "PUT",
            &format!("/api/addresses/{id}"),
            Some(&alice),
            Some(json!({"active":true,"destination":"outsider@example.net"}))
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    let (_, v, _) = f.call("GET", "/api/addresses", Some(&bob), None).await;
    assert!(v["addresses"].as_array().unwrap().is_empty());
    assert_eq!(f.mock.0.lock().unwrap().writes.len(), 1);
}
#[tokio::test]
async fn existing_addresses_cannot_be_claimed() {
    let f = Fixture::new().await;
    let alice = f.session("alice@example.com");
    assert_eq!(
        f.call(
            "POST",
            "/api/addresses",
            Some(&alice),
            Some(json!({"kind":"alias","local":"bob","domain":"example.com"}))
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    assert!(f.app.db.resources(None).unwrap().is_empty());
}
#[tokio::test]
async fn permissions_domains_and_quota_are_server_enforced() {
    let f = Fixture::new().await;
    let p = Policy {
        aliases: false,
        max_storage: 512,
        default_storage: 512,
        total_storage: 512,
        ..Policy::default()
    };
    f.app
        .db
        .save_user(&User {
            email: "alice@example.com".into(),
            enabled: true,
            policy: Some(p),
        })
        .unwrap();
    let alice = f.session("alice@example.com");
    for (data, status) in [
        (
            json!({"kind":"alias","local":"blocked","domain":"example.com"}),
            StatusCode::FORBIDDEN,
        ),
        (
            json!({"kind":"mailbox","local":"blocked","domain":"example.net","password":"Valid-password123!"}),
            StatusCode::FORBIDDEN,
        ),
        (
            json!({"kind":"mailbox","local":"blocked","domain":"example.com","password":"Valid-password123!","storage":1024}),
            StatusCode::FORBIDDEN,
        ),
    ] {
        assert_eq!(
            f.call("POST", "/api/addresses", Some(&alice), Some(data))
                .await
                .0,
            status
        );
    }
    f.mailbox(&alice, "one").await;
    assert_eq!(f.call("POST","/api/addresses",Some(&alice),Some(json!({"kind":"mailbox","local":"two","domain":"example.com","password":"Valid-password123!","storage":512}))).await.0,StatusCode::FORBIDDEN);
    let _ = alice;
}
#[tokio::test]
async fn http_200_failures_and_partial_writes_remain_reserved_until_review() {
    let f = Fixture::new().await;
    let alice = f.session("alice@example.com");
    let admin = f.session("admin@example.com");
    f.mock.0.lock().unwrap().failure = Some("partial");
    assert_eq!(
        f.call(
            "POST",
            "/api/addresses",
            Some(&alice),
            Some(json!({"kind":"alias","local":"pending","domain":"example.com"}))
        )
        .await
        .0,
        StatusCode::BAD_GATEWAY
    );
    let row = f.app.db.resources(None).unwrap().remove(0);
    assert_eq!(row.state, "pending");
    assert_eq!(
        f.call(
            "PUT",
            &format!("/api/addresses/{}", row.id),
            Some(&alice),
            Some(json!({"active":true,"destination":"alice@example.com"}))
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        f.call(
            "POST",
            &format!("/api/admin/ownership/{}", row.id),
            Some(&admin),
            Some(json!({"action":"verify","confirmation":"pending@example.com"}))
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(f.app.db.resource(row.id).unwrap().state, "managed");
    f.mock.0.lock().unwrap().failure = Some("reject");
    assert_eq!(
        f.call(
            "POST",
            "/api/addresses",
            Some(&alice),
            Some(json!({"kind":"alias","local":"rejected","domain":"example.com"}))
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    let row = f
        .app
        .db
        .resources(None)
        .unwrap()
        .into_iter()
        .find(|r| r.address.starts_with("rejected"))
        .unwrap();
    assert_eq!(row.state, "pending");
    assert_eq!(
        f.call(
            "POST",
            &format!("/api/admin/ownership/{}", row.id),
            Some(&admin),
            Some(json!({"action":"forget","confirmation":"rejected@example.com"}))
        )
        .await
        .0,
        StatusCode::OK
    );
}
#[tokio::test]
async fn external_replacement_is_quarantined() {
    let f = Fixture::new().await;
    let alice = f.session("alice@example.com");
    let a = f.alias(&alice, "changed").await;
    f.mock
        .0
        .lock()
        .unwrap()
        .aliases
        .get_mut("changed@example.com")
        .unwrap()["private_comment"] = json!("external");
    assert_eq!(
        f.call(
            "PUT",
            &format!("/api/addresses/{}", a["id"]),
            Some(&alice),
            Some(json!({"active":false,"destination":"alice@example.com"}))
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        f.app.db.resource(a["id"].as_i64().unwrap()).unwrap().state,
        "review"
    );
    assert_eq!(f.mock.0.lock().unwrap().writes.len(), 1);
}
#[tokio::test]
async fn revoked_or_deleted_primary_invalidates_sessions() {
    let f = Fixture::new().await;
    let alice = f.session("alice@example.com");
    f.mock
        .0
        .lock()
        .unwrap()
        .mailboxes
        .remove("alice@example.com");
    assert_eq!(
        f.call("GET", "/api/me", Some(&alice), None).await.0,
        StatusCode::UNAUTHORIZED
    );
    assert!(f.app.db.session(&hash(&alice.0)).unwrap().is_none());
}
#[tokio::test]
async fn duplicate_concurrent_creation_is_serialized() {
    let f = Fixture::new().await;
    let alice = f.session("alice@example.com");
    let data = json!({"kind":"alias","local":"same","domain":"example.com"});
    let (a, b) = tokio::join!(
        f.call("POST", "/api/addresses", Some(&alice), Some(data.clone())),
        f.call("POST", "/api/addresses", Some(&alice), Some(data))
    );
    let mut codes = vec![a.0.as_u16(), b.0.as_u16()];
    codes.sort();
    assert_eq!(codes, vec![200, 409]);
    assert_eq!(f.app.db.resources(None).unwrap().len(), 1);
}
#[test]
fn api_result_parser_rejects_missing_or_embedded_failures() {
    use bucket::mailcow::parse_response;
    assert!(
        parse_response(
            json!([{"type":"success"},{"type":"danger","msg":"error"}]),
            true
        )
        .is_err()
    );
    assert!(parse_response(json!({}), true).is_err());
    assert!(parse_response(json!([{"type":"success","msg":"saved"}]), true).is_ok());
}
#[test]
fn sqlite_migration_and_rate_limits_survive_reopening() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let db = bucket::db::Db::open(path.to_str().unwrap()).unwrap();
    assert!(db.limit("key", 1, 900).unwrap());
    db.save_config(&Configuration {
        domains: vec!["example.com".into()],
        policy: Policy::default(),
    })
    .unwrap();
    drop(db);
    let db = bucket::db::Db::open(path.to_str().unwrap()).unwrap();
    assert!(!db.limit("key", 1, 900).unwrap());
    assert_eq!(db.config().unwrap().domains, vec!["example.com"]);
}
#[test]
fn validates_inputs_and_passwords() {
    assert!(email("bad\r\n@example.com").is_err());
    assert!(email("someone@example.com").is_ok());
    assert!(password("short").is_err());
    assert!(!local("catch*all"));
    assert!(!local("a..b"));
    let mut p = Policy::default();
    p.default_storage = p.max_storage + 1;
    assert!(p.validate().is_err());
}

#[tokio::test]
async fn replacement_login_mailbox_requires_explicit_reapproval() {
    let f = Fixture::new().await;
    let input = json!({"email":"alice@example.com","password":"test-only-password"});
    assert_eq!(
        f.call("POST", "/api/login", None, Some(input.clone()))
            .await
            .0,
        StatusCode::OK
    );
    f.mock
        .0
        .lock()
        .unwrap()
        .mailboxes
        .get_mut("alice@example.com")
        .unwrap()["created"] = json!("2026-11-22 18:00:00");
    assert_eq!(
        f.call("POST", "/api/login", None, Some(input.clone()))
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
    let admin = f.session("admin@example.com");
    assert_eq!(
        f.call(
            "PUT",
            "/api/admin/users",
            Some(&admin),
            Some(json!({"email":"alice@example.com","enabled":true,"policy":null}))
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        f.call("POST", "/api/login", None, Some(input)).await.0,
        StatusCode::OK
    );
}

#[tokio::test]
async fn sign_in_explains_account_access_and_mailbox_rejections_without_secrets() {
    let f = Fixture::new().await;
    let input = |email: &str| json!({"email":email,"password":"must-not-be-logged-123!"});
    let (status, value, _) = f
        .call(
            "POST",
            "/api/login",
            None,
            Some(input("unknown@example.com")),
        )
        .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert!(
        value["error"]
            .as_str()
            .unwrap()
            .contains("does not have access to Bucket")
    );
    f.mock
        .0
        .lock()
        .unwrap()
        .mailboxes
        .get_mut("alice@example.com")
        .unwrap()["active"] = json!(0);
    let (_, value, _) = f
        .call("POST", "/api/login", None, Some(input("alice@example.com")))
        .await;
    assert!(value["error"].as_str().unwrap().contains("turned off"));
    f.mock
        .0
        .lock()
        .unwrap()
        .mailboxes
        .get_mut("alice@example.com")
        .unwrap()["active"] = json!(1);
    f.mock
        .0
        .lock()
        .unwrap()
        .mailboxes
        .get_mut("alice@example.com")
        .unwrap()["authsource"] = json!("ldap");
    let (_, value, _) = f
        .call("POST", "/api/login", None, Some(input("alice@example.com")))
        .await;
    assert!(
        value["error"]
            .as_str()
            .unwrap()
            .contains("external sign-in provider")
    );
    f.mock
        .0
        .lock()
        .unwrap()
        .mailboxes
        .remove("alice@example.com");
    let (_, value, _) = f
        .call("POST", "/api/login", None, Some(input("alice@example.com")))
        .await;
    assert!(value["error"].as_str().unwrap().contains("not a mailbox"));
    assert_eq!(f.auth.calls.load(Ordering::SeqCst), 0);
    let audit = serde_json::to_string(&f.app.db.audits().unwrap()).unwrap();
    assert!(audit.contains("login.access_denied"));
    assert!(audit.contains("login.mailbox_disabled"));
    assert!(!audit.contains("must-not-be-logged"));
}

#[tokio::test]
async fn sign_in_explains_imap_rejection_and_api_unavailability() {
    let mut f = Fixture::new().await;
    f.app.auth = Arc::new(FakeAuth {
        okay: false,
        calls: AtomicUsize::new(0),
    });
    let input = json!({"email":"alice@example.com","password":"test-only-password"});
    let (status, value, _) = f
        .call("POST", "/api/login", None, Some(input.clone()))
        .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert!(value["error"].as_str().unwrap().contains("did not accept"));
    f.server.abort();
    let (status, value, _) = f.call("POST", "/api/login", None, Some(input)).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert!(value["error"].as_str().unwrap().contains("API connection"));
}

#[tokio::test]
async fn alias_suffix_preferences_are_authenticated_and_user_scoped() {
    let f = Fixture::new().await;
    let alice = f.session("alice@example.com");
    let bob = f.session("bob@example.com");
    assert_eq!(
        f.call(
            "PUT",
            "/api/preferences",
            None,
            Some(json!({"alias_suffix":false}))
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    let mut bad = alice.clone();
    bad.1 = "wrong".into();
    assert_eq!(
        f.call(
            "PUT",
            "/api/preferences",
            Some(&bad),
            Some(json!({"alias_suffix":false}))
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        f.call(
            "PUT",
            "/api/preferences",
            Some(&alice),
            Some(json!({"alias_suffix":false,"email":"bob@example.com"}))
        )
        .await
        .0,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(
        f.call(
            "PUT",
            "/api/preferences",
            Some(&alice),
            Some(json!({"alias_suffix":false}))
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        f.call("GET", "/api/me", Some(&alice), None).await.1["alias_suffix"],
        false
    );
    assert_eq!(
        f.call("GET", "/api/me", Some(&bob), None).await.1["alias_suffix"],
        true
    );
}
#[test]
fn upgrades_existing_database_and_retains_alias_preferences() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("migration.db");
    let path = path.to_str().unwrap();
    let db = bucket::db::Db::open(path).unwrap();
    db.save_user(&User {
        email: "alice@example.com".into(),
        enabled: true,
        policy: None,
    })
    .unwrap();
    db.with(|c| {
        c.execute_batch("DROP TABLE preferences; PRAGMA user_version=1;")?;
        Ok(())
    })
    .unwrap();
    drop(db);
    let db = bucket::db::Db::open(path).unwrap();
    assert!(db.user("alice@example.com").unwrap().is_some());
    assert!(db.alias_suffix("alice@example.com").unwrap());
    db.save_alias_suffix("alice@example.com", false).unwrap();
    drop(db);
    let db = bucket::db::Db::open(path).unwrap();
    assert!(!db.alias_suffix("alice@example.com").unwrap());
}
