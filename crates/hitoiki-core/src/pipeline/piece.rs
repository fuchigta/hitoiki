//! パイプライン内部で使う作業用の区間表現と、分割ヘルパー。

use crate::model::{SegmentKind, SegmentStatus, WindowSpan};
use crate::Millis;

#[derive(Debug, Clone)]
pub(crate) struct Piece {
    pub start: Millis,
    pub end: Millis,
    pub kind: SegmentKind,
    /// 根拠ウインドウ（`PipelineInput::windows` の添字）。無操作区間は None。
    pub win: Option<usize>,
    pub task_id: Option<String>,
    pub label: Option<String>,
    pub confidence: Option<f64>,
    pub decided_by: Option<String>,
    /// None は「確信度で後から決める」。
    pub status: Option<SegmentStatus>,
    pub ids: Vec<String>,
    pub session: Option<String>,
    pub exe: Option<String>,
    pub title: Option<String>,
    /// 対話・会議・確定などで確定済み（ルール判定やフォールバックの対象外）。
    pub fixed: bool,
    /// exe/title の代表値を選ぶための、元になった最長区間の長さ。
    pub rep_len: Millis,
    /// 通過系アプリの短い連続（`transient::mark` が付ける吸収の候補）。
    pub transient: bool,
    /// 無視するアプリ（`collect.ignore_exes`）の区間。吸収されなかったものは `ignored` にする。
    pub ignore: bool,
    /// PC 外で過ごした時間（確定の `off_pc`）。
    pub off_pc: bool,
}

impl Piece {
    pub fn from_window(start: Millis, end: Millis, idx: usize, w: &WindowSpan) -> Self {
        Self {
            start,
            end,
            kind: SegmentKind::Work,
            win: Some(idx),
            task_id: None,
            label: None,
            confidence: None,
            decided_by: None,
            status: None,
            ids: vec![w.id.clone()],
            session: None,
            exe: w.exe_name.clone(),
            title: if w.excluded { None } else { w.title.clone() },
            fixed: false,
            rep_len: end - start,
            transient: false,
            ignore: false,
            off_pc: false,
        }
    }

    pub fn idle(start: Millis, end: Millis) -> Self {
        Self {
            start,
            end,
            kind: SegmentKind::Idle,
            win: None,
            task_id: None,
            label: None,
            confidence: None,
            decided_by: None,
            status: None,
            ids: vec![],
            session: None,
            exe: None,
            title: None,
            fixed: false,
            rep_len: end - start,
            transient: false,
            ignore: false,
            off_pc: false,
        }
    }

    pub fn len(&self) -> Millis {
        self.end - self.start
    }
}

/// `t` が区間の内側にあれば、その区間を `t` で 2 つに分ける。
pub(crate) fn split_at(ps: &mut Vec<Piece>, t: Millis) {
    if let Some(i) = ps.iter().position(|p| p.start < t && t < p.end) {
        let mut r = ps[i].clone();
        r.start = t;
        ps[i].end = t;
        ps[i].rep_len = ps[i].len();
        r.rep_len = r.len();
        ps.insert(i + 1, r);
    }
}

/// [s, e) で区間を切り、その内側にあって `pred` を満たす区間に `f` を適用する。
pub(crate) fn relabel(
    ps: &mut Vec<Piece>,
    s: Millis,
    e: Millis,
    pred: impl Fn(&Piece) -> bool,
    mut f: impl FnMut(&mut Piece),
) {
    if s >= e {
        return;
    }
    split_at(ps, s);
    split_at(ps, e);
    for p in ps.iter_mut() {
        if p.start >= s && p.end <= e && pred(p) {
            f(p);
        }
    }
}
