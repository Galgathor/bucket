# README screenshots

These images show the current Bucket interface with the fictional accounts and messages from `frontend/e2e/app.spec.ts`. Do not replace them with screenshots containing real mailbox data or credentials.

Regenerate the relevant browser artifacts from `frontend/`:

```sh
npm ci
npx playwright install --with-deps chromium
npm run test:e2e -- --grep 'layout fits|read messages and switch|create an alias through'
```

Copy the desktop `addresses.png`, `alias-review.png`, and `inbox.png` artifacts from `frontend/test-results/` into this directory. Inspect each image before committing; the public README embeds them directly. Mobile artifacts are available from the same run for layout review.
