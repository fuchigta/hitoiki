# Hitoiki インストーラー用: ユーザー PATH とログオン時自動起動の登録を操作する。
# 管理者権限は不要（HKCU のみ触る）。インストール先は環境変数 HITOIKI_DIR で受け取る。
#   -Action add    : HITOIKI_DIR をユーザー PATH に追加する（既にあれば何もしない）
#   -Action remove : HITOIKI_DIR をユーザー PATH から除去する
#   -RemoveAutostart : remove と併用。ログオン時の自動起動エントリも削除する
param(
    [Parameter(Mandatory = $true)][ValidateSet('add', 'remove')][string]$Action,
    [switch]$RemoveAutostart
)
$ErrorActionPreference = 'Stop'

$dir = $env:HITOIKI_DIR
if ([string]::IsNullOrWhiteSpace($dir)) { exit 2 }
$dir = $dir.TrimEnd('\')

# GetEnvironmentVariable('Path','User') だと %VAR% が展開された値になり、書き戻すと
# 元の REG_EXPAND_SZ の内容を壊す。レジストリを直接、展開なしで読み書きする。
$key = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey('Environment', $true)
try {
    $raw = $key.GetValue('Path', '', [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)
    $parts = @($raw -split ';' | Where-Object { $_ -ne '' })
    $others = @($parts | Where-Object { $_.TrimEnd('\') -ine $dir })
    $present = $others.Count -ne $parts.Count

    if ($Action -eq 'add') {
        if (-not $present) {
            $new = ($parts + $dir) -join ';'
            $key.SetValue('Path', $new, [Microsoft.Win32.RegistryValueKind]::ExpandString)
        }
    } else {
        if ($present) {
            $key.SetValue('Path', ($others -join ';'), [Microsoft.Win32.RegistryValueKind]::ExpandString)
        }
    }
} finally {
    $key.Close()
}

if ($Action -eq 'remove' -and $RemoveAutostart) {
    # tauri-plugin-autostart（auto-launch）は Run キーに製品名 "Hitoiki" で登録する。
    foreach ($sub in 'Software\Microsoft\Windows\CurrentVersion\Run',
                     'Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run') {
        $k = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey($sub, $true)
        if ($null -ne $k) {
            try { $k.DeleteValue('Hitoiki', $false) } finally { $k.Close() }
        }
    }
}
