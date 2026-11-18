//! Live end-to-end verification and latency benchmark for JARVIS's Free
//! Studio Neural Streaming TTS engine (`edge.rs`, `tts.rs`, `PcmCache`, offline fallback).
//!
//! Run with: `cargo run --example tts_bench`

use std::time::Instant;

use jarvis_lib::voice::edge::{self, EdgeVoice};
use jarvis_lib::voice::tts::{PcmCache, Speaker, SpeechEngine};

fn rms_pcm16(samples: &[i16]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let mean_sq: f64 = samples
        .iter()
        .map(|&s| {
            let v = s as f64 / 32768.0;
            v * v
        })
        .sum::<f64>()
        / samples.len() as f64;
    mean_sq.sqrt() as f32
}

fn peak_pcm16(samples: &[i16]) -> i16 {
    samples.iter().map(|s| s.saturating_abs()).max().unwrap_or(0)
}

fn main() {
    println!("=== JARVIS Free Neural Streaming TTS Benchmark ===\n");

    // 1. Pre-warm the Edge WebSocket pool so handshake happens ahead of speech.
    let warm_t0 = Instant::now();
    edge::prewarm();
    std::thread::sleep(std::time::Duration::from_millis(750));
    println!("[1] Warm pool pre-initialized ({:.0} ms budget)", warm_t0.elapsed().as_millis());

    // 2. Test all 4 curated free Edge Neural HD voices live!
    let test_cases = [
        (
            "edge:ru-RU-DmitryNeural",
            "Добрый день. Системы Джарвиса работают штатно, громкость 30%.",
        ),
        (
            "edge:ru-RU-SvetlanaNeural",
            "Сессия фокуса запущена на 25 минут. Отличной работы!",
        ),
        (
            "edge:en-US-AndrewMultilingualNeural",
            "Открыл VS Code и GitHub репозиторий, тесты пройдены за 180 мс.",
        ),
        (
            "edge:en-GB-RyanNeural",
            "Good afternoon, sir. All systems are online and ready.",
        ),
    ];

    for (voice_id, phrase) in test_cases {
        let sp = Speaker::new(voice_id, 1.05).expect("Speaker::new");
        let t0 = Instant::now();
        let mut first_chunk_ms = None;
        let mut chunk_count = 0usize;
        let mut samples = Vec::new();

        sp.synthesize_stream(phrase, &mut |pcm| {
            if first_chunk_ms.is_none() && !pcm.is_empty() {
                first_chunk_ms = Some(t0.elapsed().as_millis() as u64);
            }
            chunk_count += 1;
            samples.extend_from_slice(pcm);
            true
        })
        .unwrap_or_else(|e| panic!("Live synthesis failed for {voice_id}: {e}"));

        let total_ms = t0.elapsed().as_millis() as u64;
        let first_ms = first_chunk_ms.unwrap_or(total_ms);
        let rms = rms_pcm16(&samples);
        let peak = peak_pcm16(&samples);
        let duration_s = samples.len() as f64 / 24_000.0;

        println!(
            "  • {voice_id:<36} | first_chunk: {:>4} ms | total: {:>4} ms | chunks: {:>2} | {:.2}s ({} smp) | rms: {:.4} | peak: {}",
            first_ms, total_ms, chunk_count, duration_s, samples.len(), rms, peak
        );

        assert!(!samples.is_empty(), "Audio samples must be non-empty for {voice_id}");
        assert!(rms > 0.01, "Audio waveform RMS too quiet ({rms}) for {voice_id}");
    }

    // 3. Measure Warm Pool streaming latency over 3 realistic conversational turns
    //    (including a 400 ms user pause so the background pool replenish completes!).
    println!("\n[2] Warm Connection Pool First-Chunk Latency (`edge:ru-RU-DmitryNeural`):");
    let dmitry = Speaker::new("edge:ru-RU-DmitryNeural", 1.1).expect("Dmitry speaker");
    edge::prewarm();
    std::thread::sleep(std::time::Duration::from_millis(500));
    let mut warm_latencies = Vec::new();
    for (idx, line) in [
        "Слушаю вас.",
        "Проверяю открытые окна и загрузку процессора.",
        "Готово, рабочее пространство упорядочено.",
    ]
    .iter()
    .enumerate()
    {
        if idx > 0 {
            std::thread::sleep(std::time::Duration::from_millis(350));
        }
        let t0 = Instant::now();
        let mut first_ms = 0u64;
        let mut got_first = false;
        let mut count = 0usize;
        dmitry
            .synthesize_stream(line, &mut |pcm| {
                if !got_first && !pcm.is_empty() {
                    got_first = true;
                    first_ms = t0.elapsed().as_millis() as u64;
                }
                count += pcm.len();
                true
            })
            .expect("warm stream");
        let total = t0.elapsed().as_millis() as u64;
        warm_latencies.push(first_ms);
        println!(
            "  Turn {}: first_chunk = {:>3} ms | total = {:>3} ms | samples = {}",
            idx + 1,
            first_ms,
            total,
            count
        );
    }

    // 4. Verify 2-Tier RAM + Persistent Disk LRU Cache (< 10 ms!).
    println!("\n[3] 2-Tier RAM + Persistent Disk Cache Benchmark:");
    let engine = SpeechEngine::default();
    let cached_sp = engine.get("edge:ru-RU-DmitryNeural", 1.1).expect("SpeechEngine::get");
    let cached_phrase = "Сессия фокуса завершена. Отличная работа.";
    let _ = cached_sp.synthesize(cached_phrase).expect("populate cache");

    // Hot RAM lookup:
    let t_ram = Instant::now();
    let pcm_ram = cached_sp.synthesize(cached_phrase).expect("ram hit");
    let ram_us = t_ram.elapsed().as_micros();

    // Cold Disk lookup (fresh PcmCache instance reading %LOCALAPPDATA%\dev.jarvis.desktop\tts_cache):
    let cold_disk = PcmCache::default();
    let t_disk = Instant::now();
    let pcm_disk = cold_disk
        .get(&cached_sp.voice_id, cached_sp.rate_bits, cached_phrase)
        .expect("disk cache hit");
    let disk_us = t_disk.elapsed().as_micros();

    println!(
        "  • RAM tier hit : {:>5} µs ({:.2} ms) | {} samples",
        ram_us,
        ram_us as f64 / 1000.0,
        pcm_ram.len()
    );
    println!(
        "  • Disk tier hit: {:>5} µs ({:.2} ms) | {} samples",
        disk_us,
        disk_us as f64 / 1000.0,
        pcm_disk.len()
    );
    assert_eq!(pcm_ram.len(), pcm_disk.len());
    assert!(disk_us < 15_000, "Disk cache should be <15 ms, got {disk_us} µs");

    // 5. Verify Automatic Zero-Glitch Offline Fallback when Edge Voice fails before audio!
    println!("\n[4] Offline / Error Fallback Verification:");
    // An invalid/non-existent Edge voice forces Edge to fail before emitting audio,
    // exercising `Speaker::Edge`'s seamless fallback to local offline voice!
    let bad_edge = Speaker::Edge(
        EdgeVoice::new("xx-XX-NonExistentOfflineTestVoice", 1.0).unwrap(),
        std::sync::OnceLock::new(),
        1.0,
    );
    let t_fb = Instant::now();
    let mut fb_samples = Vec::new();
    bad_edge
        .synthesize_stream("Проверка автономного режима.", &mut |pcm| {
            fb_samples.extend_from_slice(pcm);
            true
        })
        .expect("Offline fallback should succeed seamlessly");
    let fb_rms = rms_pcm16(&fb_samples);
    println!(
        "  • Seamless offline fallback succeeded in {} ms | {} samples | rms: {:.4}",
        t_fb.elapsed().as_millis(),
        fb_samples.len(),
        fb_rms
    );
    assert!(!fb_samples.is_empty(), "Fallback voice must produce audio");

    println!("\n=== ALL TTS BENCHMARKS & LIVE CHECKS PASSED ===");
}
