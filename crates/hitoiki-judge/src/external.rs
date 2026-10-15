//! 外部コマンド型の自動判定（docs/dev/architecture.md「自動判定」）。標準入力に JSON、標準出力から JSON。

use crate::protocol::{Answers, Judge, JudgeRequest, JudgeResponse, QuestionSpec, State, PROTOCOL_VERSION};
use anyhow::{anyhow, bail, Context};
use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);
/// バッチモードの 1 回あたり最大件数。
pub const DEFAULT_BATCH_SIZE: usize = 20;

pub struct ExternalCommandJudge {
    name: String,
    command: Vec<String>,
    timeout: Duration,
    /// Some(n) ならバッチモード（最大 n 件ずつ送る）。
    batch_size: Option<usize>,
}

impl ExternalCommandJudge {
    pub fn new(name: impl Into<String>, command: Vec<String>, timeout: Duration) -> Self {
        Self {
            name: name.into(),
            command,
            timeout,
            batch_size: None,
        }
    }

    /// バッチモードを有効にする（`states` 配列で最大 `max` 件ずつ送る）。
    pub fn with_batch(mut self, max: usize) -> Self {
        self.batch_size = Some(max.max(1));
        self
    }

    fn ask_chunk(&self, states: &[State], questions: &[QuestionSpec]) -> anyhow::Result<Vec<Answers>> {
        let req = JudgeRequest {
            version: PROTOCOL_VERSION,
            state: None,
            states: Some(states.to_vec()),
            questions: questions.to_vec(),
        };
        let results = self
            .ask(&req)?
            .results
            .ok_or_else(|| anyhow!("バッチ応答に results がありません"))?;
        if results.len() != states.len() {
            bail!(
                "バッチ応答の件数が一致しません (送信 {} / 受信 {})",
                states.len(),
                results.len()
            );
        }
        Ok(results.into_iter().map(|r| r.answers).collect())
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
        serde_json::from_slice(&out).context("自動判定の出力が JSON として不正です")
    }

    fn ask_batch(&self, states: &[State], questions: &[QuestionSpec]) -> Vec<Option<Answers>> {
        let Some(max) = self.batch_size else {
            return states
                .iter()
                .map(
                    |s| match self.ask(&JudgeRequest::single(s.clone(), questions.to_vec())) {
                        Ok(r) => r.answers,
                        Err(e) => {
                            tracing::warn!("判定の接続先 {} の問い合わせに失敗: {e:#}", self.name);
                            None
                        }
                    },
                )
                .collect();
        };
        let mut out = Vec::with_capacity(states.len());
        for chunk in states.chunks(max) {
            match self.ask_chunk(chunk, questions) {
                Ok(rs) => out.extend(rs.into_iter().map(Some)),
                Err(e) => {
                    tracing::warn!("判定の接続先 {} のバッチ問い合わせに失敗: {e:#}", self.name);
                    out.extend(chunk.iter().map(|_| None));
                }
            }
        }
        out
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
