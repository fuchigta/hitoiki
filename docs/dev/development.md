# 開発ガイド

開発時の規約と守るべき制約は [`AGENTS.md`](../../AGENTS.md)、設計は [アーキテクチャ](./architecture.md)、リリースの手順は [リリース](./release.md) を参照してください。

## 前提

- Rust（stable）。Windows 環境が基本です（収集器と通知は Windows 専用。非 Windows でもビルドとテストは通ります）。
- Node.js 20 以上（常駐アプリのフロントエンド `ui/` 用）。
- [cargo-deny](https://github.com/EmbarkStudios/cargo-deny)（依存クレートのライセンス検査用）。
- [spotter](https://github.com/fuchigta/spotter) v0.13.0 以上（コミット前と CI の検査用。後述）。

## UI のビルド

`ui/dist` は `hitoiki-app` に埋め込まれるため、`ui/dist` が無いと `hitoiki-app` はビルドできません。Rust のビルドより先に UI をビルドします。

```powershell
cd ui
npm ci
npm run build   # 型検査（tsc --noEmit）と vite build
npm test        # vitest
cd ..
```

CLI だけなら UI のビルドは不要です（`cargo build -p hitoiki-cli`）。

## UI の確認（モックとスクリーンショット）

Tauri（Rust 側）を起動せずに、UI だけをブラウザで表示・操作・撮影できます。Tauri の IPC を `ui/src/dev/` のモックに差し替える仕組みで、AI エージェントが画面を目で確認する用途も想定しています。画面の再設計の計画と、段階ごとの確かめ方は [ui-redesign.md](./ui-redesign.md) にあります。モックは `vite --mode mock` のときだけ読み込まれ、`npm run build` の成果物には含まれません。ローカルの開発サーバ（127.0.0.1）だけを使い、外部への通信はありません。

### 起動

```powershell
cd ui
npm run dev:mock    # http://localhost:5173/ をブラウザで開く
```

URL のクエリで切り替えます。

- `?scenario=default|empty|busy`: データの状態（既定は `default`）
  - `default`: 普通の 1 日（タスク、計画ブロック、活動区間、要確認、OKR/KR、テンプレート、ルール、記録など）
  - `empty`: 初回起動直後。データなしで、はじめにの案内が出る
  - `busy`: 要確認が多い、タスクが多数、長い名前など、詰まった状態
- `?window=main|popup|quick`: ウインドウの種別（既定は `main`）。main の画面は `#/today`、`#/review` のようにハッシュで切り替える

例: `http://localhost:5173/?scenario=busy#/review`

現在時刻は 2026-10-13（火）10:30 JST に固定した基準時刻で、データはそこからの相対で作ります。状態はメモリ上だけにあり、タスクの追加・編集・削除、実績の確定、計画ブロックの操作などは画面に反映されます（再読み込みで初期状態に戻る）。ブラウザのコンソールでは `window.__mock.state` から状態を確認できます。

### モックの追加

新しい Tauri コマンドを `ui/src/api/index.ts` に足したら、`ui/src/dev/handlers.ts` のコマンド表 `H` にも同名のハンドラを足します。足し忘れると `npm test`（`ui/src/dev/mock.test.ts`）が落ちます。未実装のコマンドが呼ばれると、モックは `console.error("[mock] unhandled command: ...")` を出して reject します。フィクスチャは `ui/src/dev/state.ts` にあります。型は `api/types.ts` を使うので、`npm run typecheck` で形の食い違いが分かります。

### スクリーンショットの巡回

```powershell
cd ui
npm run shots                                        # 全シナリオ × 全画面 × 全ウインドウ
npm run shots -- --scenario default --screen today   # 絞り込み
npm run shots -- --window popup --dark               # ダークテーマも撮る
npm run shots -- --narrow                            # main を幅 800px で撮る（ファイル名に -narrow。860px 以下の崩れを見る）
```

`ui/shots/<scenario>/<window>-<screen>.png`（popup と quick は `<window>.png`、ダークは末尾に `-dark`）に保存します（`ui/shots/` は git の管理外）。Vite をプログラムから起動し、Windows に入っている Edge（なければ Chrome）を Playwright で操作するので、ブラウザのダウンロードは要りません。ビューポートは main が 1280x800（本文が長い画面は高さを伸ばして全体を撮る）、popup が 360x320、quick が 520x380 です。各ページの console error・pageerror・`[mock] unhandled` を集め、1 件でもあれば最後に一覧を出して終了コード 1 になります。

一覧で 1 つ選んだ状態（`#/tasks/<ID>`、`#/okr/kr/<ID>`、`#/okr/skill/<ID>`、`#/records/checkin/<ID>`、`#/records/reflection/<ID>`、記録の作成フォーム `#/records/new/abc`、今日のデザイン `#/today/design`、前日の今日 `#/today?date=<前日>`、今日でブロックを選んだ状態・実績の区間を選んだ状態）は、`main-today-design`、`main-today-prev`、`main-today-block-selected`、`main-today-segment-selected`、`main-tasks-selected`、`main-okr-kr-selected`、`main-okr-skill-selected`、`main-records-checkin-selected`、`main-records-reflection-selected`、`main-records-new-abc` として別に撮ります（`ui/scripts/shots.mjs` の `SELECTED`。ID はモックの状態から引き、フィクスチャに無ければ飛ばします。ブロックや区間のクリックは `act` フックで行います。`empty` では撮りませんが、`all: true` を付けたもの（今日のデザイン・前日の今日・設定のタブ）は撮ります）。新しい一覧＋詳細の画面を足したら、ここにも指定を足します。

設定の各タブ（`#/settings/<tab>`。`general` は `main-settings` と同じなので除く）は `main-settings-<tab>` として、`empty` も含めて撮ります。タブの一覧は `ui/src/settings-util.ts` の `SETTINGS_TABS` から拾うので、タブを足せば自動で撮影の対象になります。

撮影のたびに、ページの構造（`page.locator("body").ariaSnapshot()`）も `ui/shots/<scenario>/<名前>.aria.yml` に保存します（ライトテーマのときだけ）。見出しの階層やボタンの名前の崩れは、画像より差分で気づきやすくなります。

### 前回との差分（`--diff`）

```powershell
npm run shots -- --diff
```

撮影の開始時に、前回の `ui/shots/` を `ui/shots-prev/` へ退避します（どちらも git の管理外）。`--diff` を付けると、今回撮った画像と aria を前回のものと比べ、変化したファイルを一覧にします。画像は `pixelmatch` で比べ、変わった画素数と割合を出し、差分画像を `ui/shots/diff/<scenario>/<名前>.png` に保存します。aria は行単位のテキスト差分（`-` が前回、`+` が今回）を表示します。画像の大きさが変わったときは、その旨だけを出します。比べるのは今回撮ったファイルだけです（絞り込んだ撮影で、撮っていない画面は比べません）。基準画像はリポジトリに入れません。変更の前に一度撮り、変更の後に `--diff` 付きで撮る、という使い方を想定しています。

### 操作シナリオ（flows）

```powershell
npm run flows                  # ui/scripts/flows/*.mjs を全部実行
npm run flows -- --flow tasks  # 1 本だけ実行
```

主要な操作の流れ（実績・タスク・今日・記録・目標・設定）を、モックの UI で実際に操作して確かめます。1 本ごとに新しいブラウザコンテキスト（モックの状態は初期化済み）で実行し、時刻は撮影と同じく固定します。成功なら `OK`、失敗なら `NG` と理由を出し、`ui/shots/flows/<名前>.png` に失敗時の画面を保存して終了コード 1 になります。console error・pageerror・`[mock] unhandled` も失敗です。

シナリオは画面ごとに 1 ファイルで、`{ name, scenario, screen, run({ page }) }` を default export します。要素は `getByRole` と accessible name で選び（スタイルのクラス名に頼らない）、結果は `window.__mock.state`（`scripts/lib/flow.mjs` の `mockState`）か画面で確かめます。画面の構成を変えて壊れたら、該当の 1 ファイルだけを直します。Vite の起動やブラウザの起動などの共通部分は `ui/scripts/lib/harness.mjs` にあり、`npm run shots` と共有しています。

### UI の細かい不統一の検査（audit）

```powershell
cd ui
npm run audit                                      # 全シナリオ × 全画面 × 幅 1280・800
npm run audit -- --scenario default --screen report
npm run audit -- --only shift                      # 位置ずれだけ（--only style で値の集計だけ）
```

モックの画面を巡回して、操作で表示がずれる・部品の寸法がそろっていない、といった細かい不統一を機械的に拾います（`ui/scripts/audit.mjs`、ページ内の測定は `ui/scripts/lib/audit-page.mjs`）。報告は `ui/shots/audit/report.md`（人が読む）と `report.json` に出ます。許可リストで除外した分を除いて 1 件でもあれば終了コード 1 です。全部で 10 分弱かかるので、画面を絞ると速く回せます。

- **位置ずれ（shift）**: タブ・chip・ラジオ・チェックボックス・select の各 option を 1 つずつ操作し（そのたびに再読み込みして状態を戻します）、押した部品より上・同じ高さにある部品、見出し、本文の上端が 1px 以上動いたら報告します。操作部品は大きさの変化も見ます。削除・保存・確定などの名前のボタンは押しません。
- **値のばらつき（style）**: 全画面の計算済みスタイルを部品の種類（button の変種・input の種類・select・見出しのレベル・tag）ごとに集め、高さ・文字サイズ・内側の余白・角丸の少数派（8 個以上のうち 15% 未満）を報告します。
- **トークン外の値（token）**: 文字サイズ・余白・角丸・色が `styles.css` の `:root` のトークンに無いものを報告します。
- **横並びの不揃い（align）**: 同じ行に並ぶ操作部品の高さ・垂直中心の差を報告します。
- **横幅のはみ出し（overflow）**: 中身が入れ物の幅をはみ出している要素を報告します（省略表示と横スクロールにしてあるものは除く）。

操作部品の高さは `--ctl-h` でそろえています（ボタン・input・select・chip）。chip と tab は、選択中でも太字にしません（幅が変わって隣が動くため）。操作で出し入れする部品は、場所を空けておくか、高さのそろった部品にしてください。

意図した例外は `ui/scripts/audit-allow.json` に `{ "pattern": "<key に対する正規表現>", "reason": "<理由>" }` で足します（key は report.md に出ます）。使われなかった項目は報告に出ます。数値上はそろっているのに落ち着かない、といった印象の問題は検出できないので、スクリーンショットの目視で補います。

### AI エージェントが画面を確認する手順

1. UI を変えたら `npm run shots -- --scenario <シナリオ> --screen <画面>` で該当の画面だけを撮る。
2. 終了コードとエラー一覧を確認する（エラーが出たら、まずモックか UI のどちらの不具合かを切り分ける）。
3. 撮れた PNG を画像として読み、データが表示されているか、空画面やエラー表示になっていないか、レイアウトが崩れていないかを目で確認する。
4. 空の状態は `empty`、詰まった状態（長い名前・件数が多い）は `busy` でも確認する。

## ビルド・テスト・検査

```powershell
# ビルド（CLI と常駐アプリ）
cargo build --release -p hitoiki-cli -p hitoiki-app

# テスト
cargo test --workspace

# Lint
cargo clippy --workspace --all-targets -- -D warnings

# フォーマット（確認だけなら --check）
cargo fmt --all

# 依存クレートのライセンス・脆弱性検査
cargo deny check
```

CI（`.github/workflows/ci.yml`）では、fmt・clippy・test（Windows）、`cargo deny check licenses`、spotter の検査を実行します。UI は `npm ci` / `npm run build` / `npm test` を通します。

依存クレートは MIT / Apache-2.0 / BSD などの permissive ライセンスのみです。新規クレートには `publish = false` を付けます。依存は `[workspace.dependencies]` に追加して `xxx.workspace = true` で参照します。

## ドキュメント

利用者向けは [`docs/user/`](../user/)、開発者向けは `docs/dev/` に置きます。挙動を変えたら、対応する文書を同じコミットで更新します。ソースと文書の対応は spotter の `doc-sync` が検査します（対応表は `.spotter.yml`）。

## spotter（コミット前と CI の検査）

[spotter](https://github.com/fuchigta/spotter) は、コミット前（フック）と CI で同じ検査を実行する CLI です。設定は `.spotter.yml` で、`required_version` で版を固定しています。

| 検査 | 内容 |
|---|---|
| `commit-subject` | コミットの 1 行目が Conventional Commits 形式か（[コミット規約](#コミット規約)） |
| `doc-sync` | ソースを変えたら対応する文書も変えたか |
| `doc-paths` | 文書がバッククォートで挙げたパスが実在するか |
| `doc-links` | 文書の Markdown リンクが切れていないか |
| `docs-user-consistency` | `docs/user/*.md` のファイルと README からのリンクが一致するか |
| `commit-types-consistency` | `cliff.toml` と `.spotter.yml` のコミット type の一覧が一致するか |
| `unwanted-files` | DB・ログ・ビルド成果物・秘密情報・巨大ファイルの混入 |

### インストール

**spotter v0.13.0 以上**が必要です。古い版（v0.2.0 など）が PATH にあるとフックが古い版を呼んでしまうので、必ず版を確認してください（`spotter --version`）。

- spotter の [Releases](https://github.com/fuchigta/spotter/releases) からバイナリ（`spotter-windows-amd64.exe`）を入手し、`spotter.exe` として PATH 上に置く。
- mise を使う場合: `mise use -g github:fuchigta/spotter@0.13.0`
- 入っている spotter を更新する場合: `spotter update`

### フックの設置

```powershell
spotter hooks install
```

`commit-msg` と `pre-push` のフックが `.githooks/` に作られ（リポジトリにコミット済み）、`core.hooksPath` が `.githooks` に設定されます（クローンごとに 1 回実行します）。

フックは `spotter` を PATH から呼ぶので、**PATH 上の `spotter` が v0.13.0 以上であることを確認してください**（`spotter --version`）。古い版が先に見つかると、古い版が検査を実行してしまいます。mise のシムなど、別の場所に古い版が残っていないかも確認してください。

手元で検査を実行するには次のようにします。

```powershell
spotter check                       # ステージ済みの変更
spotter check --range main..HEAD    # ブランチのコミットすべて
spotter config explain doc-links    # 検査の抽出結果を確認（doc-paths / doc-links / consistency）
```

`doc-sync` などコミットの内容に依存する検査は、やむを得ない場合に限りコミットメッセージのトレーラで免除できます（`Doc-Sync: skip <理由>` の形式）。理由を必ず書き、常用しないでください。

## コミット規約

Conventional Commits 形式で書きます。1 行目は次の形です。

```
<type>(<scope>): <日本語の要約>
```

- **type**: `feat` / `fix` / `perf` / `refactor` / `docs` / `test` / `build` / `ci` / `chore` / `revert`
- **scope**（任意）: crate 名から `hitoiki-` を除いたもの（`core`, `collect`, `collect-win`, `agents`, `calendar`, `judge`, `ipc`, `cli`, `daemon`, `app`）、または `ui`, `docs`, `release` など
- 破壊的変更は `!` を付けます（例: `feat(cli)!: ...`）。
- 要約と本文は日本語で書きます。
- マージコミットは検査の対象外です。

例:

```
feat(core): 帰属ルールに git_branch フィールドを追加
fix(collect-win): ロック解除直後に区間が欠ける問題を修正
docs: 利用者向け文書を docs/user に分割
```

`CHANGELOG.md` に載る利用者向けの変更（`feat` / `fix` / `perf`）は、要約を利用者目線で書いてください。リリース時に `git-cliff` で下書きを作ります（[リリース](./release.md)）。
