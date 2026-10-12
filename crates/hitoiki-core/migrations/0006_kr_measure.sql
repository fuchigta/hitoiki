-- KR の軸（time: 時間 / days: 取り組んだ日の回数 / done: 完了したタスクの数）と、
-- 回数の軸で「取り組んだ日」とみなす 1 日の最小分数。
ALTER TABLE key_result ADD COLUMN measure TEXT NOT NULL DEFAULT 'time';
ALTER TABLE key_result ADD COLUMN day_min INTEGER NOT NULL DEFAULT 15;
