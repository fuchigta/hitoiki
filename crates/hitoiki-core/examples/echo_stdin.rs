//! テスト用の AI コマンド代替。標準入力をそのまま標準出力へ返す。
//! 引数 `fail` で終了コード 3、`sleep` で長時間待機する。

use std::io::{Read, Write};

fn main() {
    let mode = std::env::args().nth(1).unwrap_or_default();
    let mut buf = Vec::new();
    std::io::stdin().read_to_end(&mut buf).unwrap();
    match mode.as_str() {
        "fail" => {
            eprintln!("失敗しました");
            std::process::exit(3);
        }
        "sleep" => std::thread::sleep(std::time::Duration::from_secs(60)),
        _ => {}
    }
    std::io::stdout().write_all(&buf).unwrap();
}
