# Bucket operations guide

Detailed setup, maintenance, development, and troubleshooting notes. Run commands from the repository root unless noted otherwise. For a project overview, see the [README](../README.md).

## Deploy with Docker Compose

1. In Mailcow, create a **read-write administrator API key** under **System → Configuration → Access → Administrators → API**. Restrict it to the public outbound IP of the Bucket server. Bucket and Mailcow do not need a shared Docker network.
2. Copy `.env.example` to `.env` and set:
   - `BUCKET_PUBLIC_URL`: Bucket’s external HTTPS origin.
   - `BUCKET_MAILCOW_URL` and `BUCKET_MAILCOW_API_KEY`.
   - `BUCKET_IMAP_HOST`: Mailcow’s certificate-valid IMAP hostname (implicit TLS, port 993 by default).
   - `BUCKET_ADMIN_EMAIL`: an existing, active Mailcow mailbox using local authentication. This identity is the **only administrator**. It still authenticates through IMAP; there is no separate administrator password.
3. Protect the environment file and start:

   ```sh
   chmod 600 .env
   docker login registry.example.com
   docker compose pull
   docker compose up -d
   ```

4. Configure an HTTPS reverse proxy for `BUCKET_PUBLIC_URL`, forwarding to `http://127.0.0.1:8080`. Preserve the `Host` header. Do not cache `/api/` responses. No WebSocket configuration is needed.
5. Sign in as the configured administrator. The first sign-in opens **Settings**. Test the connection, select allowed domains, and save the defaults. Under **People**, add the existing mailbox accounts allowed to sign in. Choose individual limits only when needed.

All ordinary users must be explicitly allowed. Creating a mailbox does not automatically give it access to Bucket. The primary sign-in mailbox is shown as a destination, but Bucket does not take ownership of it or import existing Mailcow addresses.

For a local build instead of the registry image:

```sh
docker compose -f compose.yaml -f compose.build.yaml up -d --build
```

### Nginx Proxy Manager in Docker

A proxy running in another container cannot reach the host’s `127.0.0.1` through its own loopback. Join Bucket and the proxy to a dedicated external proxy network using a Compose override:

```yaml
services:
  bucket:
    networks: [proxy]
networks:
  proxy:
    external: true
```

Forward the proxy host to `bucket:8080`, scheme `http`, and enable SSL with forced HTTPS. The default host mapping remains loopback-only. Alternatively, bind `BUCKET_LISTEN_ADDRESS` to a private host address reachable only by the proxy and restrict it with your firewall. Do not expose port 8080 directly to the internet.

## Authentication and Mailcow requirements

Bucket uses **IMAP LOGIN over implicit TLS**, with hostname and certificate validation, then uses its backend API key for management. It never sends mailbox credentials to the administrator REST API and never stores mailbox passwords. Generated mailbox passwords are displayed before creation and cleared afterward; save or copy them before submitting.

- Native Mailcow mailbox accounts with working IMAP access are supported.
- An IMAP-enabled Mailcow app password can be used when accepted by your Mailcow configuration. Bucket does not implement browser SSO, OIDC, LDAP authentication, WebAuthn, TOTP challenges, or MFA enrollment; external `authsource` accounts are deliberately rejected. Test your specific app-password policy before allowing users.
- The initial administrator must be a mailbox account, not the Mailcow web administrator username.
- HTTPS API access and TLS IMAP access must be reachable from Bucket. TLS certificate validation cannot be disabled. With a private CA, extend the image to install your CA in the OS trust store **and** set `SSL_CERT_FILE` for IMAP and `BUCKET_CA_CERT` for API trust (see below).
- The administrator key needs read access to domains, mailboxes and aliases, and write access to mailbox and alias add/edit/delete operations. Mailcow’s own domain, quota, password and account limits still apply. API keys have broad administrator authority: keep the key private and retain IP restrictions.
- Bucket uses the current official API’s mailbox `tags` and alias `private_comment` fields as ownership markers. Do not remove or copy these markers in Mailcow. Older Mailcow releases without these fields are unsupported; update Mailcow before deploying.
- Regular aliases support custom and cryptographically random addresses, with destinations restricted to the user’s primary mailbox or verified, active mailboxes they created. Native Mailcow time-limited aliases are not managed in this release.

The connection test reads active domains and confirms the API is reachable. IMAP was verified by sign-in. It does **not** claim to test write permissions by creating or deleting a real address.

## Permissions and ownership

**Settings → General** controls domains, creation permissions, mailbox/alias counts, per-mailbox storage, default storage, and total storage. **People** controls access and optional individual limits. All checks run on the backend. Turning off access or removing a person revokes their sessions immediately; it does not delete their email accounts. The deployment administrator cannot be removed through the interface.

Mailcow remains authoritative for account existence, forwarding, storage usage, quotas and status. Bucket stores ownership, names, policy, opaque session hashes, authentication counters and audit events. Primary mailbox deletion, replacement or disabling invalidates sessions. After a successful first login, the allowed account is also bound to its Mailcow creation identity; a replacement account at the same email address needs explicit administrator reapproval under People. External changes to owned addresses are fetched from Mailcow; missing or mismatched ownership markers prevent writes.

Provisioning is serialized and journaled to SQLite **before** calling Mailcow. A timeout, rejected write, crash or mixed API result can leave an address marked **Needs review**. Bucket keeps its reservation, counts it against limits, and never automatically claims an address after an uncertain result. Under **Ownership**, an administrator can:

- Verify a pending address when its Mailcow identity and ownership marker match.
- Reassign a verified address to an explicitly enabled user, including recovery of an interrupted deletion that left the original account intact.
- Remove a stale Bucket record when the remote account is absent or its identity no longer matches. This does not change the remote account or let users claim it.

External marker removal requires investigation in Mailcow. Bucket cannot adopt an unrelated account. Reassignment is an administrator decision; existing allocations are not reduced automatically if the recipient’s limits are lower, but further creation/allocation must fit those limits.

Deleting a mailbox requires its full email address as confirmation and is blocked while regular aliases still forward to it. A mailbox enrolled for Bucket sign-in cannot have its password reset, be disabled, or be deleted by its creator until an administrator removes its Bucket access. The deployment administrator’s mailbox is always protected.

Important actions and interrupted operations appear in **Activity** and structured application logs. SQLite retains the audit history; the UI shows the most recent 200 events.

## Update and backup

```sh
docker compose pull
docker compose up -d
```

Persistent data is in the `bucket-data` named volume. The container runs as UID/GID 10001, with a read-only root filesystem and no Linux capabilities. Database migrations run in a transaction at startup. A process lock prevents two instances from sharing the database. Run **one replica** on a local disk; do not put SQLite on NFS.

Before upgrading, stop Bucket and back up the entire data volume (including SQLite WAL files, if present). Back up `.env` separately as a secret. Restore the volume with ownership `10001:10001`; the data directory should be mode `0700`. Restore a matching database backup when rolling back an incompatible schema upgrade. Never use `docker compose down -v` unless intentionally deleting Bucket’s records. Mailcow needs its own independent mailbox backups.

Changing `BUCKET_ADMIN_EMAIL` transfers administrative rights at the next request; it does not grant the old identity permanent admin rights. Removing access does not remove owned-address records; reassign them as needed.

Health is available at `/api/health`. It checks the application database without calling Mailcow, so an upstream outage does not restart a healthy Bucket process. Logs go to stdout/stderr; Compose rotates them. Shutdown drains HTTP requests on SIGTERM. Restrict access to logs and backups because they contain email addresses.

Authentication limits persist in SQLite (10 attempts per account / 15 minutes, 100 per connecting peer / 15 minutes). Bucket ignores untrusted forwarded IP headers; behind a proxy the peer limit is shared. Apply per-client sign-in limits at the reverse proxy if needed. Production cookies are Secure, HttpOnly, SameSite=Strict with a `__Host-` prefix, absolute expiry, and session-bound CSRF tokens. Mutations also require the exact configured Origin.

## Gitea Actions and container registries

`.gitea/workflows/build.yaml` runs Rust formatting, Clippy, security/operation integration tests, frontend tests/build, desktop/mobile browser tests and a production container build. A separate publication job depends on all checks passing. Pull requests never publish.

Configure the existing secrets exactly as named:

| Secret | Value |
| --- | --- |
| `CONTAINER_REGISTRY` | Registry hostname, optionally with a port; a root HTTP(S) URL is also accepted. Owner paths are rejected. |
| `PACKAGES_USERNAME` | Gitea registry login with permission to publish to the repository owner |
| `PACKAGES_TOKEN` | Gitea package write token |
| `GHCR_USERNAME` | GitHub account username; also used as the GHCR image namespace |
| `GHCR_TOKEN` | GitHub personal access token (classic) with `write:packages`; authorize organization SSO if required |

Images use Gitea’s `{registry}/{owner}/{image}` convention; owner and repository names are lowercased. For this repository: `${CONTAINER_REGISTRY}/galgathor/bucket`. Every successful push gets `sha-<full commit>`. The default branch also gets `latest`. Valid `v1.2.3` release tags get a matching image tag. Prefer immutable commit tags for controlled deployments.

The same build is also published to `ghcr.io/<lowercase-GHCR_USERNAME>/bucket`, with the same commit, default-branch `latest`, and release tags. Both registry logins must succeed before the shared Buildx publication step runs; it builds once and pushes both tag sets. There is no atomic transaction across registries, so a network failure can leave one published before the job fails. Registry build caching remains in Gitea.

GHCR publication runs from Gitea using the explicit `GHCR_USERNAME` and `GHCR_TOKEN` secrets, not Gitea’s job token. The GitHub account must be able to write the target package. New GHCR packages are private by default: change package visibility in GitHub if you want anonymous public pulls. See [GitHub’s Container registry documentation](https://docs.github.com/en/packages/working-with-a-github-packages-registry/working-with-the-container-registry). Compose uses `ghcr.io/galgathor/bucket:latest` directly. Edit the `image` field in `compose.yaml` to select another registry or pin a version; authenticate with `docker login ghcr.io` for private packages. These publishing credentials belong in Actions secrets, not Bucket’s runtime `.env`.


The `ubuntu-latest` label is a **Gitea runner label**, not a dependency on GitHub-hosted machines. Map it to a Linux runner with Git, Node 20+ for JavaScript actions, Bash, a Docker CLI and a working Docker daemon/socket. A common act_runner image is `catthehacker/ubuntu:act-22.04`. The runner must be allowed to install build/browser packages and reach GitHub action sources, crates.io, npm, Docker Hub and your registry. Buildx uses registry caching; no GitHub cache service is required. Enable Actions on the repository. Configure the runner label/network/socket on your Gitea installation if its defaults differ.

## Development

Requires current stable Rust, Node 22, `pkg-config`, and OpenSSL development headers. Production remains Docker Compose.

```sh
cargo test --manifest-path backend/Cargo.toml --locked
cargo clippy --manifest-path backend/Cargo.toml --locked --all-targets -- -D warnings
cd frontend
npm ci
npm test
npm run build
npx playwright install --with-deps chromium
npm run test:e2e
```

To run locally, export the essential `BUCKET_*` variables, use `BUCKET_PUBLIC_URL=http://localhost:5173`, `BUCKET_DATABASE=./local.db`, and `BUCKET_BIND=127.0.0.1:8080`. Start `cargo run --manifest-path backend/Cargo.toml` and `cd frontend && npm run dev -- --host localhost`. Vite proxies `/api` to the backend. HTTP is allowed only for a localhost public URL; API and IMAP connections always require verified TLS. Never expose the development server publicly.

Backend tests run against an HTTP Mailcow contract fixture, with separate real-TLS IMAP acceptance, rejection and untrusted-certificate tests. Browser tests use intercepted API fixtures. There is no production mock mode or authentication bypass. A live deployment still needs verification against your actual Mailcow version, credentials, certificates and API allowlist.

See [Mailcow API contract](mailcow-api.md) for upstream references and integration details.

## Troubleshooting sign-in

Use the **full mailbox email address**, not an alias or the Mailcow web administrator username. The initial administrator must match `BUCKET_ADMIN_EMAIL`; other accounts must be enabled under **Settings → People**. Correct mailbox credentials alone do not grant Bucket access.

Sign-in errors distinguish Bucket access, missing/disabled mailboxes, external identity providers, mailbox replacement, IMAP credential rejection, and API/IMAP connection failures. With two-step verification, use an IMAP-enabled app password if required by your Mailcow policy; Bucket cannot accept an interactive one-time code.

For deployment diagnostics, retry once and inspect `docker compose logs --since=5m bucket`. Failed account checks log a safe `reason` such as `access_denied`, `unsupported_identity`, or `identity_changed`; IMAP connection failures report `dns_failed`, `connection_failed`, `tls_failed`, `greeting_failed`, or `protocol_failed`. Passwords, API keys, and raw server responses are never included. Account rejection reasons also appear under **Settings → Activity**. Treat logs as private because they include email addresses.

## Inbox

Open **Inbox** and choose a mailbox or alias from the address switcher. Browse Inbox, Spam, Trash, and Sent; read messages and attachments; mark messages read or unread; compose plain-text messages and reply. Spam and Trash actions move messages and can be reversed with **Move to inbox**. There is no permanent deletion. Moves require server support for IMAP MOVE; Bucket never globally expunges a mailbox. Folder names are discovered through IMAP special-use flags, with common-name fallbacks.

Your primary mailbox opens during a new sign-in. Independent mailboxes require their own mailbox password or IMAP-enabled app password when first opened. Aliases share the connection to their authorized destination mailbox. Only your primary mailbox and addresses you own in Bucket are available; administrator status does not grant access to other users’ messages.

Bucket retains **authenticated IMAP and SMTP sockets in memory**, not mailbox passwords, app passwords, or message copies in SQLite. Logout closes that session’s connections; expired/revoked sessions are swept every minute. Restarting Bucket or an idle connection being closed by Mailcow means the mailbox must be opened again. Existing Bucket sessions created before this update must also open the mailbox once. The existing IMAP hostname/port settings are used. SMTP defaults to `BUCKET_IMAP_HOST`, port 587 with required STARTTLS. Override with `BUCKET_SMTP_HOST`, `BUCKET_SMTP_PORT`, and `BUCKET_SMTP_SECURITY` (`starttls` or `tls`; the latter defaults to port 465). TLS certificates are always verified. No master-user credentials are required.

Address views match exact parsed email addresses, prioritizing `X-Original-To`/`Envelope-To`, then `To`/`Cc`, then `Delivered-To` when no explicit recipients remain. Sent views match the sender; Trash matches the sender or recipient. Alias views are filters of the destination mailbox, not separate mailboxes or a delivery audit. Bcc, mailing lists, forwarding, and missing or rewritten headers can prevent a message from appearing under an address. Recipient headers can be forged; filtering is a convenience within an already authorized mailbox, not a security boundary between recipients. Messages to multiple addresses can appear in more than one view.


Lists and unread indicators refresh every 30 seconds while the page is visible, and when returning to it. Open messages and drafts stay open. Badges count unread messages in Inbox only. Locked mailboxes have unknown counts; opening their connection enables polling. Counts examine the newest 500 unread messages per mailbox, matching each alias exactly. A `+` indicates a lower-bound count; an unscanned address is not reported as having zero unread messages.

Sending opens an authenticated TLS SMTP connection during sign-in or mailbox unlock, using the password transiently. An SMTP failure does not prevent sign-in. Older sessions, closed SMTP connections, and app passwords restricted to IMAP may require the destination mailbox’s SMTP-enabled password when sending. Bucket never saves that password. Sending as an alias requires Mailcow’s `sender_allowed` permission and an authorized destination; Mailcow also enforces its own sender policy. Unavailable senders have Compose disabled.

Compose supports up to 20 recipients and 64 KiB of plain text (the UI limits drafts to 16,000 characters). Drafts exist only in the open dialog; leaving or reloading loses them. Cancelling a changed draft asks before discarding. Replies use a server-verified original message ID. Attachments and rich text are read-only features.

A successful SMTP submission is recorded before a Seen copy is appended to Sent. If that copy fails, Bucket reports that the message was sent without a saved copy. The database stores only a request ID, owner/sender, payload digest, message ID, delivery status, copy status, and timestamp for duplicate prevention—no subject, recipients, body, or password. Uncertain delivery is never automatically retried; check Sent or the mail server before manually sending another message. Submissions are limited to 20 attempts per user per five minutes.

HTML email is sanitized and displayed in an isolated sandbox. Scripts, remote images, forms, and link navigation are blocked. Attachments download as files rather than being rendered inline. Message contents and attachments are never cached by Bucket’s service worker. IMAP UIDs are checked against UID validity and recipient filters on message, attachment, read-state, reply, and move requests.

For a lightweight runtime, Bucket opens at most 8 mailboxes per session and 128 in total, with 4 concurrent inbox operations. Pages scan at most 2,000 UIDs/200 matching candidates and display up to 50 messages; **Load more** continues the scan. Large or complex emails must be opened in another client: message limit 5 MiB, formatted content limit 256 KiB, and at most 100 MIME content-type headers. These limits protect the application when processing untrusted incoming mail.
