import { useEffect, useRef, useState } from "react";
import {
  Inbox as InboxIcon,
  RefreshCw,
  ArrowLeft,
  Paperclip,
  MailOpen,
  Mail,
  Reply as ReplyIcon,
  Send,
  ShieldAlert,
  Trash2,
  Undo2,
} from "lucide-react";
import { api, ApiError, space, type Me } from "./api";
import { Busy, ErrorMessage, InfoButton } from "./components";

import { Compose, type Draft } from "./Compose";
import { VerificationCodes } from "./VerificationCodes";
import {
  MAIL_REFRESH_MS,
  type Folder,
  type MailAddress,
  type MailMessage as Message,
  type MessageBody,
} from "./mail";
type Page = { messages: Message[]; next: number | null; validity: number };
export function Inbox({
  me,
  notify,
  mailStatus,
  refreshMailStatus,
}: {
  me: Me;
  mailStatus: MailAddress[];
  refreshMailStatus: () => Promise<void>;
  notify: (message: string) => void;
}) {
  const [addresses, setAddresses] = useState<MailAddress[]>([]);
  const [address, setAddress] = useState(me.email);
  const [folder, setFolder] = useState<Folder>("inbox");
  const [composing, setComposing] = useState<Draft | null>();
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
  async function load(more = false, background = false) {
    const selected = address;
    const seq = ++listSequence.current;
    if (!background) setLoading(true);
    setError("");
    try {
      const query = new URLSearchParams({
        address: selected,
        folder,
        ...(more && next
          ? { before: String(next), validity: String(validity) }
          : {}),
      });
      const result = await api<Page>("/mail/messages?" + query);
      if (current.current !== selected || listSequence.current !== seq) return;
      setMessages((old) =>
        background && old.length > 50 && validity === result.validity
          ? Array.from(
              new Map(
                [...old, ...result.messages].map((m) => [m.uid, m]),
              ).values(),
            ).sort((a, b) => b.uid - a.uid)
          : more
            ? [
                ...old,
                ...result.messages.filter(
                  (m) => !old.some((o) => o.uid === m.uid),
                ),
              ]
            : result.messages,
      );
      if (!background || messages.length <= 50) setNext(result.next);
      if (validity && validity !== result.validity) {
        sequence.current++;
        setBody(undefined);
        setReading(false);
      }
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
  }, [address, folder]);
  useEffect(() => {
    if (mailStatus.length) setAddresses(mailStatus);
  }, [mailStatus]);
  const pollRef = useRef<() => Promise<void>>(async () => {});
  pollRef.current = async () => {
    if (!working && !loading && !reading && !locked) await load(false, true);
  };
  useEffect(() => {
    let pending = false;
    const poll = async () => {
      if (document.hidden || pending) return;
      pending = true;
      try {
        await pollRef.current();
      } finally {
        pending = false;
      }
    };
    const timer = setInterval(() => void poll(), MAIL_REFRESH_MS);
    const visible = () => {
      if (!document.hidden) void poll();
    };
    document.addEventListener("visibilitychange", visible);
    return () => {
      clearInterval(timer);
      document.removeEventListener("visibilitychange", visible);
    };
  }, []);
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
        await refreshMailStatus();
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
      folder,
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
          folder,
          uid: message.uid,
          validity,
          read: true,
        });
        if (current.current === selected && sequence.current === seq) {
          void refreshMailStatus();
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
        folder,
        uid,
        validity,
        read,
      });
      if (current.current === selected) {
        void refreshMailStatus();
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
  async function move(destination: Folder) {
    if (!body) return;
    setWorking(true);
    try {
      await api("/mail/move", "POST", {
        address,
        uid: body.message.uid,
        validity,
        folder,
        destination,
      });
      sequence.current++;
      setBody(undefined);
      setMessages((old) => old.filter((m) => m.uid !== body.message.uid));
      notify(
        `Moved to ${destination === "inbox" ? "Inbox" : destination === "spam" ? "Spam" : "Trash"}.`,
      );
      await load();
      await refreshMailStatus();
    } catch (e) {
      notify((e as Error).message);
    } finally {
      setWorking(false);
    }
  }
  function reply() {
    if (!body) return;
    const to =
      folder === "sent"
        ? body.message.to
        : body.message.reply_to || body.message.from;
    const subject = /^re:/i.test(body.message.subject)
      ? body.message.subject
      : "Re: " + body.message.subject;
    const quote = body.text.slice(0, 10000);
    setComposing({
      to,
      subject,
      text: quote
        ? `\n\nOn ${body.message.date}, ${body.message.from} wrote:\n${quote
            .split("\n")
            .map((line) => "> " + line)
            .join("\n")}`
        : "",
      reply: { uid: body.message.uid, validity, folder },
    });
  }
  const folderName =
    folder === "inbox"
      ? "Inbox"
      : folder === "spam"
        ? "Spam"
        : folder === "trash"
          ? "Trash"
          : "Sent";
  return (
    <main className="main inbox-main">
      <div className="page-heading">
        <div>
          <div className="eyebrow">Email</div>
          <h1>{folderName}</h1>
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
                  {(a.unread ?? 0) > 0 ? "● " : ""}
                  {a.address}
                  {a.kind === "alias" ? " · Alias" : ""}
                  {(a.unread ?? 0) > 0
                    ? ` · ${a.unread}${a.unread_limited ? "+" : ""} unread`
                    : ""}
                </option>
              ))
            ) : (
              <option>{me.email}</option>
            )}
          </select>
          {(chosen?.unread ?? 0) > 0 && (
            <span
              className="unread-badge"
              aria-label={`${chosen?.unread} unread messages`}
            >
              <Mail size={14} />
              {chosen?.unread}
              {chosen?.unread_limited ? "+" : ""}
            </span>
          )}
          <button
            className="button primary"
            disabled={locked || loading || chosen?.can_send === false}
            onClick={() => setComposing(null)}
          >
            <Send size={16} />
            Compose
          </button>
          <InfoButton label="About address filtering">
            Only messages matching this address in recipient or delivery headers
            are shown. Some Bcc and forwarded messages may not preserve the
            original address. Aliases use their destination mailbox.
          </InfoButton>
        </div>
      </div>
      <section className="inbox-panel" aria-label="Email inbox">
        <div className="mail-folder-tabs tabs" aria-label="Mail folders">
          {(["inbox", "spam", "trash", "sent"] as Folder[]).map((item) => (
            <button
              key={item}
              aria-pressed={folder === item}
              className={folder === item ? "active" : ""}
              disabled={working}
              onClick={() => setFolder(item)}
            >
              {item === "inbox" ? (
                <InboxIcon size={16} />
              ) : item === "spam" ? (
                <ShieldAlert size={16} />
              ) : item === "trash" ? (
                <Trash2 size={16} />
              ) : (
                <Send size={16} />
              )}
              <span>
                {item === "inbox"
                  ? "Inbox"
                  : item === "spam"
                    ? "Spam"
                    : item === "trash"
                      ? "Trash"
                      : "Sent"}
              </span>
            </button>
          ))}
        </div>
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
              void refreshMailStatus();
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
                    <div className="message-actions">
                      <button
                        className="button secondary small"
                        disabled={working || chosen?.can_send === false}
                        onClick={reply}
                      >
                        <ReplyIcon size={15} />
                        Reply
                      </button>
                      {folder !== "spam" && (
                        <button
                          className="icon-button"
                          disabled={working}
                          aria-label="Move to Spam"
                          title="Move to Spam"
                          onClick={() => void move("spam")}
                        >
                          <ShieldAlert size={18} />
                        </button>
                      )}
                      {folder !== "trash" && (
                        <button
                          className="icon-button"
                          disabled={working}
                          aria-label="Move to Trash"
                          title="Move to Trash"
                          onClick={() => void move("trash")}
                        >
                          <Trash2 size={18} />
                        </button>
                      )}
                      {(folder === "spam" || folder === "trash") && (
                        <button
                          className="icon-button"
                          disabled={working}
                          aria-label="Move to Inbox"
                          title="Move to Inbox"
                          onClick={() => void move("inbox")}
                        >
                          <Undo2 size={18} />
                        </button>
                      )}
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
                  </div>
                  <VerificationCodes
                    key={`${address}:${folder}:${validity}:${body.message.uid}`}
                    message={body}
                    notify={notify}
                  />
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
      {composing !== undefined && (
        <Compose
          address={
            chosen ?? {
              address,
              mailbox: address,
              kind: "mailbox",
              name: "",
              unlocked: true,
            }
          }
          initial={composing ?? undefined}
          onClose={() => setComposing(undefined)}
          onSent={(copySaved) => {
            setComposing(undefined);
            notify(
              copySaved
                ? "Message sent."
                : "Message sent. A copy could not be saved to Sent.",
            );
            void load(false, true);
            void refreshMailStatus();
          }}
        />
      )}
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
