import { StrictMode, useEffect, useState, useCallback } from "react";
import { createRoot } from "react-dom/client";
import * as Dropdown from "@radix-ui/react-dropdown-menu";
import {
  Mail,
  Settings,
  LogOut,
  Sun,
  Moon,
  Monitor,
  Download,
  X,
  Check,
  ChevronDown,
} from "lucide-react";
import { api, ApiError, setCsrf, type Me } from "./api";
import { Busy, ErrorMessage, InfoButton } from "./components";
import { Dashboard } from "./Dashboard";
import { Admin } from "./Admin";
import "./style.css";

type InstallEvent = Event & {
  prompt: () => Promise<void>;
  userChoice: Promise<{ outcome: string }>;
};
function App() {
  const [me, setMe] = useState<Me>();
  const [initial, setInitial] = useState(true);
  const [initialError, setInitialError] = useState("");
  const [page, setPage] = useState("addresses");
  const [toast, setToast] = useState("");
  const [theme, setTheme] = useState(
    () => localStorage.getItem("bucket-theme") ?? "system",
  );
  const [install, setInstall] = useState<InstallEvent>();
  const [update, setUpdate] = useState<ServiceWorkerRegistration>();
  const [online, setOnline] = useState(navigator.onLine);
  const notify = useCallback((message: string) => setToast(message), []);
  useEffect(() => {
    if (!toast) return;
    const t = setTimeout(() => setToast(""), 5000);
    return () => clearTimeout(t);
  }, [toast]);
  useEffect(() => {
    const query = matchMedia("(prefers-color-scheme: dark)");
    const apply = () =>
      (document.documentElement.dataset.theme =
        theme === "system" ? (query.matches ? "dark" : "light") : theme);
    apply();
    localStorage.setItem("bucket-theme", theme);
    query.addEventListener("change", apply);
    return () => query.removeEventListener("change", apply);
  }, [theme]);
  useEffect(() => {
    const callback = (e: Event) => {
      e.preventDefault();
      setInstall(e as InstallEvent);
    };
    window.addEventListener("beforeinstallprompt", callback);
    const installed = () => setInstall(undefined);
    window.addEventListener("appinstalled", installed);
    const connection = () => setOnline(navigator.onLine);
    window.addEventListener("online", connection);
    window.addEventListener("offline", connection);
    return () => {
      window.removeEventListener("beforeinstallprompt", callback);
      window.removeEventListener("appinstalled", installed);
      window.removeEventListener("online", connection);
      window.removeEventListener("offline", connection);
    };
  }, []);
  useEffect(() => {
    if (!("serviceWorker" in navigator) || !import.meta.env.PROD) return;
    void navigator.serviceWorker
      .register("/sw.js")
      .then((reg) => {
        if (reg.waiting) setUpdate(reg);
        reg.addEventListener("updatefound", () => {
          reg.installing?.addEventListener("statechange", () => {
            if (reg.waiting && navigator.serviceWorker.controller)
              setUpdate(reg);
          });
        });
      })
      .catch(() => {});
    let controlled = !!navigator.serviceWorker.controller;
    const changed = () => {
      if (controlled) window.location.reload();
      controlled = true;
    };
    navigator.serviceWorker.addEventListener("controllerchange", changed);
    return () =>
      navigator.serviceWorker.removeEventListener("controllerchange", changed);
  }, []);
  async function refreshMe() {
    try {
      const user = await api<Me>("/me");
      setCsrf(user.csrf);
      setMe(user);
      if (user.admin && !user.configured) setPage("settings");
    } catch (e) {
      if (e instanceof ApiError && e.status === 401) {
        setMe(undefined);
        setCsrf("");
      } else throw e;
    }
  }
  async function initialize() {
    setInitial(true);
    setInitialError("");
    try {
      await refreshMe();
    } catch (e) {
      setInitialError((e as Error).message);
    } finally {
      setInitial(false);
    }
  }
  useEffect(() => {
    void initialize();
    const expired = () => {
      setMe(undefined);
      setCsrf("");
    };
    window.addEventListener("bucket-session-expired", expired);
    return () => window.removeEventListener("bucket-session-expired", expired);
  }, []);
  async function signedIn() {
    await refreshMe();
  }
  async function logout() {
    try {
      await api("/logout", "POST");
      setMe(undefined);
      setCsrf("");
      setPage("addresses");
    } catch (e) {
      notify((e as Error).message);
    }
  }
  return (
    <>
      <header className="app-header">
        <div className="header-inner">
          <a href="/" className="brand" aria-label="Bucket home">
            <img src="/icon.svg" alt="" />
            Bucket
          </a>
          {me && (
            <nav aria-label="Main navigation">
              <button
                className={page === "addresses" ? "active" : ""}
                onClick={() => setPage("addresses")}
              >
                <Mail size={17} />
                <span>Addresses</span>
              </button>
              {me.admin && (
                <button
                  className={page === "settings" ? "active" : ""}
                  onClick={() => setPage("settings")}
                >
                  <Settings size={17} />
                  <span>Settings</span>
                </button>
              )}
            </nav>
          )}
          <div className="header-end">
            {install && (
              <button
                className="icon-button"
                aria-label="Install Bucket"
                onClick={async () => {
                  await install.prompt();
                  await install.userChoice;
                  setInstall(undefined);
                }}
              >
                <Download size={18} />
              </button>
            )}
            <Dropdown.Root>
              <Dropdown.Trigger
                className="icon-button theme-button"
                aria-label="Choose theme"
              >
                {theme === "dark" ? (
                  <Moon size={18} />
                ) : theme === "light" ? (
                  <Sun size={18} />
                ) : (
                  <Monitor size={18} />
                )}
              </Dropdown.Trigger>
              <Dropdown.Portal>
                <Dropdown.Content className="menu" align="end" sideOffset={10}>
                  {[
                    ["system", "System", Monitor],
                    ["light", "Light", Sun],
                    ["dark", "Dark", Moon],
                  ].map(([value, label, Icon]) => (
                    <Dropdown.Item
                      key={value as string}
                      onSelect={() => setTheme(value as string)}
                    >
                      {typeof Icon !== "string" && <Icon size={16} />}
                      <span>{label as string}</span>
                      {theme === value && <Check size={14} />}
                    </Dropdown.Item>
                  ))}
                </Dropdown.Content>
              </Dropdown.Portal>
            </Dropdown.Root>
            {me && (
              <Dropdown.Root>
                <Dropdown.Trigger
                  className="account-trigger"
                  aria-label="Account menu"
                >
                  <span className="avatar">{me.email[0].toUpperCase()}</span>
                  <ChevronDown size={14} />
                </Dropdown.Trigger>
                <Dropdown.Portal>
                  <Dropdown.Content
                    className="menu account-menu"
                    align="end"
                    sideOffset={10}
                  >
                    <Dropdown.Label>{me.email}</Dropdown.Label>
                    <Dropdown.Separator />
                    <Dropdown.Item onSelect={() => void logout()}>
                      <LogOut size={16} />
                      Sign out
                    </Dropdown.Item>
                  </Dropdown.Content>
                </Dropdown.Portal>
              </Dropdown.Root>
            )}
          </div>
        </div>
      </header>
      {!online && (
        <div className="offline-banner" role="status">
          You’re offline. Connect to manage your addresses.
        </div>
      )}
      {update && (
        <div className="update-banner">
          An update is available.
          <button
            className="text-button"
            onClick={() => update.waiting?.postMessage("SKIP_WAITING")}
          >
            Reload
          </button>
        </div>
      )}
      {initial ? (
        <div className="app-loading">
          <Busy />
        </div>
      ) : initialError ? (
        <main className="login-wrap">
          <div className="login-card">
            <h1>Could not connect</h1>
            <ErrorMessage message={initialError} />
            <button
              className="button primary"
              onClick={() => void initialize()}
            >
              Try again
            </button>
          </div>
        </main>
      ) : me ? (
        page === "settings" && me.admin ? (
          <Admin refreshMe={refreshMe} notify={notify} />
        ) : (
          <Dashboard
            key={me.email}
            me={me}
            refreshMe={refreshMe}
            notify={notify}
          />
        )
      ) : (
        <Login onSignedIn={signedIn} />
      )}
      <div className="toast-region" aria-live="polite" aria-atomic="true">
        {toast && (
          <div className="toast">
            <Check size={17} />
            <span>{toast}</span>
            <button
              className="icon-button"
              aria-label="Dismiss notification"
              onClick={() => setToast("")}
            >
              <X size={16} />
            </button>
          </div>
        )}
      </div>
      <footer className="app-footer">Bucket</footer>
    </>
  );
}
function Login({ onSignedIn }: { onSignedIn: () => Promise<void> }) {
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  async function submit(e: React.FormEvent) {
    e.preventDefault();
    setError("");
    setBusy(true);
    try {
      const data = await api<{ csrf: string }>("/login", "POST", {
        email,
        password,
      });
      setPassword("");
      setCsrf(data.csrf);
      await onSignedIn();
    } catch (e) {
      setPassword("");
      setError((e as Error).message);
    } finally {
      setBusy(false);
    }
  }
  return (
    <main className="login-wrap">
      <section className="login-card">
        <span className="login-mark">
          <Mail size={25} />
        </span>
        <h1>Sign in to Bucket</h1>
        <p className="login-description">Use your email account.</p>
        <form onSubmit={submit}>
          <fieldset disabled={busy}>
            <label>
              Email address
              <input
                autoFocus
                type="email"
                required
                autoComplete="username"
                autoCapitalize="none"
                spellCheck={false}
                placeholder="you@example.com"
                value={email}
                onChange={(e) => setEmail(e.target.value)}
              />
            </label>
            <div className="label-row">
              <label htmlFor="login-password">Password</label>
              <InfoButton>
                Use your Mailcow mailbox password or an IMAP app password. Your
                account must have access to Bucket. Browser-based sign-in and
                one-time codes are not supported.
              </InfoButton>
            </div>
            <input
              id="login-password"
              type="password"
              required
              autoComplete="current-password"
              value={password}
              onChange={(e) => setPassword(e.target.value)}
              maxLength={1024}
            />
          </fieldset>
          <ErrorMessage message={error} />
          <button className="button primary full" disabled={busy}>
            {busy ? <Busy /> : null}Sign in
          </button>
        </form>
      </section>
    </main>
  );
}

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
