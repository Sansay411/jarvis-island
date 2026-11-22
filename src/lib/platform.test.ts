import { describe, expect, it } from "vitest";
import { settingsPlatformCopy } from "./platform";

describe("settingsPlatformCopy", () => {
  it("uses Keychain and macOS login wording for a Mac webview", () => {
    const copy = settingsPlatformCopy("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15");
    expect(copy.keyStoreLocation).toBe("Связке ключей macOS");
    expect(copy.startup).toBe("Вход при запуске macOS");
    expect(copy.localTts).not.toContain("Windows");
  });
  it("preserves Windows wording and the existing fallback", () => {
    for (const ua of ["Mozilla/5.0 (Windows NT 10.0; Win64; x64)", "", "unknown"]) {
      const copy = settingsPlatformCopy(ua);
      expect(copy.keyStoreLocation).toBe("диспетчере учётных данных Windows");
      expect(copy.startup).toBe("Запускать вместе с Windows");
      expect(copy.localTts).toBe("Supertonic / Piper / Windows");
    }
  });
});
