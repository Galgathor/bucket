import { describe, it, expect } from "vitest";
import { generatePassword, randomLocal } from "./api";
describe("Generated credentials", () => {
  it("generates distinct strong passwords using browser cryptography", () => {
    const passwords = Array.from({ length: 100 }, generatePassword);
    expect(new Set(passwords).size).toBe(100);
    for (const password of passwords) {
      expect(password.length).toBeGreaterThanOrEqual(24);
      expect(password).toMatch(/[A-Z]/);
      expect(password).toMatch(/[a-z]/);
      expect(password).toMatch(/[0-9]/);
      expect(password).toMatch(/[^A-Za-z0-9]/);
    }
  });
  it("generates distinct valid alias names", () => {
    const names = Array.from({ length: 100 }, randomLocal);
    expect(new Set(names).size).toBe(100);
    expect(names.every((n) => /^[a-z0-9]{16}$/.test(n))).toBe(true);
  });
});
