; Hitoiki の NSIS インストーラーフック（Tauri v2 の bundle.windows.nsis.installerHooks）。
;
; - インストール先をユーザー PATH（HKCU\Environment\Path）に追加し、アンインストールで除去する。
; - アンインストール時（更新を除く）に自動起動（HKCU の Run、値名 "Hitoiki"）を削除する。
; - ユーザーデータ（%APPDATA%\hitoiki、%LOCALAPPDATA%\hitoiki）には触れない。
;
; PATH の更新は NSIS の文字列（NSIS_MAX_STRLEN）で扱うと長い PATH を壊すため、
; PowerShell スクリプト（hitoiki-path.ps1）に任せる。管理者権限は不要。

!macro HITOIKI_PATH_ACTION SCRIPT ARGS
  ; インストール先はスクリプトへ環境変数で渡す。
  System::Call 'Kernel32::SetEnvironmentVariable(t "HITOIKI_DIR", t "$INSTDIR")'
  nsExec::ExecToLog '"$SYSDIR\WindowsPowerShell\v1.0\powershell.exe" -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "${SCRIPT}" ${ARGS}'
  Pop $0
  ; 環境変数の変更を実行中のシェルなどへ通知する（WM_SETTINGCHANGE）。
  System::Call 'user32::SendMessageTimeoutW(p 0xffff, i 0x1A, p 0, t "Environment", i 2, i 5000, *p .r0)'
!macroend

; スクリプトは tauri.conf.json の bundle.resources でインストール先に置かれる。
!macro NSIS_HOOK_POSTINSTALL
  !insertmacro HITOIKI_PATH_ACTION "$INSTDIR\hitoiki-path.ps1" "-Action add"
!macroend

; アンインストール本体がファイル（スクリプト自身を含む）を消す前に、一時フォルダへ退避しておく。
; 実行は POSTUNINSTALL で行う（実行中アプリの確認で中断された場合に PATH だけ消えるのを避ける）。
!macro NSIS_HOOK_PREUNINSTALL
  CopyFiles /SILENT "$INSTDIR\hitoiki-path.ps1" "$TEMP\hitoiki-path-uninstall.ps1"
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  ; 更新インストール（旧版のアンインストーラーが /UPDATE で走る）では PATH は一旦除去されるが、
  ; 直後の POSTINSTALL で再追加される。自動起動の登録は更新時には消さない。
  StrCmp $UpdateMode 1 hitoiki_update hitoiki_remove_all
  hitoiki_remove_all:
    !insertmacro HITOIKI_PATH_ACTION "$TEMP\hitoiki-path-uninstall.ps1" "-Action remove -RemoveAutostart"
    Goto hitoiki_post_done
  hitoiki_update:
    !insertmacro HITOIKI_PATH_ACTION "$TEMP\hitoiki-path-uninstall.ps1" "-Action remove"
  hitoiki_post_done:
  Delete "$TEMP\hitoiki-path-uninstall.ps1"
!macroend
