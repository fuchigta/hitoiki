# インストール

GitHub Releases から取得します。コード署名は付与していません。対象は Windows（x64）です。

## インストーラー（推奨）

1. [Releases](https://github.com/fuchigta/hitoiki/releases) から `hitoiki-<ver>-windows-x64-setup.exe` をダウンロードします。
2. 実行します。SmartScreen の警告が出たら「詳細情報」→「実行」を選びます（[SmartScreen](#smartscreen-の警告が出る場合) を参照）。
3. 画面の指示に従ってインストールします。管理者権限は不要で、既定のインストール先は `%LOCALAPPDATA%\Hitoiki` です。このフォルダは DB・ログの置き場（`%LOCALAPPDATA%\hitoiki`）と同じです（Windows はフォルダ名の大文字小文字を区別しません）。プログラムとデータが同じフォルダに並ぶので、**フォルダごと手で削除しないでください**（記録も消えます）。アンインストールは下記の手順で行ってください。
4. 完了画面の「実行」のチェックを入れたままにすると、そのまま起動して初回の「はじめに」画面が開きます（[はじめに](./getting-started.md)）。

インストーラーが行うこと:

- `hitoiki.exe`（CLI）と `hitoiki-app.exe`（常駐アプリ）を同じフォルダに置きます。
- インストール先を**ユーザー**の環境変数 `Path` に追加します（重複追加はしません）。新しく開いたターミナルで `hitoiki --help` が使えます。
- スタートメニューに登録します（スタートメニュー名は「Hitoiki」です）。

## zip（ポータブル版）

インストーラーを使わず、展開するだけで使うこともできます。

1. [Releases](https://github.com/fuchigta/hitoiki/releases) から `hitoiki-<ver>-windows-x64.zip` をダウンロードします。
2. ダウンロードした zip を右クリックし、「プロパティ」を開きます。
3. 「全般」タブ下部の「セキュリティ」欄にある **「ブロックの解除」** にチェックを入れ、「OK」を押します。
4. zip を任意のフォルダ（例: `C:\Tools\hitoiki`）に展開します。`hitoiki.exe` と `hitoiki-app.exe` は同じフォルダに置いたままにしてください（`hitoiki ui` と `hitoiki daemon` は同じフォルダの `hitoiki-app.exe` を探します）。
5. 展開したフォルダを PATH に追加します（「環境変数」→ユーザー環境変数の `Path` に追加）。
6. 新しいターミナルを開き、`hitoiki --help` で動作を確認します。

## SmartScreen の警告が出る場合

コード署名がないため、実行時に「Windows によって PC が保護されました」と表示されることがあります。その場合は「詳細情報」→「実行」を選んでください（zip の場合は上記の「ブロックの解除」でも回避できます）。ダウンロード元が上記の公式 Releases であることを確認してから実行してください。

## 更新

自動更新はありません。

- 新しいインストーラーをそのまま実行すると、上書きインストールできます。データ（設定・DB）はそのまま残ります。
- 新しい版があるか確認するには `hitoiki version --check` を実行します。GitHub Releases の最新版を問い合わせます。
- 設定 `general.update_check` を `true` にすると、常駐アプリが 1 日 1 回、自動で最新版を確認します（既定は未設定で、確認しません）。新しい版があると、トーストとトレイメニュー「新しいバージョン vX.Y.Z をダウンロード…」でお知らせし、押すとリリースページをブラウザで開きます。`hitoiki status` にも表示されます。ダウンロードとインストールは自動では行いません。詳しくは [設定](./configuration.md#更新の確認) と [プライバシー](./privacy.md) を参照してください。
- zip 版は、新しい zip の `hitoiki.exe` と `hitoiki-app.exe` を同じフォルダに上書きします。常駐アプリが動いている場合は、トレイの「終了」で止めてから上書きしてください。

## アンインストール

「設定 > アプリ > インストールされているアプリ」から行います。

- **消えるもの**: インストーラーが置いたファイル（`hitoiki-app.exe`、`hitoiki.exe` など）、スタートメニューのショートカット、ユーザー `Path` に追加したエントリ、ログオン時の自動起動登録（`general.autostart`）。
- **残るもの**: Hitoiki のデータは消しません。設定とプラグイン（`%APPDATA%\hitoiki`）、DB とログ（`%LOCALAPPDATA%\hitoiki`）が残ります。インストール先とデータ置き場は同じフォルダなので、アンインストール後もこのフォルダは残ります（意図した動作です）。
- アンインストール画面の「アプリデータを削除」チェックは、Tauri の識別子（`dev.hitoiki.app`）名のフォルダ（`%APPDATA%\dev.hitoiki.app`、`%LOCALAPPDATA%\dev.hitoiki.app`、WebView2 のキャッシュなど）だけが対象です。Hitoiki の DB・設定は対象外なので、チェックしても消えません。
- DB ごと消したい場合は、上記の `%APPDATA%\hitoiki` と `%LOCALAPPDATA%\hitoiki` を手動で削除してください。

zip 版は、フォルダを削除し、`Path` に追加していれば外します。自動起動を有効にしていた場合は、`hitoiki config set general.autostart false` にしてから常駐アプリを一度起動し直すと、ログオン時の自動起動登録（レジストリ `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` の値名 `Hitoiki`）が外れます（設定は起動時に反映されます）。
