-- Multi-email author schema with normalized names

CREATE EXTENSION IF NOT EXISTS citext;

-- Authors (one per person, identified by normalized name)
CREATE TABLE IF NOT EXISTS authors (
  author_id     BIGSERIAL PRIMARY KEY,
  first_name    TEXT NOT NULL,
  last_name     TEXT,
  display_name  TEXT NOT NULL,  -- Normalized "First Last" for display
  first_seen    TIMESTAMPTZ DEFAULT NOW(),
  patch_count   INT DEFAULT 0,
  UNIQUE(first_name, last_name)  -- Prevent duplicate authors
);

-- Author emails (many-to-one: one author can have multiple emails)
CREATE TABLE IF NOT EXISTS author_emails (
  email_id      BIGSERIAL PRIMARY KEY,
  author_id     BIGINT NOT NULL REFERENCES authors(author_id) ON DELETE CASCADE,
  email         CITEXT NOT NULL UNIQUE,
  is_primary    BOOLEAN DEFAULT FALSE,
  first_seen    TIMESTAMPTZ DEFAULT NOW()
);

-- Patches (emails that are patches)
CREATE TABLE IF NOT EXISTS patches (
  patch_id      BIGSERIAL PRIMARY KEY,
  author_id     BIGINT NOT NULL REFERENCES authors(author_id) ON DELETE CASCADE,
  email_id      BIGINT REFERENCES author_emails(email_id),  -- Which email was used
  message_id    TEXT NOT NULL UNIQUE,
  subject       TEXT NOT NULL,
  sent_at       TIMESTAMPTZ NOT NULL,
  commit_hash   TEXT,
  body_text     TEXT,
  is_series     BOOLEAN DEFAULT FALSE,
  series_number INT,
  series_total  INT,
  -- Threading fields
  in_reply_to       TEXT,              -- Message-ID of parent
  thread_references TEXT[],            -- Array of Message-IDs in thread chain
  is_reply          BOOLEAN DEFAULT FALSE,
  created_at        TIMESTAMPTZ DEFAULT NOW()
);

-- Threading tables

-- Thread metadata
CREATE TABLE IF NOT EXISTS patch_threads (
  thread_id         BIGSERIAL PRIMARY KEY,
  root_patch_id     BIGINT NOT NULL UNIQUE REFERENCES patches(patch_id) ON DELETE CASCADE,
  root_message_id   TEXT NOT NULL,
  subject_base      TEXT NOT NULL,  -- Normalized subject without Re:/Fwd: prefixes
  reply_count       INT DEFAULT 0,
  participant_count INT DEFAULT 0,
  created_at        TIMESTAMPTZ DEFAULT NOW(),
  updated_at        TIMESTAMPTZ DEFAULT NOW(),
  last_activity_at  TIMESTAMPTZ DEFAULT NOW()
);

-- Reply relationships (one row per patch, links to thread and parent)
CREATE TABLE IF NOT EXISTS patch_replies (
  reply_id           BIGSERIAL PRIMARY KEY,
  thread_id          BIGINT NOT NULL REFERENCES patch_threads(thread_id) ON DELETE CASCADE,
  patch_id           BIGINT NOT NULL UNIQUE REFERENCES patches(patch_id) ON DELETE CASCADE,
  parent_patch_id    BIGINT REFERENCES patches(patch_id) ON DELETE CASCADE,
  depth_level        INT NOT NULL DEFAULT 0,
  position_in_thread INT NOT NULL,
  thread_path        BIGINT[],  -- Materialized path [root_id, parent_id, ..., this_id]
  created_at         TIMESTAMPTZ DEFAULT NOW()
);

-- Thread participants (tracks who participated in each thread)
CREATE TABLE IF NOT EXISTS thread_participants (
  thread_id     BIGINT NOT NULL REFERENCES patch_threads(thread_id) ON DELETE CASCADE,
  author_id     BIGINT NOT NULL REFERENCES authors(author_id) ON DELETE CASCADE,
  reply_count   INT DEFAULT 0,
  first_replied TIMESTAMPTZ,
  last_replied  TIMESTAMPTZ,
  PRIMARY KEY (thread_id, author_id)
);

-- Indexes for fast queries
CREATE INDEX IF NOT EXISTS patches_author_id_idx ON patches (author_id);
CREATE INDEX IF NOT EXISTS patches_email_id_idx ON patches (email_id);
CREATE INDEX IF NOT EXISTS patches_sent_at_idx ON patches (sent_at DESC);
CREATE INDEX IF NOT EXISTS patches_subject_idx ON patches USING GIN (to_tsvector('english', subject));
CREATE INDEX IF NOT EXISTS patches_in_reply_to_idx ON patches (in_reply_to);
CREATE INDEX IF NOT EXISTS patches_is_reply_idx ON patches (is_reply);
CREATE INDEX IF NOT EXISTS author_emails_email_idx ON author_emails (email);
CREATE INDEX IF NOT EXISTS author_emails_author_id_idx ON author_emails (author_id);
CREATE INDEX IF NOT EXISTS authors_display_name_idx ON authors (display_name);
CREATE INDEX IF NOT EXISTS authors_patch_count_idx ON authors (patch_count DESC);

-- Threading indexes
CREATE INDEX IF NOT EXISTS patch_threads_root_patch_idx ON patch_threads (root_patch_id);
CREATE INDEX IF NOT EXISTS patch_threads_root_message_idx ON patch_threads (root_message_id);
CREATE INDEX IF NOT EXISTS patch_threads_last_activity_idx ON patch_threads (last_activity_at DESC);
CREATE INDEX IF NOT EXISTS patch_threads_subject_idx ON patch_threads USING GIN (to_tsvector('english', subject_base));
CREATE INDEX IF NOT EXISTS patch_replies_thread_idx ON patch_replies (thread_id);
CREATE INDEX IF NOT EXISTS patch_replies_patch_idx ON patch_replies (patch_id);
CREATE INDEX IF NOT EXISTS patch_replies_parent_idx ON patch_replies (parent_patch_id);
CREATE INDEX IF NOT EXISTS patch_replies_position_idx ON patch_replies (position_in_thread);
CREATE INDEX IF NOT EXISTS patch_replies_thread_position_idx ON patch_replies (thread_id, position_in_thread);
CREATE INDEX IF NOT EXISTS thread_participants_thread_idx ON thread_participants (thread_id);
CREATE INDEX IF NOT EXISTS thread_participants_author_idx ON thread_participants (author_id);

-- Helper view for thread summaries
CREATE OR REPLACE VIEW thread_summary AS
SELECT 
  pt.thread_id,
  pt.root_patch_id,
  pt.root_message_id,
  pt.reply_count,
  pt.participant_count,
  p.sent_at as created_at,  -- Use root patch sent_at as thread creation time
  pt.updated_at,
  pt.last_activity_at,
  p.subject as root_subject,
  p.sent_at as root_sent_at,
  a.display_name as root_author,
  a.author_id as root_author_id
FROM patch_threads pt
JOIN patches p ON pt.root_patch_id = p.patch_id
JOIN authors a ON p.author_id = a.author_id;

-- Function to update thread statistics
CREATE OR REPLACE FUNCTION update_thread_stats(p_thread_id BIGINT)
RETURNS VOID AS $$
BEGIN
  UPDATE patch_threads
  SET 
    reply_count = (SELECT COUNT(*) - 1 FROM patch_replies WHERE thread_id = p_thread_id),
    participant_count = (SELECT COUNT(DISTINCT p.author_id) 
                        FROM patch_replies pr 
                        JOIN patches p ON pr.patch_id = p.patch_id 
                        WHERE pr.thread_id = p_thread_id),
    last_activity_at = (SELECT MAX(p.sent_at) 
                       FROM patch_replies pr 
                       JOIN patches p ON pr.patch_id = p.patch_id 
                       WHERE pr.thread_id = p_thread_id),
    updated_at = NOW()
  WHERE thread_id = p_thread_id;
  
  -- Update thread_participants
  DELETE FROM thread_participants WHERE thread_id = p_thread_id;
  
  INSERT INTO thread_participants (thread_id, author_id, reply_count, first_replied, last_replied)
  SELECT 
    p_thread_id,
    p.author_id,
    COUNT(*) as reply_count,
    MIN(p.sent_at) as first_replied,
    MAX(p.sent_at) as last_replied
  FROM patch_replies pr
  JOIN patches p ON pr.patch_id = p.patch_id
  WHERE pr.thread_id = p_thread_id
  GROUP BY p.author_id;
END;
$$ LANGUAGE plpgsql;

-- Function to get direct replies to a patch
CREATE OR REPLACE FUNCTION get_direct_replies(p_patch_id BIGINT)
RETURNS TABLE (
  reply_patch_id BIGINT,
  reply_subject TEXT,
  reply_author TEXT,
  reply_sent_at TIMESTAMPTZ
) AS $$
BEGIN
  RETURN QUERY
  SELECT 
    p.patch_id,
    p.subject,
    a.display_name,
    p.sent_at
  FROM patch_replies pr
  JOIN patches p ON pr.patch_id = p.patch_id
  JOIN authors a ON p.author_id = a.author_id
  WHERE pr.parent_patch_id = p_patch_id
  ORDER BY p.sent_at ASC;
END;
$$ LANGUAGE plpgsql;

-- Function to get all messages in a thread
CREATE OR REPLACE FUNCTION get_thread_messages(p_thread_id BIGINT)
RETURNS TABLE (
  patch_id BIGINT,
  parent_patch_id BIGINT,
  depth_level INT,
  position_in_thread INT,
  subject TEXT,
  author_name TEXT,
  sent_at TIMESTAMPTZ,
  message_id TEXT
) AS $$
BEGIN
  RETURN QUERY
  SELECT 
    pr.patch_id,
    pr.parent_patch_id,
    pr.depth_level,
    pr.position_in_thread,
    p.subject,
    a.display_name,
    p.sent_at,
    p.message_id
  FROM patch_replies pr
  JOIN patches p ON pr.patch_id = p.patch_id
  JOIN authors a ON p.author_id = a.author_id
  WHERE pr.thread_id = p_thread_id
  ORDER BY pr.position_in_thread ASC;
END;
$$ LANGUAGE plpgsql;