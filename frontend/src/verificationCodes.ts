// Deliberately conservative: a short token needs a nearby code label, or a
// standalone line in a message whose subject describes a verification code.
const labels = /\b(?:code|passcode|otp|pin|one[ -]time password)\b/gi;
const tokens =
  /\b(?:\d{3}[ -]\d{3}|\d{4}[ -]\d{4}|[A-Z0-9]{4}-[A-Z0-9]{4}|[A-Za-z0-9]{4,10})\b/g;
const unrelated =
  /\b(?:order|invoice|reference|tracking|phone|telephone|call|contact|copyright|total|amount|expires?|minutes?|seconds?)\b/i;

export function findVerificationCodes(
  subject: string,
  content: string,
): string[] {
  const subjectHasLabel =
    /\b(?:code|passcode|otp|pin|one[ -]time password)\b/i.test(subject);
  const found = new Set<string>();
  for (const source of [subject, content.slice(0, 128 * 1024)]) {
    // Keep offsets while ignoring links and email addresses.
    const text = source.replace(
      /https?:\/\/\S+|\b[\w.+-]{1,64}@[\w.-]{1,255}\b/gi,
      (s) => " ".repeat(s.length),
    );
    const lines = subjectHasLabel
      ? new Set(text.split("\n").map((line) => line.trim()))
      : undefined;
    for (const match of text.matchAll(tokens)) {
      const value = match[0];
      const index = match.index;
      const compact = value.replace(/[ -]/g, "");
      const numeric = /^\d{4,8}$/.test(compact);
      const mixed =
        /\d/.test(compact) && /[A-Za-z]/.test(compact) && compact.length >= 6;
      const groupedLetters = /^[A-Z]{4}-[A-Z]{4}$/.test(value);
      if (!numeric && !mixed && !groupedLetters) continue;
      // Reject fragments of phone numbers, dates, decimal amounts, and IDs.
      if (
        /[\w@/:+-]/.test(text[index - 1] ?? "") ||
        /[\w@/:+-]/.test(text[index + value.length] ?? "") ||
        /\w\.$/.test(text.slice(Math.max(0, index - 2), index)) ||
        /^\.\w/.test(text.slice(index + value.length, index + value.length + 2))
      )
        continue;
      // Search only the local context, so even a large email with many code
      // labels cannot turn detection into quadratic work on the UI thread.
      const windowStart = Math.max(0, index - 100);
      const context = text.slice(windowStart, index + value.length + 100);
      const nearest = [...context.matchAll(labels)]
        .map((label) => {
          const position = windowStart + label.index;
          const start =
            position < index
              ? position + label[0].length
              : index + value.length;
          const end = position < index ? index : position;
          const gap = text.slice(start, end);
          // A label after the token should describe it, not a later sentence.
          if (position > index && !/^\s*(?:is\s+)?(?:your|the)\b/i.test(gap))
            return undefined;
          return gap;
        })
        .filter((gap): gap is string => gap !== undefined)
        .sort((a, b) => a.length - b.length)[0];
      const labeled =
        nearest !== undefined &&
        nearest.length <= 60 &&
        !unrelated.test(nearest);
      if (labeled || lines?.has(value)) found.add(value);
      if (found.size === 3) return [...found];
    }
  }
  return [...found];
}

// A template is inert: never insert email markup into the live document to
// extract its text. In particular, links, scripts, and images cannot execute.
export function codeTextFromHtml(html: string): string {
  const template = document.createElement("template");
  template.innerHTML = html;
  template.content
    .querySelectorAll(
      "script,style,head,template,noscript,[hidden],[aria-hidden='true']",
    )
    .forEach((node) => node.remove());
  template.content
    .querySelectorAll("br,p,div,td,th,li,tr,h1,h2,h3,section")
    .forEach((node) => node.appendChild(document.createTextNode("\n")));
  return template.content.textContent ?? "";
}
