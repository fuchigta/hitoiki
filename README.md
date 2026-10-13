# Hitoiki（一息）

> **開発中です。** まだ機能は揃っておらず、仕様・コマンドは変わる可能性があります。

「一息つきませんか」と寄り添って声をかける、個人用の自己管理ツールです。Windows 向けで、タスク・時間の使い方・モチベーション・ストレス・スキルを 1 つのタイムライン上にまとめ、記録・可視化・内省を支援します。

前面のウインドウと無操作の時間を自動で記録し、Claude Code などのコーディングエージェントとの対話時間のような「名前の無い工数」もタスクに紐づけて見えるようにします。確信が持てない判定は捨てず「要確認」として溜め、振り返りのときにあなたが確定します。時間の使い方に良し悪しの評価はつけません。

## プライバシーの要点

- **ローカル完結**: データはすべてこの PC の SQLite に保存します。同期・クラウド保存・テレメトリはありません。
- **ネットワークポートを開かない**: 待ち受けサーバはなく、常駐アプリと CLI は Windows の名前付きパイプ（作成者ユーザーのみアクセス可）で話します。
- **キー入力を記録しない**: キーの内容も回数も取得せず、低レベルフックや DLL インジェクションも使いません。管理者権限も不要です。

詳しくは [プライバシー](./docs/user/privacy.md) を参照してください。

## インストール

[Releases](https://github.com/fuchigta/hitoiki/releases) から `hitoiki-<ver>-windows-x64-setup.exe` をダウンロードして実行します（コード署名が無いため SmartScreen の警告が出たら「詳細情報」→「実行」）。管理者権限は不要です。zip のポータブル版、更新、アンインストールは [インストール](./docs/user/install.md) にあります。

## 最初の一歩

インストーラーの最後に「実行」を選ぶと、トレイに常駐して初回の案内（オンボーディング）が開きます。記録はその時点から始まっています。

```powershell
hitoiki task add "設計書のレビュー" --important   # タスクを作る
hitoiki task start <id>                           # 作業を開始する
hitoiki status                                    # 現在の状況を見る
hitoiki ui                                        # 画面を開く
```

## ドキュメント

利用者向け（`docs/user/`）:

- [インストール](./docs/user/install.md): インストーラー、zip、SmartScreen、更新、アンインストール
- [はじめに](./docs/user/getting-started.md): 初回起動とオンボーディング、最初にやること
- [日々の使い方](./docs/user/daily-use.md): 今日・実績・通知・タスク・計画・記録（振り返りを含む）・目標・レポート、トレイ
- [CLI リファレンス](./docs/user/cli.md): コマンド一覧、`--json`、終了コード、エージェントからの利用
- [設定](./docs/user/configuration.md): 設定ファイルの場所と主なキー
- [プライバシー](./docs/user/privacy.md): 記録するもの・しないもの、通信が起きる場面
- [トラブルシューティング](./docs/user/troubleshooting.md): `hitoiki doctor`、通知、記録の抜け

開発者向けは [`docs/dev/`](./docs/dev/development.md)（アーキテクチャ、開発手順、リリース手順）にあります。変更履歴は [CHANGELOG.md](./CHANGELOG.md) です。

## ライセンス

MIT License（[LICENSE-MIT](./LICENSE-MIT)）と Apache License, Version 2.0（[LICENSE-APACHE](./LICENSE-APACHE)）のデュアルライセンスで、どちらかを選んで利用できます。
