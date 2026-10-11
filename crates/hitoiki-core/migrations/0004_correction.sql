-- 時間帯の範囲指定による実績の補正（docs/dev/architecture.md「実績の補正」）。
-- 補正は segment_override（ユーザー確定）として保存する。1 回の付け替えを segment_correction でひとまとまりにし、取り消せるようにする。

-- offline: PC 外で過ごした時間（無操作・記録なしの範囲を割り当てたもの）の印。
-- correction_id: この override を作った補正（segment_correction.id）。NULL は補正以外（仕分け）。
ALTER TABLE segment_override ADD COLUMN offline INTEGER NOT NULL DEFAULT 0;
ALTER TABLE segment_override ADD COLUMN correction_id TEXT;
CREATE INDEX idx_segment_override_correction ON segment_override(correction_id);

-- 派生層にも印を持たせる（再計算でも override から再現される）。
ALTER TABLE activity_segment ADD COLUMN offline INTEGER NOT NULL DEFAULT 0;

CREATE TABLE segment_correction (
  id             TEXT PRIMARY KEY,
  start_at       INTEGER NOT NULL,
  end_at         INTEGER NOT NULL,
  -- 割り当て先。task_id が NULL で kind も NULL なら「タスクなし」、kind = 'break' なら休憩。
  task_id        TEXT,
  kind           TEXT,
  activity_label TEXT,
  -- 付け替えた時間の合計と、そのうち PC 外の時間（確定時点の値）。segment_count は対象にした区間数。
  total_ms       INTEGER NOT NULL,
  offline_ms     INTEGER NOT NULL,
  segment_count  INTEGER NOT NULL,
  created_at     INTEGER NOT NULL
);
CREATE INDEX idx_segment_correction_start ON segment_correction(start_at);
CREATE INDEX idx_segment_correction_task ON segment_correction(task_id);
