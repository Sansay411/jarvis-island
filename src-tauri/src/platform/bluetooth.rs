//! Bluetooth devices and their batteries on Windows.
//!
//! Paired devices come from the PnP tree (CfgMgr32), grouped by their device
//! container: the `DEV_` node has the name, the class of device, the vendor
//! and the connection; the battery Windows reads (hands-free AT commands,
//! the GATT battery service) sits on whichever node of the device reported
//! it. These are the same properties the Settings app shows.
//!
//! Apple headphones tell more in their «proximity pairing» advert: each bud,
//! the case, charging, in-ear and the lid. A passive LE scan listens for it
//! only while such headphones are connected, and takes only adverts of that
//! very model, close by: the neighbours' AirPods stay theirs.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use windows::core::{GUID, PCWSTR};
use windows::Devices::Bluetooth::Advertisement::{
    BluetoothLEAdvertisementReceivedEventArgs, BluetoothLEAdvertisementWatcher, BluetoothLEScanningMode,
};
use windows::Foundation::TypedEventHandler;
use windows::Storage::Streams::DataReader;
use windows::Win32::Devices::DeviceAndDriverInstallation::{
    CM_Get_DevNode_PropertyW, CM_Get_Device_ID_ListW, CM_Get_Device_ID_List_SizeW, CM_Locate_DevNodeW, CM_GETIDLIST_FILTER_ENUMERATOR,
    CM_GETIDLIST_FILTER_PRESENT, CM_LOCATE_DEVNODE_NORMAL, CR_SUCCESS,
};
use windows::Win32::Devices::Properties::{
    DEVPROPTYPE, DEVPROP_TYPE_BOOLEAN, DEVPROP_TYPE_BYTE, DEVPROP_TYPE_GUID, DEVPROP_TYPE_STRING, DEVPROP_TYPE_STRING_LIST, DEVPROP_TYPE_UINT16,
    DEVPROP_TYPE_UINT32,
};
use windows::Win32::Foundation::DEVPROPKEY;

pub use super::data_bluetooth::*;
use super::{from_wide, wide};
use crate::error::AppResult;

const BT: GUID = GUID::from_u128(0x2bd67d8b_8beb_48d5_87e0_6cda3428040a);
const KEY_VID: DEVPROPKEY = DEVPROPKEY { fmtid: BT, pid: 7 };
const KEY_PID: DEVPROPKEY = DEVPROPKEY { fmtid: BT, pid: 8 };
const KEY_CLASS: DEVPROPKEY = DEVPROPKEY { fmtid: BT, pid: 10 };
/// The battery level Settings shows (`DEVPKEY_Bluetooth_Battery`), a byte.
const KEY_BATTERY: DEVPROPKEY = DEVPROPKEY { fmtid: GUID::from_u128(0x104ea319_6ee2_4701_bd47_8ddbf425bbe5), pid: 2 };
/// Whether the device is connected now (the flag Settings reads), a boolean.
const KEY_CONNECTED: DEVPROPKEY = DEVPROPKEY { fmtid: GUID::from_u128(0x83da6326_97a6_4088_9453_a1923f573b29), pid: 15 };
const KEY_CONTAINER: DEVPROPKEY = DEVPROPKEY { fmtid: GUID::from_u128(0x8c7ed206_3f8a_4827_b3ab_ae9e1faefc6c), pid: 2 };
const KEY_FRIENDLY_NAME: DEVPROPKEY = DEVPROPKEY { fmtid: GUID::from_u128(0xa45c254e_df1c_4efd_8020_67d146a850e0), pid: 14 };
const KEY_CATEGORY: DEVPROPKEY = DEVPROPKEY { fmtid: GUID::from_u128(0x78c34fc8_104a_4aca_9ea4_524d52996e57), pid: 90 };

/// Present device instance ids of one enumerator (`BTHENUM`, `BTHLE`).
fn instance_ids(enumerator: &str) -> Vec<String> {
    let filter = wide(enumerator);
    let flags = CM_GETIDLIST_FILTER_ENUMERATOR | CM_GETIDLIST_FILTER_PRESENT;
    // The list can grow between the two calls: retry a few times.
    for _ in 0..3 {
        let mut len = 0u32;
        if unsafe { CM_Get_Device_ID_List_SizeW(&mut len, PCWSTR(filter.as_ptr()), flags) } != CR_SUCCESS || len == 0 {
            return Vec::new();
        }
        let mut buf = vec![0u16; len as usize];
        if unsafe { CM_Get_Device_ID_ListW(PCWSTR(filter.as_ptr()), &mut buf, flags) } == CR_SUCCESS {
            return buf.split(|&c| c == 0).filter(|s| !s.is_empty()).map(String::from_utf16_lossy).collect();
        }
    }
    Vec::new()
}

fn devnode(id: &str) -> Option<u32> {
    let id = wide(id);
    let mut inst = 0u32;
    (unsafe { CM_Locate_DevNodeW(&mut inst, PCWSTR(id.as_ptr()), CM_LOCATE_DEVNODE_NORMAL) } == CR_SUCCESS).then_some(inst)
}

fn property(inst: u32, key: &DEVPROPKEY) -> Option<(DEVPROPTYPE, Vec<u8>)> {
    let mut kind = DEVPROPTYPE(0);
    let mut size = 0u32;
    unsafe { CM_Get_DevNode_PropertyW(inst, key, &mut kind, None, &mut size, 0) };
    if size == 0 {
        return None;
    }
    let mut buf = vec![0u8; size as usize];
    (unsafe { CM_Get_DevNode_PropertyW(inst, key, &mut kind, Some(buf.as_mut_ptr()), &mut size, 0) } == CR_SUCCESS).then_some((kind, buf))
}

fn prop_u8(inst: u32, key: &DEVPROPKEY) -> Option<u8> {
    property(inst, key).and_then(|(t, b)| (t == DEVPROP_TYPE_BYTE).then(|| b.first().copied()).flatten())
}

fn prop_u16(inst: u32, key: &DEVPROPKEY) -> Option<u16> {
    property(inst, key).and_then(|(t, b)| (t == DEVPROP_TYPE_UINT16 && b.len() >= 2).then(|| u16::from_le_bytes([b[0], b[1]])))
}

fn prop_u32(inst: u32, key: &DEVPROPKEY) -> Option<u32> {
    property(inst, key).and_then(|(t, b)| (t == DEVPROP_TYPE_UINT32 && b.len() >= 4).then(|| u32::from_le_bytes([b[0], b[1], b[2], b[3]])))
}

fn prop_bool(inst: u32, key: &DEVPROPKEY) -> Option<bool> {
    // DEVPROP_BOOLEAN: 0xFF true, 0 false.
    property(inst, key).and_then(|(t, b)| (t == DEVPROP_TYPE_BOOLEAN).then(|| b.first().is_some_and(|&v| v != 0)))
}

fn utf16(b: &[u8]) -> Vec<u16> {
    b.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect()
}

fn prop_string(inst: u32, key: &DEVPROPKEY) -> Option<String> {
    property(inst, key).and_then(|(t, b)| (t == DEVPROP_TYPE_STRING).then(|| from_wide(&utf16(&b)))).filter(|s| !s.is_empty())
}

fn prop_strings(inst: u32, key: &DEVPROPKEY) -> Vec<String> {
    match property(inst, key) {
        Some((t, b)) if t == DEVPROP_TYPE_STRING_LIST => {
            utf16(&b).split(|&c| c == 0).filter(|s| !s.is_empty()).map(String::from_utf16_lossy).collect()
        }
        _ => Vec::new(),
    }
}

fn prop_guid(inst: u32, key: &DEVPROPKEY) -> Option<String> {
    property(inst, key).and_then(|(t, b)| (t == DEVPROP_TYPE_GUID && b.len() == 16).then(|| {
        let g = GUID::from_values(
            u32::from_le_bytes([b[0], b[1], b[2], b[3]]),
            u16::from_le_bytes([b[4], b[5]]),
            u16::from_le_bytes([b[6], b[7]]),
            [b[8], b[9], b[10], b[11], b[12], b[13], b[14], b[15]],
        );
        format!("{g:?}").to_lowercase()
    }))
}

/// The device node of a paired device (as opposed to one of its services).
fn is_device_node(id: &str) -> bool {
    let up = id.to_ascii_uppercase();
    up.starts_with("BTHENUM\\DEV_") || up.starts_with("BTHLE\\DEV_")
}

#[derive(Default)]
struct Group {
    name: Option<String>,
    connected: bool,
    battery: Option<u8>,
    class: Option<u32>,
    category: Option<&'static str>,
    vid: u16,
    pid: u16,
    has_device_node: bool,
}

/// Paired Bluetooth devices with what Windows knows of their batteries.
/// Blocking (CfgMgr32 reads, a few milliseconds).
pub fn devices() -> AppResult<Vec<BtDevice>> {
    let mut groups: BTreeMap<String, Group> = BTreeMap::new();
    for id in instance_ids("BTHENUM").into_iter().chain(instance_ids("BTHLE")) {
        let Some(inst) = devnode(&id) else { continue };
        let Some(container) = prop_guid(inst, &KEY_CONTAINER) else { continue };
        let g = groups.entry(container).or_default();
        if g.battery.is_none() {
            g.battery = prop_u8(inst, &KEY_BATTERY).filter(|b| *b <= 100);
        }
        if !is_device_node(&id) {
            continue;
        }
        g.has_device_node = true;
        g.connected |= prop_bool(inst, &KEY_CONNECTED).unwrap_or(false);
        // A classic node names the device the way the user paired it.
        if g.name.is_none() || id.to_ascii_uppercase().starts_with("BTHENUM") {
            if let Some(n) = prop_string(inst, &KEY_FRIENDLY_NAME) {
                g.name = Some(n);
            }
        }
        g.class = g.class.or(prop_u32(inst, &KEY_CLASS).filter(|c| *c != 0));
        if g.vid == 0 {
            g.vid = prop_u16(inst, &KEY_VID).unwrap_or(0);
            g.pid = prop_u16(inst, &KEY_PID).unwrap_or(0);
        }
        if g.category.is_none() {
            g.category = prop_strings(inst, &KEY_CATEGORY).iter().find_map(|c| kind_from_category(c));
        }
    }
    Ok(groups
        .into_iter()
        .filter(|(_, g)| g.has_device_node && g.name.is_some())
        .map(|(id, g)| {
            let apple = g.vid == APPLE;
            let model = if apple { apple_model(g.pid) } else { None };
            let kind = if apple && apple_earbuds(g.pid) {
                "earbuds"
            } else {
                match g.class.map(kind_from_class) {
                    Some(k) if k != "other" => k,
                    _ => g.category.unwrap_or("other"),
                }
            };
            BtDevice {
                id,
                name: g.name.unwrap_or_default(),
                kind,
                connected: g.connected,
                battery: g.battery,
                model,
                vendor: vendor_name(g.vid),
                vid: g.vid,
                pid: g.pid,
                ..Default::default()
            }
        })
        .collect())
}

/// The latest proximity advert of the watched model.
#[derive(Clone, Copy)]
struct Heard {
    at: Instant,
    rssi: i16,
    advert: Proximity,
}

/// Adverts this faint are a room away, not the user's ears.
const MIN_RSSI: i16 = -72;
/// An advert older than this no longer says how things are.
const FRESH_FOR: Duration = Duration::from_secs(90);

/// Listens for the proximity adverts of one Apple model while asked to.
#[derive(Default)]
pub struct PodsWatch {
    watcher: Mutex<Option<(u16, BluetoothLEAdvertisementWatcher)>>,
    heard: Arc<Mutex<Option<Heard>>>,
}

impl PodsWatch {
    /// Scan for `model`'s adverts, or stop with None. Cheap to call often.
    pub fn watch(&self, model: Option<u16>) {
        let mut cur = self.watcher.lock();
        if cur.as_ref().map(|(m, _)| *m) == model {
            return;
        }
        if let Some((_, w)) = cur.take() {
            let _ = w.Stop();
        }
        *self.heard.lock() = None;
        let Some(model) = model else { return };
        match self.start(model) {
            Ok(w) => {
                tracing::info!(model = format!("{model:#06x}"), "bluetooth: listening for the headphones' own battery adverts");
                *cur = Some((model, w));
            }
            Err(e) => tracing::info!("bluetooth: LE scan unavailable: {e}"),
        }
    }

    fn start(&self, model: u16) -> windows::core::Result<BluetoothLEAdvertisementWatcher> {
        let watcher = BluetoothLEAdvertisementWatcher::new()?;
        watcher.SetScanningMode(BluetoothLEScanningMode::Passive)?;
        let heard = self.heard.clone();
        watcher.Received(&TypedEventHandler::new(move |_, args: windows::core::Ref<BluetoothLEAdvertisementReceivedEventArgs>| {
            let Some(args) = args.as_ref() else { return Ok(()) };
            let rssi = args.RawSignalStrengthInDBm()?;
            if rssi < MIN_RSSI {
                return Ok(());
            }
            let list = args.Advertisement()?.ManufacturerData()?;
            for i in 0..list.Size()? {
                let m = list.GetAt(i)?;
                if m.CompanyId()? != APPLE {
                    continue;
                }
                let buf = m.Data()?;
                let mut bytes = vec![0u8; buf.Length()? as usize];
                DataReader::FromBuffer(&buf)?.ReadBytes(&mut bytes)?;
                let Some(advert) = decode_proximity(&bytes).filter(|a| a.model == model) else { continue };
                let mut h = heard.lock();
                // The strongest recent advert is the user's own pair.
                let keep = h.is_some_and(|old| old.at.elapsed() < Duration::from_secs(8) && old.rssi > rssi + 6);
                if !keep {
                    *h = Some(Heard { at: Instant::now(), rssi, advert });
                }
            }
            Ok(())
        }))?;
        watcher.Start()?;
        Ok(watcher)
    }

    /// Buds and case from a fresh advert of the watched model.
    pub fn latest(&self) -> Option<Proximity> {
        self.heard.lock().filter(|h| h.at.elapsed() < FRESH_FOR).map(|h| h.advert)
    }
}

#[cfg(test)]
mod live {
    /// `cargo test --lib live_bluetooth -- --ignored --nocapture`: paired
    /// devices and, for 20 s, the proximity adverts of a connected AirPods.
    #[test]
    #[ignore]
    fn live_bluetooth() {
        super::super::com::ensure_mta();
        let devices = super::devices().unwrap();
        for d in &devices {
            println!("{d:?}");
        }
        let pods = super::PodsWatch::default();
        // `JARVIS_BT_MODEL=2024` listens for that model even when not connected.
        let forced = std::env::var("JARVIS_BT_MODEL").ok().and_then(|m| u16::from_str_radix(m.trim_start_matches("0x"), 16).ok());
        let model = forced.or(devices.iter().find(|d| d.connected && d.vid == super::APPLE && d.model.is_some()).map(|d| d.pid));
        pods.watch(model);
        for _ in 0..20 {
            std::thread::sleep(std::time::Duration::from_secs(1));
            println!("{:?}", pods.latest());
        }
    }
}
