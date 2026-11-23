export type Policy = {
  mailboxes: boolean;
  aliases: boolean;
  max_mailboxes: number;
  max_aliases: number;
  max_storage: number;
  default_storage: number;
  total_storage: number;
};
export type Me = {
  email: string;
  admin: boolean;
  csrf: string;
  configured: boolean;
  domains: string[];
  policy: Policy;
  destinations: string[];
  alias_suffix: boolean;
};
export type Address = {
  id: number;
  address: string;
  kind: "mailbox" | "alias";
  name: string;
  state: string;
  active: number;
  destination: string;
  storage: number;
  used: number;
};
export type User = { email: string; enabled: boolean; policy: Policy | null };
export type Ownership = {
  id: number;
  address: string;
  kind: string;
  owner: string;
  state: string;
  exists: boolean;
  verified: boolean;
  operation: string | null;
};
export type Audit = {
  at: number;
  actor: string;
  action: string;
  address: string;
  outcome: string;
};
let csrf = "";
export function setCsrf(value: string) {
  csrf = value;
}
export class ApiError extends Error {
  constructor(
    message: string,
    public status: number,
  ) {
    super(message);
  }
}
export async function api<T = Record<string, unknown>>(
  path: string,
  method = "GET",
  body?: unknown,
): Promise<T> {
  let response: Response;
  try {
    response = await fetch("/api" + path, {
      method,
      credentials: "same-origin",
      headers: {
        ...(body ? { "Content-Type": "application/json" } : {}),
        ...(method !== "GET" ? { "X-CSRF-Token": csrf } : {}),
      },
      body: body ? JSON.stringify(body) : undefined,
    });
  } catch {
    throw new ApiError(
      navigator.onLine
        ? "Could not connect. Try again shortly."
        : "You’re offline. Connect to manage your addresses.",
      0,
    );
  }
  let data;
  try {
    data = await response.json();
  } catch {
    throw new ApiError(
      "The service is unavailable. Try again shortly.",
      response.status,
    );
  }
  if (response.status === 401 && path !== "/login")
    window.dispatchEvent(new Event("bucket-session-expired"));
  if (!response.ok)
    throw new ApiError(
      typeof data.error === "string"
        ? data.error
        : "Could not save this change. Try again.",
      response.status,
    );
  return data as T;
}
export function generatePassword(): string {
  const alphabet =
    "ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz23456789!@#$%*-_";
  // Rejection sampling avoids modulo bias; ensure every server policy character class.
  let password = "";
  const threshold = 256 - (256 % alphabet.length);
  while (password.length < 24) {
    const bytes = crypto.getRandomValues(new Uint8Array(32));
    for (const b of bytes) {
      if (b < threshold) password += alphabet[b % alphabet.length];
      if (password.length === 24) break;
    }
  }
  return /[A-Z]/.test(password) &&
    /[a-z]/.test(password) &&
    /[0-9]/.test(password) &&
    /[!@#$%*_-]/.test(password)
    ? password
    : generatePassword();
}
export function randomLocal(): string {
  return Array.from(crypto.getRandomValues(new Uint8Array(8)), (b) =>
    b.toString(16).padStart(2, "0"),
  ).join("");
}
export function space(bytes: number): string {
  if (bytes === 0) return "0 MB";
  if (bytes >= 1073741824)
    return `${(bytes / 1073741824).toFixed(bytes % 1073741824 === 0 ? 0 : 1)} GB`;
  return `${Math.ceil(bytes / 1048576)} MB`;
}

export function aliasBase(name: string): string {
  return name
    .normalize("NFKD")
    .replace(/[\u0300-\u036f]/g, "")
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "");
}
export function aliasSuffix(): string {
  const alphabet = "abcdefghijklmnopqrstuvwxyz0123456789";
  let result = "";
  while (result.length < 3) {
    for (const b of crypto.getRandomValues(new Uint8Array(8))) {
      if (b < 252) result += alphabet[b % 36];
      if (result.length === 3) break;
    }
  }
  return result;
}
