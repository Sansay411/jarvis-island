export function settingsPlatformCopy(userAgent = typeof navigator === "undefined" ? "" : navigator.userAgent) {
  const mac = /Macintosh|Mac OS X/i.test(userAgent);
  return {
    keyStoreLocation: mac ? "Связке ключей macOS" : "диспетчере учётных данных Windows",
    startup: mac ? "Вход при запуске macOS" : "Запускать вместе с Windows",
    localTts: mac ? "Supertonic / Piper" : "Supertonic / Piper / Windows",
  };
}
