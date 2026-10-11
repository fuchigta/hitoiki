-- ブロックのテンプレート（働き方のパターン。docs/dev/architecture.md「ブロックのテンプレート」）。
-- 時刻はローカル時刻の「0 時からの分」で持つ（適用する日のタイムゾーンで UTC に変換する）。
CREATE TABLE block_template (
  id          TEXT PRIMARY KEY,
  name        TEXT NOT NULL UNIQUE,
  description TEXT,
  -- 割り当てる曜日。JSON 配列（1=月 … 7=日）。空配列は割り当てなし。
  weekdays    TEXT NOT NULL DEFAULT '[]',
  created_at  INTEGER NOT NULL,
  updated_at  INTEGER NOT NULL
);

CREATE TABLE block_template_item (
  id          TEXT PRIMARY KEY,
  template_id TEXT NOT NULL REFERENCES block_template(id) ON DELETE CASCADE,
  start_min   INTEGER NOT NULL,  -- 0〜1439
  end_min     INTEGER NOT NULL,  -- 1〜1440（1440 は翌 0 時）
  label       TEXT,
  focus       INTEGER NOT NULL DEFAULT 0,
  task_id     TEXT REFERENCES task(id)
);
CREATE INDEX idx_block_template_item_tpl ON block_template_item(template_id, start_min);

-- 適用の履歴。直前の適用を取り消す（作ったブロックを削除する）ために使う。
CREATE TABLE block_template_application (
  id            TEXT PRIMARY KEY,
  template_name TEXT NOT NULL,
  date          TEXT NOT NULL,        -- 適用先のローカル日付 YYYY-MM-DD
  applied_at    INTEGER NOT NULL,
  block_ids     TEXT NOT NULL,        -- 作成した time_block.id の JSON 配列
  undone_at     INTEGER
);
