use crate::{
    App,
    db::now,
    error::{Error, Result},
    hash,
    mailcow::{self, active},
    model::*,
    token,
};
use axum::{
    Json, Router,
    extract::{ConnectInfo, DefaultBodyLimit, Path, Request, State},
    http::{HeaderMap, HeaderValue, Method, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post, put},
};
use serde_json::{Value, json};
use std::net::SocketAddr;
use tower_http::services::{ServeDir, ServeFile};

pub fn router(app: App) -> Router {
    let api = Router::new()
        .route("/health", get(health))
        .route("/login", post(login))
        .route("/logout", post(logout))
        .route("/me", get(me))
        .route("/addresses", get(addresses).post(create))
        .route("/addresses/{id}", put(edit).delete(delete))
        .route("/admin/config", get(configuration).put(save_configuration))
        .route("/admin/connection", post(connection))
        .route("/admin/users", get(users).put(save_user))
        .route("/admin/users/{email}", axum::routing::delete(delete_user))
        .route("/admin/ownership", get(ownership))
        .route("/admin/ownership/{id}", post(resolve))
        .route("/admin/audit", get(audit))
        .fallback(|| async { Error::new(StatusCode::NOT_FOUND, "Page not found.") });
    let static_service = ServeDir::new(&app.config.frontend).not_found_service(ServeFile::new(
        format!("{}/index.html", app.config.frontend),
    ));
    Router::new()
        .nest("/api", api)
        .fallback_service(static_service)
        .layer(DefaultBodyLimit::max(16 * 1024))
        .layer(middleware::from_fn_with_state(app.clone(), security))
        .with_state(app)
}
async fn health(State(app): State<App>) -> Result<Json<Value>> {
    app.db.config()?;
    Ok(Json(json!({"status":"ok"})))
}
async fn security(State(app): State<App>, request: Request, next: Next) -> Response {
    let mutation = !matches!(
        *request.method(),
        Method::GET | Method::HEAD | Method::OPTIONS
    );
    let origin = request
        .headers()
        .get("origin")
        .and_then(|v| v.to_str().ok());
    let mut response = if mutation && origin != Some(&app.config.public_url) {
        Error::forbidden("Refresh this page and try again.").into_response()
    } else {
        next.run(request).await
    };
    if response.status().is_client_error()
        && !response
            .headers()
            .get("content-type")
            .is_some_and(|v| v.as_bytes().starts_with(b"application/json"))
    {
        response = Error::new(
            response.status(),
            "Check the information you entered and try again.",
        )
        .into_response();
    }
    let h = response.headers_mut();
    for (k, v) in [
        ("x-content-type-options", "nosniff"),
        ("x-frame-options", "DENY"),
        ("referrer-policy", "no-referrer"),
        (
            "permissions-policy",
            "camera=(), microphone=(), geolocation=()",
        ),
        (
            "content-security-policy",
            "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; font-src 'self'; connect-src 'self'; frame-ancestors 'none'; base-uri 'none'; form-action 'self'; worker-src 'self'; manifest-src 'self'",
        ),
        ("cache-control", "no-store"),
    ] {
        h.insert(
            axum::http::header::HeaderName::from_static(k),
            HeaderValue::from_static(v),
        );
    }
    if app.config.secure {
        h.insert(
            "strict-transport-security",
            HeaderValue::from_static("max-age=31536000"),
        );
    }
    response
}
fn session_cookie(app: &App, headers: &HeaderMap) -> Option<String> {
    headers
        .get("cookie")?
        .to_str()
        .ok()?
        .split(';')
        .filter_map(|c| c.trim().split_once('='))
        .find(|(k, _)| *k == app.config.cookie())
        .map(|(_, v)| v.to_owned())
}
fn cookie(app: &App, value: &str, age: i64) -> String {
    format!(
        "{}={}; Path=/; HttpOnly; SameSite=Strict; Max-Age={}{}",
        app.config.cookie(),
        value,
        age,
        if app.config.secure { "; Secure" } else { "" }
    )
}
fn unauthorized() -> Error {
    Error::new(StatusCode::UNAUTHORIZED, "Sign in to continue.")
}
async fn authenticated(app: &App, headers: &HeaderMap, mutation: bool) -> Result<(User, Session)> {
    let value = session_cookie(app, headers).ok_or_else(unauthorized)?;
    if value.len() != 64 {
        return Err(unauthorized());
    }
    let session = app.db.session(&hash(&value))?.ok_or_else(unauthorized)?;
    if mutation && headers.get("x-csrf-token").and_then(|v| v.to_str().ok()) != Some(&session.csrf)
    {
        return Err(Error::forbidden("Refresh this page and try again."));
    }
    let user = app
        .db
        .user(&session.email)?
        .filter(|u| u.enabled)
        .ok_or_else(unauthorized)?;
    let primary = app.mailcow.get(&Kind::Mailbox, &user.email).await?;
    if !primary.as_ref().is_some_and(|v| {
        active(v) == 1
            && v["username"] == user.email
            && mailcow::identity(v) == session.identity
            && !session.identity.is_empty()
            && v.get("authsource").is_none_or(|s| s == "mailcow")
    }) {
        app.db.delete_session(&session.token)?;
        return Err(unauthorized());
    }
    Ok((user, session))
}
async fn administrator(app: &App, headers: &HeaderMap, mutation: bool) -> Result<User> {
    let (user, _) = authenticated(app, headers, mutation).await?;
    if user.email != app.config.admin {
        return Err(Error::forbidden("Administrator access is required."));
    }
    Ok(user)
}
async fn login(
    State(app): State<App>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(input): Json<Login>,
) -> Result<Response> {
    // Forwarded headers are intentionally ignored. The peer limit is generous for a shared reverse proxy.
    let ip = app
        .db
        .limit(&format!("ip:{}", hash(&peer.ip().to_string())), 100, 900)?;
    if !ip {
        return Err(Error::new(
            StatusCode::TOO_MANY_REQUESTS,
            "Too many sign-in attempts. Try again in 15 minutes.",
        ));
    }
    let normalized = email(&input.email)?;
    let account = app
        .db
        .limit(&format!("email:{}", hash(&normalized)), 10, 900)?;
    if !account {
        return Err(Error::new(
            StatusCode::TOO_MANY_REQUESTS,
            "Too many sign-in attempts. Try again in 15 minutes.",
        ));
    }
    let denied = |reason: &str, message: &str| {
        tracing::warn!(account=%normalized,reason,"sign-in rejected");
        if app
            .db
            .audit(&normalized, &format!("login.{reason}"), "", "failed")
            .is_err()
        {
            tracing::error!("could not record failed sign-in");
        }
        Error::new(StatusCode::UNAUTHORIZED, message)
    };
    if input.password.is_empty() || input.password.len() > 1024 {
        return Err(denied(
            "credentials_rejected",
            "The mail server did not accept this email and password. Use your full mailbox email address and an IMAP-enabled password.",
        ));
    }
    let user = app
        .db
        .user(&normalized)?
        .filter(|u| u.enabled)
        .ok_or_else(||denied("access_denied","This account does not have access to Bucket. Ask an administrator to enable it under People."))?;
    let mailbox = app
        .mailcow
        .get(&Kind::Mailbox, &normalized)
        .await.map_err(|_| {
            tracing::warn!(account=%normalized,reason="api_unavailable","sign-in could not check mailbox");
            Error::new(StatusCode::SERVICE_UNAVAILABLE,"Bucket could not check your account with Mailcow. Ask the administrator to check the API connection, key permissions, and allowed IP addresses.")
        })?
        .ok_or_else(||denied("mailbox_missing","This email address is not a mailbox on the connected mail server. Use a full mailbox address, not an alias or the Mailcow web administrator username."))?;
    if mailbox["username"] != normalized {
        return Err(denied(
            "mailbox_mismatch",
            "The mail server returned a different mailbox. Ask the administrator to check the Mailcow connection.",
        ));
    }
    if active(&mailbox) != 1 {
        return Err(denied(
            "mailbox_disabled",
            "This mailbox is turned off or cannot sign in. Ask your mail administrator to enable it.",
        ));
    }
    if mailbox.get("authsource").is_some_and(|v| v != "mailcow") {
        return Err(denied(
            "unsupported_identity",
            "This mailbox uses an external sign-in provider. Bucket currently supports locally authenticated Mailcow mailboxes only.",
        ));
    }
    if app
        .db
        .user_identity(&normalized)?
        .is_some_and(|id| id != mailcow::identity(&mailbox))
    {
        return Err(denied(
            "identity_changed",
            "This mailbox has changed since access was approved. Ask an administrator to save its access again under People.",
        ));
    }
    if mailcow::identity(&mailbox).is_empty() {
        return Err(denied(
            "identity_missing",
            "The mail server did not return a mailbox creation identity. Ask your administrator to check that the Mailcow version is supported.",
        ));
    }
    if !app
        .auth
        .authenticate(normalized.clone(), input.password)
        .await?
    {
        return Err(denied(
            "credentials_rejected",
            "The mail server did not accept this email and password. Use your full mailbox email address and an IMAP-enabled password. If you use two-step verification, check whether your mail server requires an app password.",
        ));
    }
    let _guard = app.writes.lock().await;
    if !app.db.user(&normalized)?.is_some_and(|u| u.enabled) {
        return Err(denied(
            "access_changed",
            "Your access changed during sign-in. Ask an administrator to check your access under People.",
        ));
    }
    let current = app
        .mailcow
        .get(&Kind::Mailbox, &normalized)
        .await?
        .ok_or_else(|| {
            denied(
                "mailbox_missing",
                "This mailbox was removed during sign-in. Contact your mail administrator.",
            )
        })?;
    if active(&current) != 1
        || mailcow::identity(&current) != mailcow::identity(&mailbox)
        || app
            .db
            .user_identity(&normalized)?
            .is_some_and(|id| id != mailcow::identity(&current))
    {
        return Err(denied(
            "identity_changed",
            "This mailbox changed during sign-in. Ask an administrator to check its access under People.",
        ));
    }
    app.db
        .bind_identity(&normalized, &mailcow::identity(&mailbox))?;
    if let Some(old) = session_cookie(&app, &headers) {
        app.db.delete_session(&hash(&old))?;
    }
    let value = token();
    let session = Session {
        token: hash(&value),
        email: user.email.clone(),
        csrf: token(),
        expires: now() + app.config.session_hours * 3600,
        identity: mailcow::identity(&mailbox),
    };
    app.db.save_session(&session)?;
    app.db.audit(&user.email, "login", "", "success")?;
    let mut response =
        Json(json!({"email":user.email,"admin":user.email==app.config.admin,"csrf":session.csrf}))
            .into_response();
    response.headers_mut().insert(
        "set-cookie",
        cookie(&app, &value, app.config.session_hours * 3600)
            .parse()
            .map_err(|_| Error::internal())?,
    );
    Ok(response)
}
async fn logout(State(app): State<App>, headers: HeaderMap) -> Result<Response> {
    // Allow logout even if the user's mailbox has just been removed.
    if let Some(value) = session_cookie(&app, &headers)
        && let Some(session) = app.db.session(&hash(&value))?
    {
        if headers.get("x-csrf-token").and_then(|v| v.to_str().ok()) != Some(&session.csrf) {
            return Err(Error::forbidden("Refresh this page and try again."));
        }
        app.db.delete_session(&session.token)?;
    }
    let mut response = Json(json!({"ok":true})).into_response();
    response.headers_mut().insert(
        "set-cookie",
        cookie(&app, "", 0).parse().map_err(|_| Error::internal())?,
    );
    Ok(response)
}
async fn me(State(app): State<App>, headers: HeaderMap) -> Result<Json<Value>> {
    let (user, session) = authenticated(&app, &headers, false).await?;
    let config = app.db.config()?;
    Ok(Json(
        json!({"email":user.email,"admin":user.email==app.config.admin,"csrf":session.csrf,"configured":!config.domains.is_empty(),"domains":config.domains,"policy":app.policy(&user)?,"destinations":app.destinations(&user.email).await?}),
    ))
}
async fn addresses(State(app): State<App>, headers: HeaderMap) -> Result<Json<Value>> {
    let (user, _) = authenticated(&app, &headers, false).await?;
    let _guard = app.writes.lock().await;
    Ok(Json(json!({"addresses":app.list(&user.email).await?})))
}
async fn create(
    State(app): State<App>,
    headers: HeaderMap,
    Json(input): Json<Create>,
) -> Result<Json<Value>> {
    let (user, _) = authenticated(&app, &headers, true).await?;
    if !app
        .db
        .limit(&format!("create:{}", hash(&user.email)), 30, 300)?
    {
        return Err(Error::new(
            StatusCode::TOO_MANY_REQUESTS,
            "Please wait a few minutes before creating more addresses.",
        ));
    }
    Ok(Json(app.create(&user, input).await?))
}
async fn edit(
    State(app): State<App>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Json(input): Json<Edit>,
) -> Result<Json<Value>> {
    let (user, _) = authenticated(&app, &headers, true).await?;
    Ok(Json(app.edit(id, &user, input).await?))
}
async fn delete(
    State(app): State<App>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Json(input): Json<Delete>,
) -> Result<Json<Value>> {
    let (user, _) = authenticated(&app, &headers, true).await?;
    app.delete(id, &user, &input.confirmation).await?;
    Ok(Json(json!({"ok":true})))
}
async fn configuration(State(app): State<App>, headers: HeaderMap) -> Result<Json<Value>> {
    administrator(&app, &headers, false).await?;
    Ok(Json(
        json!({"configuration":app.db.config()?,"available_domains":app.mailcow.domains().await?}),
    ))
}
async fn save_configuration(
    State(app): State<App>,
    headers: HeaderMap,
    Json(mut input): Json<Configuration>,
) -> Result<Json<Value>> {
    let user = administrator(&app, &headers, true).await?;
    input.validate()?;
    let _guard = app.writes.lock().await;
    let available = app.mailcow.domains().await?;
    if input.domains.iter().any(|d| !available.contains(d)) {
        return Err(Error::bad("Choose active domains from your mail server."));
    }
    app.db.save_config(&input)?;
    app.db.audit(&user.email, "settings.save", "", "success")?;
    Ok(Json(json!({"ok":true})))
}
async fn connection(State(app): State<App>, headers: HeaderMap) -> Result<Json<Value>> {
    administrator(&app, &headers, true).await?;
    let domains = app.mailcow.domains().await?;
    // Sign-in already proves IMAP authentication; this tests API read access without modifying Mailcow.
    Ok(Json(
        json!({"ok":true,"domains":domains.len(),"imap":"verified at sign-in","write_access":"Requires a read-write Mailcow API key"}),
    ))
}
async fn users(State(app): State<App>, headers: HeaderMap) -> Result<Json<Value>> {
    administrator(&app, &headers, false).await?;
    Ok(Json(
        json!({"users":app.db.users()?,"administrator":app.config.admin}),
    ))
}
async fn save_user(
    State(app): State<App>,
    headers: HeaderMap,
    Json(mut input): Json<User>,
) -> Result<Json<Value>> {
    let actor = administrator(&app, &headers, true).await?;
    input.email = email(&input.email)?;
    if let Some(p) = &input.policy {
        p.validate()?;
    }
    if input.email == app.config.admin && !input.enabled {
        return Err(Error::bad(
            "The deployment administrator cannot be turned off here.",
        ));
    }
    let _guard = app.writes.lock().await;
    let mailbox = app
        .mailcow
        .get(&Kind::Mailbox, &input.email)
        .await?
        .ok_or_else(|| Error::bad("Choose an active Mailcow mailbox with local authentication."))?;
    if active(&mailbox) != 1
        || mailbox["username"] != input.email
        || mailbox.get("authsource").is_some_and(|s| s != "mailcow")
        || mailcow::identity(&mailbox).is_empty()
    {
        return Err(Error::bad(
            "Choose an active Mailcow mailbox with local authentication.",
        ));
    }
    app.db
        .save_user_identity(&input, Some(&mailcow::identity(&mailbox)))?;
    app.db
        .audit(&actor.email, "user.save", &input.email, "success")?;
    Ok(Json(json!({"ok":true})))
}
async fn delete_user(
    State(app): State<App>,
    headers: HeaderMap,
    Path(email): Path<String>,
    Json(input): Json<Delete>,
) -> Result<Json<Value>> {
    let actor = administrator(&app, &headers, true).await?;
    if email == app.config.admin {
        return Err(Error::bad(
            "The deployment administrator cannot be removed here.",
        ));
    }
    if input.confirmation != email {
        return Err(Error::bad("Type the full email address to confirm."));
    }
    let _guard = app.writes.lock().await;
    app.db.delete_user(&email)?;
    app.db
        .audit(&actor.email, "user.remove", &email, "success")?;
    Ok(Json(json!({"ok":true})))
}
async fn ownership(State(app): State<App>, headers: HeaderMap) -> Result<Json<Value>> {
    administrator(&app, &headers, false).await?;
    let _guard = app.writes.lock().await;
    let mut rows = vec![];
    for row in app.db.resources(None)? {
        let remote = app.remote(&row).await?;
        let matched = remote.as_ref().is_some_and(|v| mailcow::matches(&row, v));
        if row.state == "managed" && !matched {
            app.db.review(row.id)?;
        }
        rows.push(json!({"id":row.id,"address":row.address,"owner":row.owner,"kind":row.kind,"state":if row.state=="managed" && !matched{"review"}else{&row.state},"operation":row.operation,"exists":remote.is_some(),"verified":matched}));
    }
    Ok(Json(json!({"resources":rows})))
}
async fn resolve(
    State(app): State<App>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Json(input): Json<Resolve>,
) -> Result<Json<Value>> {
    let actor = administrator(&app, &headers, true).await?;
    app.resolve(id, &actor.email, input).await?;
    Ok(Json(json!({"ok":true})))
}
async fn audit(State(app): State<App>, headers: HeaderMap) -> Result<Json<Value>> {
    administrator(&app, &headers, false).await?;
    Ok(Json(json!({"events":app.db.audits()?})))
}
