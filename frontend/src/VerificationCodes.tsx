import { useMemo, useState } from "react";
import { Check, Copy } from "lucide-react";
import { codeTextFromHtml, findVerificationCodes } from "./verificationCodes";
import type { MessageBody } from "./mail";

export function VerificationCodes({
  message,
  notify,
}: {
  message: MessageBody;
  notify: (message: string) => void;
}) {
  const codes = useMemo(
    () =>
      findVerificationCodes(
        message.message.subject,
        message.text +
          (message.html ? "\n" + codeTextFromHtml(message.html) : ""),
      ),
    [message.message.subject, message.text, message.html],
  );
  const [copied, setCopied] = useState("");
  async function copy(code: string) {
    try {
      await navigator.clipboard.writeText(code);
      setCopied(code);
      notify("Code copied.");
    } catch {
      notify("Could not copy the code. Select it and copy it manually.");
    }
  }
  if (!codes.length) return null;
  return (
    <div
      className="verification-codes"
      role="group"
      aria-label="Verification codes"
    >
      {codes.map((code) => (
        <div className="verification-code" key={code}>
          <span>Verification code</span>
          <code>{code}</code>
          <button
            className="button secondary small"
            aria-label={`Copy code ${code}`}
            onClick={() => void copy(code)}
          >
            {copied === code ? <Check size={15} /> : <Copy size={15} />}
            {copied === code ? "Copied" : "Copy code"}
          </button>
        </div>
      ))}
    </div>
  );
}
