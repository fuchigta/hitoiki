//! ロック/スリープの収集（6.2）。非表示のトップレベルウインドウで
//! `WM_WTSSESSION_CHANGE` と `WM_POWERBROADCAST` を受ける。
//! （`HWND_MESSAGE` のウインドウにはセッション通知が届かないため通常ウインドウを使う。）

use crossbeam_channel::Sender;
use hitoiki_collect::{SessionChange, SessionCollector, StopToken};
use hitoiki_core::Clock;
use std::cell::RefCell;
use std::sync::Arc;
use windows::core::w;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::RemoteDesktop::{
    WTSRegisterSessionNotification, WTSUnRegisterSessionNotification, NOTIFY_FOR_THIS_SESSION,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW, KillTimer, RegisterClassW,
    SetTimer, TranslateMessage, UnregisterClassW, MSG, WINDOW_EX_STYLE, WINDOW_STYLE, WM_POWERBROADCAST,
    WM_TIMER, WM_WTSSESSION_CHANGE, WNDCLASSW,
};

const WTS_SESSION_LOCK: usize = 0x7;
const WTS_SESSION_UNLOCK: usize = 0x8;
const PBT_APMSUSPEND: usize = 0x4;
const PBT_APMRESUMESUSPEND: usize = 0x7;
const PBT_APMRESUMEAUTOMATIC: usize = 0x12;
const TICK_MS: u32 = 250;

pub struct WinSessionCollector {
    clock: Arc<dyn Clock>,
}

impl WinSessionCollector {
    pub fn new(clock: Arc<dyn Clock>) -> Self {
        Self { clock }
    }
}

struct State {
    tx: Sender<SessionChange>,
    clock: Arc<dyn Clock>,
    closed: bool,
}

thread_local! {
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
}

fn emit(f: impl FnOnce(i64) -> SessionChange) {
    STATE.with(|s| {
        if let Some(st) = s.borrow_mut().as_mut() {
            if st.tx.send(f(st.clock.now_ms())).is_err() {
                st.closed = true;
            }
        }
    });
}

unsafe extern "system" fn wnd_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_WTSSESSION_CHANGE => match wp.0 {
            WTS_SESSION_LOCK => emit(|at| SessionChange::Locked { at }),
            WTS_SESSION_UNLOCK => emit(|at| SessionChange::Unlocked { at }),
            _ => {}
        },
        WM_POWERBROADCAST => match wp.0 {
            PBT_APMSUSPEND => emit(|at| SessionChange::Suspended { at }),
            PBT_APMRESUMEAUTOMATIC | PBT_APMRESUMESUSPEND => emit(|at| SessionChange::Resumed { at }),
            _ => {}
        },
        _ => return DefWindowProcW(hwnd, msg, wp, lp),
    }
    LRESULT(1) // WM_POWERBROADCAST は TRUE を返す。他は無視される。
}

impl SessionCollector for WinSessionCollector {
    fn run(&self, tx: Sender<SessionChange>, stop: StopToken) -> anyhow::Result<()> {
        STATE.with(|s| {
            *s.borrow_mut() = Some(State {
                tx,
                clock: self.clock.clone(),
                closed: false,
            })
        });
        // SAFETY: ウインドウ・タイマー・通知登録はこのスレッドで作成し、
        // 同じスレッドのメッセージループで処理し、戻る前に必ず解除する。
        let r = unsafe { message_loop(&stop) };
        STATE.with(|s| *s.borrow_mut() = None);
        r
    }
}

unsafe fn message_loop(stop: &StopToken) -> anyhow::Result<()> {
    let hinst = GetModuleHandleW(None)?;
    let class = w!("HitoikiSessionWatcher");
    let wc = WNDCLASSW {
        lpfnWndProc: Some(wnd_proc),
        hInstance: hinst.into(),
        lpszClassName: class,
        ..Default::default()
    };
    if RegisterClassW(&wc) == 0 {
        anyhow::bail!("RegisterClassW に失敗しました");
    }
    // WS_OVERLAPPED のまま ShowWindow しないので表示されない。
    let hwnd = CreateWindowExW(
        WINDOW_EX_STYLE(0),
        class,
        w!("Hitoiki session watcher"),
        WINDOW_STYLE(0),
        0,
        0,
        0,
        0,
        None,
        None,
        Some(hinst.into()),
        None,
    );
    let hwnd = match hwnd {
        Ok(h) => h,
        Err(e) => {
            let _ = UnregisterClassW(class, Some(hinst.into()));
            return Err(e.into());
        }
    };
    let registered = WTSRegisterSessionNotification(hwnd, NOTIFY_FOR_THIS_SESSION);
    let timer = SetTimer(Some(hwnd), 1, TICK_MS, None);

    let mut res = Ok(());
    if let Err(e) = registered {
        res = Err(anyhow::anyhow!("WTSRegisterSessionNotification に失敗: {e}"));
    } else {
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).0 > 0 {
            if msg.message == WM_TIMER {
                if stop.is_stopped() || STATE.with(|s| s.borrow().as_ref().is_none_or(|s| s.closed)) {
                    break;
                }
                continue;
            }
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        let _ = WTSUnRegisterSessionNotification(hwnd);
    }
    if timer != 0 {
        let _ = KillTimer(Some(hwnd), 1);
    }
    let _ = DestroyWindow(hwnd);
    let _ = UnregisterClassW(class, Some(hinst.into()));
    res
}
