/* Pro waitlist: one row per address. */
CREATE TABLE waitlist (
  email TEXT PRIMARY KEY,
  plan TEXT NOT NULL,
  lang TEXT NOT NULL,
  country TEXT,
  created_at TEXT NOT NULL
);

/* Downloads per UTC day and platform, nothing about the visitor. */
CREATE TABLE downloads (
  day TEXT NOT NULL,
  platform TEXT NOT NULL,
  count INTEGER NOT NULL DEFAULT 0,
  PRIMARY KEY (day, platform)
);
