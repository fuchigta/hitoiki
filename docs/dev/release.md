# リリース手順

タグ `v*` を push すると、GitHub Actions（`.github/workflows/release.yml`）がインストーラーと zip をビルドして GitHub Release に添付します。コード署名は付けていません。

## 手順

1. **バージョンを上げる**: 次の 2 か所の `version` を同じ値にします。
   - ルート `Cargo.toml` の `[workspace.package]` の `version`（各クレートはこれを継承します）
   - `crates/hitoiki-app/tauri.conf.json` の `version`（インストーラーの表示に使われます）

   2 つが食い違うと、インストーラーの表示が実体と合わなくなります。`release.yml` は、タグと `tauri.conf.json` の `version` が一致しなければ失敗します。`Cargo.lock` も更新されるので、`cargo build` などで反映してコミットに含めます。
2. **CHANGELOG を作る**: 後述の手順で `CHANGELOG.md` に新しい版の節 `## [x.y.z] - YYYY-MM-DD` を追加します。節が無いと `release.yml` は失敗します。
3. **確認する**: [開発ガイド](./development.md) の fmt・clippy・test・`cargo deny check`・`spotter check --range` を通します。
4. **コミットしてタグを打つ**: バージョンと CHANGELOG の変更を `chore(release): vx.y.z` などでコミットし、タグを push します。

   ```powershell
   git tag vx.y.z
   git push origin main vx.y.z
   ```

## CHANGELOG の作り方

`CHANGELOG.md` は [Keep a Changelog](https://keepachangelog.com/ja/1.1.0/) 風の日本語で、利用者目線で書きます。内部だけの変更（リファクタ、テスト、CI など）は載せません。

[git-cliff](https://git-cliff.org/) で下書きを作り、手で書き直して反映します。設定は `cliff.toml` で、Conventional Commits の type ごとに見出し（追加、修正、性能改善など）を付けます。

```powershell
# git-cliff の入手（どれか 1 つ）
cargo install git-cliff
winget install git-cliff
mise use -g git-cliff

# 前回のタグ以降（未リリースの分）の下書き
git cliff --unreleased

# 次の版の名前を付けた下書き
git cliff --unreleased --tag vx.y.z
```

1. 出力された下書きを `CHANGELOG.md` の `## [Unreleased]` の下にある内容と見比べ、利用者に関係する変更だけを、利用者の言葉で書き直して `## [x.y.z] - 日付` の節にします。
2. `## [Unreleased]` は空にして残します。
3. 節の見出しは、タグ名から `v` を除いた版番号（`## [0.2.0] - 2026-11-01`）にします。`release.yml` はタグ名からこの節を探します。

下書きの精度はコミットメッセージの書き方に依存します（[コミット規約](./development.md#コミット規約)）。

## release.yml の流れ

タグ `v*` の push で、`windows-latest` 上で次を順に行います。

1. 依存の取得、Rust と Node.js の準備。
2. UI のビルド（`ui/dist` は `hitoiki-app` に埋め込まれる）。
3. バージョンの検査: タグと `tauri.conf.json` の `version` の一致。
4. CHANGELOG の節の抽出: タグ名から該当する版の節を取り出し、GitHub Release の本文にします。節が無い、または空なら失敗します。
5. CLI とアプリのビルド（ポータブル版 zip 用）。
6. CLI をサイドカーとして `crates/hitoiki-app/binaries/` に配置。
7. NSIS インストーラーのビルド。
8. 成果物の梱包: `hitoiki-<ver>-windows-x64-setup.exe` と、`hitoiki.exe`・`hitoiki-app.exe`・`README.md`・`CHANGELOG.md`・`docs/`・ライセンスを含む `hitoiki-<ver>-windows-x64.zip`。
9. GitHub Release への添付（本文は手順 4 で抽出した CHANGELOG の節）。

## インストーラーをローカルでビルドする

リリースと同じ手順を手元で行います（PowerShell）。

```powershell
# 1. UI をビルドする
cd ui; npm ci; npm run build; cd ..

# 2. CLI をビルドし、サイドカーとして配置する（Tauri はターゲットトリプル付きの名前を要求する）
cargo build --release -p hitoiki-cli
New-Item -ItemType Directory -Force crates/hitoiki-app/binaries | Out-Null
Copy-Item target/release/hitoiki.exe crates/hitoiki-app/binaries/hitoiki-x86_64-pc-windows-msvc.exe

# 3. NSIS インストーラーを作る
cd crates/hitoiki-app
../../ui/node_modules/.bin/tauri build --bundles nsis --config tauri.bundle.conf.json
```

生成物は `target/release/bundle/nsis/` に出力されます。

注意点:

- `externalBin`（サイドカー）は `tauri.bundle.conf.json` にだけ書きます。`tauri.conf.json` に書くと、サイドカーが無い状態の `cargo build` / `cargo test` が失敗します。
- 製品名（`productName`）は `HitoikiApp` です。`Hitoiki` にすると、既定のインストール先が `%LOCALAPPDATA%\hitoiki`（データ置き場）と衝突します。
- 自動起動エントリ名は `crates/hitoiki-app/src/lib.rs` で `Hitoiki` に固定しています。インストーラーのフック（`crates/hitoiki-app/windows/hitoiki-path.ps1`）もこの名前を削除します。
- 実行ファイル名は `mainBinaryName` で `hitoiki-app.exe` に固定します（収集器が自己除外を exe 名で判定するため）。

インストーラーの仕様の詳細は [アーキテクチャ](./architecture.md) の「配布とライセンス」を参照してください。
