//! Bluetooth devices and their batteries: what both platforms report, and the
//! pure decoders behind it (class of device, Apple proximity adverts).

use serde::Serialize;

/// A paired Bluetooth device as the island shows it.
#[derive(Debug, Clone, Serialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct BtDevice {
    /// Stable across restarts: the device container (Windows) or the address.
    pub id: String,
    pub name: String,
    /// headphones | earbuds | headset | speaker | keyboard | mouse | gamepad |
    /// phone | watch | computer | audio | other
    pub kind: &'static str,
    pub connected: bool,
    /// The level the system reads for the whole device, 0..=100. For a
    /// disconnected device it is the last one known.
    pub battery: Option<u8>,
    /// Each bud and the case, when the headphones tell them apart.
    pub left: Option<BtPart>,
    pub right: Option<BtPart>,
    #[serde(rename = "case")]
    pub case_part: Option<BtPart>,
    /// The case lid is open (only known while a bud is in it).
    pub lid_open: Option<bool>,
    /// «AirPods Pro 2», when the model is known.
    pub model: Option<&'static str>,
    pub vendor: Option<&'static str>,
    /// Bluetooth SIG vendor and product ids (0 when unknown).
    #[serde(skip)]
    pub vid: u16,
    #[serde(skip)]
    pub pid: u16,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct BtPart {
    /// 0..=100; None when this part is not reporting (out of range, in a
    /// closed case).
    pub level: Option<u8>,
    pub charging: bool,
    /// In an ear (buds only).
    pub in_ear: Option<bool>,
}

impl BtDevice {
    /// Headphones, earbuds, a headset or a speaker: what the battery deck is about.
    pub fn is_audio(&self) -> bool {
        matches!(self.kind, "headphones" | "earbuds" | "headset" | "speaker" | "audio")
    }

    /// The lowest level among the parts that are in use, or the device's own.
    pub fn level(&self) -> Option<u8> {
        let buds = [self.left, self.right].into_iter().flatten().filter_map(|p| p.level).min();
        buds.or(self.battery)
    }
}

/// Bluetooth SIG company id of Apple.
pub const APPLE: u16 = 0x004C;

/// Apple and Beats headphones by product id (the same id the Bluetooth
/// device reports and the proximity advert carries).
pub fn apple_model(pid: u16) -> Option<&'static str> {
    Some(match pid {
        0x2002 => "AirPods",
        0x200F => "AirPods 2",
        0x2013 => "AirPods 3",
        0x2019 => "AirPods 4",
        0x201B => "AirPods 4 с шумоподавлением",
        0x200E => "AirPods Pro",
        0x2014 => "AirPods Pro 2",
        0x2024 => "AirPods Pro 2 (USB‑C)",
        0x2027 => "AirPods Pro 3",
        0x200A => "AirPods Max",
        0x201F => "AirPods Max (USB‑C)",
        0x2003 => "Powerbeats3",
        0x2005 => "BeatsX",
        0x2006 => "Beats Solo3",
        0x2009 => "Beats Studio3",
        0x200B => "Powerbeats Pro",
        0x200C => "Beats Solo Pro",
        0x200D => "Powerbeats",
        0x2010 => "Beats Flex",
        0x2011 => "Beats Studio Buds",
        0x2012 => "Beats Fit Pro",
        0x2016 => "Beats Studio Buds+",
        0x2017 => "Beats Studio Pro",
        0x201D => "Powerbeats Pro 2",
        _ => return None,
    })
}

/// One battery for the whole device: over-ear headphones and on-ear Beats.
pub fn apple_single(pid: u16) -> bool {
    matches!(pid, 0x200A | 0x201F | 0x2006 | 0x2009 | 0x200C | 0x2017 | 0x2003 | 0x2005 | 0x2010)
}

/// Vendor names for the ids the island is likely to meet.
pub fn vendor_name(vid: u16) -> Option<&'static str> {
    Some(match vid {
        APPLE => "Apple",
        0x046D => "Logitech",
        0x054C => "Sony",
        0x0075 => "Samsung",
        0x009E => "Bose",
        0x0057 => "Harman",
        0x00E0 => "Google",
        0x0006 => "Microsoft",
        0x038F => "Xiaomi",
        0x027D => "Huawei",
        0x02B0 => "Jabra",
        0x0067 => "GN Netcom",
        0x0103 => "Bang & Olufsen",
        0x0310 => "Sennheiser",
        0x0082 => "Sennheiser",
        _ => return None,
    })
}

/// The kind of device from its Bluetooth Class of Device.
pub fn kind_from_class(cod: u32) -> &'static str {
    let major = (cod >> 8) & 0x1F;
    let minor = (cod >> 2) & 0x3F;
    match major {
        0x01 => "computer",
        0x02 => "phone",
        0x04 => match minor {
            0x01 | 0x02 => "headset",
            0x06 => "headphones",
            0x05 | 0x07 | 0x0A => "speaker",
            _ => "audio",
        },
        0x05 => match ((cod >> 6) & 0x03, (cod >> 2) & 0x0F) {
            (_, 0x01 | 0x02) => "gamepad",
            (0x01, _) => "keyboard",
            (0x02, _) => "mouse",
            (0x03, _) => "keyboard",
            _ => "other",
        },
        0x07 => "watch",
        _ => "other",
    }
}

/// The kind from Windows' device container category (`Audio.Headphone`,
/// `Input.Keyboard`…), for Bluetooth LE devices that have no class of device.
pub fn kind_from_category(category: &str) -> Option<&'static str> {
    let c = category.to_ascii_lowercase();
    Some(if c.contains("headphone") {
        "headphones"
    } else if c.contains("headset") {
        "headset"
    } else if c.contains("speaker") {
        "speaker"
    } else if c.contains("keyboard") {
        "keyboard"
    } else if c.contains("mouse") || c.contains("pointing") {
        "mouse"
    } else if c.contains("gaming") || c.contains("gamepad") || c.contains("joystick") {
        "gamepad"
    } else if c.contains("phone") {
        "phone"
    } else if c.contains("watch") || c.contains("wearable") {
        "watch"
    } else if c.contains("audio") {
        "audio"
    } else {
        return None;
    })
}

/// In-ear Apple and Beats models: earbuds, not over-ear headphones.
pub fn apple_earbuds(pid: u16) -> bool {
    apple_model(pid).is_some() && !apple_single(pid)
}

/// What Apple headphones broadcast about themselves (the «proximity pairing»
/// message an iPhone shows the battery card from).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Proximity {
    pub model: u16,
    pub left: BtPart,
    pub right: BtPart,
    pub case_part: BtPart,
    /// Only meaningful while a bud is in the case.
    pub lid_open: Option<bool>,
    /// One battery for the whole device (AirPods Max, Beats Solo…).
    pub single: Option<BtPart>,
}

/// A battery nibble: 0–10 is ×10 %, 15 means «not reporting».
fn nibble_level(n: u8) -> Option<u8> {
    (n <= 10).then_some(n * 10)
}

/// Decode Apple manufacturer data (company id stripped, starting at the
/// message type). Layout from public reverse engineering (furiousMAC
/// continuity, OpenPods, AirPodsDesktop, CAPod), as confirmed on AirPods Pro
/// 2 and 3:
///
/// * 0 type `0x07`, 1 length `0x19`, 2 prefix `0x01` (paired), 3–4 model (LE);
/// * 5 status: `0x20` the broadcasting bud is the left one, `0x02` / `0x08`
///   the first / second bud slot is in an ear (the first slot is the left bud
///   when exactly one of `0x20` and `0x40` is set), `0x04` both in the case;
/// * 6 batteries: low nibble the broadcasting bud, high nibble the other;
/// * 7 low nibble the case, high nibble charging (`1` broadcasting bud,
///   `2` the other, `4` the case);
/// * 8 lid: `0x08` closed.
pub fn decode_proximity(data: &[u8]) -> Option<Proximity> {
    // Several messages may share one advert: walk the type-length chain.
    let mut at = 0;
    while at + 2 <= data.len() {
        let (kind, len) = (data[at], data[at + 1] as usize);
        if kind == 0x07 && len == 0x19 && at + 2 + len <= data.len() {
            return decode_pairing(&data[at..at + 2 + len]);
        }
        at += 2 + len;
    }
    None
}

fn decode_pairing(m: &[u8]) -> Option<Proximity> {
    if m.len() < 27 || m[2] != 0x01 {
        return None;
    }
    let model = u16::from_le_bytes([m[3], m[4]]);
    let (status, pods, flags, lid) = (m[5], m[6], m[7], m[8]);
    let primary_left = status & 0x20 != 0;
    let first_left = primary_left ^ (status & 0x40 != 0);
    let (first_in_ear, second_in_ear) = (status & 0x02 != 0, status & 0x08 != 0);
    let charging = flags >> 4;
    let primary = BtPart { level: nibble_level(pods & 0x0F), charging: charging & 0x1 != 0, in_ear: None };
    let other = BtPart { level: nibble_level(pods >> 4), charging: charging & 0x2 != 0, in_ear: None };
    let (mut left, mut right) = if primary_left { (primary, other) } else { (other, primary) };
    left.in_ear = Some(if first_left { first_in_ear } else { second_in_ear });
    right.in_ear = Some(if first_left { second_in_ear } else { first_in_ear });
    let case_part = BtPart { level: nibble_level(flags & 0x0F), charging: charging & 0x4 != 0, in_ear: None };
    let in_case = status & 0x04 != 0 || status & 0x10 != 0 || status & 0x40 != 0;
    let single = apple_single(model).then_some(BtPart { in_ear: None, ..primary });
    Some(Proximity { model, left, right, case_part, lid_open: in_case.then_some(lid & 0x08 == 0), single })
}

/// Battery samples over time → minutes left at the measured rate. Only a
/// clear trend counts: at least a 5 % drop over a quarter of an hour, while
/// not charging (a rise in between restarts the run).
pub fn minutes_left(samples: &[(i64, u8)], now_ms: i64) -> Option<u32> {
    const WINDOW_MS: i64 = 4 * 3600 * 1000;
    let recent: Vec<&(i64, u8)> = samples.iter().filter(|(t, _)| now_ms - t <= WINDOW_MS).collect();
    // The run of falling (or flat) levels that ends with the latest sample.
    let mut start = recent.len().checked_sub(1)?;
    while start > 0 && recent[start - 1].1 >= recent[start].1 {
        start -= 1;
    }
    let (first, last) = (recent[start], recent[recent.len() - 1]);
    let drop = first.1.saturating_sub(last.1) as f64;
    let hours = (last.0 - first.0) as f64 / 3_600_000.0;
    if drop < 5.0 || hours < 0.25 {
        return None;
    }
    let per_hour = drop / hours;
    let minutes = last.1 as f64 / per_hour * 60.0;
    (minutes.is_finite() && minutes > 0.0).then_some(minutes.min(48.0 * 60.0) as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn advert(status: u8, pods: u8, flags: u8, lid: u8) -> Vec<u8> {
        let mut m = vec![0x07, 0x19, 0x01, 0x24, 0x20, status, pods, flags, lid, 0x00, 0x00];
        m.resize(27, 0xAA);
        m
    }

    #[test]
    fn decodes_both_buds_and_the_case_on_either_side() {
        // Broadcast from the left bud, both in ears, case closed far away.
        let p = decode_proximity(&advert(0x20 | 0x02 | 0x08, 0x89, 0x0F, 0x00)).unwrap();
        assert_eq!(p.model, 0x2024);
        assert_eq!(p.left, BtPart { level: Some(90), charging: false, in_ear: Some(true) });
        assert_eq!(p.right, BtPart { level: Some(80), charging: false, in_ear: Some(true) });
        assert_eq!(p.case_part.level, None);
        assert_eq!(p.lid_open, None);
        // Broadcast from the right bud: the nibbles swap sides.
        let p = decode_proximity(&advert(0x02, 0x37, 0x05, 0x00)).unwrap();
        assert_eq!(p.right.level, Some(70));
        assert_eq!(p.left.level, Some(30));
        assert_eq!(p.case_part.level, Some(50));
        assert!(p.right.in_ear.unwrap() && !p.left.in_ear.unwrap());
    }

    #[test]
    fn charging_bits_and_the_lid_follow_the_buds_in_the_case() {
        // Left broadcasting from the case (0x40 flips the in-ear slots), both
        // buds charging, the case charging too, lid open.
        let p = decode_proximity(&advert(0x20 | 0x40 | 0x04, 0xA6, 0x70 | 0x08, 0x31)).unwrap();
        assert!(p.left.charging && p.right.charging && p.case_part.charging);
        assert_eq!(p.left.level, Some(60));
        assert_eq!(p.right.level, Some(100));
        assert_eq!(p.case_part.level, Some(80));
        assert_eq!(p.lid_open, Some(true));
        let closed = decode_proximity(&advert(0x20 | 0x40 | 0x04, 0xA6, 0x08, 0x39)).unwrap();
        assert_eq!(closed.lid_open, Some(false));
    }

    #[test]
    fn other_apple_messages_and_short_data_are_not_headphones() {
        assert_eq!(decode_proximity(&[]), None);
        assert_eq!(decode_proximity(&[0x10, 0x05, 0x01, 0x18, 0x44, 0x00, 0x00]), None);
        let mut pairing_mode = advert(0x22, 0x99, 0x0F, 0x00);
        pairing_mode[2] = 0x00;
        assert_eq!(decode_proximity(&pairing_mode), None);
        // A «nearby info» message first, then the pairing one.
        let mut chained = vec![0x10, 0x02, 0x0B, 0x1C];
        chained.extend(advert(0x22, 0x99, 0x0F, 0x00));
        assert_eq!(decode_proximity(&chained).unwrap().left.level, Some(90));
    }

    #[test]
    fn classes_of_device_name_the_kind() {
        assert_eq!(kind_from_class(0x240418), "headphones");
        assert_eq!(kind_from_class(0x240404), "headset");
        assert_eq!(kind_from_class(0x240414), "speaker");
        assert_eq!(kind_from_class(0x002540), "keyboard");
        assert_eq!(kind_from_class(0x002580), "mouse");
        assert_eq!(kind_from_class(0x002508), "gamepad");
        assert_eq!(kind_from_class(0x7A020C), "phone");
        assert_eq!(kind_from_category("Audio.Headphone"), Some("headphones"));
        assert_eq!(kind_from_category("Input.Keyboard"), Some("keyboard"));
        assert_eq!(kind_from_category("Other"), None);
        assert_eq!(apple_model(0x2024), Some("AirPods Pro 2 (USB‑C)"));
        assert!(apple_earbuds(0x2024) && !apple_earbuds(0x200A));
    }

    #[test]
    fn minutes_left_needs_a_clear_falling_run() {
        let h = 3_600_000;
        // 100 → 80 in two hours: 10 %/h, 80 % left → eight hours.
        let s = [(0, 100), (h, 90), (2 * h, 80)];
        assert_eq!(minutes_left(&s, 2 * h), Some(480));
        // Too little to tell.
        assert_eq!(minutes_left(&[(0, 100), (10 * 60_000, 98)], 10 * 60_000), None);
        // Charged in between: only the run after the charge counts.
        let s = [(0, 50), (h, 40), (2 * h, 90), (3 * h, 85)];
        assert_eq!(minutes_left(&s, 3 * h), Some(1020));
        assert_eq!(minutes_left(&[], 0), None);
        // Old samples fall out of the window.
        assert_eq!(minutes_left(&[(0, 100), (h, 80)], 10 * h), None);
    }

    #[test]
    fn the_device_level_is_the_lowest_bud_in_use() {
        let mut d = BtDevice { battery: Some(70), ..Default::default() };
        assert_eq!(d.level(), Some(70));
        d.left = Some(BtPart { level: Some(40), ..Default::default() });
        d.right = Some(BtPart { level: Some(90), ..Default::default() });
        assert_eq!(d.level(), Some(40));
    }
}
