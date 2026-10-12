-- KR への紐づけの提案で「付けない」を選んだ候補。同じ候補は再び提案しない。
-- kind は 'task' | 'skill'。target_id は task.id または skill.id（参照制約は付けない。対象が消えても行は無害）。
CREATE TABLE kr_suggest_reject (
  kr_id     TEXT NOT NULL,
  kind      TEXT NOT NULL,
  target_id TEXT NOT NULL,
  at        INTEGER NOT NULL,
  PRIMARY KEY (kr_id, kind, target_id)
);
