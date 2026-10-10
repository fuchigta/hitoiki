//! 無操作の収集（6.2）。`GetLastInputInfo` を 1 秒間隔でポーリングする。
//! 入力の内容・回数は取得しない（最終入力時刻のみ）。

use crossbeam_channel::Sender;
use hitoiki_collect::{IdleChange, IdleCollector, StopToken};
use hitoiki_core::Clock;
use std::mem::size_of;
use std::sync::Arc;
use std::time::Duration;
use windows::Win32::System::SystemInformation::GetTickCount64;
use windows::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};

pub struct WinIdleCollector {
    clock: Arc<dyn Clock>,
    threshold_ms: u64,
}

impl WinIdleCollector {
    pub fn new(clock: Arc<dyn Clock>, threshold_sec: u64) -> Self {
        Self {
            clock,
            threshold_ms: threshold_sec.saturating_mul(1000),
        }
    }
}

/// 最終入力からの経過ミリ秒。取得できなければ None。
fn idle_elapsed_ms() -> Option<u64> {
    let mut info = LASTINPUTINFO {
        cbSize: size_of::<LASTINPUTINFO>() as u32,
        dwTime: 0,
    };
    // SAFETY: cbSize を設定した有効な構造体へのポインタを渡している。
    if !unsafe { GetLastInputInfo(&mut info) }.as_bool() {
        return None;
    }
    // dwTime は 32 ビットの tick カウント。下位 32 ビット同士で引けば周回しても正しい。
    let now = unsafe { GetTickCount64() } as u32;
    Some(u64::from(now.wrapping_sub(info.dwTime)))
}

impl IdleCollector for WinIdleCollector {
    fn run(&self, tx: Sender<IdleChange>, stop: StopToken) -> anyhow::Result<()> {
        let mut idle = false;
        while !stop.sleep(Duration::from_secs(1)) {
            let Some(elapsed) = idle_elapsed_ms() else {
                continue;
            };
            let now = self.clock.now_ms();
            let ev = if !idle && elapsed >= self.threshold_ms {
                idle = true;
                Some(IdleChange::Started {
                    at: now - elapsed as i64,
                })
            } else if idle && elapsed < self.threshold_ms {
                idle = false;
                Some(IdleChange::Ended { at: now })
            } else {
                None
            };
            if let Some(ev) = ev {
                if tx.send(ev).is_err() {
                    break;
                }
            }
        }
        Ok(())
    }
}
