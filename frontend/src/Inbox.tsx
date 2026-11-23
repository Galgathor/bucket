import { useEffect, useRef, useState } from "react";
import {
  Inbox as InboxIcon,
  RefreshCw,
  ArrowLeft,
  Paperclip,
  MailOpen,
  Mail,
} from "lucide-react";
import { api, ApiError, space, type Me } from "./api";
import { Busy, ErrorMessage, InfoButton } from "./components";

type MailAddress = {
  address: string;
  mailbox: string;
  kind: string;
  name: string;
  unlocked: boolean;
};
type Message = {
  uid: number;
  subject: string;
  from: string;
  to: string;
  date: string;
  read: boolean;
  size: number;
};
type MessageBody = {
  message: Message;
  text: string;
  html: string;
  attachments: { part: number; name: string; size: number }[];
};
type Page = { messages: Message[]; next: number | null; validity: number };
export function Inbox({
  me,
  notify,
}: {
  me: Me;
  notify: (message: string) => void;
}) {
  const [addresses, setAddresses] = useState<MailAddress[]>([]);
  const [address, setAddress] = useState(me.email);
  const [messages, setMessages] = useState<Message[]>([]);
  const [body, setBody] = useState<MessageBody>();
  const [loading, setLoading] = useState(true);
  const [reading, setReading] = useState(false);
  const [working, setWorking] = useState(false);
  const [error, setError] = useState("");
  const [bodyError, setBodyError] = useState("");
  const [locked, setLocked] = useState(false);
  const [password, setPassword] = useState("");
  const [next, setNext] = useState<number | null>(null);
  const [validity, setValidity] = useState(0);
  const sequence = useRef(0);
  const listSequence = useRef(0);
  const current = useRef(address);
  current.current = address;
  const chosen = addresses.find((a) => a.address === address);
  async function loadAddresses() {
    const result = await api<{ addresses: MailAddress[] }>("/mail/addresses");
    setAddresses(result.addresses);
    return result.addresses;
  }
  async function load(more = false) {
    const selected = address;
    const seq = ++listSequence.current;
    setLoading(true);
    setError("");
    try {
      const query = new URLSearchParams({
        address: selected,
        ...(more && next
          ? { before: String(next), validity: String(validity) }
          : {}),
      });
      const result = await api<Page>("/mail/messages?" + query);
      if (current.current !== selected || listSequence.current !== seq) return;
      setMessages((old) =>
        more
          ? [
              ...old,
              ...result.messages.filter(
                (m) => !old.some((o) => o.uid === m.uid),
              ),
            ]
          : result.messages,
      );
      setNext(result.next);
      setValidity(result.validity);
      setLocked(false);
    } catch (e) {
      if (current.current !== selected || listSequence.current !== seq) return;
      if (
        e instanceof ApiError &&
        (e.status === 423 ||
          (e.status === 503 && e.message.includes("inbox connection")))
      ) {
        setLocked(true);
        setMessages([]);
      } else {
        setError((e as Error).message);
      }
    } finally {
      if (current.current === selected && listSequence.current === seq)
        setLoading(false);
    }
  }
  useEffect(() => {
    let cancelled = false;
    void loadAddresses().catch((e) => {
      if (!cancelled) setError((e as Error).message);
    });
    return () => {
      cancelled = true;
    };
  }, [me.email]);
  useEffect(() => {
    setBody(undefined);
    setBodyError("");
    setReading(false);
    setMessages([]);
    setPassword("");
    setLocked(false);
    setNext(null);
    void load();
    return () => {
      sequence.current++;
      listSequence.current++;
    };
  }, [address]);
  async function unlock(e: React.FormEvent) {
    e.preventDefault();
    const selected = address;
    setWorking(true);
    setError("");
    try {
      await api("/mail/unlock", "POST", { address: selected, password });
      if (current.current === selected) {
        setPassword("");
        await loadAddresses();
        await load();
      }
    } catch (e) {
      if (current.current === selected) setError((e as Error).message);
    } finally {
      setPassword("");
      setWorking(false);
    }
  }
  function messageQuery(uid: number, part?: number) {
    return new URLSearchParams({
      address,
      uid: String(uid),
      validity: String(validity),
      ...(part !== undefined ? { part: String(part) } : {}),
    });
  }
  async function open(message: Message) {
    const selected = address;
    const seq = ++sequence.current;
    setReading(true);
    setBody(undefined);
    setBodyError("");
    try {
      const result = await api<MessageBody>(
        "/mail/message?" + messageQuery(message.uid),
      );
      if (current.current !== selected || sequence.current !== seq) return;
      setBody(result);
      if (!message.read) {
        await api("/mail/read", "PUT", {
          address: selected,
          uid: message.uid,
          validity,
          read: true,
        });
        if (current.current === selected && sequence.current === seq) {
          setMessages((old) =>
            old.map((m) => (m.uid === message.uid ? { ...m, read: true } : m)),
          );
          setBody((old) =>
            old ? { ...old, message: { ...old.message, read: true } } : old,
          );
        }
      }
    } catch (e) {
      if (current.current === selected && sequence.current === seq)
        setBodyError((e as Error).message);
    } finally {
      if (current.current === selected && sequence.current === seq)
        setReading(false);
    }
  }
  async function toggleRead() {
    if (!body) return;
    const selected = address;
    const uid = body.message.uid;
    const read = !body.message.read;
    setWorking(true);
    try {
      await api("/mail/read", "PUT", {
        address: selected,
        uid,
        validity,
        read,
      });
      if (current.current === selected) {
        setBody((old) =>
          old?.message.uid === uid
            ? { ...old, message: { ...old.message, read } }
            : old,
        );
        setMessages((old) =>
          old.map((m) => (m.uid === uid ? { ...m, read } : m)),
        );
      }
    } catch (e) {
      notify((e as Error).message);
    } finally {
      setWorking(false);
    }
  }
  return (
    <main className="main inbox-main">
      <div className="page-heading">
        <div>
          <div className="eyebrow">Email</div>
          <h1>Inbox</h1>
        </div>
        <div className="inbox-switch">
          <label className="sr-only" htmlFor="inbox-address">
            Email address
          </label>
          <select
            id="inbox-address"
            value={address}
            disabled={working}
            onChange={(e) => {
              setAddress(e.target.value);
              setReading(false);
            }}
          >
            {addresses.length ? (
              addresses.map((a) => (
                <option key={a.address} value={a.address}>
                  {a.address}
                  {a.kind === "alias" ? " · Alias" : ""}
                </option>
              ))
            ) : (
              <option>{me.email}</option>
            )}
          </select>
          <InfoButton label="About address filtering">
            Only messages matching this address in recipient or delivery headers
            are shown. Some Bcc and forwarded messages may not preserve the
            original address. Aliases use their destination mailbox.
          </InfoButton>
        </div>
      </div>
      <section className="inbox-panel" aria-label="Email inbox">
        <div className="inbox-toolbar">
          <span>
            <InboxIcon size={17} />
            {address}
          </span>
          <button
            className="icon-button"
            aria-label="Refresh inbox"
            disabled={loading || working || reading}
            onClick={() => {
              sequence.current++;
              setReading(false);
              setBody(undefined);
              void load();
            }}
          >
            <RefreshCw size={17} className={loading ? "spin" : ""} />
          </button>
        </div>
        <ErrorMessage message={error} />
        {locked ? (
          <div className="mail-unlock">
            <Mail size={25} />
            <h2>Open mailbox</h2>
            <p>{chosen?.mailbox ?? address}</p>
            <form onSubmit={unlock}>
              <label>
                Password
                <input
                  autoFocus
                  type="password"
                  required
                  autoComplete="current-password"
                  maxLength={1024}
                  value={password}
                  disabled={working}
                  onChange={(e) => setPassword(e.target.value)}
                />
              </label>
              <button
                className="button primary full"
                disabled={working || !password}
              >
                {working ? <Busy /> : null}Open inbox
              </button>
            </form>
            <InfoButton>
              Enter the destination mailbox’s password or IMAP-enabled app
              password. The connection stays open for this Bucket session.
              Bucket does not save the password.
            </InfoButton>
          </div>
        ) : (
          <div
            className={
              "mail-layout " +
              (body || reading || bodyError ? "show-message" : "")
            }
          >
            <div className="mail-list" aria-label="Messages">
              {loading && messages.length === 0 ? (
                <div className="empty">
                  <Busy />
                  <p>Loading messages</p>
                </div>
              ) : messages.length === 0 ? (
                <div className="empty">
                  <InboxIcon size={25} />
                  <h2>{error ? "Messages unavailable" : "No messages"}</h2>
                </div>
              ) : (
                messages.map((message) => (
                  <button
                    key={message.uid}
                    className={
                      "mail-row " +
                      (!message.read ? "unread " : "") +
                      (body?.message.uid === message.uid ? "selected" : "")
                    }
                    onClick={() => void open(message)}
                    aria-label={
                      (message.read ? "" : "Unread: ") +
                      (message.subject || "No subject")
                    }
                  >
                    <div>
                      <span className="mail-sender">
                        {message.from || "Unknown sender"}
                      </span>
                      <time>{displayDate(message.date)}</time>
                    </div>
                    <strong>{message.subject || "No subject"}</strong>
                    <span className="mail-recipient">
                      To {message.to || address}
                    </span>
                  </button>
                ))
              )}
              {next !== null && (
                <button
                  className="button secondary more-messages"
                  disabled={loading}
                  onClick={() => void load(true)}
                >
                  {loading ? <Busy /> : null}Load more
                </button>
              )}
            </div>
            <div className="mail-reader" aria-label="Message">
              <button
                className="text-button mail-back"
                onClick={() => {
                  sequence.current++;
                  setBody(undefined);
                  setBodyError("");
                  setReading(false);
                }}
              >
                <ArrowLeft size={16} />
                Messages
              </button>
              <ErrorMessage message={bodyError} />
              {reading ? (
                <div className="empty">
                  <Busy />
                </div>
              ) : body ? (
                <>
                  <div className="message-heading">
                    <div>
                      <h2>{body.message.subject || "No subject"}</h2>
                      <p>
                        <strong>From</strong> {body.message.from}
                      </p>
                      <p>
                        <strong>To</strong> {body.message.to || address}
                      </p>
                      <p>{body.message.date}</p>
                    </div>
                    <button
                      className="icon-button"
                      disabled={working}
                      aria-label={
                        body.message.read ? "Mark unread" : "Mark read"
                      }
                      onClick={() => void toggleRead()}
                    >
                      {body.message.read ? (
                        <Mail size={18} />
                      ) : (
                        <MailOpen size={18} />
                      )}
                    </button>
                  </div>
                  {body.html ? (
                    <iframe
                      className="message-html"
                      title="Email content"
                      sandbox=""
                      referrerPolicy="no-referrer"
                      srcDoc={body.html}
                    />
                  ) : (
                    <pre className="message-text">
                      {body.text || "No message content"}
                    </pre>
                  )}
                  {body.attachments.length > 0 && (
                    <div className="message-attachments">
                      <h3>Attachments</h3>
                      {body.attachments.map((a) => (
                        <a
                          key={a.part}
                          className="attachment"
                          href={
                            "/api/mail/attachment?" +
                            messageQuery(body.message.uid, a.part)
                          }
                          download
                        >
                          <Paperclip size={16} />
                          <span>{a.name}</span>
                          <small>{fileSize(a.size)}</small>
                        </a>
                      ))}
                    </div>
                  )}
                </>
              ) : (
                !bodyError && (
                  <div className="empty">
                    <MailOpen size={28} />
                    <p>Select a message</p>
                  </div>
                )
              )}
            </div>
          </div>
        )}
      </section>
    </main>
  );
}
function displayDate(value: string) {
  const date = new Date(value);
  return Number.isNaN(date.getTime())
    ? ""
    : date.toLocaleDateString(undefined, { month: "short", day: "numeric" });
}

function fileSize(bytes: number) {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1048576) return `${Math.ceil(bytes / 1024)} KB`;
  return space(bytes);
}
