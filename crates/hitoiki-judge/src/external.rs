//! 外部コマンド判定器（8.3）。標準入力に JSON、標準出力から JSON。

use crate::protocol::{Judge, JudgeRequest, JudgeResponse};
use anyhow::{anyhow, bail, Context};
use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);

pub struct ExternalCommandJudge {
    name: String,
    command: Vec<String>,
    timeout: Duration,
}

impl ExternalCommandJudge {
    pub fn new(name: impl Into<String>, command: Vec<String>, timeout: Duration) -> Self {
        Self {
            name: name.into(),
            command,
            timeout,
        }
    }
}

impl Judge for ExternalCommandJudge {
    fn name(&self) -> &str {
        &self.name
    }

    fn ask(&self, req: &JudgeRequest) -> anyhow::Result<JudgeResponse> {
        let input = serde_json::to_vec(req)?;
        let out = run_command(&self.command, &input, self.timeout)?;
        // 前後に余計な出力があっても、JSON 全体が解釈できなければエラーにする。
        serde_json::from_slice(&out).context("判定器の出力が JSON として不正です")
    }
}

/// コマンドを起動して stdin を渡し、stdout を返す。タイムアウトで kill。
pub(crate) fn run_command(command: &[String], input: &[u8], timeout: Duration) -> anyhow::Result<Vec<u8>> {
    let (prog, args) = command
        .split_first()
        .ok_or_else(|| anyhow!("判定コマンドが空です"))?;
    let mut cmd = Command::new(prog);
    cmd.args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let mut child = cmd
        .spawn()
        .with_context(|| format!("判定コマンドを起動できません: {prog}"))?;

    // 子が stdin を読まずに居座っても詰まらないよう、書き込みは別スレッドで行う。
    let mut stdin = child.stdin.take().expect("piped");
    let data = input.to_vec();
    let writer = std::thread::spawn(move || {
        let _ = stdin.write_all(&data);
        // drop で閉じる
    });
    let mut stdout = child.stdout.take().expect("piped");
    let reader = std::thread::spawn(move || {
        let mut buf = vec![];
        let _ = stdout.read_to_end(&mut buf);
        buf
    });

    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait()? {
            Some(s) => break s,
            None if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = writer.join();
                let _ = reader.join();
                bail!("判定コマンドがタイムアウトしました ({}秒)", timeout.as_secs_f64());
            }
            None => std::thread::sleep(Duration::from_millis(10)),
        }
    };
    let _ = writer.join();
    let out = reader.join().map_err(|_| anyhow!("出力の読み取りに失敗"))?;
    if !status.success() {
        bail!("判定コマンドが異常終了しました: {status}");
    }
    Ok(out)
}
