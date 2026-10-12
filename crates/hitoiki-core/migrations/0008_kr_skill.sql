-- KR に付けるスキル。このスキル（子スキルを含む）が付いたタスク、または活動ルールでこのスキルが付く活動の
-- 時間が、その KR の時間になる（タスクとの紐付けは補助）。
CREATE TABLE kr_skill (
  kr_id    TEXT NOT NULL REFERENCES key_result(id),
  skill_id TEXT NOT NULL REFERENCES skill(id),
  PRIMARY KEY (kr_id, skill_id)
);
CREATE INDEX idx_kr_skill_skill ON kr_skill(skill_id);
