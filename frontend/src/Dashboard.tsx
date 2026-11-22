import { useEffect, useMemo, useState } from "react";
import * as Dropdown from "@radix-ui/react-dropdown-menu";
import {
  Plus,
  Search,
  Copy,
  MoreHorizontal,
  Mail,
  AtSign,
  Pencil,
  Trash2,
  RefreshCw,
  Inbox,
} from "lucide-react";
import { api, space, type Address, type Me } from "./api";
import { AddressForm, DeleteDialog } from "./AddressForm";
import { InfoButton, ErrorMessage, Busy } from "./components";
export function Dashboard({
  me,
  refreshMe,
  notify,
}: {
  me: Me;
  refreshMe: () => Promise<void>;
  notify: (s: string) => void;
}) {
  const [addresses, setAddresses] = useState<Address[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState("");
  const [query, setQuery] = useState("");
  const [filter, setFilter] = useState("all");
  const [status, setStatus] = useState("all");
  const [sort, setSort] = useState("newest");
  const [create, setCreate] = useState(false);
  const [edit, setEdit] = useState<Address>();
  const [remove, setRemove] = useState<Address>();
  async function load() {
    setLoading(true);
    setError("");
    try {
      const data = await api<{ addresses: Address[] }>("/addresses");
      setAddresses(data.addresses);
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setLoading(false);
    }
  }
  useEffect(() => {
    void load();
  }, []);
  async function saved() {
    await Promise.all([load(), refreshMe()]);
  }
  const filtered = useMemo(
    () =>
      addresses
        .filter(
          (a) =>
            (filter === "all" || a.kind === filter) &&
            (status === "all" ||
              (status === "on"
                ? a.active === 1 && a.state === "managed"
                : a.active !== 1 || a.state !== "managed")) &&
            [a.address, a.name, a.destination].some((s) =>
              s.toLowerCase().includes(query.toLowerCase()),
            ),
        )
        .sort((a, b) =>
          sort === "az"
            ? a.address.localeCompare(b.address)
            : sort === "type"
              ? a.kind.localeCompare(b.kind)
              : b.id - a.id,
        ),
    [addresses, query, filter, status, sort],
  );
  const mailboxCount = addresses.filter((a) => a.kind === "mailbox").length;
  const aliasCount = addresses.length - mailboxCount;
  async function copy(value: string) {
    try {
      await navigator.clipboard.writeText(value);
      notify("Address copied.");
    } catch {
      notify("Could not copy. Select the address to copy it.");
    }
  }
  const allowed =
    me.configured &&
    ((me.policy.mailboxes && mailboxCount < me.policy.max_mailboxes) ||
      (me.policy.aliases && aliasCount < me.policy.max_aliases));
  return (
    <main className="main">
      <div className="page-heading">
        <div>
          <div className="eyebrow">Your account</div>
          <h1>Email addresses</h1>
        </div>
        <button
          className="button primary"
          onClick={() => setCreate(true)}
          disabled={!allowed || loading}
        >
          <Plus size={18} />
          Create address
        </button>
      </div>
      <div className="overview">
        <div>
          <span className="overview-icon">
            <Mail size={19} />
          </span>
          <div>
            <strong>
              {mailboxCount}
              <span> / {me.policy.max_mailboxes}</span>
            </strong>
            <span>
              Mailboxes{" "}
              <InfoButton>A mailbox has its own inbox and password.</InfoButton>
            </span>
          </div>
        </div>
        <div>
          <span className="overview-icon">
            <AtSign size={19} />
          </span>
          <div>
            <strong>
              {aliasCount}
              <span> / {me.policy.max_aliases}</span>
            </strong>
            <span>
              Aliases{" "}
              <InfoButton>
                An alias forwards messages to one of your mailboxes.
              </InfoButton>
            </span>
          </div>
        </div>
        <div className="primary-inbox">
          <span className="overview-icon">
            <Inbox size={19} />
          </span>
          <div>
            <span>Primary mailbox</span>
            <strong title={me.email}>{me.email}</strong>
          </div>
        </div>
      </div>
      {!me.configured && (
        <div className="notice">
          {me.admin
            ? "Choose available domains in Settings before creating addresses."
            : "Address creation is not available. Contact your administrator."}
        </div>
      )}
      <section className="address-panel" aria-label="Your addresses">
        <div className="panel-toolbar">
          <div className="tabs" aria-label="Address type">
            {[
              ["all", "All addresses"],
              ["mailbox", "Mailboxes"],
              ["alias", "Aliases"],
            ].map(([value, label]) => (
              <button
                key={value}
                className={filter === value ? "active" : ""}
                aria-pressed={filter === value}
                onClick={() => setFilter(value)}
              >
                {label}
              </button>
            ))}
          </div>
          <button
            className="icon-button"
            aria-label="Refresh addresses"
            disabled={loading}
            onClick={() => void saved()}
          >
            <RefreshCw size={17} className={loading ? "spin" : ""} />
          </button>
        </div>
        <div className="search-toolbar">
          <div className="search-input">
            <Search size={17} />
            <input
              aria-label="Search addresses"
              placeholder="Search addresses"
              value={query}
              onChange={(e) => setQuery(e.target.value)}
            />
          </div>
          <select
            aria-label="Filter by status"
            value={status}
            onChange={(e) => setStatus(e.target.value)}
          >
            <option value="all">All statuses</option>
            <option value="on">On</option>
            <option value="off">Off or needs review</option>
          </select>
          <select
            aria-label="Sort addresses"
            value={sort}
            onChange={(e) => setSort(e.target.value)}
          >
            <option value="newest">Newest first</option>
            <option value="az">A to Z</option>
            <option value="type">By type</option>
          </select>
        </div>
        <ErrorMessage message={error} />
        {loading && addresses.length === 0 ? (
          <div className="empty">
            <Busy />
            <p>Loading addresses</p>
          </div>
        ) : filtered.length === 0 ? (
          <div className="empty">
            <span className="empty-icon">
              <Mail size={25} />
            </span>
            <h2>
              {query || filter !== "all" || status !== "all"
                ? "No matching addresses"
                : "No addresses yet"}
            </h2>
            {!query && filter === "all" && status === "all" && allowed && (
              <button className="text-button" onClick={() => setCreate(true)}>
                Create an address
              </button>
            )}
          </div>
        ) : (
          <div className="address-list">
            {filtered.map((a) => (
              <article className="address-row" key={a.id}>
                <div className={`address-symbol ${a.kind}`} aria-hidden="true">
                  {a.kind === "mailbox" ? (
                    <Mail size={20} />
                  ) : (
                    <AtSign size={20} />
                  )}
                </div>
                <div className="address-details">
                  <div className="address-title">
                    <strong>{a.address}</strong>
                    <button
                      className="icon-button copy-button"
                      aria-label={"Copy " + a.address}
                      onClick={() => void copy(a.address)}
                    >
                      <Copy size={15} />
                    </button>
                  </div>
                  <div className="address-meta">
                    <span>{a.kind === "mailbox" ? "Mailbox" : "Alias"}</span>
                    {a.name && (
                      <>
                        <span className="separator">·</span>
                        <span>{a.name}</span>
                      </>
                    )}
                    {a.kind === "alias" && a.state === "managed" && (
                      <span className="destination">
                        Forwards to {a.destination}
                      </span>
                    )}
                  </div>
                  {a.kind === "mailbox" && a.state === "managed" && (
                    <div className="storage">
                      <meter
                        min={0}
                        max={a.storage || 1}
                        value={a.used}
                        aria-label="Storage used"
                      />
                      <span>
                        {space(a.used)} of{" "}
                        {a.storage ? space(a.storage) : "unlimited"}
                      </span>
                    </div>
                  )}
                </div>
                <span
                  className={
                    "status " +
                    (a.state !== "managed"
                      ? "review"
                      : a.active === 1
                        ? "on"
                        : "off")
                  }
                >
                  {a.state !== "managed"
                    ? "Needs review"
                    : a.active === 1
                      ? "On"
                      : a.active === 2
                        ? "Login off"
                        : "Off"}
                </span>
                <Dropdown.Root>
                  <Dropdown.Trigger
                    className="icon-button"
                    aria-label={"Manage " + a.address}
                  >
                    <MoreHorizontal size={21} />
                  </Dropdown.Trigger>
                  <Dropdown.Portal>
                    <Dropdown.Content
                      className="menu"
                      align="end"
                      sideOffset={6}
                    >
                      <Dropdown.Item onSelect={() => void copy(a.address)}>
                        <Copy size={16} />
                        Copy address
                      </Dropdown.Item>
                      <Dropdown.Item
                        disabled={a.state !== "managed"}
                        onSelect={() => setEdit(a)}
                      >
                        <Pencil size={16} />
                        Edit address
                      </Dropdown.Item>
                      <Dropdown.Separator />
                      <Dropdown.Item
                        disabled={a.state !== "managed"}
                        className="danger-text"
                        onSelect={() => setRemove(a)}
                      >
                        <Trash2 size={16} />
                        Delete address
                      </Dropdown.Item>
                    </Dropdown.Content>
                  </Dropdown.Portal>
                </Dropdown.Root>
              </article>
            ))}
          </div>
        )}
        <div className="panel-footer">
          {filtered.length} {filtered.length === 1 ? "address" : "addresses"}
          {filter !== "all" || query || status !== "all"
            ? ` of ${addresses.length}`
            : ""}
        </div>
      </section>
      {create && (
        <AddressForm
          me={{
            ...me,
            policy: {
              ...me.policy,
              mailboxes:
                me.policy.mailboxes && mailboxCount < me.policy.max_mailboxes,
              aliases: me.policy.aliases && aliasCount < me.policy.max_aliases,
            },
          }}
          onClose={() => setCreate(false)}
          onSaved={saved}
          notify={notify}
        />
      )}
      {edit && (
        <AddressForm
          me={me}
          address={edit}
          onClose={() => setEdit(undefined)}
          onSaved={saved}
          notify={notify}
        />
      )}
      {remove && (
        <DeleteDialog
          address={remove}
          onClose={() => setRemove(undefined)}
          onSaved={saved}
          notify={notify}
        />
      )}
    </main>
  );
}
