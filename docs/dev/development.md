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
