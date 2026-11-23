import { useState } from "react";
import { api, aliasBase, aliasSuffix, type Me } from "./api";
import { Modal, Busy, ErrorMessage } from "./components";

type Props = {
  me: Me;
  onClose: () => void;
  onSaved: () => Promise<void>;
  onPreferenceSaved: () => Promise<void>;
  notify: (message: string) => void;
};
export function AliasForm({
  me,
  onClose,
  onSaved,
  onPreferenceSaved,
  notify,
}: Props) {
  const [step, setStep] = useState(0);
  const [name, setName] = useState("");
  const [source, setSource] = useState("");
  const [base, setBase] = useState("");
  const [suffix] = useState(aliasSuffix);
  const [useSuffix, setUseSuffix] = useState(me.alias_suffix ?? true);
  const [domain, setDomain] = useState(
    me.domains.includes(me.email.split("@")[1])
      ? me.email.split("@")[1]
      : (me.domains[0] ?? ""),
  );
  const [destination, setDestination] = useState(me.email);
  const [busy, setBusy] = useState(false);
  const [savingPreference, setSavingPreference] = useState(false);
  const [error, setError] = useState("");
  const local = base + (useSuffix ? "-" + suffix : "");
  const address = local + "@" + domain;
  const locked = busy || savingPreference;
  async function preference(value: boolean) {
    setSavingPreference(true);
    setError("");
    const old = useSuffix;
    setUseSuffix(value);
    try {
      await api("/preferences", "PUT", { alias_suffix: value });
      if (value)
        setBase((current) => current.slice(0, 60).replace(/[-._]+$/, ""));
      try {
        await onPreferenceSaved();
      } catch {
        setError(
          "Your preference was saved, but account details could not be refreshed.",
        );
      }
    } catch (e) {
      setUseSuffix(old);
      setError((e as Error).message);
    } finally {
      setSavingPreference(false);
    }
  }
  async function submit(e: React.FormEvent) {
    e.preventDefault();
    setError("");
    if (locked) return;
    if (step === 0) {
      if (name !== source) {
        setBase(aliasBase(name).slice(0, 60).replace(/-+$/, ""));
        setSource(name);
      }
      setStep(1);
      return;
    }
    if (step === 1) {
      setStep(2);
      return;
    }
    setBusy(true);
    try {
      await api("/addresses", "POST", {
        kind: "alias",
        name: name.trim(),
        local,
        domain,
        destination,
      });
      notify("Alias created.");
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
      title={step === 3 ? "Review changes" : "Create Alias"}
      onClose={() => {
        if (!locked) onClose();
      }}
    >
      <form onSubmit={submit}>
        <fieldset disabled={locked}>
          <ol className="alias-steps" aria-label="Alias creation steps">
            {["Name", "Email address", "Forwards to"].map((label, index) => (
              <li
                key={label}
                aria-current={step === index ? "step" : undefined}
                className={step >= index ? "current" : ""}
              >
                {label}
              </li>
            ))}
          </ol>
          {step === 0 && (
            <label>
              Name
              <input
                autoFocus
                required
                maxLength={100}
                value={name}
                onChange={(e) => setName(e.target.value)}
                placeholder="e.g. Shopping"
              />
            </label>
          )}
          {step === 1 && (
            <>
              <label htmlFor="alias-local">Email address</label>
              <div className="address-input alias-address">
                <input
                  autoFocus
                  id="alias-local"
                  required
                  maxLength={useSuffix ? 60 : 64}
                  pattern="[a-z0-9]([a-z0-9._\-]*[a-z0-9])?"
                  autoCapitalize="none"
                  autoComplete="off"
                  spellCheck={false}
                  value={base}
                  onChange={(e) => setBase(e.target.value.toLowerCase())}
                />
                {useSuffix && (
                  <span className="alias-suffix" aria-label="Random suffix">
                    -{suffix}
                  </span>
                )}
                <span>@</span>
                <select
                  aria-label="Domain"
                  required
                  value={domain}
                  onChange={(e) => setDomain(e.target.value)}
                >
                  {me.domains.map((d) => (
                    <option key={d}>{d}</option>
                  ))}
                </select>
              </div>
              <label className="check-row">
                <input
                  type="checkbox"
                  checked={useSuffix}
                  onChange={(e) => void preference(e.target.checked)}
                />
                Add 3 random characters
              </label>
              <div className="alias-preview" aria-label="Suggested alias">
                {address}
              </div>
            </>
          )}
          {step === 2 && (
            <>
              <div className="confirm-address">{address}</div>
              <label>
                Forwards to
                <select
                  autoFocus
                  required
                  value={destination}
                  onChange={(e) => setDestination(e.target.value)}
                >
                  {me.destinations.map((d) => (
                    <option key={d}>{d}</option>
                  ))}
                </select>
              </label>
            </>
          )}
          {step === 3 && (
            <dl className="alias-review">
              <div>
                <dt>Name</dt>
                <dd>{name.trim()}</dd>
              </div>
              <div>
                <dt>Email address</dt>
                <dd>{address}</dd>
              </div>
              <div>
                <dt>Type</dt>
                <dd>Alias</dd>
              </div>
              <div>
                <dt>Forwards to</dt>
                <dd>{destination}</dd>
              </div>
              <div>
                <dt>Status</dt>
                <dd>On</dd>
              </div>
            </dl>
          )}
        </fieldset>
        <ErrorMessage message={error} />
        <div className="modal-footer alias-footer">
          {step > 0 && (
            <button
              type="button"
              className="text-button back-button"
              disabled={locked}
              onClick={() => {
                setError("");
                setStep(step === 3 ? 2 : step - 1);
              }}
            >
              Back
            </button>
          )}
          <button
            type="button"
            className="button secondary"
            disabled={locked}
            onClick={onClose}
          >
            Cancel
          </button>
          {step === 2 && (
            <button
              type="button"
              className="button secondary"
              disabled={locked}
              onClick={() => {
                setError("");
                setStep(3);
              }}
            >
              Review Changes
            </button>
          )}
          <button
            className="button primary"
            disabled={locked || (step === 0 && !name.trim())}
          >
            {locked ? <Busy /> : null}
            {step < 2 ? "Next" : "Create"}
          </button>
        </div>
      </form>
    </Modal>
  );
}
