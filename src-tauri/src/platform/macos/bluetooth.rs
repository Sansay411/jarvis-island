//! Bluetooth devices and their batteries on macOS: `system_profiler` already
//! tells each AirPod and the case apart, so no advert scan is needed.

pub use super::data_bluetooth::*;
use super::native;
use crate::error::{AppError, AppResult};
use serde_json::Value;

/// Paired Bluetooth devices with their batteries. Blocking (~1 s).
pub fn devices() -> AppResult<Vec<BtDevice>> {
    let out = native::execute("/usr/sbin/system_profiler", &["SPBluetoothDataType", "-json", "-detailLevel", "basic"], None)?;
    parse(&out)
}

fn percent(v: Option<&Value>) -> Option<u8> {
    v?.as_str()?.trim().trim_end_matches('%').trim().parse::<u8>().ok().filter(|p| *p <= 100)
}

fn hex(v: Option<&Value>) -> u16 {
    v.and_then(Value::as_str).and_then(|s| u16::from_str_radix(s.trim().trim_start_matches("0x").trim_start_matches("0X"), 16).ok()).unwrap_or(0)
}

fn kind_from_minor(minor: &str, vid: u16, pid: u16) -> &'static str {
    if vid == APPLE && apple_earbuds(pid) {
        return "earbuds";
    }
    let m = minor.to_ascii_lowercase();
    if m.contains("trackpad") {
        return "mouse";
    }
    if m.contains("smartphone") || m.contains("phone") {
        return "phone";
    }
    kind_from_category(&m).unwrap_or("other")
}

fn part(level: Option<u8>) -> Option<BtPart> {
    level.map(|l| BtPart { level: Some(l), ..Default::default() })
}

fn parse(json: &str) -> AppResult<Vec<BtDevice>> {
    let root: Value = serde_json::from_str(json).map_err(|_| AppError::platform("Некорректный ответ system_profiler"))?;
    let mut out = Vec::new();
    for controller in root["SPBluetoothDataType"].as_array().into_iter().flatten() {
        for (key, connected) in [("device_connected", true), ("device_not_connected", false)] {
            for entry in controller[key].as_array().into_iter().flatten() {
                let Some(map) = entry.as_object() else { continue };
                for (name, d) in map {
                    let (vid, pid) = (hex(d.get("device_vendorID")), hex(d.get("device_productID")));
                    let address = d.get("device_address").and_then(Value::as_str).unwrap_or(name);
                    let left = percent(d.get("device_batteryLevelLeft"));
                    let right = percent(d.get("device_batteryLevelRight"));
                    let case = percent(d.get("device_batteryLevelCase"));
                    let main = percent(d.get("device_batteryLevelMain")).or(percent(d.get("device_batteryLevel")));
                    out.push(BtDevice {
                        id: address.to_ascii_lowercase(),
                        name: name.clone(),
                        kind: kind_from_minor(d.get("device_minorType").and_then(Value::as_str).unwrap_or(""), vid, pid),
                        connected,
                        battery: main.or_else(|| [left, right].into_iter().flatten().min()),
                        left: part(left),
                        right: part(right),
                        case_part: part(case),
                        model: if vid == APPLE { apple_model(pid) } else { None },
                        vendor: vendor_name(vid),
                        vid,
                        pid,
                        ..Default::default()
                    });
                }
            }
        }
    }
    Ok(out)
}

/// macOS reports each bud itself: nothing to listen for.
#[derive(Default)]
pub struct PodsWatch;

impl PodsWatch {
    pub fn watch(&self, _model: Option<u16>) {}

    pub fn latest(&self) -> Option<Proximity> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_airpods_parts_and_plain_devices() {
        let json = r#"{"SPBluetoothDataType":[{"controller_properties":{},
            "device_connected":[{"AirPods Pro":{"device_address":"A4:F6:E6:84:B6:7A","device_batteryLevelCase":"41%",
                "device_batteryLevelLeft":"100%","device_batteryLevelRight":"95%","device_minorType":"Headphones",
                "device_productID":"0x2024","device_vendorID":"0x004C"}}],
            "device_not_connected":[{"Keyboard K380":{"device_address":"F4:73:35:AF:DA:82","device_minorType":"Keyboard",
                "device_productID":"0xB342","device_vendorID":"0x046D","device_batteryLevelMain":"60%"}}]}]}"#;
        let d = parse(json).unwrap();
        assert_eq!(d.len(), 2);
        let pods = &d[0];
        assert!(pods.connected);
        assert_eq!(pods.kind, "earbuds");
        assert_eq!(pods.model, Some("AirPods Pro 2 (USB‑C)"));
        assert_eq!(pods.left.unwrap().level, Some(100));
        assert_eq!(pods.case_part.unwrap().level, Some(41));
        assert_eq!(pods.battery, Some(95));
        let kb = &d[1];
        assert!(!kb.connected);
        assert_eq!(kb.kind, "keyboard");
        assert_eq!(kb.vendor, Some("Logitech"));
        assert_eq!(kb.battery, Some(60));
        assert!(parse("nope").is_err());
    }
}
