-- Hitoiki 初期スキーマ（docs/architecture.md「データモデル」）
-- 時刻はすべて UTC の Unix ミリ秒（INTEGER）。ID は ULID（TEXT）。

-- ===== 生データ層 =====

CREATE TABLE window_span (
  id         TEXT PRIMARY KEY,
  start_at   INTEGER NOT NULL,
  end_at     INTEGER,               -- NULL = 進行中
  hwnd       TEXT,
  pid        INTEGER,
  exe_path   TEXT,
  exe_name   TEXT,
  title      TEXT,
  excluded   INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX idx_window_span_start ON window_span(start_at);

CREATE TABLE idle_span (
  id       TEXT PRIMARY KEY,
  start_at INTEGER NOT NULL,
  end_at   INTEGER,                 -- NULL = 進行中
  reason   TEXT NOT NULL            -- 'no_input' | 'locked' | 'sleep'
);
CREATE INDEX idx_idle_span_start ON idle_span(start_at);

CREATE TABLE agent_event (
  id             TEXT PRIMARY KEY,
  at             INTEGER NOT NULL,
  agent          TEXT NOT NULL,
  session_id     TEXT NOT NULL,
  kind           TEXT NOT NULL,     -- 'session_start' | 'prompt_submit' | 'response_complete' | 'session_end'
  cwd            TEXT,
  git_branch     TEXT,
  prompt_excerpt TEXT,
  source_file    TEXT,
  source_offset  INTEGER
);
CREATE INDEX idx_agent_event_at ON agent_event(at);
CREATE INDEX idx_agent_event_session ON agent_event(agent, session_id, at);
CREATE UNIQUE INDEX uq_agent_event_dedup ON agent_event(agent, session_id, kind, at);

-- エージェントログの取り込み位置（docs/architecture.md「エージェントアダプタ」）
CREATE TABLE agent_ingest_offset (
  source_file TEXT PRIMARY KEY,
  agent       TEXT NOT NULL,
  offset      INTEGER NOT NULL,
  updated_at  INTEGER NOT NULL
);

CREATE TABLE agent_session_window (
  session_id    TEXT NOT NULL,
  agent         TEXT NOT NULL,
  hwnd          TEXT NOT NULL,
  title_at_bind TEXT,
  bound_at      INTEGER NOT NULL,
  PRIMARY KEY (agent, session_id, bound_at)
);

CREATE TABLE context_hint (
  id     TEXT PRIMARY KEY,
  at     INTEGER NOT NULL,
  source TEXT NOT NULL,
  hwnd   TEXT,
  kind   TEXT NOT NULL,
  value  TEXT NOT NULL
);
CREATE INDEX idx_context_hint_at ON context_hint(at);

CREATE TABLE calendar_event (
  id          TEXT PRIMARY KEY,
  source      TEXT NOT NULL,
  external_id TEXT NOT NULL,
  start_at    INTEGER NOT NULL,
  end_at      INTEGER NOT NULL,
  all_day     INTEGER NOT NULL DEFAULT 0,
  title       TEXT NOT NULL,
  location    TEXT,
  is_busy     INTEGER NOT NULL DEFAULT 1,
  fetched_at  INTEGER NOT NULL
);
CREATE UNIQUE INDEX uq_calendar_event ON calendar_event(source, external_id, start_at);
CREATE INDEX idx_calendar_event_start ON calendar_event(start_at);

-- ===== 派生層 =====

CREATE TABLE activity_segment (
  id               TEXT PRIMARY KEY,
  start_at         INTEGER NOT NULL,
  end_at           INTEGER NOT NULL,
  kind             TEXT NOT NULL,   -- work|agent_dialog|agent_watch|meeting|idle|break|interrupt
  task_id          TEXT,
  activity_label   TEXT,
  confidence       REAL,
  decided_by       TEXT,            -- rule|judge:<name>|calendar|user|doing
  status           TEXT NOT NULL,   -- auto|needs_review|confirmed
  window_span_ids  TEXT NOT NULL DEFAULT '[]',
  agent_session_id TEXT,
  exe_name         TEXT,            -- 表示・判定用の非正規化（根拠ウインドウの代表値）
  title            TEXT
);
CREATE INDEX idx_activity_segment_start ON activity_segment(start_at);

-- ===== ユーザー層 =====

CREATE TABLE objective (
  id             TEXT PRIMARY KEY,
  title          TEXT NOT NULL,
  description_md TEXT,
  period_start   INTEGER,
  period_end     INTEGER,
  status         TEXT NOT NULL DEFAULT 'active'
);

CREATE TABLE key_result (
  id           TEXT PRIMARY KEY,
  objective_id TEXT NOT NULL REFERENCES objective(id),
  title        TEXT NOT NULL,
  target       TEXT,
  current      TEXT,
  status       TEXT NOT NULL DEFAULT 'active'
);

CREATE TABLE task (
  id               TEXT PRIMARY KEY,
  title            TEXT NOT NULL,
  body_md          TEXT NOT NULL DEFAULT '',
  status           TEXT NOT NULL DEFAULT 'todo',   -- todo|doing|done|dropped
  kind             TEXT NOT NULL DEFAULT 'normal', -- normal|interrupt
  urgent           INTEGER,
  important        INTEGER,
  estimate_min     INTEGER,
  planned_start_at INTEGER,
  planned_end_at   INTEGER,
  key_result_id    TEXT REFERENCES key_result(id),
  external_url     TEXT,
  created_at       INTEGER NOT NULL,
  updated_at       INTEGER NOT NULL,
  done_at          INTEGER
);

-- doing の履歴（docs/architecture.md「帰属判定パイプライン」で「その時点の doing タスク」を再計算可能にするため）
CREATE TABLE task_doing_log (
  id       TEXT PRIMARY KEY,
  task_id  TEXT NOT NULL REFERENCES task(id),
  start_at INTEGER NOT NULL,
  end_at   INTEGER
);
CREATE INDEX idx_task_doing_log_start ON task_doing_log(start_at);

CREATE TABLE task_match_rule (
  id           TEXT PRIMARY KEY,
  task_id      TEXT NOT NULL REFERENCES task(id),
  field        TEXT NOT NULL,       -- exe|title|cwd|repo|url|git_branch
  pattern      TEXT NOT NULL,
  pattern_type TEXT NOT NULL DEFAULT 'contains', -- glob|regex|contains
  priority     INTEGER NOT NULL DEFAULT 0,
  enabled      INTEGER NOT NULL DEFAULT 1
);

CREATE TABLE activity_rule (
  id             TEXT PRIMARY KEY,
  field          TEXT NOT NULL,
  pattern        TEXT NOT NULL,
  pattern_type   TEXT NOT NULL DEFAULT 'contains',
  activity_label TEXT NOT NULL,
  priority       INTEGER NOT NULL DEFAULT 0,
  enabled        INTEGER NOT NULL DEFAULT 1
);

CREATE TABLE segment_override (
  id             TEXT PRIMARY KEY,
  start_at       INTEGER NOT NULL,
  end_at         INTEGER NOT NULL,
  kind           TEXT,
  task_id        TEXT,
  activity_label TEXT,
  created_at     INTEGER NOT NULL
);
CREATE INDEX idx_segment_override_start ON segment_override(start_at);

CREATE TABLE time_block (
  id       TEXT PRIMARY KEY,
  start_at INTEGER NOT NULL,
  end_at   INTEGER NOT NULL,
  task_id  TEXT,
  label    TEXT,
  focus    INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX idx_time_block_start ON time_block(start_at);

CREATE TABLE skill (
  id          TEXT PRIMARY KEY,
  name        TEXT NOT NULL UNIQUE,
  parent_id   TEXT REFERENCES skill(id),
  description TEXT,
  preset      INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE task_skill (
  task_id  TEXT NOT NULL REFERENCES task(id),
  skill_id TEXT NOT NULL REFERENCES skill(id),
  PRIMARY KEY (task_id, skill_id)
);

CREATE TABLE activity_skill_rule (
  id           TEXT PRIMARY KEY,
  field        TEXT NOT NULL,
  pattern      TEXT NOT NULL,
  pattern_type TEXT NOT NULL DEFAULT 'contains',
  skill_id     TEXT NOT NULL REFERENCES skill(id)
);

CREATE TABLE checkin (
  id      TEXT PRIMARY KEY,
  at      INTEGER NOT NULL,
  energy  INTEGER,
  mood    INTEGER,
  stress  INTEGER,
  erg_tag TEXT,
  note    TEXT,
  push_id TEXT
);

CREATE TABLE hassle (
  id         TEXT PRIMARY KEY,
  at         INTEGER NOT NULL,
  text       TEXT NOT NULL,
  intensity  INTEGER NOT NULL DEFAULT 3,
  segment_id TEXT,
  push_id    TEXT
);

CREATE TABLE abc_record (
  id                    TEXT PRIMARY KEY,
  at                    INTEGER NOT NULL,
  activating_event      TEXT NOT NULL,
  belief                TEXT,
  consequence_emotion   TEXT,
  consequence_intensity INTEGER,
  dispute               TEXT,
  ai_dialog_md          TEXT,
  hassle_id             TEXT
);

CREATE TABLE reflection (
  id           TEXT PRIMARY KEY,
  at           INTEGER NOT NULL,
  kind         TEXT NOT NULL,       -- day_start|day_end|weekly
  context_json TEXT NOT NULL,
  questions_md TEXT NOT NULL,
  answers_md   TEXT,
  summary_md   TEXT
);

CREATE TABLE push_log (
  id                TEXT PRIMARY KEY,
  at                INTEGER NOT NULL,
  trigger           TEXT NOT NULL,
  payload_json      TEXT NOT NULL DEFAULT '{}',
  delivered         INTEGER NOT NULL DEFAULT 0,
  responded_at      INTEGER,
  response_json     TEXT,
  snoozed_until     INTEGER,
  suppressed_reason TEXT
);
CREATE INDEX idx_push_log_at ON push_log(at);

CREATE TABLE setting (
  key        TEXT PRIMARY KEY,
  value_json TEXT NOT NULL
);
