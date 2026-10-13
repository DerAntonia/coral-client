// Wie schnell wir Chunks verarbeiten koennen -- die Zahl, die wir dem Server nach jedem
// Chunk-Stapel zurueckmelden (s. packets::CB_PLAY_CHUNK_BATCH_START/FINISHED).
//
// Zeilengetreuer Port von net.minecraft.client.network.ChunkBatchSizeCalculator aus dem
// Yarn-gemappten 1.21.11-Jar. Bewusst dieselben Konstanten und dieselbe Mittelwertbildung:
// der Wert steuert, wie viele Chunks der Server pro Tick nachschiebt, und ein zu grosser
// Wert laesst ihn schneller senden, als wir meshen koennen.
//
//   averageNanosPerChunk startet bei 2 ms  -> erster Wunsch = 7 ms / 2 ms = 3.5 Chunks/Tick
//   jede Messung wird auf [Mittel/3, Mittel*3] geklemmt (Ausreisser, z.B. ein einzelner
//   Stapel waehrend eines Ladehaengers, duerfen den Mittelwert nicht kippen)
//   sampleSize waechst bis 49 -- danach gleitender Mittelwert mit festem Gewicht
pub struct ChunkBatchSizeCalculator {
    average_nanos_per_chunk: f64,
    sample_size: u32,
    start: std::time::Instant,
}

impl Default for ChunkBatchSizeCalculator {
    fn default() -> Self {
        Self::new()
    }
}

impl ChunkBatchSizeCalculator {
    pub fn new() -> Self {
        Self { average_nanos_per_chunk: 2_000_000.0, sample_size: 1, start: std::time::Instant::now() }
    }

    // Chunk Batch Start: ab hier laeuft die Uhr fuer diesen Stapel.
    pub fn on_start_chunk_send(&mut self) {
        self.start = std::time::Instant::now();
    }

    // Chunk Batch Finished: Dauer / Anzahl = Zeit pro Chunk. Ein leerer Stapel (batch_size
    // <= 0) sagt nichts ueber unser Tempo aus und wird uebersprungen -- wie in Vanilla.
    pub fn on_chunk_sent(&mut self, batch_size: i32) {
        if batch_size <= 0 {
            return;
        }
        let elapsed = self.start.elapsed().as_nanos() as f64;
        let per_chunk = elapsed / batch_size as f64;
        let clamped = per_chunk.clamp(self.average_nanos_per_chunk / 3.0, self.average_nanos_per_chunk * 3.0);
        self.average_nanos_per_chunk = (self.average_nanos_per_chunk * self.sample_size as f64 + clamped) / (self.sample_size + 1) as f64;
        self.sample_size = (self.sample_size + 1).min(49);
    }

    // Der Wunsch fuer das Bestaetigungspaket: 7 ms Chunk-Arbeit pro 50-ms-Tick.
    pub fn desired_chunks_per_tick(&self) -> f32 {
        (7_000_000.0 / self.average_nanos_per_chunk) as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn startwert_entspricht_vanilla() {
        // 7 ms / 2 ms -- der Wert, den ein frisch verbundener Vanilla-Client meldet.
        assert!((ChunkBatchSizeCalculator::new().desired_chunks_per_tick() - 3.5).abs() < 1e-6);
    }

    #[test]
    fn leerer_stapel_aendert_nichts() {
        let mut c = ChunkBatchSizeCalculator::new();
        let vorher = c.desired_chunks_per_tick();
        c.on_start_chunk_send();
        c.on_chunk_sent(0);
        assert_eq!(c.desired_chunks_per_tick(), vorher);
    }

    #[test]
    fn ausreisser_wird_auf_das_dreifache_geklemmt() {
        // Ein absurd langsamer Stapel (Uhr laeuft seit new(), Messung kuenstlich gross):
        // die Klemmung laesst den Mittelwert hoechstens aufs Dreifache steigen, also den
        // Wunsch nie unter ein Drittel fallen.
        let mut c = ChunkBatchSizeCalculator::new();
        c.on_start_chunk_send();
        std::thread::sleep(std::time::Duration::from_millis(30));
        c.on_chunk_sent(1);
        let wunsch = c.desired_chunks_per_tick();
        assert!(wunsch >= 3.5 / 3.0 - 1e-3, "Wunsch {wunsch} unter der Klemmgrenze");
        assert!(wunsch < 3.5, "eine langsame Messung muss den Wunsch senken, nicht heben");
    }
}
