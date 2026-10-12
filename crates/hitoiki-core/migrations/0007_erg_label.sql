-- ERG（存在・関係・成長）のタグ（docs/dev/architecture.md「ERG の判定」）。
-- 記録（チェックイン・ハッスル・ABC・振り返りの回答）ごとに最大 1 件。
-- record_kind: 'checkin' | 'hassle' | 'abc' | 'reflection'
-- tag:    'existence' | 'relatedness' | 'growth' | 'none'（どれにも関わらない）
-- source: 'user'（ユーザーが付けた・確定した。AI は上書きしない）
--         'ai'（確信度が閾値以上で AI が採用した）
--         'suggested'（確認待ち。AI の提案で、確定はしていない）
CREATE TABLE erg_label (
  record_kind TEXT NOT NULL,
  record_id   TEXT NOT NULL,
  tag         TEXT NOT NULL,
  source      TEXT NOT NULL,
  confidence  REAL,
  reason      TEXT,
  at          INTEGER NOT NULL,
  PRIMARY KEY (record_kind, record_id)
);
CREATE INDEX idx_erg_label_source ON erg_label(source, at);

-- 従来のチェックインの ERG タグはユーザーが選んだものとして引き継ぐ（checkin.erg_tag 列は使わなくなる）。
INSERT INTO erg_label (record_kind, record_id, tag, source, confidence, reason, at)
  SELECT 'checkin', id, erg_tag, 'user', NULL, NULL, at FROM checkin WHERE erg_tag IS NOT NULL;
