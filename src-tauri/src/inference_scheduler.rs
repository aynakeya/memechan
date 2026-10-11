use std::sync::{Condvar, Mutex};

#[derive(Default)]
struct State {
    busy: bool,
    generation: u64,
    waiting_search: Option<u64>,
}

// Separate from the model mutex so input can cancel queued work during inference.
#[derive(Default)]
pub struct InferenceScheduler {
    state: Mutex<State>,
    changed: Condvar,
}

impl InferenceScheduler {
    pub fn invalidate_searches(&self) -> u64 {
        let mut state = self.state.lock().unwrap();
        state.generation += 1;
        state.waiting_search = None;
        self.changed.notify_all();
        state.generation
    }

    pub fn is_search_current(&self, generation: u64) -> bool {
        self.state.lock().unwrap().generation == generation
    }

    pub fn acquire_search_permit(&self, generation: u64) -> Option<InferencePermit<'_>> {
        let mut state = self.state.lock().unwrap();
        if state.generation != generation {
            return None;
        }
        state.waiting_search = Some(generation);
        self.changed.notify_all();
        state = self
            .changed
            .wait_while(state, |state| state.busy && state.generation == generation)
            .unwrap();
        if state.generation != generation {
            return None;
        }
        state.waiting_search = None;
        state.busy = true;
        Some(InferencePermit(self))
    }

    pub fn acquire_background_permit(&self) -> InferencePermit<'_> {
        let mut state = self
            .changed
            .wait_while(self.state.lock().unwrap(), |state| {
                state.busy || state.waiting_search.is_some()
            })
            .unwrap();
        state.busy = true;
        InferencePermit(self)
    }
}

pub struct InferencePermit<'a>(&'a InferenceScheduler);

impl Drop for InferencePermit<'_> {
    fn drop(&mut self) {
        self.0.state.lock().unwrap().busy = false;
        self.0.changed.notify_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        sync::{mpsc, Arc},
        thread,
        time::Duration,
    };

    fn wait_for_search(scheduler: &InferenceScheduler, generation: u64) {
        let (state, _) = scheduler
            .changed
            .wait_timeout_while(
                scheduler.state.lock().unwrap(),
                Duration::from_secs(5),
                |state| state.waiting_search != Some(generation),
            )
            .unwrap();
        assert_eq!(
            state.waiting_search,
            Some(generation),
            "search did not enter the queue"
        );
    }

    #[test]
    fn only_latest_queued_search_runs_before_background_work() {
        let scheduler = Arc::new(InferenceScheduler::default());
        let current_image = scheduler.acquire_background_permit();
        let old_generation = scheduler.invalidate_searches();
        let old_search = {
            let scheduler = Arc::clone(&scheduler);
            thread::spawn(move || {
                assert!(scheduler.acquire_search_permit(old_generation).is_none())
            })
        };
        wait_for_search(&scheduler, old_generation);
        let generation = scheduler.invalidate_searches();
        // Cancellation wakes the obsolete waiter even while inference is still busy.
        old_search.join().unwrap();
        assert!(scheduler.acquire_search_permit(old_generation).is_none());

        let (started_tx, started_rx) = mpsc::channel();
        let (finish_tx, finish_rx) = mpsc::channel();
        let search = {
            let scheduler = Arc::clone(&scheduler);
            let started = started_tx.clone();
            thread::spawn(move || {
                let _permit = scheduler.acquire_search_permit(generation).unwrap();
                started.send("search").unwrap();
                finish_rx.recv_timeout(Duration::from_secs(5)).unwrap();
            })
        };
        wait_for_search(&scheduler, generation);
        let background = {
            let scheduler = Arc::clone(&scheduler);
            thread::spawn(move || {
                let _permit = scheduler.acquire_background_permit();
                started_tx.send("background").unwrap();
            })
        };
        drop(current_image);
        assert_eq!(
            started_rx.recv_timeout(Duration::from_secs(5)).unwrap(),
            "search"
        );
        assert!(started_rx.try_recv().is_err());
        finish_tx.send(()).unwrap();
        assert_eq!(
            started_rx.recv_timeout(Duration::from_secs(5)).unwrap(),
            "background"
        );
        search.join().unwrap();
        background.join().unwrap();
    }
}
