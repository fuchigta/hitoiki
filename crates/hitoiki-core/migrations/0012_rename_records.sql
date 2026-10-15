-- 記録まわりの物理名を用語集（GLOSSARY.md）に揃える（docs/adr/0001-physical-names-follow-glossary.md）。
-- 調子（checkin -> condition）、気になったこと（hassle -> concern）、捉え直し（abc_record -> reframe）。

-- 調子。erg_tag 列は 0007 で erg_label に移行済みで、どこからも読み書きしない。
ALTER TABLE checkin RENAME TO condition;
ALTER TABLE condition DROP COLUMN erg_tag;

ALTER TABLE hassle RENAME TO concern;

ALTER TABLE abc_record RENAME TO reframe;
ALTER TABLE reframe RENAME COLUMN activating_event TO event;
ALTER TABLE reframe RENAME COLUMN consequence_emotion TO emotion;
ALTER TABLE reframe RENAME COLUMN consequence_intensity TO emotion_intensity;
ALTER TABLE reframe RENAME COLUMN dispute TO alternative;
ALTER TABLE reframe RENAME COLUMN ai_dialog_md TO deepening_md;
ALTER TABLE reframe RENAME COLUMN hassle_id TO concern_id;

-- 保存済みの値。
-- ERG ラベル: 記録の種類と、確定待ちの出所（suggested -> unconfirmed）。
UPDATE erg_label SET record_kind = CASE record_kind
  WHEN 'checkin' THEN 'condition'
  WHEN 'hassle' THEN 'concern'
  WHEN 'abc' THEN 'reframe'
  ELSE record_kind END;
UPDATE erg_label SET source = 'unconfirmed' WHERE source = 'suggested';

-- 振り返り: 構造化された問いの kind と event（旧 activating_event）、回答の対応表の kind、
-- 作成時の文脈 JSON のキー。いずれも serde_json が作った詰めた形の JSON を文字列として書き換える。
UPDATE reflection SET structured_json =
  replace(replace(replace(replace(structured_json,
    '"kind":"checkin"', '"kind":"condition"'),
    '"kind":"hassle"', '"kind":"concern"'),
    '"kind":"abc"', '"kind":"reframe"'),
    '"activating_event":', '"event":')
  WHERE structured_json IS NOT NULL;

UPDATE reflection SET answers_json =
  replace(replace(replace(answers_json,
    '"kind":"checkin"', '"kind":"condition"'),
    '"kind":"hassle"', '"kind":"concern"'),
    '"kind":"abc"', '"kind":"reframe"')
  WHERE answers_json IS NOT NULL;

UPDATE reflection SET context_json =
  replace(replace(replace(replace(replace(replace(context_json,
    '"checkins":', '"conditions":'),
    '"hassles":', '"concerns":'),
    '"abc_records":', '"reframes":'),
    '"activating_event":', '"event":'),
    '"has_dispute":', '"has_alternative":'),
    '"pending":', '"unconfirmed":');
