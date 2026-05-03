# Sonium — Buffer Stability & 20 ms Minimum (Snapcast parity)

## Context

El issue [snapcast#329](https://github.com/snapcast/snapcast/issues/329) y la respuesta de DeepWiki señalan que Snapcast permite un mínimo técnico de **20 ms** desde 0.26.0, sostenido por: (1) Time-messages continuos con quick-sync inicial, (2) corrección dinámica drop/duplicate de samples (a veces rate adjustment), (3) chunks pequeños (mín 10 ms) timestampados con reloj del servidor.

Sonium hoy queda lejos:
- `crates/sync/src/buffer.rs:122` clampa `set_target_buffer_ms` a **`[40, 10_000]`** (mínimo 2× el de Snapcast).
- `client/src/controller.rs:178` envía Time-messages cada **1 s** sin quick-sync ramp en el connect; el median de 200 muestras tarda ~3 minutos en llenarse.
- `client/src/player.rs:228-243` solo **dropea** un frame cuando `age_us > 2_000 µs`; no duplica, no ajusta tasa, `rubato` está en `Cargo.toml` pero sin usar.
- Coexisten dos paths de playout: TCP-prefill cada 5 ms (primario tras `d8df34e`) y callback-driven `pop_due_exact` (volátil). El callback path falló al promoverse y fue revertido.
- Server: `chunk_ms` default 20 ms (`crates/common/src/config.rs:126`), pero falta confirmar si hay un piso a 20 que impida bajar a 10.

Decisión del usuario: **dos fases** — primero endurecer estabilidad en buffers actuales (200–500 ms), luego habilitar el mínimo de 20 ms con resampling adaptativo `rubato`.

---

## Fase A — Estabilidad sin tocar el mínimo

Orden: A1 → A2 → A4 → A5 → A3.

### A1. Quick-sync ramp on connect
- **Tocar:** `client/src/controller.rs:178` (sustituir el `sync_tick` único por una máquina de estados con `quick_sync_remaining: u8` inicial 50 y `quick_sync_tick = interval(Duration::from_millis(100))`); reusar `send_time_request` en `:572-585`.
- **Cómo:** mientras `quick_sync_remaining > 0` o `time_provider.sample_count() < 50`, disparar el tick rápido; al cumplirse, caer al `interval(Duration::from_secs(1))` actual. Reset implícito en reconnect porque `run_session` se reinicia.
- **Reusa:** `TimeProvider::sample_count()`, `send_time_request`.
- **Test:** test de controller con writer mock — ≥50 mensajes Time en los primeros 6 s, ≤1/s después.

### A2. Drift correction bidireccional (drop + duplicate)
- **Tocar:** `client/src/player.rs:228-243` (`DriftCorrector`) y call site `:205-208`.
- **Cómo:** añadir `should_duplicate_frame(age_us)` simétrico a `should_drop_frame`: drop si `age_us > +2_000`, duplicate si `age_us < -2_000`, ambos con `callbacks_since_last_correction >= 2` (renombrar el contador). En el call site, cuando duplicate dispara, copiar el frame previo desde `chunk.samples[chunk.read_pos - channels..chunk.read_pos]` antes de avanzar. Añadir `drop_count` / `dup_count` a `HealthState` (`client/src/player.rs:31`).
- **Reusa:** aritmética `read_pos`/`channels` existente, `HealthState`.
- **Test:** unit test del módulo — secuencias `age_us` con histéresis (no oscila ante ±2_500, corrige cuando se sostiene).

### A4. Telemetría — wire counters al HealthReport
- **Tocar:** `crates/sync/src/buffer.rs:308-315` (`get_report`); `crates/protocol/src/messages/health_report.rs:81-149` (añadir `drift_drop_count: u32`, `drift_dup_count: u32` con `#[serde(default)]` y defaults en `new`); `client/src/controller.rs:234-258` (poblar desde `player.take_health()`).
- **Reusa:** `stale_drop_count`, `underrun_count`, `jitter_us` ya existentes en `SyncBuffer`; patrón `take_health()` ya implementado.
- **Test:** extender el round-trip existente de `health_report` para asegurar serde de los campos nuevos.

### A5. Integration test bajo jitter sintético
- **Tocar:** nuevo `crates/sync/tests/jitter_loopback.rs`.
- **Cómo:** instanciar `SyncBuffer` a `target_buffer_ms = 400`, push de `PcmChunk` con perturbación temporal ±50 ms (rand_pcg), drive `pop_ready` cada 5 ms. Asserts: `underrun_count == 0`, `stale_drop_count <= 0.001 * total`.
- **Reusa:** `PcmChunk::new`, `SyncBuffer::push/pop_ready`.
- **Depende de:** A1, A2, A4 (contadores precisos).

### A3. Unificar playout path — mantener TCP-prefill y documentar
- **Tocar:** `client/src/controller.rs:185-209`, `client/src/player.rs` (path `pop_due_exact`).
- **Cómo:** root-cause primero. Hipótesis a verificar: el callback path usaba `pop_due_exact` (sin `lead_us`) mientras `stale_threshold = (target/2).clamp(100ms, 2s)`; con `target_buffer_us` chico se carrera-condicionaba el stale-drop. Acción: (a) dejar TCP-prefill primario, (b) doc comment en `:185` referenciando commit `d8df34e` y la interacción `lead_us`/stale-drop, (c) gatear el callback path detrás de un flag `experimental_callback_playout` para que solo corra uno a la vez. Sin nuevos módulos.
- **Test:** smoke test que asegura un solo pump activo según el flag.

---

## Fase B — Mínimo 20 ms con resampling adaptativo

Orden: B1 → B3 → B4 → B2 → B5.

### B1. Bajar el clamp y reescalar `lead_us` / stale-floor
- **Tocar:** `crates/sync/src/buffer.rs:122` (`clamp(40, 10_000)` → `clamp(20, 10_000)`); `:124` (`lead_us = (target_buffer_us / 4).clamp(2_000, 100_000)` para que a 20 ms quede 5 ms y nunca supere `target/2`); `:158` y `:237` (`stale_threshold_us = (target_buffer_us / 2).clamp(10_000, 2_000_000)`); `:159` (`low_water_us = self.lead_us.max(2_000)`).
- **Reusa:** todo el módulo; solo cambian constantes/derivaciones.
- **Test:** unit tests en el mismo módulo — `set_target_buffer_ms(20)` ⇒ `target_buffer_us = 20_000`, `lead_us = 5_000`; los tests de release-timing existentes siguen verdes en el nuevo piso.

### B3. Permitir `chunk_ms = 10` en el servidor
- **Tocar:** `crates/common/src/config.rs:33,92,126,184-185` (validar/aceptar 10 ms; actualizar doc comment de `effective_chunk_ms`); `server/src/streamreader.rs` y `server/src/encoder.rs` (localizar y eliminar piso `chunk_ms.max(20)` si existe).
- **Cómo:** bajar piso a 10 ms; default sigue 20. A 20 ms target el buffer queda con 2 chunks → margen para drift correction.
- **Test:** test de servidor con `chunk_ms = 10` y aserción de framing en `WireChunk` siguiendo el patrón actual de `encoder.rs`.

### B4. Convergencia de time-sync más rápida en buffer bajo
- **Tocar:** `crates/sync/src/time_provider.rs:34` (extraer `SAMPLE_BUFFER_SIZE` a parámetro de runtime, convertir array fijo a `Vec<i64>`); `client/src/controller.rs` (cuando `server_buffer_ms <= 50`, `time_provider.set_window(50)` en config-change boundary, no en hot path).
- **Cómo:** ventana mediana de 50 muestras bajo 50 ms target → reacciona en ~5 s en lugar de ~50 s. Mantiene atomic offset lock-free (solo el `Mutex<SampleBuffer>` se redimensiona).
- **Test:** push de 50 muestras con distribución conocida; asegurar convergencia.

### B2. Adaptive rate correction vía `rubato`
- **Tocar:** `client/src/player.rs:228` (añadir `struct RateController` junto a `DriftCorrector`, propiedad de `Player`); call site del loop de copia en `:214-217`.
- **Cómo:** instanciar `rubato::SincFixedOut::<f32>` parametrizado por `fmt.rate`/`fmt.channels`, chunk pequeño que case con el callback (~480 frames a 48 kHz). Driver de feedback: `SyncBuffer::buffer_depth_us() - target_buffer_us` → ajuste ±300 ppm con EMA (τ ≈ 2 s) para evitar pitch wobble. Engage solo si `target_buffer_ms ≤ 50`; en otro caso `ratio = 1.0` y se deja que A2 (drop/dup) maneje. Reemplazar el `for src in &chunk.samples[...]` por un writer resampler-aware cuando engage.
- **Reusa:** `chunk.read_pos`, ring de audio, `HealthState` (añadir `resample_ratio_ppm: AtomicI32`).
- **Test:** resamplear 1 s de seno 1 kHz a 48 kHz con ratio = 1.0003; asserts: count de samples y pico FFT dentro de ±2 Hz.
- **Depende de:** B1.

### B5. Validación end-to-end a 20 ms
- **Tocar:** nuevo test bajo `tests/` (root) con feature flag `snapcast-interop` y `#[ignore]` para CI.
- **Cómo:** lanzar `snapserver` real a `buffer = 20, chunk_ms = 10`, correr `sonium-client` 30 s; asserts: `underrun_count == 0`, latencia ≈ 20 ms. Simétrico con `snapclient` real contra `sonium-server`.
- **Depende de:** B1–B4.

---

## Verification

- **Fase A unit / integration:** `cargo test --workspace`. A5 corre como `cargo test -p sonium-sync jitter_loopback`.
- **Fase A soak:** script bajo `scripts/` que corre cliente+servidor 24 h con jitter sintético a `buffer = 400`. `HealthReport` debe mostrar 0 underruns y `<0.1%` drift events.
- **Fase B unit:** `cargo test --workspace` con tests nuevos. Bench opcional para `rubato`: <1 ms por chunk de 10 ms.
- **Fase B interop:** `cargo test --features snapcast-interop -- --ignored` contra `snapserver`/`snapclient` reales. Ear-test 5 min de música a 20 ms sin clicks.

## Critical files

- `crates/sync/src/buffer.rs`
- `crates/sync/src/time_provider.rs`
- `client/src/player.rs`
- `client/src/controller.rs`
- `crates/protocol/src/messages/health_report.rs`
- `crates/common/src/config.rs`
- `server/src/streamreader.rs`, `server/src/encoder.rs`
