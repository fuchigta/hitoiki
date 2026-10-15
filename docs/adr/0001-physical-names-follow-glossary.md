# 物理名を用語集に揃え、旧名との互換は持たない

画面の言葉を平易にした結果、コード・DB・設定・CLI の名前（物理名）が旧来の OKR やカウンセリング用語のまま残り、同じ概念に複数の名前、同じ名前に複数の概念が生じた。利用者がまだ開発者本人だけのうちに、物理名を [GLOSSARY.md](../../GLOSSARY.md) の英語に揃える。旧名の別名（CLI の旧コマンド、旧ルート、旧設定キー）は残さず、DB はマイグレーションでテーブル・列・保存済みの文字列値を書き換える。設定ファイルの旧キーは読み飛ばされるので、利用者が書き直す。

## Consequences

- CLI の `--json` は形が変わるため、外枠の `version` を 2 に上げる。
- 物理名は次のとおり変わる（左が旧、右が新）。

| 概念 | 旧 | 新 |
|---|---|---|
| 目標 | `objective`, `Objective`, `okr`（CLI・ルート・Tauri） | `goal`, `Goal`, `goal` / `key-result`（CLI）, `goals`（ルート） |
| 成果の指標 | `kr`, `Kr*`, `kr_skill`, `kr_progress`, `kr_suggest_reject` | `key_result`, `KeyResult*`, `key_result_skill`, `key_result_progress`, `link_suggestion_reject` |
| 目標・成果の指標の状態 | `active` / `done` / `dropped` | `active` / `achieved` / `dropped` |
| 数え方 | `time` / `days` / `done`, `day_min` | `time` / `active_days` / `done_tasks`, `active_day_min` |
| 名前の近さ | `local` | `name_match` |
| 緊急・重要（集計の軸） | `quadrant` | `urgent_important` |
| 成果の指標（集計の軸） | `okr`, `kr:` | `key_result`, `key_result:` |
| 調子 | `checkin`, `Checkin`, CLI `check` | `condition`, `Condition`, CLI `condition` |
| 気になったこと | `hassle` | `concern` |
| 捉え直し | `abc`, `abc_record`, `activating_event`, `consequence_emotion`, `consequence_intensity`, `dispute`, `ai_dialog_md`, CLI `abc dispute` | `reframe`, `reframe`, `event`, `emotion`, `emotion_intensity`, `alternative`, `deepening_md`, CLI `reframe deepen` |
| 未確定の ERG | `suggested`, `pending` | `unconfirmed` |
| 振り返りのプロンプト | `templates/`, `ai.templates_dir` | `prompts/`, `ai.prompts_dir` |
| 実績 | CLI `log` / `review`, ルート `log`, 設定 `[review]` | CLI `actual`, ルート `actual`, 設定 `[actual]` |
| AI の作業待ち | `agent_watch`, `watch_ms` | `agent_wait`, `wait_ms` |
| 確定 | `segment_override`, `decided_by = 'user'` | `segment_confirmation`, `decided_by = 'confirmation'` |
| PC 外 | `offline` | `off_pc` |
| 未収集 | `gap`, `gap:` | `uncollected`, `uncollected:` |
| 活動 | `activity_label`, CLI `activity-label` | `activity`, CLI `activity` |
| タスクの割り当てルール | `task_match_rule`, `TaskMatchRule`, `attribute` | `task_assign_rule`, `TaskAssignRule`, `assign` |
| 問いかけ | `push`, `push_log`, `PushTrigger`, 設定 `[push]` | `nudge`, `nudge_log`, `NudgeTrigger`, 設定 `[nudge]` |
| 長時間の作業 | `long_focus`, `long_focus_min` | `long_work`, `long_work_min` |
| 計画ブロック | `time_block`, `TimeBlock` | `plan_block`, `PlanBlock` |
| はじめに | `onboarding` | `welcome` |
