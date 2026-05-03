use sonium_common::SampleFormat;
use sonium_sync::{PcmChunk, SyncBuffer, DriftCorrector};

#[test]
fn test_sync_buffer_jitter_estimation() {
    let fmt = SampleFormat::new(48000, 16, 2);
    let mut buf = SyncBuffer::new(fmt);
    buf.set_target_buffer_ms(100);

    let start_us = 1_000_000; // 1s

    // Push 20ms chunks with +/- 5ms jitter
    for i in 0..20 {
        let playout_us = start_us + i * 20_000;
        let jitter = if i % 2 == 0 { 5_000 } else { -5_000 };
        let arrival_us = playout_us + jitter;
        
        let samples = vec![0i16; 1920]; // 20ms at 48kHz, 2 channels
        buf.push(PcmChunk::new(playout_us, samples, fmt), arrival_us);
    }

    let jitter_us = buf.jitter_us();
    println!("Estimated jitter: {} us", jitter_us);
    
    // We expect jitter to be non-zero and roughly reflect the 5-10ms variations
    assert!(jitter_us > 1000);
    assert!(jitter_us < 20000);
}

#[test]
fn test_sync_buffer_stale_drops() {
    let fmt = SampleFormat::new(48000, 16, 2);
    let mut buf = SyncBuffer::new(fmt);
    buf.set_target_buffer_ms(100);

    let now_us = 2_000_000;
    
    // Push a chunk that is already very stale (more than 100ms old)
    let stale_playout = now_us - 500_000;
    buf.push(PcmChunk::new(stale_playout, vec![0i16; 100], fmt), now_us);
    
    // It should be dropped on pop_ready
    let chunk = buf.pop_ready(now_us);
    assert!(chunk.is_none());
    assert!(buf.take_stale_drops() >= 1);
}

#[test]
fn test_drift_corrector_logic() {
    let mut drift = DriftCorrector::default();
    
    // Threshold is 2000us, cooldown is 2 ticks
    
    // Case 1: Positive drift (age > 2000us) -> Drop frame
    // Tick 1
    assert!(!drift.should_drop_frame(3000)); 
    // Tick 2
    assert!(drift.should_drop_frame(3000));
    // Tick 3 (cooldown reset)
    assert!(!drift.should_drop_frame(3000));
    
    // Case 2: Negative drift (age < -2000us) -> Duplicate frame
    drift.ticks_since_last_correction = 0; // reset
    // Tick 1
    assert!(!drift.should_duplicate_frame(-3000));
    // Tick 2
    assert!(drift.should_duplicate_frame(-3000));
}
