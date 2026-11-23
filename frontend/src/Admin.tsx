import { useEffect, useState } from "react";
import {
  CheckCircle2,
  Plus,
  RefreshCw,
  ShieldCheck,
  UserRound,
  Pencil,
  Trash2,
} from "lucide-react";
import { api, type Policy, type User, type Ownership, type Audit } from "./api";
import {
  Busy,
  ErrorMessage,
  InfoButton,
  Modal,
  PolicyFields,
} from "./components";

type Config = { domains: string[]; policy: Policy };
export function Admin({
  refreshMe,
  notify,
}: {
  refreshMe: () => Promise<void>;
  notify: (s: string) => void;
}) {
  const [tab, setTab] = useState("general");
  return (
    <main className="main admin-main">
      <div className="page-heading">
        <div>
          <div className="eyebrow">Administration</div>
          <h1>Settings</h1>
        </div>
        <span className="admin-badge">
          <ShieldCheck size={16} />
          Administrator
        </span>
      </div>
      <div className="admin-tabs tabs">
        {[
          ["general", "General"],
          ["access", "People"],
          ["ownership", "Ownership"],
          ["activity", "Activity"],
        ].map(([v, l]) => (
          <button
            key={v}
            aria-pressed={tab === v}
            className={tab === v ? "active" : ""}
            onClick={() => setTab(v)}
          >
            {l}
          </button>
        ))}
      </div>
      {tab === "general" ? (
        <General refreshMe={refreshMe} notify={notify} />
      ) : tab === "access" ? (
        <Access notify={notify} />
      ) : tab === "ownership" ? (
        <OwnershipPanel notify={notify} />
      ) : (
        <Activity />
      )}
    </main>
  );
}
function General({
  refreshMe,
  notify,
}: {
  refreshMe: () => Promise<void>;
  notify: (s: string) => void;
}) {
  const [config, setConfig] = useState<Config>();
  const [domains, setDomains] = useState<string[]>([]);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const [checking, setChecking] = useState(false);
  const [connected, setConnected] = useState(false);
  async function load() {
    setError("");
    try {
      const d = await api<{
        configuration: Config;
        available_domains: string[];
      }>("/admin/config");
      setConfig(d.configuration);
      setDomains(d.available_domains);
    } catch (e) {
      setError((e as Error).message);
    }
  }
  useEffect(() => {
    void load();
  }, []);
  async function save(e: React.FormEvent) {
    e.preventDefault();
    setBusy(true);
    setError("");
    try {
      await api("/admin/config", "PUT", config);
      notify("Settings saved.");
      await refreshMe();
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setBusy(false);
    }
  }
  async function test() {
    setChecking(true);
    setConnected(false);
    setError("");
    try {
      await api("/admin/connection", "POST");
      setConnected(true);
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setChecking(false);
    }
  }
  return (
    <>
      <section className="settings-card">
        <div className="section-heading">
          <div>
            <h2>Mail server</h2>
            <p>Mailcow connection</p>
          </div>
          <button
            className="button secondary"
            onClick={() => void test()}
            disabled={checking}
          >
            {checking ? <Busy /> : <RefreshCw size={16} />}Test connection
          </button>
        </div>
        {connected && (
          <div className="success-message">
            <CheckCircle2 size={17} />
            Connected. API reads and sign-in are working.
            <InfoButton>
              This test does not change your mail server. A read-write API key
              is required to save addresses.
            </InfoButton>
          </div>
        )}
      </section>
      <ErrorMessage message={error} />
      {config ? (
        <form onSubmit={save}>
          <section className="settings-card">
            <div className="section-heading">
              <h2>Available domains</h2>
              <InfoButton>
                People can create addresses only on the domains selected here.
                Domains must already exist in Mailcow.
              </InfoButton>
            </div>
            <fieldset disabled={busy}>
              <div className="domain-options">
                {Array.from(new Set([...domains, ...config.domains])).map(
                  (d) => (
                    <label key={d} className="domain-option">
                      <input
                        type="checkbox"
                        checked={config.domains.includes(d)}
                        onChange={(e) =>
                          setConfig({
                            ...config,
                            domains: e.target.checked
                              ? [...config.domains, d]
                              : config.domains.filter((v) => v !== d),
                          })
                        }
                      />
                      {d}
                      {!domains.includes(d) && (
                        <span className="status review">Unavailable</span>
                      )}
                    </label>
                  ),
                )}
                {domains.length === 0 && (
                  <p className="muted">No active domains on the mail server.</p>
                )}
              </div>
            </fieldset>
          </section>
          <section className="settings-card">
            <div className="section-heading">
              <h2>Default limits</h2>
              <InfoButton>
                These limits apply to everyone unless you set individual limits
                under People. Storage is measured in MiB.
              </InfoButton>
            </div>
            <PolicyFields
              value={config.policy}
              onChange={(policy) => setConfig({ ...config, policy })}
              disabled={busy}
            />
          </section>
          <div className="settings-save">
            <button className="button primary" disabled={busy}>
              {busy ? <Busy /> : null}Save settings
            </button>
          </div>
        </form>
      ) : !error ? (
        <div className="loading">
          <Busy />
        </div>
      ) : (
        <button className="button secondary" onClick={() => void load()}>
          Try again
        </button>
      )}
    </>
  );
}
function Access({ notify }: { notify: (s: string) => void }) {
  const [users, setUsers] = useState<User[]>([]);
  const [administrator, setAdministrator] = useState("");
  const [policy, setPolicy] = useState<Policy>();
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState("");
  const [editing, setEditing] = useState<User | null>();
  const [removing, setRemoving] = useState<User>();
  async function load() {
    setLoading(true);
    setError("");
    try {
      const [d, c] = await Promise.all([
        api<{ users: User[]; administrator: string }>("/admin/users"),
        api<{ configuration: Config }>("/admin/config"),
      ]);
      setUsers(d.users);
      setAdministrator(d.administrator);
      setPolicy(c.configuration.policy);
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setLoading(false);
    }
  }
  useEffect(() => {
    void load();
  }, []);
  return (
    <section className="settings-card">
      <div className="section-heading">
        <h2>People with access</h2>
        <button
          className="button primary"
          disabled={!policy}
          onClick={() => setEditing(null)}
        >
          <Plus size={17} />
          Add person
        </button>
      </div>
      <ErrorMessage message={error} />
      {loading ? (
        <div className="loading">
          <Busy />
        </div>
      ) : (
        users.map((u) => (
          <div className="person-row" key={u.email}>
            <span className="person-icon">
              <UserRound size={18} />
            </span>
            <div className="person-detail">
              <strong>{u.email}</strong>
              <span>
                {u.email === administrator
                  ? "Administrator"
                  : u.policy
                    ? "Individual limits"
                    : "Default limits"}
              </span>
            </div>
            <span className={"status " + (u.enabled ? "on" : "off")}>
              {u.enabled ? "On" : "Off"}
            </span>
            {u.email !== administrator && (
              <>
                <button
                  className="icon-button"
                  aria-label={"Edit access for " + u.email}
                  onClick={() => setEditing(u)}
                >
                  <Pencil size={17} />
                </button>
                <button
                  className="icon-button danger-text"
                  aria-label={"Remove access for " + u.email}
                  onClick={() => setRemoving(u)}
                >
                  <Trash2 size={17} />
                </button>
              </>
            )}
          </div>
        ))
      )}
      {editing !== undefined && policy && (
        <UserForm
          user={editing}
          defaults={policy}
          onClose={() => setEditing(undefined)}
          saved={load}
          notify={notify}
        />
      )}{" "}
      {removing && (
        <ConfirmAction
          title="Remove access?"
          description="Their email addresses will stay on the mail server. They will no longer be able to sign in to Bucket."
          address={removing.email}
          label="Remove access"
          danger
          onClose={() => setRemoving(undefined)}
          action={async (confirmation) => {
            await api(
              "/admin/users/" + encodeURIComponent(removing.email),
              "DELETE",
              { confirmation },
            );
            notify("Access removed.");
            await load();
          }}
        />
      )}
    </section>
  );
}
function UserForm({
  user,
  defaults,
  onClose,
  saved,
  notify,
}: {
  user: User | null;
  defaults: Policy;
  onClose: () => void;
  saved: () => Promise<void>;
  notify: (s: string) => void;
}) {
  const [email, setEmail] = useState(user?.email ?? "");
  const [enabled, setEnabled] = useState(user?.enabled ?? true);
  const [individual, setIndividual] = useState(!!user?.policy);
  const [policy, setPolicy] = useState(user?.policy ?? defaults);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  async function submit(e: React.FormEvent) {
    e.preventDefault();
    setBusy(true);
    setError("");
    try {
      await api("/admin/users", "PUT", {
        email,
        enabled,
        policy: individual ? policy : null,
      });
      notify("Access saved.");
      onClose();
      await saved();
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setBusy(false);
    }
  }
  return (
    <Modal
      title={user ? "Edit access" : "Add person"}
      onClose={() => {
        if (!busy) onClose();
      }}
    >
      <form onSubmit={submit}>
        <fieldset disabled={busy}>
          <label>
            Email address
            <input
              autoFocus
              type="email"
              required
              disabled={!!user}
              value={email}
              onChange={(e) => setEmail(e.target.value)}
              autoCapitalize="none"
            />
          </label>
          <label className="check-row">
            <input
              type="checkbox"
              checked={enabled}
              onChange={(e) => setEnabled(e.target.checked)}
            />
            Allow sign-in
          </label>
          <label className="check-row">
            <input
              type="checkbox"
              checked={individual}
              onChange={(e) => setIndividual(e.target.checked)}
            />
            Set individual limits
          </label>
          {individual && <PolicyFields value={policy} onChange={setPolicy} />}
        </fieldset>
        <ErrorMessage message={error} />
        <div className="modal-footer">
          <button
            type="button"
            className="button secondary"
            disabled={busy}
            onClick={onClose}
          >
            Cancel
          </button>
          <button className="button primary" disabled={busy}>
            {busy ? <Busy /> : null}Save access
          </button>
        </div>
      </form>
    </Modal>
  );
}
function OwnershipPanel({ notify }: { notify: (s: string) => void }) {
  const [rows, setRows] = useState<Ownership[]>([]);
  const [users, setUsers] = useState<User[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState("");
  const [selected, setSelected] = useState<Ownership>();
  async function load() {
    setLoading(true);
    setError("");
    try {
      const [data, people] = await Promise.all([
        api<{ resources: Ownership[] }>("/admin/ownership"),
        api<{ users: User[] }>("/admin/users"),
      ]);
      setRows(data.resources);
      setUsers(people.users);
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setLoading(false);
    }
  }
  useEffect(() => {
    void load();
  }, []);
  return (
    <section className="settings-card">
      <div className="section-heading">
        <div className="inline">
          <h2>Address ownership</h2>
          <InfoButton>
            Bucket checks its ownership marker against Mailcow before making
            changes. Review interrupted changes here. Addresses created outside
            Bucket cannot be claimed.
          </InfoButton>
        </div>
        <button
          className="icon-button"
          aria-label="Refresh ownership"
          disabled={loading}
          onClick={() => void load()}
        >
          <RefreshCw size={17} className={loading ? "spin" : ""} />
        </button>
      </div>
      <ErrorMessage message={error} />
      {loading ? (
        <div className="loading">
          <Busy />
        </div>
      ) : rows.length === 0 ? (
        <p className="muted">No addresses yet</p>
      ) : (
        rows.map((r) => (
          <div className="ownership-row" key={r.id}>
            <div className="person-detail">
              <strong>{r.address}</strong>
              <span>Owned by {r.owner}</span>
            </div>
            <span
              className={"status " + (r.state === "managed" ? "on" : "review")}
            >
              {r.state === "managed" ? "Verified" : "Needs review"}
            </span>
            <button
              className="button secondary small"
              onClick={() => setSelected(r)}
            >
              Review
            </button>
          </div>
        ))
      )}
      {selected && (
        <OwnershipForm
          row={selected}
          users={users.filter((u) => u.enabled)}
          onClose={() => setSelected(undefined)}
          saved={load}
          notify={notify}
        />
      )}
    </section>
  );
}
function OwnershipForm({
  row,
  users,
  onClose,
  saved,
  notify,
}: {
  row: Ownership;
  users: User[];
  onClose: () => void;
  saved: () => Promise<void>;
  notify: (s: string) => void;
}) {
  const [action, setAction] = useState(
    row.exists && row.verified
      ? row.operation === "delete"
        ? "transfer"
        : "verify"
      : "forget",
  );
  const [owner, setOwner] = useState(row.owner);
  return (
    <ConfirmAction
      title="Review ownership"
      description={
        !row.exists
          ? "This address no longer exists in Mailcow."
          : !row.verified
            ? "The address has changed outside Bucket. Remove its record to release the reservation. This does not change Mailcow."
            : row.operation === "delete"
              ? "Deletion was interrupted. The address still exists. Reassign it to its current owner to restore access."
              : "The address matches Bucket’s ownership record."
      }
      address={row.address}
      label="Confirm change"
      onClose={onClose}
      action={async (confirmation) => {
        await api("/admin/ownership/" + row.id, "POST", {
          action,
          confirmation,
          ...(action === "transfer" ? { owner } : {}),
        });
        notify("Ownership updated.");
        await saved();
      }}
    >
      <label>
        Action
        <select value={action} onChange={(e) => setAction(e.target.value)}>
          {row.exists && row.verified ? (
            <>
              {row.operation !== "delete" && (
                <option value="verify">Verify ownership</option>
              )}
              <option value="transfer">Reassign address</option>
            </>
          ) : (
            <option value="forget">Remove Bucket record</option>
          )}
        </select>
      </label>
      {action === "transfer" && (
        <label>
          Owner
          <select
            required
            value={owner}
            onChange={(e) => setOwner(e.target.value)}
          >
            {users.map((u) => (
              <option key={u.email}>{u.email}</option>
            ))}
          </select>
        </label>
      )}
    </ConfirmAction>
  );
}
function ConfirmAction({
  title,
  description,
  address,
  label,
  danger = false,
  onClose,
  action,
  children,
}: {
  title: string;
  description: string;
  address: string;
  label: string;
  danger?: boolean;
  onClose: () => void;
  action: (confirmation: string) => Promise<void>;
  children?: React.ReactNode;
}) {
  const [confirmation, setConfirmation] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  async function submit(e: React.FormEvent) {
    e.preventDefault();
    setBusy(true);
    setError("");
    try {
      await action(confirmation);
      onClose();
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setBusy(false);
    }
  }
  return (
    <Modal
      title={title}
      description={description}
      onClose={() => {
        if (!busy) onClose();
      }}
    >
      <form onSubmit={submit}>
        <fieldset disabled={busy}>
          <div className="confirm-address">{address}</div>
          {children}
          <label>
            Type the email address to confirm
            <input
              autoComplete="off"
              value={confirmation}
              onChange={(e) => setConfirmation(e.target.value)}
            />
          </label>
        </fieldset>
        <ErrorMessage message={error} />
        <div className="modal-footer">
          <button
            type="button"
            className="button secondary"
            disabled={busy}
            onClick={onClose}
          >
            Cancel
          </button>
          <button
            className={"button " + (danger ? "danger" : "primary")}
            disabled={busy || confirmation !== address}
          >
            {busy ? <Busy /> : null}
            {label}
          </button>
        </div>
      </form>
    </Modal>
  );
}
const actionNames: Record<string, string> = {
  login: "Signed in",
  "login.access_denied": "Sign-in blocked: Bucket access not enabled",
  "login.mailbox_missing": "Sign-in blocked: mailbox not found",
  "login.mailbox_disabled": "Sign-in blocked: mailbox turned off",
  "login.mailbox_mismatch": "Sign-in blocked: unexpected mailbox",
  "login.unsupported_identity": "Sign-in blocked: external identity provider",
  "login.identity_changed": "Sign-in blocked: mailbox identity changed",
  "login.identity_missing": "Sign-in blocked: mailbox identity unavailable",
  "login.access_changed": "Sign-in blocked: access changed",
  "login.credentials_rejected": "Sign-in rejected by mail server",
  create: "Created address",
  "create.started": "Creating address",
  "create.failed": "Creation needs review",
  edit: "Updated address",
  "edit.started": "Updating address",
  "edit.failed": "Update needs review",
  delete: "Deleted address",
  "delete.started": "Deleting address",
  "delete.failed": "Deletion needs review",
  "user.save": "Updated access",
  "user.remove": "Removed access",
  "settings.save": "Saved settings",
  "ownership.verify": "Verified ownership",
  "ownership.forget": "Removed ownership record",
  "ownership.transfer": "Reassigned ownership",
};
function Activity() {
  const [events, setEvents] = useState<Audit[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState("");
  async function load() {
    setLoading(true);
    setError("");
    try {
      setEvents((await api<{ events: Audit[] }>("/admin/audit")).events);
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setLoading(false);
    }
  }
  useEffect(() => {
    void load();
  }, []);
  return (
    <section className="settings-card">
      <div className="section-heading">
        <h2>Recent activity</h2>
        <button
          className="icon-button"
          aria-label="Refresh activity"
          disabled={loading}
          onClick={() => void load()}
        >
          <RefreshCw size={17} className={loading ? "spin" : ""} />
        </button>
      </div>
      <ErrorMessage message={error} />
      {loading ? (
        <div className="loading">
          <Busy />
        </div>
      ) : events.length === 0 ? (
        <p className="muted">No activity yet</p>
      ) : (
        <div className="activity-list">
          {events.map((e, i) => (
            <div className="activity-row" key={i}>
              <div>
                <strong>{actionNames[e.action] ?? e.action}</strong>
                <span>{e.address || e.actor}</span>
                {e.address && <small>{e.actor}</small>}
              </div>
              <div>
                <time dateTime={new Date(e.at * 1000).toISOString()}>
                  {new Date(e.at * 1000).toLocaleString()}
                </time>
                {e.outcome !== "success" && (
                  <span className="status review">
                    {e.outcome === "pending"
                      ? "Started"
                      : e.outcome === "failed"
                        ? "Failed"
                        : "Needs review"}
                  </span>
                )}
              </div>
            </div>
          ))}
        </div>
      )}
    </section>
  );
}
