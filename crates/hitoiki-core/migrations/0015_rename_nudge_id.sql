-- 物理名を用語集に揃える（調子・気になったことが参照する問いかけ。docs/adr/0001-physical-names-follow-glossary.md）。
-- 0012（記録）と 0014（問いかけ）の両方が揃ったあとで、condition / concern の push_id を nudge_id にする。

ALTER TABLE condition RENAME COLUMN push_id TO nudge_id;
ALTER TABLE concern RENAME COLUMN push_id TO nudge_id;
