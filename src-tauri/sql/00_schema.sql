-- Simplified schema: Authors and their patches

CREATE EXTENSION IF NOT EXISTS citext;

-- Authors (email senders)
CREATE TABLE IF NOT EXISTS authors (
  author_id     BIGSERIAL PRIMARY KEY,
  name          TEXT,
  email         CITEXT NOT NULL UNIQUE,
  first_seen    TIMESTAMPTZ DEFAULT NOW(),
  patch_count   INT DEFAULT 0
);

-- Patches (emails that are patches)
CREATE TABLE IF NOT EXISTS patches (
  patch_id      BIGSERIAL PRIMARY KEY,
  author_id     BIGINT NOT NULL REFERENCES authors(author_id) ON DELETE CASCADE,
  message_id    TEXT NOT NULL UNIQUE,
  subject       TEXT NOT NULL,
  sent_at       TIMESTAMPTZ NOT NULL,
  commit_hash   TEXT,           -- The git commit hash where this patch was stored
  body_text     TEXT,
  is_series     BOOLEAN DEFAULT FALSE,
  series_number INT,            -- e.g., 1/3, 2/3, 3/3
  series_total  INT,
  created_at    TIMESTAMPTZ DEFAULT NOW()
);

-- Indexes for fast queries
CREATE INDEX IF NOT EXISTS patches_author_id_idx ON patches (author_id);
CREATE INDEX IF NOT EXISTS patches_sent_at_idx ON patches (sent_at DESC);
CREATE INDEX IF NOT EXISTS patches_subject_idx ON patches USING GIN (to_tsvector('english', subject));
CREATE INDEX IF NOT EXISTS authors_email_idx ON authors (email);
CREATE INDEX IF NOT EXISTS authors_patch_count_idx ON authors (patch_count DESC);

-- Trigger function to update author patch_count
CREATE OR REPLACE FUNCTION update_author_patch_count()
RETURNS TRIGGER AS $$
BEGIN
  IF TG_OP = 'INSERT' THEN
    UPDATE authors SET patch_count = patch_count + 1 WHERE author_id = NEW.author_id;
    RETURN NEW;
  ELSIF TG_OP = 'DELETE' THEN
    UPDATE authors SET patch_count = patch_count - 1 WHERE author_id = OLD.author_id;
    RETURN OLD;
  END IF;
  RETURN NULL;
END;
$$ LANGUAGE plpgsql;

-- Trigger to automatically update patch_count when patches are inserted or deleted
DROP TRIGGER IF EXISTS trigger_update_author_patch_count ON patches;
CREATE TRIGGER trigger_update_author_patch_count
  AFTER INSERT OR DELETE ON patches
  FOR EACH ROW
  EXECUTE FUNCTION update_author_patch_count();

-- Initialize patch_count for existing authors
UPDATE authors a
SET patch_count = (SELECT COUNT(*) FROM patches p WHERE p.author_id = a.author_id);