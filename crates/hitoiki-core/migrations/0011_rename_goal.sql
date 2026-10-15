-- 物理名を用語集に揃える（目標・成果の指標・数え方・紐づけの提案）。
-- 目標: objective -> goal、状態 done -> achieved（目標・成果の指標とも）。
ALTER TABLE objective RENAME TO goal;
ALTER TABLE key_result RENAME COLUMN objective_id TO goal_id;
UPDATE goal SET status = 'achieved' WHERE status = 'done';
UPDATE key_result SET status = 'achieved' WHERE status = 'done';

-- 数え方: days -> active_days、done -> done_tasks。最小分数の列は day_min -> active_day_min。
ALTER TABLE key_result RENAME COLUMN day_min TO active_day_min;
UPDATE key_result SET measure = 'active_days' WHERE measure = 'days';
UPDATE key_result SET measure = 'done_tasks' WHERE measure = 'done';

-- 成果の指標のスキル: kr_skill -> key_result_skill。
DROP INDEX idx_kr_skill_skill;
ALTER TABLE kr_skill RENAME TO key_result_skill;
ALTER TABLE key_result_skill RENAME COLUMN kr_id TO key_result_id;
CREATE INDEX idx_key_result_skill_skill ON key_result_skill(skill_id);

-- 現在値の記録: kr_progress -> key_result_progress。
DROP INDEX idx_kr_progress_kr_at;
ALTER TABLE kr_progress RENAME TO key_result_progress;
ALTER TABLE key_result_progress RENAME COLUMN kr_id TO key_result_id;
CREATE INDEX idx_key_result_progress_key_result_at ON key_result_progress(key_result_id, at);

-- 紐づけの提案で「付けない」を選んだ候補: kr_suggest_reject -> link_suggestion_reject。
ALTER TABLE kr_suggest_reject RENAME TO link_suggestion_reject;
ALTER TABLE link_suggestion_reject RENAME COLUMN kr_id TO key_result_id;
