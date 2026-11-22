import { test, expect, type Page } from "@playwright/test";
const policy = {
  mailboxes: true,
  aliases: true,
  max_mailboxes: 3,
  max_aliases: 50,
  max_storage: 5120,
  default_storage: 1024,
  total_storage: 10240,
};
async function setup(
  page: Page,
  {
    admin = false,
    signedIn = true,
  }: { admin?: boolean; signedIn?: boolean } = {},
) {
  let authenticated = signedIn;
  let config = { domains: ["example.com"], policy };
  let addresses = [
    {
      id: 1,
      address: "work@example.com",
      kind: "mailbox",
      name: "Work",
      state: "managed",
      active: 1,
      destination: "",
      storage: 1073741824,
      used: 139460608,
    },
    {
      id: 2,
      address: "shopping@example.com",
      kind: "alias",
      name: "Shopping",
      state: "managed",
      active: 1,
      destination: "alex@example.com",
      storage: 0,
      used: 0,
    },
  ];
  const requests: { path: string; body: Record<string, unknown> }[] = [];
  await page.route("**/api/**", async (route) => {
    const req = route.request();
    const path = new URL(req.url()).pathname;
    const method = req.method();
    const body = method === "GET" ? {} : (req.postDataJSON() ?? {});
    requests.push({ path, body });
    const respond = (data: unknown, status = 200) =>
      route.fulfill({ status, json: data });
    if (path === "/api/login") {
      authenticated = true;
      return respond({ csrf: "test-csrf" });
    }
    if (path === "/api/logout") {
      authenticated = false;
      return respond({ ok: true });
    }
    if (path === "/api/me")
      return authenticated
        ? respond({
            email: "alex@example.com",
            admin,
            csrf: "test-csrf",
            configured: true,
            domains: config.domains,
            policy: config.policy,
            destinations: ["alex@example.com", "work@example.com"],
          })
        : respond({ error: "Sign in to continue." }, 401);
    if (path === "/api/addresses" && method === "GET")
      return respond({ addresses });
    if (path === "/api/addresses" && method === "POST") {
      const a = {
        id: 3,
        address: body.local + "@" + body.domain,
        kind: body.kind,
        name: body.name,
        state: "managed",
        active: 1,
        destination: body.destination ?? "",
        storage: (body.storage ?? 0) * 1048576,
        used: 0,
      };
      addresses.unshift(a);
      return respond(a);
    }
    if (path.startsWith("/api/addresses/") && method === "PUT") {
      addresses = addresses.map((a) =>
        a.id === Number(path.split("/").at(-1))
          ? {
              ...a,
              ...body,
              active: body.active ? 1 : 0,
              storage: body.storage ? body.storage * 1048576 : a.storage,
            }
          : a,
      );
      return respond({ ok: true });
    }
    if (path.startsWith("/api/addresses/") && method === "DELETE") {
      addresses = addresses.filter(
        (a) => a.id !== Number(path.split("/").at(-1)),
      );
      return respond({ ok: true });
    }
    if (path === "/api/admin/config") {
      if (method === "PUT") config = body;
      return respond({
        configuration: config,
        available_domains: ["example.com", "example.org"],
      });
    }
    if (path === "/api/admin/connection") return respond({ ok: true });
    if (path === "/api/admin/users")
      return respond({
        users: [
          { email: "alex@example.com", enabled: true, policy: null },
          { email: "jane@example.com", enabled: true, policy: null },
        ],
        administrator: "alex@example.com",
      });
    if (path === "/api/admin/ownership")
      return respond({
        resources: [
          {
            id: 2,
            address: "shopping@example.com",
            kind: "alias",
            owner: "alex@example.com",
            state: "review",
            exists: true,
            verified: true,
            operation: "create",
          },
        ],
      });
    if (path === "/api/admin/audit")
      return respond({
        events: [
          {
            at: 1795380000,
            actor: "alex@example.com",
            action: "create",
            address: "shopping@example.com",
            outcome: "success",
          },
        ],
      });
    return respond({ ok: true });
  });
  await page.goto("/");
  return requests;
}
test("sign in, sign out and theme choice", async ({ page }) => {
  await setup(page, { signedIn: false });
  await expect(
    page.getByRole("heading", { name: "Sign in to Bucket" }),
  ).toBeVisible();
  await page
    .getByLabel("Email address", { exact: true })
    .fill("alex@example.com");
  await page.getByLabel("Password", { exact: true }).fill("Test-password123!");
  await page.getByRole("button", { name: "Sign in", exact: true }).click();
  await expect(
    page.getByRole("heading", { name: "Email addresses" }),
  ).toBeVisible();
  await page.getByRole("button", { name: "Choose theme" }).click();
  await page.getByRole("menuitem", { name: "Dark", exact: true }).click();
  await expect(page.locator("html")).toHaveAttribute("data-theme", "dark");
  await page.getByRole("button", { name: "Account menu" }).click();
  await page.getByRole("menuitem", { name: "Sign out" }).click();
  await expect(
    page.getByRole("heading", { name: "Sign in to Bucket" }),
  ).toBeVisible();
});
test("create an alias in a short form", async ({ page }) => {
  const requests = await setup(page);
  await page
    .getByRole("button", { name: "Create address", exact: true })
    .click();
  await expect(page.getByRole("dialog")).toBeVisible();
  await page.getByLabel("Email address", { exact: true }).fill("receipts");
  await page.getByLabel("Name Optional").fill("Receipts");
  await page
    .getByRole("dialog")
    .getByRole("button", { name: "Create address", exact: true })
    .click();
  await expect(
    page.getByText("receipts@example.com", { exact: true }),
  ).toBeVisible();
  expect(
    requests.find(
      (r) => r.path === "/api/addresses" && r.body.local === "receipts",
    )?.body.destination,
  ).toBe("alex@example.com");
  await expect(page.getByRole("dialog")).not.toBeVisible();
});
test("generate a mailbox password and show storage", async ({ page }) => {
  const requests = await setup(page);
  await page
    .getByRole("button", { name: "Create address", exact: true })
    .click();
  await page
    .getByRole("button", { name: "Mailbox Its own inbox and password" })
    .click();
  await page.getByLabel("Email address", { exact: true }).fill("personal");
  await page.getByRole("button", { name: "Generate password" }).click();
  await expect(page.getByLabel("Password", { exact: true })).toHaveValue(
    /.{24}/,
  );
  await page
    .getByRole("dialog")
    .getByRole("button", { name: "Create address", exact: true })
    .click();
  await expect(
    page.getByText("personal@example.com", { exact: true }),
  ).toBeVisible();
  expect(requests.find((r) => r.body.local === "personal")?.body.storage).toBe(
    1024,
  );
});
test("search, edit, and delete with explicit confirmation", async ({
  page,
}) => {
  await setup(page);
  await page
    .getByRole("textbox", { name: "Search addresses" })
    .fill("shopping");
  await expect(
    page.getByText("work@example.com", { exact: true }),
  ).not.toBeVisible();
  await page
    .getByRole("button", { name: "Manage shopping@example.com" })
    .click();
  await page.getByRole("menuitem", { name: "Edit address" }).click();
  await page.getByLabel("Name Optional").fill("Online shops");
  await page.getByRole("switch", { name: "Address is on" }).uncheck();
  await page.getByRole("button", { name: "Save changes" }).click();
  await expect(page.getByText("Online shops")).toBeVisible();
  await page
    .getByRole("button", { name: "Manage shopping@example.com" })
    .click();
  await page.getByRole("menuitem", { name: "Delete address" }).click();
  await expect(
    page.getByRole("button", { name: "Delete address", exact: true }),
  ).toBeDisabled();
  await page
    .getByLabel("Type the email address to confirm")
    .fill("shopping@example.com");
  await page
    .getByRole("button", { name: "Delete address", exact: true })
    .click();
  await expect(
    page.getByRole("heading", { name: "No matching addresses" }),
  ).toBeVisible();
});
test("administrator can configure domains, access and ownership", async ({
  page,
}) => {
  const requests = await setup(page, { admin: true });
  await page.getByRole("button", { name: "Settings", exact: true }).click();
  await page.getByRole("button", { name: "Test connection" }).click();
  await expect(
    page.getByText("Connected. API reads and sign-in are working."),
  ).toBeVisible();
  await page.getByLabel("example.org", { exact: true }).check();
  await page.getByRole("button", { name: "Save settings" }).click();
  await expect
    .poll(() =>
      requests.some(
        (r) =>
          r.path === "/api/admin/config" &&
          Array.isArray(r.body.domains) &&
          r.body.domains.includes("example.org"),
      ),
    )
    .toBeTruthy();
  await page.getByRole("button", { name: "People", exact: true }).click();
  await page.getByRole("button", { name: "Add person" }).click();
  await page
    .getByLabel("Email address", { exact: true })
    .fill("new@example.com");
  await page.getByLabel("Set individual limits").check();
  await page.getByLabel("Aliases per person").fill("10");
  await page.getByRole("button", { name: "Save access" }).click();
  await expect
    .poll(() =>
      requests.some(
        (r) =>
          r.path === "/api/admin/users" && r.body.email === "new@example.com",
      ),
    )
    .toBeTruthy();
  await page.getByRole("button", { name: "Ownership", exact: true }).click();
  await page.getByRole("button", { name: "Review", exact: true }).click();
  await page
    .getByLabel("Type the email address to confirm")
    .fill("shopping@example.com");
  await page.getByRole("button", { name: "Confirm change" }).click();
  await expect
    .poll(() =>
      requests.some(
        (r) =>
          r.path === "/api/admin/ownership/2" && r.body.action === "verify",
      ),
    )
    .toBeTruthy();
  await page.getByRole("button", { name: "Activity", exact: true }).click();
  await expect(page.getByText("Created address")).toBeVisible();
});
test("network failure is readable and no raw response is displayed", async ({
  page,
}) => {
  await setup(page);
  await page.route("**/api/addresses", (route) =>
    route.fulfill({
      status: 502,
      json: { error: "Mail service is unavailable. Try again shortly." },
    }),
  );
  await page.getByRole("button", { name: "Refresh addresses" }).click();
  await expect(page.getByRole("alert")).toHaveText(
    "Mail service is unavailable. Try again shortly.",
  );
});
test("layout fits the viewport and dialogs support keyboard dismissal", async ({
  page,
}, info) => {
  await setup(page);
  await expect(
    page.getByRole("heading", { name: "Email addresses" }),
  ).toBeVisible();
  expect(
    await page.evaluate(
      () => document.documentElement.scrollWidth <= window.innerWidth,
    ),
  ).toBeTruthy();
  await page.screenshot({
    path: info.outputPath("addresses.png"),
    fullPage: true,
  });
  await page
    .getByRole("button", { name: "Create address", exact: true })
    .click();
  await expect(page.getByLabel("Email address", { exact: true })).toBeFocused();
  await page.keyboard.press("Escape");
  await expect(page.getByRole("dialog")).not.toBeVisible();
});

test("accessible dashboard, forms, and dark theme", async ({ page }) => {
  const { default: AxeBuilder } = await import("@axe-core/playwright");
  await setup(page, { admin: true });
  await expect(
    page.getByRole("heading", { name: "Email addresses" }),
  ).toBeVisible();
  const check = async () => {
    const results = await new AxeBuilder({ page })
      .withTags(["wcag2a", "wcag2aa", "wcag21aa"])
      .analyze();
    expect(
      results.violations.map((v) => ({
        id: v.id,
        nodes: v.nodes.map((n) => ({
          target: n.target,
          summary: n.failureSummary,
        })),
      })),
    ).toEqual([]);
  };
  await check();
  await page.getByRole("button", { name: "Choose theme" }).click();
  await page.getByRole("menuitem", { name: "Dark", exact: true }).click();
  await check();
  await page
    .getByRole("button", { name: "Create address", exact: true })
    .click();
  await check();
  await page.keyboard.press("Escape");
  await page.getByRole("button", { name: "Settings", exact: true }).click();
  await expect(
    page.getByRole("heading", { name: "Default limits" }),
  ).toBeVisible();
  await check();
});
