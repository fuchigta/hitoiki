-- AI 振り返りの構造化出力（docs/dev/architecture.md「AI 振り返り」）。
-- structured_json: AI が返した問い（service::structured::StructuredOutput）。NULL は従来の Markdown のみ。
-- answers_json: 構造化された問いへの回答の対応表（各問いの種類と、保存した checkin / hassle / abc_record の ID）。
ALTER TABLE reflection ADD COLUMN structured_json TEXT;
ALTER TABLE reflection ADD COLUMN answers_json TEXT;
