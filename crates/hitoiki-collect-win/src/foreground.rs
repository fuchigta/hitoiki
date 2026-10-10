//! 前面ウインドウの収集（6.1）。WinEventHook + ポーリング補正、専用メッセージループスレッド。

use crossbeam_channel::Sender;
use hitoiki_collect::{ForegroundChange, ForegroundCollector, StopToken};
use hitoiki_core::Clock;
use std::cell::RefCell;
use std::sync::Arc;
use std::time::{Duration, Instant};
use windows::core::PWSTR;
use windows::Win32::Foundation::{CloseHandle, HWND};
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::Accessibility::{SetWinEventHook, UnhookWinEvent, HWINEVENTHOOK};
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, GetForegroundWindow, GetMessageW, GetWindowTextLengthW, GetWindowTextW,
    GetWindowThreadProcessId, KillTimer, SetTimer, TranslateMessage, EVENT_OBJECT_NAMECHANGE,
    EVENT_SYSTEM_FOREGROUND, MSG, OBJID_WINDOW, WINEVENT_OUTOFCONTEXT, WINEVENT_SKIPOWNPROCESS, WM_TIMER,
};

/// 停止要求の確認間隔（ミリ秒）。
const TICK_MS: u32 = 250;

pub struct WinForegroundCollector {
    clock: Arc<dyn Clock>,
    poll_interval: Duration,
}

impl WinForegroundCollector {
    pub fn new(clock: Arc<dyn Clock>, poll_interval_sec: u64) -> Self {
        Self {
            clock,
            poll_interval: Duration::from_secs(poll_interval_sec.max(1)),
        }
    }
}

/// フックのコールバックは文脈を持てないので、メッセージループのスレッドに状態を置く。
struct State {
    tx: Sender<ForegroundChange>,
    clock: Arc<dyn Clock>,
    last: Option<(isize, String)>,
    closed: bool,
}

thread_local! {
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
}

impl ForegroundCollector for WinForegroundCollector {
    fn run(&self, tx: Sender<ForegroundChange>, stop: StopToken) -> anyhow::Result<()> {
        STATE.with(|s| {
            *s.borrow_mut() = Some(State {
                tx,
                clock: self.clock.clone(),
                last: None,
                closed: false,
            })
        });
        // SAFETY: フックとタイマーはこのスレッドで作成し、同じスレッドのメッセージループで
        // 処理し、戻る前に必ず解除する。コールバックは thread_local の状態のみ触る。
        let result = unsafe { message_loop(self.poll_interval, &stop) };
        STATE.with(|s| *s.borrow_mut() = None);
        result
    }
}

unsafe fn message_loop(poll_interval: Duration, stop: &StopToken) -> anyhow::Result<()> {
    let flags = WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS;
    let h_fg = SetWinEventHook(
        EVENT_SYSTEM_FOREGROUND,
        EVENT_SYSTEM_FOREGROUND,
        None,
        Some(win_event_proc),
        0,
        0,
        flags,
    );
    let h_name = SetWinEventHook(
        EVENT_OBJECT_NAMECHANGE,
        EVENT_OBJECT_NAMECHANGE,
        None,
        Some(win_event_proc),
        0,
        0,
        flags,
    );
    if h_fg.is_invalid() || h_name.is_invalid() {
        for h in [h_fg, h_name] {
            if !h.is_invalid() {
                let _ = UnhookWinEvent(h);
            }
        }
        anyhow::bail!("SetWinEventHook に失敗しました");
    }
    let timer = SetTimer(None, 0, TICK_MS, None);

    // 起動直後の前面ウインドウ。
    check(GetForegroundWindow());
    let mut last_poll = Instant::now();
    let mut msg = MSG::default();
    while GetMessageW(&mut msg, None, 0, 0).0 > 0 {
        if msg.message == WM_TIMER {
            if stop.is_stopped() || STATE.with(|s| s.borrow().as_ref().is_none_or(|s| s.closed)) {
                break;
            }
            if last_poll.elapsed() >= poll_interval {
                last_poll = Instant::now();
                check(GetForegroundWindow());
            }
            continue;
        }
        let _ = TranslateMessage(&msg);
        DispatchMessageW(&msg);
    }
    if timer != 0 {
        let _ = KillTimer(None, timer);
    }
    let _ = UnhookWinEvent(h_fg);
    let _ = UnhookWinEvent(h_name);
    Ok(())
}

unsafe extern "system" fn win_event_proc(
    _hook: HWINEVENTHOOK,
    event: u32,
    hwnd: HWND,
    id_object: i32,
    _id_child: i32,
    _thread: u32,
    _time: u32,
) {
    // 名前変更は前面ウインドウ自身（OBJID_WINDOW）のものだけ処理する。
    let relevant = event == EVENT_SYSTEM_FOREGROUND
        || (event == EVENT_OBJECT_NAMECHANGE && id_object == OBJID_WINDOW.0 && hwnd == GetForegroundWindow());
    if relevant {
        check(hwnd);
    }
}

/// `hwnd` の現在の状態を読み、前回と違えば送る。
unsafe fn check(hwnd: HWND) {
    if hwnd.0.is_null() {
        return;
    }
    let title = window_title(hwnd);
    let key = (hwnd.0 as isize, title);
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        let Some(st) = s.as_mut() else { return };
        if st.last.as_ref() == Some(&key) {
            return;
        }
        let (pid, exe_path) = process_info(hwnd);
        let exe_name = exe_path
            .as_deref()
            .and_then(|p| p.rsplit(['\\', '/']).next())
            .map(str::to_owned);
        let ev = ForegroundChange {
            at: st.clock.now_ms(),
            hwnd: format!("{:#x}", key.0),
            pid: pid.map(i64::from),
            exe_path,
            exe_name,
            title: key.1.clone(),
        };
        st.last = Some(key);
        if st.tx.send(ev).is_err() {
            st.closed = true;
        }
    });
}

unsafe fn window_title(hwnd: HWND) -> String {
    let len = GetWindowTextLengthW(hwnd);
    if len <= 0 {
        return String::new();
    }
    let mut buf = vec![0u16; len as usize + 1];
    let n = GetWindowTextW(hwnd, &mut buf);
    String::from_utf16_lossy(&buf[..n.max(0) as usize])
}

/// (pid, 実行ファイルのフルパス)。権限不足などで取れない部分は None。
unsafe fn process_info(hwnd: HWND) -> (Option<u32>, Option<String>) {
    let mut pid = 0u32;
    GetWindowThreadProcessId(hwnd, Some(&mut pid));
    if pid == 0 {
        return (None, None);
    }
    let Ok(h) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
        return (Some(pid), None);
    };
    let mut buf = [0u16; 1024];
    let mut len = buf.len() as u32;
    let ok = QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len);
    let _ = CloseHandle(h);
    let path = ok.ok().map(|_| String::from_utf16_lossy(&buf[..len as usize]));
    (Some(pid), path)
}
