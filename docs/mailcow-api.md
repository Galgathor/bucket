# Mailcow API contract

Reviewed against Mailcow’s official current OpenAPI specification on 2026-11-22:

- [Official OpenAPI specification](https://github.com/mailcow/mailcow-dockerized/blob/master/data/web/api/openapi.yaml)
- [API setup and IP restrictions](https://docs.mailcow.email/manual-guides/mailcow-UI/u_e-mailcow_ui-api/)
- [IMAP client connection settings](https://docs.mailcow.email/client/client-manual/)
- [Gitea OCI registry convention](https://docs.gitea.com/usage/packages/container/)

Bucket requires the mailbox tag and alias private-comment fields documented by the current specification. No Mailcow internals or password tables are accessed.

| Operation | Method and route | Body / notes |
| --- | --- | --- |
| Domains | `GET /api/v1/get/domain/all` | `domain_name`, `active`/`active_int` |
| Read mailbox | `GET /api/v1/get/mailbox/{address}` | `username`, `active`/`active_int`, `quota`, `quota_used`, `created`, `tags`, `authsource` |
| Read alias | `GET /api/v1/get/alias/{address-or-id}` | `address`, `id`, `goto`, `private_comment`, `created`, `active`/`active_int` |
| All regular aliases | `GET /api/v1/get/alias/all` | Used to block deletion of a mailbox with incoming regular forwards |
| Create mailbox | `POST /api/v1/add/mailbox` | `local_part`, `domain`, `name`, `password`, `password2`, `quota` (MiB), `active`, `tags`, `authsource=mailcow` |
| Create alias | `POST /api/v1/add/alias` | `address`, `goto`, `active`, `sogo_visible`, `sender_allowed`, `private_comment` |
| Edit mailbox | `POST /api/v1/edit/mailbox` | `{items: [address], attr: {...}}`; only name, active, optional quota and password/password2 |
| Edit alias | `POST /api/v1/edit/alias` | `{items: [id], attr: {active, goto}}`; labels stay in Bucket |
| Delete mailbox | `POST /api/v1/delete/mailbox` | `[address]` |
| Delete alias | `POST /api/v1/delete/alias` | `[id]` |

All identifiers are encoded as a single URL segment. The `X-API-Key` header is backend-only. Mailcow follows HTTP 200 with write-message arrays or objects; Bucket requires at least one `success`, rejects any `danger`, `error`, or `warning`, and rejects malformed or empty acknowledgments. It never treats transport success alone as operation success.

The read-back after a successful write checks the address and ownership marker, plus activity and quota/destination where applicable. Mailbox password changes cannot be read back; a positive API acknowledgment confirms them. Mailcow may enforce a stricter password policy than Bucket’s minimum length. Mailbox quotas are sent in MiB and read in bytes. Unlimited quota is never offered by Bucket.

An address is reserved in SQLite before a write. Unknown outcomes remain pending, including HTTP 200 failures, because Mailcow can partially apply a write. The administrator explicitly reconciles reservations; ordinary users cannot adopt remote addresses. Ownership uses a random marker plus the remote creation timestamp; alias edits/deletes also target the saved remote ID. An operator with full Mailcow administrator access can copy these markers, so administrators remain within the trust boundary.

Native time-limited aliases use separate `time_limited_aliases` routes and server-generated addresses; they do not have the same ownership marker and destination-edit model as regular aliases. This release deliberately manages regular aliases only. Native time-limited aliases, external forwarding mechanisms, and Mailcow administrator changes must be reviewed directly in Mailcow before deleting a mailbox; Bucket’s deletion guard inspects regular aliases.

## Verification scope

The Rust integration suite runs the real HTTP router and Mailcow client against a local contract fixture; only mailbox credential checking is injected in those tests. Separate tests exercise the actual IMAP client over TLS, including rejected credentials and an untrusted certificate. Production has no injection switch or mock mode.

Before using a production Mailcow server, the operator should verify sign-in with the intended account/password policy, API source-IP restrictions, allowed domain discovery, creation/edit/deletion of disposable addresses, storage reporting, and restart persistence. Automated fixtures verify the documented contract, not the behavior of every historical Mailcow version or an unprovided live installation.

## Dependency notes

The IMAP client is pinned to `imap` 3.0.0-alpha.15 to use the newer protocol parser and avoid the old parser’s unsound `lexical-core` dependency. Its TLS login behavior is covered by real-socket tests. The transitive `chrono` dependency currently has an upstream unmaintained advisory (RUSTSEC-2026-0543) with no fixed release; it is not used for Bucket sessions or authorization. The current Cargo audit reports no vulnerability advisories, with this maintenance warning remaining. Recheck advisories during updates.

## Inbox transport

The reader uses standard TLS IMAP `LOGIN`, `LIST`, `SELECT`, `UID SEARCH`, `UID FETCH` with `BODY.PEEK` and partial size limits, `UID STORE` for the Seen flag, `UID MOVE` for reversible moves, and `APPEND` for Sent copies. Standard authenticated SMTP submission uses required STARTTLS (587 by default) or implicit TLS (465), with verified certificates. Alias sending requires Mailcow’s `sender_allowed`/`sender_allowed_int` permission; sender authorization is checked on the backend and by the SMTP server. No admin API credential is used to send mail. REST API calls still authorize every selected primary/owned mailbox or alias destination before an IMAP request. A UID is not accepted from another address view without checking recipient headers and UID validity. Message and attachment access never use the Mailcow administrator API key as an IMAP credential. Connections are session-bound; no mailbox password persistence or Mailcow master-user access is introduced.
