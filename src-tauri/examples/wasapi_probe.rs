//! Isolate native Windows capture initialization from CPAL, ASR and the UI.
//! No microphone samples are read or saved. Kill this probe if Windows hangs.
use windows::Win32::Media::Audio::{eCapture, eConsole, IAudioClient, IMMDeviceEnumerator, MMDeviceEnumerator, AUDCLNT_SHAREMODE_SHARED};
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CoTaskMemFree, CLSCTX_ALL, COINIT_MULTITHREADED};

fn main() -> windows::core::Result<()> {
    unsafe {
        CoInitializeEx(None, COINIT_MULTITHREADED).ok()?;
        let devices: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
        let device = devices.GetDefaultAudioEndpoint(eCapture, eConsole)?;
        eprintln!("Activating native Windows capture");
        let client: IAudioClient = device.Activate(CLSCTX_ALL, None)?;
        let format = client.GetMixFormat()?;
        eprintln!("Calling IAudioClient::Initialize (shared mode, no CPAL)");
        let start = std::time::Instant::now();
        let result = client.Initialize(AUDCLNT_SHAREMODE_SHARED, 0, 0, 0, format, None);
        CoTaskMemFree(Some(format.cast()));
        eprintln!("Initialize returned in {} ms: {result:?}", start.elapsed().as_millis());
        result?;
        client.Start()?;
        std::thread::sleep(std::time::Duration::from_millis(100));
        client.Stop()?;
        eprintln!("Native capture start/stop succeeded; no audio read");
    }
    Ok(())
}
