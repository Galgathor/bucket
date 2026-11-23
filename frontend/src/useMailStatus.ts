import { useCallback, useEffect, useRef, useState } from "react";
import { api } from "./api";
import { MAIL_REFRESH_MS, type MailAddress } from "./mail";
export function useMailStatus(email: string | undefined) {
  const [addresses, setAddresses] = useState<MailAddress[]>([]);
  const generation = useRef(0);
  const pending = useRef(false);
  const refresh = useCallback(async () => {
    if (!email || pending.current) return;
    const seq = generation.current;
    pending.current = true;
    try {
      const data = await api<{ addresses: MailAddress[] }>("/mail/status");
      if (seq === generation.current && Array.isArray(data.addresses))
        setAddresses(data.addresses);
    } catch {
      if (seq === generation.current) setAddresses([]);
    } finally {
      if (seq === generation.current) pending.current = false;
    }
  }, [email]);
  useEffect(() => {
    generation.current++;
    pending.current = false;
    setAddresses([]);
    if (!email) return;
    void refresh();
    const visible = () => {
      if (!document.hidden) void refresh();
    };
    const timer = setInterval(visible, MAIL_REFRESH_MS);
    document.addEventListener("visibilitychange", visible);
    return () => {
      generation.current++;
      pending.current = false;
      clearInterval(timer);
      document.removeEventListener("visibilitychange", visible);
    };
  }, [email, refresh]);
  return { mailStatus: addresses, refreshMailStatus: refresh };
}
