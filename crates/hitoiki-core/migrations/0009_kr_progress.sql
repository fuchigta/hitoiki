-- KR の現在値（current）の変更履歴。値が変わるたびに 1 行。
-- value は text の先頭の数値（数として読めなければ NULL）、text は入力された現在値そのもの（消したときは NULL）。
CREATE TABLE kr_progress (
  id    TEXT PRIMARY KEY,
  kr_id TEXT NOT NULL REFERENCES key_result(id),
  value REAL,
  text  TEXT,
  note  TEXT,
  at    INTEGER NOT NULL
);
CREATE INDEX idx_kr_progress_kr_at ON kr_progress(kr_id, at);
