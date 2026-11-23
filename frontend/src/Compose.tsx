import { useState } from "react";
import { Send, Info } from "lucide-react";
import { api, ApiError } from "./api";
import { Modal, Busy, ErrorMessage, InfoButton } from "./components";
import type { MailAddress, Reply } from "./mail";
export type Draft = {
  to: string;
  subject: string;
  text: string;
  reply?: Reply;
};
export function Compose({
  address,
  initial,
  onClose,
  onSent,
}: {
  address: MailAddress;
  initial?: Draft;
  onClose: () => void;
  onSent: (copySaved: boolean) => void;
}) {
  const [to, setTo] = useState(initial?.to ?? "");
  const [subject, setSubject] = useState(initial?.subject ?? "");
  const [text, setText] = useState(initial?.text ?? "");
  const [password, setPassword] = useState("");
  const [needsPassword, setNeedsPassword] = useState(!address.sending_unlocked);
  const [id, setId] = useState(() => crypto.randomUUID().replaceAll("-", ""));
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [discard, setDiscard] = useState(false);
  const [uncertain, setUncertain] = useState(false);
  const dirty = !!(to || subject || text);
  function close() {
    if (busy) return;
    if (dirty && !uncertain) setDiscard(true);
    else onClose();
  }
  async function submit(e: React.FormEvent) {
    e.preventDefault();
    if (busy || uncertain) return;
    setBusy(true);
    setError("");
    try {
      const result = await api<{ sent: boolean; copy_saved: boolean }>(
        "/mail/send",
        "POST",
        {
          id,
          address: address.address,
          to,
          subject,
          text,
          ...(initial?.reply ? { reply: initial.reply } : {}),
          ...(needsPassword ? { password } : {}),
        },
      );
      onSent(result.copy_saved);
    } catch (e) {
      setError((e as Error).message);
      if (e instanceof ApiError) {
        if (e.status === 423) setNeedsPassword(true);
        if (e.status === 409 && e.message.includes("confirm delivery"))
          setUncertain(true);
        if (e.status === 422) setId(crypto.randomUUID().replaceAll("-", ""));
      }
    } finally {
      setPassword("");
      setBusy(false);
    }
  }
  if (discard)
    return (
      <Modal
        title="Discard draft?"
        description="Your unsent message will be removed."
        onClose={() => setDiscard(false)}
      >
        <div className="modal-footer">
          <button
            className="button secondary"
            onClick={() => setDiscard(false)}
          >
            Keep writing
          </button>
          <button className="button danger" onClick={onClose}>
            Discard
          </button>
        </div>
      </Modal>
    );
  return (
    <Modal title={initial?.reply ? "Reply" : "New message"} onClose={close}>
      <form onSubmit={submit}>
        <fieldset disabled={busy || uncertain}>
          <div className="compose-from">
            <span>From</span>
            <strong>{address.address}</strong>
          </div>
          <label>
            To
            <input
              autoFocus
              required
              value={to}
              maxLength={2048}
              autoComplete="off"
              onChange={(e) => setTo(e.target.value)}
              placeholder="person@example.com"
            />
          </label>
          <label>
            Subject
            <input
              value={subject}
              maxLength={200}
              onChange={(e) => setSubject(e.target.value)}
            />
          </label>
          <label>
            Message
            <textarea
              required
              value={text}
              maxLength={16000}
              rows={10}
              onChange={(e) => setText(e.target.value)}
            />
          </label>
          {needsPassword && (
            <>
              <div className="label-row">
                <label htmlFor="send-password">Mailbox password</label>
                <InfoButton>
                  Use the password or SMTP-enabled app password for{" "}
                  {address.mailbox}. Bucket keeps the authenticated connection
                  for this session and does not save the password.
                </InfoButton>
              </div>
              <input
                id="send-password"
                type="password"
                required
                autoComplete="current-password"
                value={password}
                maxLength={1024}
                onChange={(e) => setPassword(e.target.value)}
              />
            </>
          )}
        </fieldset>
        <ErrorMessage message={error} />
        {uncertain && (
          <p className="compose-notice">
            <Info size={16} />
            Check Sent before creating another message. This draft will not be
            resent automatically.
          </p>
        )}
        <div className="modal-footer">
          <button
            type="button"
            className="button secondary"
            disabled={busy}
            onClick={close}
          >
            {uncertain ? "Close" : "Cancel"}
          </button>
          <button className="button primary" disabled={busy || uncertain}>
            {busy ? <Busy /> : <Send size={16} />}Send
          </button>
        </div>
      </form>
    </Modal>
  );
}
