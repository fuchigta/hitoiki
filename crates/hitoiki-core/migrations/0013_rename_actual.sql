-- 実績まわりの物理名を用語集（GLOSSARY.md）に揃える（docs/adr/0001-physical-names-follow-glossary.md）。
-- 確定（segment_override → segment_confirmation）、PC 外（offline → off_pc）、活動（activity_label → activity）、
-- AI の作業待ち（agent_watch → agent_wait）、判定の根拠（decided_by の user → confirmation）、
-- タスクの割り当てルール（task_match_rule → task_assign_rule）、設定の [review] → [actual] に伴うキー。

-- ===== 確定 =====
ALTER TABLE segment_override RENAME TO segment_confirmation;
DROP INDEX idx_segment_override_start;
DROP INDEX idx_segment_override_correction;
CREATE INDEX idx_segment_confirmation_start ON segment_confirmation(start_at);
CREATE INDEX idx_segment_confirmation_correction ON segment_confirmation(correction_id);
ALTER TABLE segment_confirmation RENAME COLUMN offline TO off_pc;
ALTER TABLE segment_confirmation RENAME COLUMN activity_label TO activity;

-- ===== 区間（派生層）=====
ALTER TABLE activity_segment RENAME COLUMN offline TO off_pc;
ALTER TABLE activity_segment RENAME COLUMN activity_label TO activity;
UPDATE activity_segment SET kind = 'agent_wait' WHERE kind = 'agent_watch';
UPDATE activity_segment SET decided_by = 'confirmation' WHERE decided_by = 'user';

-- ===== 補正 =====
ALTER TABLE segment_correction RENAME COLUMN offline_ms TO off_pc_ms;
ALTER TABLE segment_correction RENAME COLUMN activity_label TO activity;
UPDATE segment_correction SET kind = 'agent_wait' WHERE kind = 'agent_watch';
UPDATE segment_confirmation SET kind = 'agent_wait' WHERE kind = 'agent_watch';

-- ===== 活動ルール =====
ALTER TABLE activity_rule RENAME COLUMN activity_label TO activity;

-- ===== タスクの割り当てルール =====
-- pattern_type の既定値を、CLI・サービスの既定（glob）に揃えるため、テーブルを作り直す。
CREATE TABLE task_assign_rule (
  id           TEXT PRIMARY KEY,
  task_id      TEXT NOT NULL REFERENCES task(id),
  field        TEXT NOT NULL,       -- exe|title|cwd|repo|url|git_branch
  pattern      TEXT NOT NULL,
  pattern_type TEXT NOT NULL DEFAULT 'glob', -- glob|regex|contains
  priority     INTEGER NOT NULL DEFAULT 0,
  enabled      INTEGER NOT NULL DEFAULT 1
);
INSERT INTO task_assign_rule (id, task_id, field, pattern, pattern_type, priority, enabled)
  SELECT id, task_id, field, pattern, pattern_type, priority, enabled FROM task_match_rule;
DROP TABLE task_match_rule;

-- ===== 設定のキー（[review] → [actual]）=====
UPDATE setting SET key = 'actual.rule_suggest_dismissed' WHERE key = 'review.rule_suggest_dismissed';
UPDATE setting SET key = 'actual.ignore_suggest_dismissed' WHERE key = 'review.ignore_suggest_dismissed';

-- ===== 自動判定のキャッシュ =====
-- 答えの JSON のキー名（activity_label → activity）が変わるので、作り直せるキャッシュは捨てる。
DELETE FROM judge_cache;
