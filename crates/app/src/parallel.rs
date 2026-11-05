// Kleiner Thread-Pool fuer Arbeit INNERHALB eines Frames: `run(n, f)` ruft f(0..n) verteilt
// auf die Worker und den aufrufenden Thread und kehrt erst zurueck, wenn alle fertig sind.
// Deshalb darf f Daten des Aufrufers leihen (wie std::thread::scope), ohne dass dafuer je
// Frame Threads gestartet werden -- das kostete je Thread einige zehn Mikrosekunden.
//
// Der Haupt-Thread arbeitet selbst mit: wacht ein Worker zu spaet auf, macht er einfach
// weniger, gewartet wird nur auf die Aufgaben, die schon laufen.

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};

// Die laufende Aufgabe. Die Zeiger zeigen auf den Stack von `run`; gueltig, solange
// State::job sie haelt -- `run` nimmt sie heraus und wartet, bis kein Worker mehr darin ist.
#[derive(Clone, Copy)]
struct Job {
    f: *const (dyn Fn(usize) + Sync),
    next: *const AtomicUsize,
    n: usize,
}

// Die Zeiger werden nur benutzt, solange `run` wartet (s. oben); f ist Sync, next atomar.
unsafe impl Send for Job {}

#[derive(Default)]
struct State {
    job: Option<Job>,
    generation: u64,
    // Worker, die gerade in `job` arbeiten.
    active: usize,
    panicked: bool,
    quit: bool,
}

#[derive(Default)]
struct Shared {
    state: Mutex<State>,
    wake: Condvar,
    done: Condvar,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

pub(crate) struct FramePool {
    shared: Arc<Shared>,
    workers: usize,
}

// Aufgaben abholen, bis keine mehr da ist.
fn work(f: &(dyn Fn(usize) + Sync), next: &AtomicUsize, n: usize) {
    loop {
        let i = next.fetch_add(1, Ordering::Relaxed);
        if i >= n {
            return;
        }
        f(i);
    }
}

impl FramePool {
    pub(crate) fn new(workers: usize) -> Self {
        let shared = Arc::new(Shared::default());
        for k in 0..workers {
            let shared = Arc::clone(&shared);
            let _ = std::thread::Builder::new().name(format!("frame-{k}")).spawn(move || {
                let mut gesehen = 0;
                loop {
                    let job = {
                        let mut st = shared.lock();
                        loop {
                            if st.quit {
                                return;
                            }
                            if st.generation != gesehen {
                                gesehen = st.generation;
                                if let Some(job) = st.job {
                                    st.active += 1;
                                    break job;
                                }
                            }
                            st = shared.wake.wait(st).unwrap_or_else(PoisonError::into_inner);
                        }
                    };
                    // SAFETY: `run` haelt die Zeiger gueltig, bis `active` wieder 0 ist.
                    let ok = catch_unwind(AssertUnwindSafe(|| unsafe { work(&*job.f, &*job.next, job.n) })).is_ok();
                    let mut st = shared.lock();
                    st.panicked |= !ok;
                    st.active -= 1;
                    if st.active == 0 {
                        shared.done.notify_all();
                    }
                }
            });
        }
        Self { shared, workers }
    }

    // Ein Worker je zwei Kerne ausser dem eigenen, hoechstens vier: daneben laufen Chunk-
    // Meshing, Netzwerk und Ton.
    pub(crate) fn with_default_size() -> Self {
        let kerne = std::thread::available_parallelism().map_or(1, std::num::NonZero::get);
        Self::new((kerne.saturating_sub(1) / 2).min(4))
    }

    // f(0), ..., f(n - 1), verteilt; kehrt zurueck, wenn alle gelaufen sind.
    pub(crate) fn run(&self, n: usize, f: &(dyn Fn(usize) + Sync)) {
        let next = AtomicUsize::new(0);
        if self.workers == 0 || n <= 1 {
            work(f, &next, n);
            return;
        }
        // SAFETY (Lebensdauer weg): die Zeiger verlassen `run` nicht -- unten wird der Job
        // zurueckgenommen und gewartet, bis kein Worker mehr darin arbeitet.
        let f_static: *const (dyn Fn(usize) + Sync) = unsafe { std::mem::transmute::<&(dyn Fn(usize) + Sync), &'static (dyn Fn(usize) + Sync)>(f) };
        {
            let mut st = self.shared.lock();
            st.job = Some(Job { f: f_static, next: &raw const next, n });
            st.generation += 1;
        }
        self.shared.wake.notify_all();
        let eigen = catch_unwind(AssertUnwindSafe(|| work(f, &next, n)));
        let panicked = {
            let mut st = self.shared.lock();
            st.job = None;
            while st.active > 0 {
                st = self.shared.done.wait(st).unwrap_or_else(PoisonError::into_inner);
            }
            std::mem::take(&mut st.panicked)
        };
        if let Err(p) = eigen {
            std::panic::resume_unwind(p);
        }
        assert!(!panicked, "FramePool: eine Aufgabe ist in einem Worker abgestuerzt");
    }
}

impl Drop for FramePool {
    fn drop(&mut self) {
        self.shared.lock().quit = true;
        self.shared.wake.notify_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Jede Aufgabe genau einmal, auch viele Male hintereinander (Generationen) und mit
    // geliehenen Daten.
    #[test]
    fn jede_aufgabe_genau_einmal() {
        let pool = FramePool::new(3);
        for runde in 0..500 {
            let n = runde % 37;
            let zaehler: Vec<AtomicUsize> = (0..n).map(|_| AtomicUsize::new(0)).collect();
            pool.run(n, &|i| {
                zaehler[i].fetch_add(1, Ordering::Relaxed);
            });
            assert!(zaehler.iter().all(|z| z.load(Ordering::Relaxed) == 1), "Runde {runde}");
        }
    }

    #[test]
    fn ohne_worker_seriell() {
        let pool = FramePool::new(0);
        let summe = AtomicUsize::new(0);
        pool.run(10, &|i| {
            summe.fetch_add(i, Ordering::Relaxed);
        });
        assert_eq!(summe.load(Ordering::Relaxed), 45);
    }
}
