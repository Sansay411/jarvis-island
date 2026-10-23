//! Developer probe for native integrations, runnable without the UI:
//!   cargo run --example probe -- <command> [args]
//! audio | sessions | devices | brightness | media | windows | monitors | apps Q
//! capture OUT.jpg | uia WINDOW_QUERY | uia-press WINDOW_QUERY NAME...
use jarvis_lib::platform::{self, apps, audio, brightness, capture, display, media, uia, winmgr};

fn main() {
    // Match the app: per-monitor DPI aware v2, so coordinates are physical.
    unsafe {
        let _ = windows::Win32::UI::HiDpi::SetProcessDpiAwarenessContext(windows::Win32::UI::HiDpi::DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
    platform::com::ensure_mta();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cmd = args.first().map(String::as_str).unwrap_or("");
    let t = std::time::Instant::now();
    let out = match cmd {
        // files-inspect A [B…] | files-run ACTION A [B…] — island file actions
        // sapi-voices | sapi-say TOKEN_ID TEXT OUT.wav — SAPI 5 engines (Silero…)
        "sapi-voices" => jarvis_lib::voice::sapi::voices().iter().map(|v| format!("{} | {} | lang={}", v.name, v.id, v.languages)).collect::<Vec<_>>().join("
"),
        "sapi-say" => {
            let v = jarvis_lib::voice::sapi::SapiVoice::new(&args[1], 1.1).expect("voice");
            let t0 = std::time::Instant::now();
            let pcm = v.synthesize(&args[2]).expect("synth");
            let first = t0.elapsed().as_millis();
            let t1 = std::time::Instant::now();
            let _ = v.synthesize("Готово.");
            let t2 = std::time::Instant::now();
            let again = v.synthesize(&args[2]).expect("synth");
            eprintln!("warm full phrase: {:.2} s of audio in {} ms", again.len() as f64 / 24000.0, t2.elapsed().as_millis());
            let ms = format!("{first} ms (first, includes engine warm-up); a short phrase after that {} ms", t1.elapsed().as_millis());
            std::fs::write(&args[3], jarvis_lib::voice::tts::wav_bytes(&pcm, 24_000)).unwrap();
            format!("{} — {:.2} s of audio in {ms} ms", v.name, pcm.len() as f64 / 24000.0)
        }
        "files-inspect" => match jarvis_lib::filekit::inspect(&args[1..]) {
            Ok(mut i) => {
                for f in &mut i.files {
                    f.thumbnail = f.thumbnail.as_ref().map(|t| format!("<{} bytes>", t.len()));
                }
                serde_json::to_string_pretty(&i).unwrap()
            }
            Err(e) => format!("error: {e}"),
        },
        "files-transcribe" => {
            let rec = jarvis_lib::voice::stt::Recognizer::load_default().expect("GigaAM");
            let pcm = jarvis_lib::filekit::media::decode_pcm16k(std::path::Path::new(&args[1]), || false).expect("decode");
            let t0 = std::time::Instant::now();
            let phrases = jarvis_lib::voice::transcribe_segments(&pcm, |s| rec.transcribe(s), |_| true).expect("transcribe");
            format!("{:.1} s audio in {} ms
{}", pcm.len() as f64 / 16000.0, t0.elapsed().as_millis(), phrases.iter().map(|(s, e, t)| format!("[{} → {}] {t}", jarvis_lib::filekit::media::srt_time(*s), jarvis_lib::filekit::media::srt_time(*e))).collect::<Vec<_>>().join("
"))
        }
        "files-run" => {
            let mut progress = |_x: f32| true;
            match jarvis_lib::filekit::execute(None, &args[2..], &args[1], &mut progress) {
                Ok(o) => format!("{}
outputs: {:?}
text: {}", o.message, o.outputs, o.text.unwrap_or_default().chars().take(400).collect::<String>()),
                Err(e) => format!("error: {e}"),
            }
        }
        "audio" => serde_json::to_string_pretty(&audio::get_state()).unwrap(),
        "sessions" => serde_json::to_string_pretty(&audio::list_app_sessions()).unwrap(),
        "devices" => serde_json::to_string_pretty(&audio::list_output_devices()).unwrap(),
        "brightness" => serde_json::to_string_pretty(&brightness::list()).unwrap(),
        "media" => serde_json::to_string_pretty(&media::snapshot()).unwrap(),
        "windows" => serde_json::to_string_pretty(&winmgr::list_windows()).unwrap(),
        "close" => {
            let w = winmgr::find_windows(&args[1]).into_iter().next().expect("window");
            serde_json::to_string_pretty(&winmgr::close(w.hwnd)).unwrap()
        }
        "devtools" => format!(
            "code={:?}
codex={:?}
claude={:?}
recent={:?}",
            jarvis_lib::tools::vscode_exe(),
            jarvis_lib::tools::codex_entry(),
            jarvis_lib::tools::claude_exe(),
            jarvis_lib::tools::vscode_recent().into_iter().take(5).collect::<Vec<_>>()
        ),
        "codex-run" => {
            let (node, script) = jarvis_lib::tools::codex_entry().expect("codex");
            let rt = tokio::runtime::Runtime::new().unwrap();
            let spec = jarvis_lib::platform::supervisor::Spec {
                program: node.display().to_string(),
                args: vec![script.display().to_string(), "exec".into(), "--json".into(), "--skip-git-repo-check".into(), "-C".into(), args[1].clone(), "-s".into(), "read-only".into(), "-".into()],
                cwd: Some(args[1].clone().into()),
                env: vec![],
                timeout: std::time::Duration::from_secs(180),
                stdin: Some(args[2].clone()),
            };
            let sink: jarvis_lib::platform::supervisor::LineSink = std::sync::Arc::new(|l: &str| { if l.starts_with('{') { println!("  | {}", l.chars().take(160).collect::<String>()) } });
            let r = rt.block_on(jarvis_lib::platform::supervisor::run(spec, tokio_util::sync::CancellationToken::new(), Some(sink))).unwrap();
            format!("exit={:?} success={} ms={}", r.exit_code, r.success, r.duration_ms)
        }
        "browser" => {
            let wins = jarvis_lib::platform::browser::browser_windows(None);
            let mut s = String::new();
            for w in wins.iter().take(2) {
                match jarvis_lib::platform::browser::state(w.hwnd) {
                    Ok(st) => s.push_str(&format!("{} tabs={} url={:?} active={:?}
", st.browser, st.tabs.len(), st.url.is_some(), st.tabs.iter().position(|t| t.selected))),
                    Err(e) => s.push_str(&format!("{}: {e}
", w.process)),
                }
            }
            s
        }
        "uia-roles" => {
            let w = winmgr::find_windows(&args[1]).into_iter().next().expect("window");
            let s = uia::snapshot(winmgr::to_hwnd(w.hwnd), 2000).unwrap();
            let mut h = std::collections::BTreeMap::new();
            for e in &s.elements { *h.entry(e.role).or_insert(0) += 1; }
            format!("{} elements of {} total: {:?}", s.elements.len(), s.total_found, h)
        }
        "tabs" => {
            let w = jarvis_lib::platform::browser::browser_windows(Some(&args[1])).into_iter().next().expect("browser");
            let st = jarvis_lib::platform::browser::state(w.hwnd).unwrap();
            format!("url={:?}
{}", st.url, st.tabs.iter().map(|t| format!("[{}]{} {}", t.index, if t.selected {"*"} else {" "}, t.title)).collect::<Vec<_>>().join("
"))
        }
        "switch" => serde_json::to_string(&jarvis_lib::platform::browser::switch_tab(jarvis_lib::platform::browser::browser_windows(Some(&args[1]))[0].hwnd, Some(&args[2]), None).map(|s| s.tabs.into_iter().filter(|t| t.selected).collect::<Vec<_>>())).unwrap(),
        "page" => jarvis_lib::platform::browser::page_text(jarvis_lib::platform::browser::browser_windows(Some(&args[1]))[0].hwnd, 400).unwrap_or_else(|e| e.to_string()),
        "closetab" => format!("{:?}", jarvis_lib::platform::browser::close_tab(jarvis_lib::platform::browser::browser_windows(Some(&args[1]))[0].hwnd, Some(&args[2]), None).map(|(n, s)| (n, s.tabs.len()))),
        "audio-test" => {
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(async {
                let devs = jarvis_lib::voice::audio::list_devices();
                let (io, mut mic) = jarvis_lib::voice::audio::AudioIo::start("", "").unwrap();
                let t0 = std::time::Instant::now();
                let mut chunks = 0; let mut samples = 0; let mut max_in = 0f32;
                while t0.elapsed() < std::time::Duration::from_millis(1500) {
                    if let Ok(Some(c)) = tokio::time::timeout(std::time::Duration::from_millis(200), mic.recv()).await { chunks += 1; samples += c.len(); }
                    max_in = max_in.max(io.levels.input());
                }
                let tone: Vec<i16> = (0..9600).map(|i| ((i as f32 * 440.0 * 2.0 * std::f32::consts::PI / 24000.0).sin() * 0.05 * 32767.0) as i16).collect();
                io.play_pcm16(&tone);
                let mut max_out = 0f32;
                let t1 = std::time::Instant::now();
                while t1.elapsed() < std::time::Duration::from_millis(500) { max_out = max_out.max(io.levels.output()); tokio::time::sleep(std::time::Duration::from_millis(20)).await; }
                let playing_after = io.is_playing();
                io.stop();
                format!("in={} out={}
inputs={:?}
mic chunks={chunks} samples={samples} (expect ~15 x 1600) max_in_level={max_in:.2}
tone max_out_level={max_out:.2} still_queued_after_500ms={playing_after}", io.input_device, io.output_device, devs.inputs)
            })
        }
        "fg" => {
            let f = winmgr::foreground_window().unwrap();
            let (mon, cx) = (display::primary_monitor().unwrap(), 0);
            let _ = cx;
            format!("{} state={} rect={:?} monitor={} | shape={:?}", f.process, f.state, f.rect, f.monitor, display::foreground_shape(0, mon.id, mon.bounds.x + mon.bounds.width / 2, 43))
        }
        "configure-provider" => {
            // Reads the API key from stdin so it never appears in process arguments.
            let mut key = String::new();
            std::io::stdin().read_line(&mut key).unwrap();
            jarvis_lib::store::secrets::set(jarvis_lib::store::secrets::LLM_API_KEY, key.trim()).unwrap();
            let db = dirs::data_local_dir().unwrap().join("dev.jarvis.desktop").join("jarvis.db");
            let store = jarvis_lib::store::Store::open(&db).unwrap();
            let mut s = store.settings();
            let p = jarvis_lib::store::settings::preset(&args[1]);
            s.provider.preset = p.id.into();
            s.provider.base_url = String::new();
            s.provider.model = p.default_model.into();
            store.save_settings(&s).unwrap();
            store.kv_set(jarvis_lib::ai::KEY_ORIGIN, &jarvis_lib::ai::origin_of(&s.provider.effective_base_url())).unwrap();
            format!("provider={} model={} key_saved={}", s.provider.preset, s.provider.model, jarvis_lib::store::secrets::exists(jarvis_lib::store::secrets::LLM_API_KEY))
        }
        "api" => {
            // api GET|POST URL [BODY_JSON_FILE] — authenticated with the stored
            // LLM key; prints status and the body, never the key.
            let key = jarvis_lib::store::secrets::get(jarvis_lib::store::secrets::LLM_API_KEY).unwrap().unwrap_or_default();
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(async {
                let client = reqwest::Client::new();
                let req = match args[1].as_str() {
                    "POST" => client.post(&args[2]).header("content-type", "application/json").body(std::fs::read(&args[3]).unwrap()),
                    _ => client.get(&args[2]),
                };
                match req.bearer_auth(key).send().await {
                    Ok(r) => {
                        let status = r.status();
                        let ct = r.headers().get("content-type").and_then(|v| v.to_str().ok()).unwrap_or("").to_string();
                        let body = r.bytes().await.unwrap_or_default();
                        let text = if ct.starts_with("audio") { format!("<{} bytes of {ct}>", body.len()) } else { String::from_utf8_lossy(&body).chars().take(6000).collect() };
                        format!("{status} {ct}\n{text}")
                    }
                    Err(e) => format!("error: {e}"),
                }
            })
        }
        "model-dl" => {
            // model-dl ID — download a speech model, same code as Settings → Voice.
            let a = jarvis_lib::voice::models::asset(&args[1]).expect("unknown model id");
            let rt = tokio::runtime::Runtime::new().unwrap();
            let res = rt.block_on(jarvis_lib::voice::models::download(a, Default::default(), |d, t| {
                eprintln!("{:.1}% ({d}/{t})", d as f64 * 100.0 / t as f64);
            }));
            format!("{:?}", res.map(|p| p.display().to_string()))
        }
        "cpal-steps" => {
            // Time each cpal step separately to find a device that hangs.
            use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
            let t = std::time::Instant::now();
            let host = cpal::default_host();
            eprintln!("host {:?} {} ms", host.id(), t.elapsed().as_millis());
            let skip_in = std::env::var("SKIP_IN").is_ok();
            // CPAL_IN=substring picks a non-default microphone.
            let input = match std::env::var("CPAL_IN") {
                Ok(want) => host.input_devices().ok().and_then(|mut it| it.find(|d| d.description().map(|x| x.name().contains(want.as_str())).unwrap_or(false))),
                Err(_) => host.default_input_device(),
            };
            for (kind, dev) in [("in", input), ("out", host.default_output_device())] {
                if skip_in && kind == "in" {
                    continue;
                }
                let t = std::time::Instant::now();
                let Some(dev) = dev else { eprintln!("{kind}: none"); continue };
                let name = dev.description().map(|d| d.name().to_string()).unwrap_or_default();
                eprintln!("{kind} device '{name}' {} ms", t.elapsed().as_millis());
                let t = std::time::Instant::now();
                let cfg = if kind == "in" { dev.default_input_config() } else { dev.default_output_config() };
                eprintln!("{kind} config {:?} {} ms", cfg.as_ref().map(|c| (c.sample_rate(), c.channels(), c.sample_format())), t.elapsed().as_millis());
                let Ok(cfg) = cfg else { continue };
                let t = std::time::Instant::now();
                let stream = if kind == "in" {
                    dev.build_input_stream::<f32, _, _>(cfg.config(), |_d, _| {}, |e| eprintln!("err {e}"), None)
                } else {
                    dev.build_output_stream::<f32, _, _>(cfg.config(), |d: &mut [f32], _| d.fill(0.0), |e| eprintln!("err {e}"), None)
                };
                eprintln!("{kind} build {:?} {} ms", stream.as_ref().err(), t.elapsed().as_millis());
                if let Ok(s) = stream {
                    let t = std::time::Instant::now();
                    eprintln!("{kind} play {:?} {} ms", s.play().err(), t.elapsed().as_millis());
                }
            }
            let t = std::time::Instant::now();
            let ins: Vec<String> = host.input_devices().map(|d| d.map(|d| d.description().map(|x| x.name().to_string()).unwrap_or_default()).collect()).unwrap_or_default();
            eprintln!("inputs {ins:?} {} ms", t.elapsed().as_millis());
            let t = std::time::Instant::now();
            let outs: Vec<String> = host.output_devices().map(|d| d.map(|d| d.description().map(|x| x.name().to_string()).unwrap_or_default()).collect()).unwrap_or_default();
            eprintln!("outputs {outs:?} {} ms", t.elapsed().as_millis());
            "done".into()
        }
        "key-scan" => {
            // key-scan PATH... | - — count occurrences of the stored LLM key in
            // files (recursively; "-" = stdin). Prints counts, never the key.
            let key = jarvis_lib::store::secrets::get(jarvis_lib::store::secrets::LLM_API_KEY).unwrap().unwrap_or_default();
            assert!(key.len() >= 8, "no key stored");
            let needle = key.as_bytes();
            let count = |hay: &[u8]| hay.windows(needle.len()).filter(|w| *w == needle).count();
            let mut out = String::new();
            let mut files = 0usize;
            for arg in &args[1..] {
                if arg == "-" {
                    let mut buf = Vec::new();
                    std::io::Read::read_to_end(&mut std::io::stdin(), &mut buf).unwrap();
                    out.push_str(&format!("stdin ({} bytes): {}\n", buf.len(), count(&buf)));
                    continue;
                }
                let mut stack = vec![std::path::PathBuf::from(arg)];
                while let Some(p) = stack.pop() {
                    if p.is_dir() {
                        if p.file_name().is_some_and(|n| n == "target" || n == "node_modules") {
                            continue;
                        }
                        for e in std::fs::read_dir(&p).into_iter().flatten().flatten() {
                            stack.push(e.path());
                        }
                    } else if let Ok(bytes) = std::fs::read(&p) {
                        files += 1;
                        let n = count(&bytes);
                        if n > 0 {
                            out.push_str(&format!("FOUND {n}× in {}\n", p.display()));
                        }
                    }
                }
            }
            format!("{out}scanned {files} files")
        }
        "tts-eval" => {
            // tts-eval VOICE "текст" [OUT.wav] — synthesis time, pitch (male ≈85–155 Hz,
            // female ≈165–255 Hz) and intelligibility: GigaAM reads the result back.
            let t = std::time::Instant::now();
            let sp = jarvis_lib::voice::tts::Speaker::new(&args[1], 1.0).unwrap();
            let load = t.elapsed().as_millis();
            let t = std::time::Instant::now();
            let pcm = sp.synthesize(&args[2], 16_000).unwrap();
            let synth = t.elapsed().as_millis();
            let f: Vec<f32> = pcm.iter().map(|s| *s as f32 / 32768.0).collect();
            // Median F0 by autocorrelation over loud 40 ms frames.
            let mut f0s = Vec::new();
            for frame in f.chunks(640) {
                let e: f32 = frame.iter().map(|x| x * x).sum::<f32>() / frame.len() as f32;
                if frame.len() < 640 || e < 1e-3 {
                    continue;
                }
                let (mut best, mut lag) = (0f32, 0usize);
                for l in 16_000 / 400..=16_000 / 70 {
                    let c: f32 = (0..frame.len() - l).map(|i| frame[i] * frame[i + l]).sum();
                    if c > best {
                        best = c;
                        lag = l;
                    }
                }
                if lag > 0 {
                    f0s.push(16_000.0 / lag as f32);
                }
            }
            f0s.sort_by(|a, b| a.partial_cmp(b).unwrap());
            let f0 = f0s.get(f0s.len() / 2).copied().unwrap_or(0.0);
            let r = jarvis_lib::voice::stt::Recognizer::load_default().unwrap();
            let back = r.transcribe(&f).unwrap();
            if let Some(out) = args.get(3) {
                std::fs::write(out, jarvis_lib::voice::tts::wav_bytes(&pcm, 16_000)).unwrap();
            }
            format!(
                "{} | load {load} ms, synth {synth} ms for {:.2} s (RTF {:.2}) | F0 {f0:.0} Hz | heard: {}",
                sp.name(),
                f.len() as f64 / 16000.0,
                synth as f64 / 1000.0 / (f.len() as f64 / 16000.0),
                back.text
            )
        }
        "tts-voices" =>serde_json::to_string_pretty(&jarvis_lib::voice::tts::voices()).unwrap(),
        "tts-wav" => {
            // tts-wav "текст" OUT.wav [RATE] — synthesise with the first Russian voice.
            let rate: u32 = args.get(3).and_then(|r| r.parse().ok()).unwrap_or(16_000);
            let sp = jarvis_lib::voice::tts::Speaker::new(&std::env::var("TTS_VOICE").unwrap_or_default(), std::env::var("TTS_RATE").ok().and_then(|v| v.parse().ok()).unwrap_or(1.0)).unwrap();
            let t = std::time::Instant::now();
            let pcm = sp.synthesize(&args[1], rate).unwrap();
            let ms = t.elapsed().as_millis();
            let w = jarvis_lib::voice::tts::wav_bytes(&pcm, rate);
            std::fs::write(&args[2], w).unwrap();
            format!("voice={} synth {ms} ms, {:.2} s of audio", sp.name(), pcm.len() as f64 / rate as f64)
        }
        "asr" => {
            // asr FILE.wav... — GigaAM on 16 kHz mono PCM16 WAV files.
            let t = std::time::Instant::now();
            let r = jarvis_lib::voice::stt::Recognizer::load_default().unwrap();
            let mut out = format!("load {} ms
", t.elapsed().as_millis());
            for f in &args[1..] {
                let (rate, pcm) = jarvis_lib::voice::tts::parse_wav(&std::fs::read(f).unwrap()).unwrap();
                let tr = r.transcribe_at(rate as i32, &pcm).unwrap();
                let name = std::path::Path::new(f).file_name().unwrap().to_string_lossy().to_string();
                out.push_str(&format!("{name}: {:.2} s audio, {} ms → {}
", pcm.len() as f64 / rate as f64, tr.ms, tr.text));
            }
            out
        }
        "monitors" => serde_json::to_string_pretty(&display::list_monitors()).unwrap(),
        "apps" => {
            let idx = apps::AppIndex::default();
            serde_json::to_string_pretty(&idx.search(args.get(1).map(String::as_str).unwrap_or(""), 10)).unwrap()
        }
        "capture" => {
            let mon = display::primary_monitor().unwrap();
            let frame = capture::capture_rect(mon.bounds, None).unwrap();
            let enc = capture::encode(frame, capture::MAX_SIDE).unwrap();
            use base64::Engine;
            let b64 = enc.data_url.split(',').nth(1).unwrap();
            std::fs::write(&args[1], base64::engine::general_purpose::STANDARD.decode(b64).unwrap()).unwrap();
            format!("{}x{} scale={} capture={}ms encode={}ms bytes={}", enc.image_width, enc.image_height, enc.scale, enc.capture_ms, enc.encode_ms, enc.bytes)
        }
        "uia" => {
            let w = winmgr::find_windows(&args[1]).into_iter().next().expect("window");
            let s = uia::snapshot(winmgr::to_hwnd(w.hwnd), 220).unwrap();
            format!("snapshot {} «{}» {} of {} elements in {} ms\n{}", s.snapshot_id, s.window_title, s.elements.len(), s.total_found, s.elapsed_ms, uia::render(&s))
        }
        "uia-press" => {
            let w = winmgr::find_windows(&args[1]).into_iter().next().expect("window");
            let mut log = String::new();
            for name in &args[2..] {
                let s = uia::snapshot(winmgr::to_hwnd(w.hwnd), 400).unwrap();
                let el = s.elements.iter().find(|e| e.role == "button" && e.name == *name).expect("button");
                let o = uia::act(s.snapshot_id, el.id, "invoke", None).unwrap();
                log.push_str(&format!("{} ", o.element));
            }
            std::thread::sleep(std::time::Duration::from_millis(200));
            let s = uia::snapshot(winmgr::to_hwnd(w.hwnd), 400).unwrap();
            let result: Vec<_> = s.elements.iter().filter(|e| e.name.contains("Отображать как") || e.name.contains("Display is")).map(|e| e.name.clone()).collect();
            format!("{log}\nresult: {result:?}")
        }
        other => {
            eprintln!("unknown probe {other}");
            std::process::exit(2);
        }
    };
    println!("{out}\n-- {} ms", t.elapsed().as_millis());
}
