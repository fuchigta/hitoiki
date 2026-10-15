-- 物理名を用語集に揃える（問いかけ・計画ブロック・はじめに。docs/adr/0001-physical-names-follow-glossary.md）。
-- push_log -> nudge_log、time_block -> plan_block、setting のキー onboarding.* -> welcome.*。
-- 保存済みの文字列値（きっかけ・アクション ID・入力欄 ID・open_screen・コンテキストの JSON キー）も新しい名前に書き換える。

ALTER TABLE push_log RENAME TO nudge_log;
DROP INDEX idx_push_log_at;
CREATE INDEX idx_nudge_log_at ON nudge_log(at);

ALTER TABLE time_block RENAME TO plan_block;
DROP INDEX idx_time_block_start;
CREATE INDEX idx_plan_block_start ON plan_block(start_at);

-- きっかけ（trigger）。
UPDATE nudge_log SET trigger = 'key_result_idle' WHERE trigger = 'kr_idle';
UPDATE nudge_log SET trigger = 'long_work' WHERE trigger = 'long_focus';

-- ペイロード（NudgePayload の JSON。serde_json のコンパクトな形で保存されている）。
UPDATE nudge_log SET payload_json = replace(payload_json, '"trigger":"kr_idle"', '"trigger":"key_result_idle"')
  WHERE payload_json LIKE '%"trigger":"kr_idle"%';
UPDATE nudge_log SET payload_json = replace(payload_json, '"trigger":"long_focus"', '"trigger":"long_work"')
  WHERE payload_json LIKE '%"trigger":"long_focus"%';
UPDATE nudge_log SET payload_json = replace(payload_json, '"id":"kr_skip_week"', '"id":"key_result_skip_week"')
  WHERE payload_json LIKE '%"id":"kr_skip_week"%';
UPDATE nudge_log SET payload_json = replace(payload_json, '"id":"hassle"', '"id":"concern"')
  WHERE payload_json LIKE '%"id":"hassle"%';
UPDATE nudge_log SET payload_json = replace(payload_json, '"open_screen":"reflection/', '"open_screen":"records/new/')
  WHERE payload_json LIKE '%"open_screen":"reflection/%';
UPDATE nudge_log SET payload_json = replace(payload_json, '"open_screen":"checkin"', '"open_screen":"records/new/condition"')
  WHERE payload_json LIKE '%"open_screen":"checkin"%';
UPDATE nudge_log SET payload_json = replace(payload_json, '"open_screen":"hassle"', '"open_screen":"records/new/concern"')
  WHERE payload_json LIKE '%"open_screen":"hassle"%';
UPDATE nudge_log SET payload_json = replace(payload_json, '"open_screen":"review"', '"open_screen":"actual?status=review"')
  WHERE payload_json LIKE '%"open_screen":"review"%';

-- 応答（NudgeResponse の JSON）。
UPDATE nudge_log SET response_json = replace(response_json, '"action":"kr_skip_week"', '"action":"key_result_skip_week"')
  WHERE response_json LIKE '%"action":"kr_skip_week"%';

-- 振り返りの材料（コンテキスト）の JSON キー。
UPDATE reflection SET context_json = replace(context_json, '"time_blocks"', '"plan_blocks"')
  WHERE context_json LIKE '%"time_blocks"%';

-- はじめにの完了状態。
UPDATE setting SET key = 'welcome.completed_at' WHERE key = 'onboarding.completed_at';
