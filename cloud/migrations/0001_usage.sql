-- Daily counters: per device (uuid), per IP (ip:...) and the whole service (global).
CREATE TABLE IF NOT EXISTS usage (
  day TEXT NOT NULL,
  subject TEXT NOT NULL,
  kind TEXT NOT NULL,
  amount REAL NOT NULL DEFAULT 0,
  PRIMARY KEY (day, subject, kind)
);
