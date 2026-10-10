//! テスト用コレクタ。与えたイベントを順に流して戻る。

use crate::{
    ForegroundChange, ForegroundCollector, IdleChange, IdleCollector, SessionChange, SessionCollector,
    StopToken,
};
use crossbeam_channel::Sender;

#[derive(Debug, Clone)]
pub struct MockCollector<T> {
    pub events: Vec<T>,
}

impl<T> MockCollector<T> {
    pub fn new(events: Vec<T>) -> Self {
        Self { events }
    }
}

impl<T: Clone> MockCollector<T> {
    fn emit(&self, tx: &Sender<T>, stop: &StopToken) -> anyhow::Result<()> {
        for e in &self.events {
            if stop.is_stopped() || tx.send(e.clone()).is_err() {
                break;
            }
        }
        Ok(())
    }
}

impl ForegroundCollector for MockCollector<ForegroundChange> {
    fn run(&self, tx: Sender<ForegroundChange>, stop: StopToken) -> anyhow::Result<()> {
        self.emit(&tx, &stop)
    }
}

impl IdleCollector for MockCollector<IdleChange> {
    fn run(&self, tx: Sender<IdleChange>, stop: StopToken) -> anyhow::Result<()> {
        self.emit(&tx, &stop)
    }
}

impl SessionCollector for MockCollector<SessionChange> {
    fn run(&self, tx: Sender<SessionChange>, stop: StopToken) -> anyhow::Result<()> {
        self.emit(&tx, &stop)
    }
}
