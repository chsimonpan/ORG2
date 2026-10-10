//! Content-free monotonic observations; provider_call includes adapter work, not pure network.
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;
static NEXT: AtomicU64 = AtomicU64::new(1);
tokio::task_local! { static REQUEST: u64; }
pub(crate) fn next_request() -> u64 {
    REQUEST.try_with(|id| *id).unwrap_or_else(|_| NEXT.fetch_add(1, Ordering::Relaxed))
}
pub(crate) async fn correlated<F: std::future::Future>(request: u64, future: F) -> F::Output {
    REQUEST.scope(request, future).await
}
pub(crate) struct Phase { request: u64, phase: &'static str, start: Instant, done: bool }
impl Phase {
    pub(crate) fn new(request: u64, phase: &'static str) -> Self {
        tracing::info!(request, phase, status = "started", "[compaction_phase]");
        Self { request, phase, start: Instant::now(), done: false }
    }
    pub(crate) fn finish(mut self, status: &'static str) { self.emit(status); self.done = true; }
    fn emit(&self, status: &'static str) {
        tracing::info!(request = self.request, phase = self.phase, status,
            elapsed_us = self.start.elapsed().as_micros() as u64, "[compaction_phase]");
    }
}
impl Drop for Phase { fn drop(&mut self) { if !self.done { self.emit("interrupted"); } } }

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn compaction_phase_real_delay_lock_wait_and_cancel() {
        let request = next_request();
        correlated(request, async {
            assert_eq!(next_request(), request);
            let phase = Phase::new(request, "provider_call");
            tokio::time::sleep(std::time::Duration::from_millis(15)).await;
            assert!(phase.start.elapsed().as_millis() >= 15);
            phase.finish("error");
            let lock = tokio::sync::Mutex::new(());
            let held = lock.lock().await;
            let mut waiter = Box::pin(async {
                let phase = Phase::new(next_request(), "state_lock_wait");
                let _held = lock.lock().await;
                phase.finish("ok");
            });
            tokio::select! {
                _ = &mut waiter => panic!("held lock unexpectedly acquired"),
                _ = tokio::time::sleep(std::time::Duration::from_millis(15)) => {}
            }
            drop(waiter);
            drop(held);
        }).await;
    }
}
