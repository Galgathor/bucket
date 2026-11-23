import { describe, expect, it } from "vitest";
import { findVerificationCodes } from "./verificationCodes";

describe("verification code detection", () => {
  it("finds numeric codes and preserves leading zeroes", () => {
    expect(
      findVerificationCodes(
        "Sign in",
        "Your verification code is 012345. Do not share it.",
      ),
    ).toEqual(["012345"]);
    expect(findVerificationCodes("Your code", "0042")).toEqual(["0042"]);
  });
  it("finds codes in subjects, mixed tokens, and grouped codes", () => {
    expect(
      findVerificationCodes("123456 is your verification code", ""),
    ).toEqual(["123456"]);
    expect(
      findVerificationCodes("", "Use passcode A7C9F2 to sign in."),
    ).toEqual(["A7C9F2"]);
    expect(findVerificationCodes("", "Your code: 123 456")).toEqual([
      "123 456",
    ]);
    expect(findVerificationCodes("", "Your recovery code: ABCD-EFGH")).toEqual([
      "ABCD-EFGH",
    ]);
  });
  it("does not surface routine numbers, addresses, links, dates, or phone fragments", () => {
    expect(
      findVerificationCodes(
        "Your receipt",
        "Order 123456 costs $1234.50. Call +1-555-123-4567 on 2026-11-23.",
      ),
    ).toEqual([]);
    expect(
      findVerificationCodes(
        "Verification code",
        "Visit https://example.com/code/123456 or contact user123456@example.net. Order 987654. Call 5551234.",
      ),
    ).toEqual([]);
    expect(
      findVerificationCodes(
        "",
        "Your code is 123456. It expires in 2026 minutes. Call 5551234 for help.",
      ),
    ).toEqual(["123456"]);
  });
  it("deduplicates and limits suggestions", () => {
    expect(
      findVerificationCodes(
        "Code 123456",
        "Your code is 123456. Backup code: 654321.",
      ),
    ).toEqual(["123456", "654321"]);
    expect(
      findVerificationCodes(
        "",
        "Code 111111. Code 222222. Code 333333. Code 444444.",
      ),
    ).toHaveLength(3);
  });
  it("ignores long identifiers and code-like tokens without a nearby label", () => {
    expect(
      findVerificationCodes(
        "Newsletter",
        "1234567890 and abc123xyz. A code will be sent separately.",
      ),
    ).toEqual([]);
    expect(
      findVerificationCodes(
        "Newsletter",
        "Order 123456.\n" +
          "Some other information. ".repeat(10) +
          "Your code will follow.",
      ),
    ).toEqual([]);
  });
});
