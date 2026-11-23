# Repository Guidelines

## Project Structure & Module Organization

Bucket runs a Rust/Axum backend and React/TypeScript frontend in one container, with SQLite persistence.

- `backend/src/`: HTTP routes, authentication/Mailcow integration, ownership services, inbox reader, models, and database migrations.
- `backend/tests/`: integration tests and TLS fixtures; unit tests also live beside Rust modules.
- `frontend/src/`: components, API client, styles, and Vitest tests.
- `frontend/e2e/`: Playwright desktop/mobile and accessibility tests.
- `frontend/public/`: icons and PWA manifest; `frontend/scripts/` generates icons and the service worker.
- `scripts/`: registry naming tests and container smoke checks.
- `docs/mailcow-api.md`: upstream API contract and integration constraints.
- `.gitea/workflows/build.yaml`: CI checks and container publication.

## Build, Test, and Development Commands

Use stable Rust, Node 22, OpenSSL development headers, and `pkg-config`. From the repository root:

```sh
cargo run --manifest-path backend/Cargo.toml
cargo test --manifest-path backend/Cargo.toml --locked
cargo fmt --manifest-path backend/Cargo.toml --check
cargo clippy --manifest-path backend/Cargo.toml --locked --all-targets -- -D warnings
node --test scripts/image-names.test.mjs
```

Export development variables as documented in `README.md`; the backend does not automatically load `.env`.

From `frontend/`, run `npm ci`, then:

- `npm run dev -- --host localhost`: start Vite with the backend proxy.
- `npm test`: run Vitest.
- `npm run build`: type-check, bundle, and generate the service worker.
- `npx playwright install --with-deps chromium`, then `npm run test:e2e`: run browser checks.

Validate deployment with `docker build -t bucket:check .` and `bash scripts/container-smoke.sh bucket:check`.

## Coding Style & Naming Conventions

Use rustfmt defaults, four-space Rust indentation, and snake_case functions/modules. Follow existing Prettier-style TypeScript: two-space indentation, PascalCase component filenames, and camelCase functions. TypeScript is strict; avoid unused declarations. Run Clippy with warnings denied. Keep backend authorization separate from presentation.

## Testing Guidelines

Add behavioral regression tests for authentication, ownership, permissions, provisioning, and inbox changes. Use descriptive snake_case Rust test names, `*.test.ts` for Vitest, and `*.spec.ts` for Playwright. No coverage percentage is configured. Fixtures do not replace verification against a live Mailcow deployment.

## Commit & Pull Request Guidelines

Use concise imperative subjects, following history: “Add…”, “Fix…”, or “Upgrade…”. PRs should explain behavior, relevant validation, configuration/migration implications, and known limits. Link related issues when applicable; include desktop/mobile screenshots for visible changes.

## Security & Configuration

Never commit real credentials, persist mailbox passwords, disable TLS validation, or expose API keys/message content in logs. Enforce ownership and permissions server-side. Preserve CSRF/session protections, transactional migrations, and PWA exclusions for API responses. Update `README.md` and `.env.example` when configuration changes.
