import { useState } from "react";
import { AtSign, Mail, Shuffle, Copy, Eye, EyeOff, Check } from "lucide-react";
import {
  api,
  generatePassword,
  randomLocal,
  type Address,
  type Me,
} from "./api";
import { Modal, InfoButton, Busy, ErrorMessage } from "./components";

type Props = {
  me: Me;
  address?: Address;
  onClose: () => void;
  onSaved: () => Promise<void>;
  notify: (s: string) => void;
};
export function AddressForm({ me, address, onClose, onSaved, notify }: Props) {
  const [kind, setKind] = useState<"mailbox" | "alias">(
    address?.kind ?? (me.policy.aliases ? "alias" : "mailbox"),
  );
  const [local, setLocal] = useState("");
  const [domain, setDomain] = useState(me.domains[0] ?? "");
  const [name, setName] = useState(address?.name ?? "");
  const [destination, setDestination] = useState(
    address?.destination ?? me.email,
  );
  const [storage, setStorage] = useState(
    address ? Math.ceil(address.storage / 1048576) : me.policy.default_storage,
  );
  const [password, setPassword] = useState("");
  const [visible, setVisible] = useState(false);
  const [active, setActive] = useState(address ? address.active === 1 : true);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [copied, setCopied] = useState(false);
  async function copy() {
    try {
      await navigator.clipboard.writeText(password);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    } catch {
      notify("Could not copy. Select the password to copy it.");
    }
  }
  async function submit(e: React.FormEvent) {
    e.preventDefault();
    setError("");
    setBusy(true);
    const data: Record<string, unknown> = {
      name,
      ...(kind === "mailbox"
        ? { storage, ...(password ? { password } : {}) }
        : { destination }),
    };
    try {
      if (address)
        await api("/addresses/" + address.id, "PUT", { ...data, active });
      else await api("/addresses", "POST", { ...data, kind, local, domain });
      setPassword("");
      notify(address ? "Address updated." : "Address created.");
      onClose();
      await onSaved();
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setBusy(false);
    }
  }
  return (
    <Modal
      title={address ? "Edit address" : "Create address"}
      description={address?.address}
      onClose={() => {
        if (!busy) {
          setPassword("");
          onClose();
        }
      }}
    >
      <form onSubmit={submit}>
        <fieldset disabled={busy}>
          {!address && (
            <>
              <div
                className="kind-choice"
                role="group"
                aria-label="Address type"
              >
                {me.policy.aliases && (
                  <button
                    type="button"
                    className={kind === "alias" ? "kind selected" : "kind"}
                    onClick={() => {
                      setKind("alias");
                      setPassword("");
                    }}
                  >
                    <AtSign size={21} />
                    <span>
                      <strong>Alias</strong>
                      <small>Forwards to your inbox</small>
                    </span>
                    {kind === "alias" && <Check size={17} />}
                  </button>
                )}
                {me.policy.mailboxes && (
                  <button
                    type="button"
                    className={kind === "mailbox" ? "kind selected" : "kind"}
                    onClick={() => setKind("mailbox")}
                  >
                    <Mail size={21} />
                    <span>
                      <strong>Mailbox</strong>
                      <small>Its own inbox and password</small>
                    </span>
                    {kind === "mailbox" && <Check size={17} />}
                  </button>
                )}
              </div>
              <div className="label-row">
                <label htmlFor="local-part">Email address</label>
                {kind === "alias" && (
                  <button
                    type="button"
                    className="text-button"
                    onClick={() => setLocal(randomLocal())}
                  >
                    <Shuffle size={14} /> Random address
                  </button>
                )}
              </div>
              <div className="address-input">
                <input
                  id="local-part"
                  autoFocus
                  required
                  autoComplete="off"
                  autoCapitalize="none"
                  spellCheck={false}
                  placeholder="name"
                  maxLength={64}
                  pattern="[a-zA-Z0-9][a-zA-Z0-9._\-]*"
                  value={local}
                  onChange={(e) => setLocal(e.target.value.toLowerCase())}
                />
                <span>@</span>
                <select
                  aria-label="Domain"
                  value={domain}
                  onChange={(e) => setDomain(e.target.value)}
                  required
                >
                  {me.domains.map((d) => (
                    <option key={d}>{d}</option>
                  ))}
                </select>
              </div>
            </>
          )}
          <label>
            Name <span className="optional">Optional</span>
            <input
              maxLength={100}
              placeholder="e.g. Shopping"
              value={name}
              onChange={(e) => setName(e.target.value)}
            />
          </label>
          {kind === "alias" ? (
            <label>
              Forwards to
              <select
                value={destination}
                required
                onChange={(e) => setDestination(e.target.value)}
              >
                {Array.from(new Set([destination, ...me.destinations]))
                  .filter(Boolean)
                  .map((d) => (
                    <option key={d}>{d}</option>
                  ))}
              </select>
            </label>
          ) : (
            <>
              <div className="label-row">
                <label htmlFor="mailbox-password">
                  {address ? "New password" : "Password"}{" "}
                  {address && <span className="optional">Optional</span>}
                </label>
                <InfoButton>
                  {address
                    ? "Leave blank to keep the current password."
                    : "Save this password before creating the mailbox. Bucket does not keep a copy."}
                </InfoButton>
              </div>
              <div className="password-input">
                <input
                  id="mailbox-password"
                  required={!address}
                  type={visible ? "text" : "password"}
                  autoComplete="new-password"
                  minLength={12}
                  maxLength={128}
                  value={password}
                  onChange={(e) => setPassword(e.target.value)}
                />
                <button
                  type="button"
                  className="icon-button"
                  aria-label={visible ? "Hide password" : "Show password"}
                  onClick={() => setVisible(!visible)}
                >
                  {visible ? <EyeOff size={18} /> : <Eye size={18} />}
                </button>
              </div>
              <div className="password-tools">
                <button
                  type="button"
                  className="text-button"
                  onClick={() => {
                    setPassword(generatePassword());
                    setVisible(true);
                    setCopied(false);
                  }}
                >
                  <Shuffle size={14} /> Generate password
                </button>
                <button
                  type="button"
                  className="text-button"
                  disabled={!password}
                  onClick={copy}
                >
                  {copied ? <Check size={14} /> : <Copy size={14} />}{" "}
                  {copied ? "Copied" : "Copy password"}
                </button>
              </div>
              <div className="label-row">
                <label htmlFor="storage">Storage space</label>
                <InfoButton>
                  {me.policy.max_storage.toLocaleString()} MB per mailbox, up to{" "}
                  {me.policy.total_storage.toLocaleString()} MB across your
                  mailboxes.
                </InfoButton>
              </div>
              <div className="unit-input">
                <input
                  id="storage"
                  type="number"
                  required
                  min={128}
                  max={me.policy.max_storage}
                  value={storage}
                  onChange={(e) => setStorage(e.target.valueAsNumber || 0)}
                />
                <span>MB</span>
              </div>
            </>
          )}
          {address && (
            <label className="switch-label">
              <span>Address is on</span>
              <input
                type="checkbox"
                role="switch"
                checked={active}
                onChange={(e) => setActive(e.target.checked)}
              />
            </label>
          )}
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
            {busy ? <Busy /> : null}
            {address ? "Save changes" : "Create address"}
          </button>
        </div>
      </form>
    </Modal>
  );
}
export function DeleteDialog({
  address,
  onClose,
  onSaved,
  notify,
}: {
  address: Address;
  onClose: () => void;
  onSaved: () => Promise<void>;
  notify: (s: string) => void;
}) {
  const [confirmation, setConfirmation] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  async function submit(e: React.FormEvent) {
    e.preventDefault();
    setBusy(true);
    setError("");
    try {
      await api("/addresses/" + address.id, "DELETE", { confirmation });
      notify("Address deleted.");
      onClose();
      await onSaved();
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setBusy(false);
    }
  }
  return (
    <Modal
      title="Delete address?"
      description={
        address.kind === "mailbox"
          ? "This permanently deletes the mailbox and all its messages."
          : "This address will stop receiving messages."
      }
      onClose={() => {
        if (!busy) onClose();
      }}
    >
      <form onSubmit={submit}>
        <div className="confirm-address">{address.address}</div>
        <label>
          Type the email address to confirm
          <input
            autoFocus
            autoComplete="off"
            spellCheck={false}
            value={confirmation}
            onChange={(e) => setConfirmation(e.target.value)}
            disabled={busy}
          />
        </label>
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
            className="button danger"
            disabled={busy || confirmation !== address.address}
          >
            {busy ? <Busy /> : null}Delete address
          </button>
        </div>
      </form>
    </Modal>
  );
}
