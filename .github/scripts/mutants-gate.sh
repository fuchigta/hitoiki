#!/usr/bin/env bash
# ミューテーションテストのゲート。各シャードの mutants.out/outcomes.json を合計して判定する。
#
# 環境変数（mutation.yml から渡す）:
#   EXPECTED        plan で数えた、差分に掛かるミュータントの件数
#   MAX_MISSED      生き残り（missed）の上限。超えたら落とす
#   PLAN_RESULT     plan ジョブの結果（success 以外なら落とす）
#   MUTANTS_RESULT  シャードのジョブの結果（EXPECTED が 0 のときは skipped になる）
#   OUTCOMES_DIR    mutants-out-<シャード>/ を置いたディレクトリ（既定はカレント）
set -euo pipefail
: "${MAX_MISSED:?}"
dir="${OUTCOMES_DIR:-.}"

if [[ "${PLAN_RESULT:-}" != "success" ]]; then
  echo "::error::plan ジョブが ${PLAN_RESULT:-不明} で終わったため、判定できない"
  exit 1
fi
: "${EXPECTED:?}"

if [[ "$EXPECTED" == "0" ]]; then
  echo "差分に掛かるミュータントは 0 件。試すものが無いので成功として扱う"
  exit 0
fi

if [[ "${MUTANTS_RESULT:-}" != "success" ]]; then
  echo "::error::シャードのジョブが ${MUTANTS_RESULT:-不明} で終わった（未変異のテストの失敗やビルドの失敗など）"
  exit 1
fi

shopt -s nullglob
files=("$dir"/mutants-out-*/outcomes.json)
if (( ${#files[@]} == 0 )); then
  echo "::error::outcomes.json が 1 つも無い。ミュータントを試した証拠が無いため落とす"
  exit 1
fi

sum() { jq -s "map(.$1) | add" "${files[@]}"; }
tested=$(sum total_mutants)
caught=$(sum caught)
missed=$(sum missed)
timeout=$(sum timeout)
unviable=$(sum unviable)

echo "試したミュータント: ${tested} 件（plan の見込み ${EXPECTED} 件、シャード ${#files[@]} 個）"
echo "  caught ${caught} / missed ${missed} / timeout ${timeout} / unviable ${unviable}"
viable=$(( caught + missed + timeout ))
if (( viable > 0 )); then
  # 時間切れは変異でテストが止まらなくなった＝検出できたものとして数える
  echo "  スコア（(caught + timeout) / (caught + missed + timeout)）: $(( (caught + timeout) * 1000 / viable / 10 )).$(( (caught + timeout) * 1000 / viable % 10 ))%"
fi

if (( tested != EXPECTED )); then
  echo "::error::試した件数 ${tested} が plan の見込み ${EXPECTED} と一致しない（シャードの欠けや空振りの疑い）"
  exit 1
fi

if (( missed > 0 )); then
  echo "生き残ったミュータント:"
  cat "$dir"/mutants-out-*/missed.txt | sed 's/^/  /'
fi

if (( missed > MAX_MISSED )); then
  echo "::error::生き残り ${missed} 件が上限 MAX_MISSED=${MAX_MISSED} を超えた。テストを足すか、等価なミュータントなら PR に理由を書いて上限を見直す"
  exit 1
fi
echo "生き残り ${missed} 件は上限 ${MAX_MISSED} 以内"
