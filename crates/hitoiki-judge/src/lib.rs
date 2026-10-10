//! 判定器（8章）。型付き質問セット → 型付き回答＋確信度の差し替え可能な IF と、
//! 外部コマンド / OpenAI 互換バックエンド、帰属判定パイプラインへのアダプタ。

mod adapter;
mod external;
mod openai;
pub mod protocol;
pub mod questions;

pub use adapter::{JudgeAdapter, JudgeContext};
pub use external::{ExternalCommandJudge, DEFAULT_TIMEOUT};
pub use openai::{build_schema, parse_chat_response, OpenAiCompatibleJudge};
pub use protocol::{
    Answer, Answers, Judge, JudgeRequest, JudgeResponse, OptionSpec, QuestionSpec, QuestionType,
    SegmentState, State, StateContext, PROTOCOL_VERSION,
};
pub use questions::{OptionSources, QuestionSet};

use anyhow::{anyhow, Context};
use hitoiki_core::config::{Config, JudgeBackend};
use hitoiki_core::pipeline::SegmentJudge;
use std::time::Duration;

/// 設定のバックエンドから判定器を作る。
pub fn backend_from_config(cfg: &Config) -> anyhow::Result<Box<dyn Judge>> {
    let name = &cfg.judge.default;
    let backend = cfg
        .judge
        .backends
        .get(name)
        .ok_or_else(|| anyhow!("judge.default が指す判定器 {name} が judge.backends にありません"))?;
    Ok(match backend {
        JudgeBackend::OpenaiCompatible {
            base_url,
            model,
            api_key_env,
            timeout_sec,
        } => Box::new(OpenAiCompatibleJudge::new(
            name.clone(),
            base_url,
            model.clone(),
            api_key_env.clone(),
            Duration::from_secs(*timeout_sec),
        )),
        JudgeBackend::ExternalCommand { command, timeout_sec } => {
            if command.is_empty() {
                return Err(anyhow!("judge.backends.{name}.command が空です"));
            }
            Box::new(ExternalCommandJudge::new(
                name.clone(),
                command.clone(),
                Duration::from_secs(*timeout_sec),
            ))
        }
    })
}

/// `judge.enabled = false` なら None。有効なら `SegmentJudge` を返す。
pub fn from_config(cfg: &Config, mut ctx: JudgeContext) -> anyhow::Result<Option<Box<dyn SegmentJudge>>> {
    if !cfg.judge.enabled {
        return Ok(None);
    }
    if ctx.activity_labels.is_empty() {
        ctx.activity_labels = cfg.activity.labels.clone();
    }
    let judge = backend_from_config(cfg).context("判定器を構築できません")?;
    Ok(Some(Box::new(JudgeAdapter::new(judge, ctx))))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_is_none() {
        let cfg = Config::default();
        assert!(from_config(&cfg, JudgeContext::new(chrono_tz::UTC))
            .unwrap()
            .is_none());
    }

    #[test]
    fn enabled_without_backend_errors() {
        let mut cfg = Config::default();
        cfg.judge.enabled = true;
        assert!(from_config(&cfg, JudgeContext::new(chrono_tz::UTC)).is_err());
    }

    #[test]
    fn enabled_with_backend() {
        let cfg: Config = toml::from_str(
            r#"
[judge]
enabled = true
default = "c"
[judge.backends.c]
type = "external-command"
command = ["x"]
"#,
        )
        .unwrap();
        assert!(from_config(&cfg, JudgeContext::new(chrono_tz::UTC))
            .unwrap()
            .is_some());
    }
}
