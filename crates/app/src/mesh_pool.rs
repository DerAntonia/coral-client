// Dedizierter, in der Groesse einstellbarer Thread-Pool fuer Chunk-Meshing (s. Settings::
// chunk_update_threads) -- bewusst NICHT tokios Blocking-Pool (der hinter tokio_rt steckt
// und auch die Netzwerk-Verbindung traegt, s. App::conn_task): dessen Groesse liesse sich
// nur durch Neubau der GESAMTEN Runtime aendern, was live eine bestehende Verbindung killen
// wuerde. Dieser Pool ist komplett unabhaengig und darf jederzeit umgroesst werden.

use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};

enum PoolJob {
    Run(Box<dyn FnOnce() + Send>),
    // "Ein Worker weniger": beim Verkleinern werden genau so viele Stop-Jobs eingereiht wie
    // Worker wegsollen -- jeder Stop-Job beendet GENAU EINEN Worker, der ihn aus der
    // gemeinsamen Warteschlange zieht (FIFO, kein Vordraengeln vor echten Jobs).
    Stop,
}

pub struct MeshPool {
    sender: Sender<PoolJob>,
    // Mehrere Worker-Threads teilen sich EINEN Receiver -- std::sync::mpsc kennt nur einen
    // Konsumenten, Mutex macht ihn sicher fuer mehrere. Wer zuerst den Lock bekommt, zieht
    // den naechsten Job; das ist die uebliche kleine MPMC-per-Mutex-Loesung fuer eine
    // Handvoll Worker, keine hochfrequente Warteschlange, die das je spueren wuerde.
    receiver: Arc<Mutex<Receiver<PoolJob>>>,
    worker_count: usize,
}

impl MeshPool {
    pub fn new(worker_count: usize) -> Self {
        let (sender, receiver) = channel();
        let mut pool = Self { sender, receiver: Arc::new(Mutex::new(receiver)), worker_count: 0 };
        pool.resize(worker_count);
        pool
    }

    // Wichtig: die Job-Warteschlange selbst bleibt beim Umgroessen unveraendert (derselbe
    // Sender/Receiver) -- ein Verkleinern verwirft NIE bereits eingereihte Mesh-Jobs, es
    // aendert nur, wie viele Worker sie abarbeiten. Ohne das gaebe es Chunk-Spalten, die
    // durch einen Resize mitten im Flug ihren Mesh-Job verlieren und nie fertig wuerden (s.
    // world::World::dirty -- take_dirty() hat die Spalte da schon aus der Dirty-Menge
    // gestrichen).
    pub fn resize(&mut self, target: usize) {
        let target = target.max(1);
        if target > self.worker_count {
            for _ in 0..(target - self.worker_count) {
                let receiver = Arc::clone(&self.receiver);
                std::thread::spawn(move || loop {
                    let job = { receiver.lock().unwrap().recv() };
                    match job {
                        Ok(PoolJob::Run(f)) => f(),
                        Ok(PoolJob::Stop) | Err(_) => break,
                    }
                });
            }
        } else if target < self.worker_count {
            for _ in 0..(self.worker_count - target) {
                let _ = self.sender.send(PoolJob::Stop);
            }
        }
        if self.worker_count != target {
            println!("[mesh_pool] Worker-Anzahl {} -> {target}", self.worker_count);
        }
        self.worker_count = target;
    }

    pub fn spawn(&self, job: impl FnOnce() + Send + 'static) {
        let _ = self.sender.send(PoolJob::Run(Box::new(job)));
    }
}
