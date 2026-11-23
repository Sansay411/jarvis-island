//! The headphones' battery, all of it: the level the system reads, each AirPod
//! and the case, charging, in-ear, where the sound goes, how fast the charge
//! drains today and how long it will last. Read in the background (cheap
//! property reads), pushed to the island when something changes.

use std::collections::{HashMap, VecDeque};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::platform::bluetooth::{self as bt, BtDevice, PodsWatch};
use crate::state::AppState;

pub const EVT_DEVICES: &str = "bluetooth:devices";
/// A connected headset fell to 20 % or 10 %: `{ name, level }`.
pub const EVT_LOW: &str = "bluetooth:low";
const HISTORY_KEY: &str = "bluetooth.history";
const KEEP_MS: i64 = 24 * 3600 * 1000;
/// A flat level is still written down this often, so the drain rate knows
/// how long it held.
const SAMPLE_EVERY_MS: i64 = 10 * 60 * 1000;

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BtView {
    #[serde(flatten)]
    pub device: BtDevice,
    /// Sound plays through it right now.
    pub audio_output: bool,
    /// Level samples over the last day (unix ms, %), oldest first.
    pub history: Vec<(i64, u8)>,
    /// Minutes left at the rate measured today (not while charging).
    pub minutes_left: Option<u32>,
    /// %/hour the level falls at, when measured.
    pub drain_per_hour: Option<f32>,
    /// When the level was last read (unix ms).
    pub updated_at: i64,
}

impl BtView {
    fn charging(&self) -> bool {
        let d = &self.device;
        [d.left, d.right].into_iter().flatten().any(|p| p.charging)
    }
}

#[derive(Default)]
struct State {
    devices: Vec<BtView>,
    history: HashMap<String, VecDeque<(i64, u8)>>,
    history_saved_at: i64,
    history_dirty: bool,
    /// The last low-battery step announced per device (20, 10).
    warned: HashMap<String, u8>,
    loaded: bool,
}

pub struct Bluetooth {
    app: AppHandle,
    pods: PodsWatch,
    state: Mutex<State>,
    wake: Mutex<Option<mpsc::Sender<()>>>,
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

/// The output device's name contains the headphones' («Наушники (AirPods
/// Pro)», «Headphones (WH-1000XM5)»).
fn plays_through(output: &str, name: &str) -> bool {
    let (o, n) = (output.to_lowercase(), name.to_lowercase());
    !n.trim().is_empty() && o.contains(n.trim())
}

fn default_output() -> Option<String> {
    use cpal::traits::{DeviceTrait, HostTrait};
    let d = cpal::default_host().default_output_device()?;
    d.description().ok().map(|x| x.name().to_string())
}

/// Connected audio devices first (the one playing first of all), then the
/// rest of the connected, then the paired ones that are away.
fn rank(v: &BtView) -> (u8, String) {
    let d = &v.device;
    let r = match (d.connected, v.audio_output, d.is_audio()) {
        (true, true, _) => 0,
        (true, false, true) => 1,
        (true, false, false) => 2,
        (false, _, true) => 3,
        _ => 4,
    };
    (r, d.name.to_lowercase())
}

/// Average fall in %/hour over the samples `minutes_left` would use.
fn drain_per_hour(samples: &[(i64, u8)], minutes_left: Option<u32>) -> Option<f32> {
    let m = minutes_left?;
    let level = samples.last()?.1 as f32;
    (m > 0).then(|| level / (m as f32 / 60.0))
}

impl Bluetooth {
    pub fn new(app: AppHandle) -> Arc<Self> {
        Arc::new(Bluetooth { app, pods: PodsWatch::default(), state: Mutex::new(State::default()), wake: Mutex::new(None) })
    }

    /// Read the devices now and then every so often, on a thread of its own.
    pub fn spawn(self: &Arc<Self>) {
        let me = Arc::clone(self);
        let (tx, rx) = mpsc::channel::<()>();
        *self.wake.lock() = Some(tx);
        let started = std::thread::Builder::new().name("bluetooth".into()).spawn(move || {
            #[cfg(windows)]
            crate::platform::com::ensure_mta();
            loop {
                let busy = me.refresh();
                // While headphones are on, often: the buds' adverts change by
                // the second. Otherwise a slow look for new connections.
                let every = match (busy, cfg!(target_os = "macos")) {
                    (true, false) => Duration::from_secs(5),
                    (true, true) => Duration::from_secs(30),
                    (false, false) => Duration::from_secs(20),
                    (false, true) => Duration::from_secs(90),
                };
                match rx.recv_timeout(every) {
                    Ok(()) | Err(mpsc::RecvTimeoutError::Timeout) => {}
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                }
            }
        });
        if let Err(e) = started {
            tracing::warn!("bluetooth watcher not started: {e}");
        }
    }

    /// Read again right away (the island just opened the headphones deck).
    pub fn refresh_soon(&self) {
        if let Some(tx) = self.wake.lock().as_ref() {
            let _ = tx.send(());
        }
    }

    pub fn snapshot(&self) -> Vec<BtView> {
        self.state.lock().devices.clone()
    }

    fn load_history(&self, st: &mut State) {
        if std::mem::replace(&mut st.loaded, true) {
            return;
        }
        let store = &self.app.state::<AppState>().store;
        if let Some(saved) = store.kv_get(HISTORY_KEY).and_then(|j| serde_json::from_str::<HashMap<String, VecDeque<(i64, u8)>>>(&j).ok()) {
            st.history = saved;
        }
    }

    fn save_history(&self, st: &mut State, now: i64) {
        if !st.history_dirty || now - st.history_saved_at < 5 * 60 * 1000 {
            return;
        }
        if let Ok(json) = serde_json::to_string(&st.history) {
            if self.app.state::<AppState>().store.kv_set(HISTORY_KEY, &json).is_ok() {
                st.history_saved_at = now;
                st.history_dirty = false;
            }
        }
    }

    /// One look at the devices. Returns whether headphones are connected.
    fn refresh(&self) -> bool {
        let raw = match bt::devices() {
            Ok(d) => d,
            Err(e) => {
                tracing::debug!("bluetooth devices unavailable: {e}");
                return false;
            }
        };
        // Apple headphones say more about themselves in their adverts.
        let apple = raw.iter().find(|d| d.connected && d.vid == bt::APPLE && bt::apple_model(d.pid).is_some()).map(|d| d.pid);
        self.pods.watch(apple);
        let advert = self.pods.latest();
        let output = default_output().unwrap_or_default();
        let now = now_ms();

        let mut st = self.state.lock();
        self.load_history(&mut st);
        let mut views = Vec::with_capacity(raw.len());
        for mut d in raw {
            if let Some(a) = advert.filter(|a| d.connected && d.vid == bt::APPLE && a.model == d.pid) {
                if let Some(single) = a.single {
                    d.battery = single.level.or(d.battery);
                } else {
                    d.left = Some(a.left);
                    d.right = Some(a.right);
                    d.case_part = Some(a.case_part);
                    d.lid_open = a.lid_open;
                }
            }
            let level = d.level();
            let history = st.history.entry(d.id.clone()).or_default();
            if d.connected {
                if let Some(level) = level {
                    let fresh = history.back().is_none_or(|(t, l)| *l != level || now - t >= SAMPLE_EVERY_MS);
                    if fresh {
                        history.push_back((now, level));
                        st.history_dirty = true;
                    }
                }
            }
            let history = st.history.entry(d.id.clone()).or_default();
            while history.front().is_some_and(|(t, _)| now - t > KEEP_MS) {
                history.pop_front();
            }
            let samples: Vec<(i64, u8)> = history.iter().copied().collect();
            let audio_output = d.connected && d.is_audio() && plays_through(&output, &d.name);
            let mut view = BtView { device: d, audio_output, history: samples, minutes_left: None, drain_per_hour: None, updated_at: now };
            if view.device.connected && !view.charging() {
                view.minutes_left = bt::minutes_left(&view.history, now);
                view.drain_per_hour = drain_per_hour(&view.history, view.minutes_left);
            }
            views.push(view);
        }
        views.sort_by_key(rank);
        self.warn_low(&mut st, &views);
        let busy = views.iter().any(|v| v.device.connected && v.device.is_audio());
        // `updated_at` moves every time: compare without it.
        let same = st.devices.len() == views.len()
            && st.devices.iter().zip(&views).all(|(a, b)| BtView { updated_at: 0, ..a.clone() } == BtView { updated_at: 0, ..b.clone() });
        st.devices = views;
        self.save_history(&mut st, now);
        if !same {
            let _ = self.app.emit(EVT_DEVICES, &st.devices);
        }
        busy
    }

    /// Say once when connected headphones fall to 20 % and to 10 %.
    fn warn_low(&self, st: &mut State, views: &[BtView]) {
        for v in views.iter().filter(|v| v.device.connected && v.device.is_audio()) {
            let Some(level) = v.device.level() else { continue };
            let warned = st.warned.get(&v.device.id).copied().unwrap_or(100);
            if level > 30 || v.charging() {
                st.warned.remove(&v.device.id);
                continue;
            }
            let step = if level <= 10 { 10 } else if level <= 20 { 20 } else { continue };
            if step < warned {
                st.warned.insert(v.device.id.clone(), step);
                tracing::info!(name = %v.device.name, level, "bluetooth: headphones running low");
                let _ = self.app.emit(EVT_LOW, serde_json::json!({ "name": v.device.name, "level": level }));
            }
        }
    }
}

/// One spoken line about the headphones' charge for the voice and the chat.
pub fn describe(views: &[BtView]) -> String {
    let Some(v) = views.iter().find(|v| v.device.connected && v.device.is_audio()) else {
        return match views.iter().find(|v| v.device.is_audio()) {
            Some(v) => match v.device.battery {
                Some(b) => format!("{} сейчас не подключены, в последний раз было {b} %.", v.device.name),
                None => format!("{} сейчас не подключены.", v.device.name),
            },
            None => "Наушников с Bluetooth не вижу.".into(),
        };
    };
    let d = &v.device;
    let part = |label: &str, p: Option<bt::BtPart>| {
        p.and_then(|p| p.level.map(|l| format!("{label} {l} %{}", if p.charging { ", заряжается" } else { "" })))
    };
    let parts: Vec<String> = [part("левый", d.left), part("правый", d.right), part("кейс", d.case_part)].into_iter().flatten().collect();
    let mut line = if !parts.is_empty() {
        format!("{}: {}.", d.name, parts.join(", "))
    } else if let Some(b) = d.battery {
        format!("{}: {b} %.", d.name)
    } else {
        format!("{} подключены, но заряд они не сообщают.", d.name)
    };
    if let Some(m) = v.minutes_left {
        line.push_str(&format!(" Хватит примерно на {}.", crate::util::human_minutes(m)));
    }
    line
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::bluetooth::BtPart;

    fn view(name: &str, connected: bool, kind: &'static str) -> BtView {
        BtView {
            device: BtDevice { id: name.into(), name: name.into(), kind, connected, ..Default::default() },
            audio_output: false,
            history: Vec::new(),
            minutes_left: None,
            drain_per_hour: None,
            updated_at: 0,
        }
    }

    #[test]
    fn the_output_device_name_carries_the_headphones() {
        assert!(plays_through("Наушники (AirPods Pro🔥 - Find My)", "AirPods Pro🔥 - Find My"));
        assert!(plays_through("Headphones (WH-1000XM5)", "WH-1000XM5"));
        assert!(!plays_through("Динамики (Realtek(R) Audio)", "WH-1000XM5"));
        assert!(!plays_through("Динамики", ""));
    }

    #[test]
    fn playing_headphones_come_first() {
        let mut v = vec![view("Mouse", true, "mouse"), view("Old", false, "headphones"), view("Buds", true, "earbuds")];
        v[2].audio_output = true;
        v.sort_by_key(rank);
        assert_eq!(v.iter().map(|v| v.device.name.as_str()).collect::<Vec<_>>(), ["Buds", "Mouse", "Old"]);
    }

    #[test]
    fn describes_buds_case_and_time_left() {
        let mut v = view("AirPods", true, "earbuds");
        v.device.left = Some(BtPart { level: Some(90), ..Default::default() });
        v.device.right = Some(BtPart { level: Some(80), charging: true, ..Default::default() });
        v.device.case_part = Some(BtPart { level: Some(40), ..Default::default() });
        v.minutes_left = Some(150);
        assert_eq!(describe(&[v]), "AirPods: левый 90 %, правый 80 %, заряжается, кейс 40 %. Хватит примерно на 2 ч 30 мин.");
        let mut plain = view("WH-1000XM5", true, "headphones");
        plain.device.battery = Some(55);
        assert_eq!(describe(&[plain]), "WH-1000XM5: 55 %.");
        let mut away = view("WH-1000XM5", false, "headphones");
        away.device.battery = Some(30);
        assert_eq!(describe(&[away]), "WH-1000XM5 сейчас не подключены, в последний раз было 30 %.");
        assert_eq!(describe(&[]), "Наушников с Bluetooth не вижу.");
    }
}
