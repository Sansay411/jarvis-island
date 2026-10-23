//! Offline timing on synthetic fixtures; no microphone, cloud or user data.
//! cargo run --example voice_bench -- name.wav command.wav not-name.wav
use jarvis_lib::voice::{models, stt::Recognizer, tts, vad::{Vad,VadEvent},wake};
fn main() {
    let rec=Recognizer::load_default().unwrap();
    let silero=models::asset("silero-vad").unwrap().path();
    let mut failures=0;
    for path in std::env::args().skip(1) {
        let (rate,pcm)=tts::parse_wav(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(rate,16000);
        let name=std::path::Path::new(&path).file_name().unwrap().to_string_lossy();
        let expected_name=name=="name.wav" || name=="command.wav";
        let expected_segment=!name.contains("noise");
        let mut samples:Vec<f32>=vec![0.;8000];
        samples.extend(pcm.iter().copied());
        samples.extend(vec![0.;32000]);
        let mut vad=Vad::new(&silero).unwrap();
        vad.wake_preview=true;
        vad.echo_guard=name.contains("echo");
        let mut preview_ms=None;
        let mut end_ms=None;
        let mut asr_ms=0;
        let mut previews=0;
        let mut segments=0;
        for (i,chunk) in samples.chunks(512).enumerate() {
            let clock_ms=(i+1)*32;
            for event in vad.push(chunk) {
                match event {
                    VadEvent::Preview(pcm) if preview_ms.is_none() => {
                        previews+=1;
                        let t=rec.transcribe(&pcm).unwrap();
                        asr_ms+=t.ms;
                        if wake::addressed(&t.text).is_some() { preview_ms=Some(clock_ms); }
                    }
                    VadEvent::Ended(pcm) => {
                        segments+=1;
                        end_ms=Some(clock_ms);
                        let t=rec.transcribe(&pcm).unwrap();
                        asr_ms+=t.ms;
                        assert_eq!(wake::addressed(&t.text).is_some(),expected_name,"{}: {}",path,t.text);
                        if name.contains("paused") { assert!(t.text.to_lowercase().contains("какая"),"first word lost: {}",t.text); }
                    }
                    _=>{}
                }
            }
        }
        let ok=(if expected_name { preview_ms.zip(end_ms).is_some_and(|(p,e)|p+250<e) } else { preview_ms.is_none() }) && segments==usize::from(expected_segment);
        println!("{}",serde_json::json!({"file":name,"ok":ok,"early_wake_at_ms":preview_ms,"utterance_end_at_ms":end_ms,"previews":previews,"asr_total_ms":asr_ms,"segments":segments}));
        if !ok { failures+=1; }
    }
    assert_eq!(failures,0,"early wake timing/false positive regression");
}
