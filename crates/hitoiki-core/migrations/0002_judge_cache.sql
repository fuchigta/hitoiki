-- 判定器の答えのキャッシュ（docs/architecture.md「判定器」）。定期再計算で同じ区間を判定器に送り直さないため。
-- key は区間の内容（exe・正規化タイトル・根拠ウインドウ）と、判定器の設定・未完了タスク集合のハッシュから作る。
CREATE TABLE judge_cache (
  key         TEXT PRIMARY KEY,
  answer_json TEXT NOT NULL,
  judge_name  TEXT NOT NULL,
  created_at  INTEGER NOT NULL
);
CREATE INDEX idx_judge_cache_created ON judge_cache(created_at);
