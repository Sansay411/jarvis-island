//! Display brightness.
//!  * Internal panels: WMI `root\WMI` — `WmiMonitorBrightness` (read) and
//!    `WmiMonitorBrightnessMethods.WmiSetBrightness` (write).
//!  * External monitors: Monitor Configuration API over DDC/CI (dxva2).
//! Not every monitor supports software brightness; unsupported displays are
//! reported with `supported = false` instead of being silently skipped.

use std::time::Duration;

use serde::Serialize;
use windows::core::{BSTR, PCWSTR};
use windows::Win32::System::Variant::VARIANT;
use windows::Win32::Devices::Display::{
    DestroyPhysicalMonitors, GetMonitorBrightness, GetNumberOfPhysicalMonitorsFromHMONITOR,
    GetPhysicalMonitorsFromHMONITOR, SetMonitorBrightness, PHYSICAL_MONITOR,
};
use windows::Win32::Foundation::{LPARAM, RECT};
use windows::Win32::Graphics::Gdi::{EnumDisplayMonitors, HDC, HMONITOR};
use windows::Win32::System::Com::{
    CoCreateInstance, CoSetProxyBlanket, CLSCTX_INPROC_SERVER, EOAC_NONE, RPC_C_AUTHN_LEVEL_CALL,
    RPC_C_IMP_LEVEL_IMPERSONATE,
};
use windows::Win32::System::Wmi::{
    IWbemClassObject, IWbemLocator, IWbemServices, WbemLocator, WBEM_FLAG_FORWARD_ONLY,
    WBEM_FLAG_RETURN_IMMEDIATELY, WBEM_GENERIC_FLAG_TYPE, WBEM_INFINITE,
};

use super::from_wide;
use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DisplayBrightness {
    pub id: String,
    pub name: String,
    /// "internal" (WMI) or "external" (DDC/CI)
    pub kind: &'static str,
    pub brightness: Option<u8>,
    pub supported: bool,
}

// ---------- WMI (internal panels) ----------

const RPC_C_AUTHN_WINNT: u32 = 10;
const RPC_C_AUTHZ_NONE: u32 = 0;

fn wmi() -> AppResult<IWbemServices> {
    unsafe {
        let locator: IWbemLocator = CoCreateInstance(&WbemLocator, None, CLSCTX_INPROC_SERVER)?;
        let svc = locator.ConnectServer(
            &BSTR::from("ROOT\\WMI"),
            &BSTR::new(),
            &BSTR::new(),
            &BSTR::new(),
            0,
            &BSTR::new(),
            None,
        )?;
        CoSetProxyBlanket(
            &svc,
            RPC_C_AUTHN_WINNT,
            RPC_C_AUTHZ_NONE,
            PCWSTR::null(),
            RPC_C_AUTHN_LEVEL_CALL,
            RPC_C_IMP_LEVEL_IMPERSONATE,
            None,
            EOAC_NONE,
        )?;
        Ok(svc)
    }
}

fn query(svc: &IWbemServices, wql: &str) -> AppResult<Vec<IWbemClassObject>> {
    unsafe {
        let en = svc.ExecQuery(
            &BSTR::from("WQL"),
            &BSTR::from(wql),
            WBEM_FLAG_FORWARD_ONLY | WBEM_FLAG_RETURN_IMMEDIATELY,
            None,
        )?;
        let mut out = Vec::new();
        loop {
            let mut objs = [None];
            let mut returned = 0u32;
            let hr = en.Next(WBEM_INFINITE, &mut objs, &mut returned);
            if hr.is_err() || returned == 0 {
                break;
            }
            if let Some(o) = objs[0].take() {
                out.push(o);
            }
        }
        Ok(out)
    }
}

fn prop(obj: &IWbemClassObject, name: &str) -> AppResult<VARIANT> {
    let wname = super::wide(name);
    let mut v = VARIANT::default();
    unsafe { obj.Get(PCWSTR(wname.as_ptr()), 0, &mut v, None, None)? };
    Ok(v)
}

fn wmi_panels(svc: &IWbemServices) -> AppResult<Vec<DisplayBrightness>> {
    let mut out = Vec::new();
    for obj in query(svc, "SELECT InstanceName, CurrentBrightness FROM WmiMonitorBrightness WHERE Active=TRUE")? {
        let instance = prop(&obj, "InstanceName")?.to_string();
        let level = u32::try_from(&prop(&obj, "CurrentBrightness")?).ok().map(|v| v.min(100) as u8);
        out.push(DisplayBrightness {
            id: format!("wmi:{instance}"),
            name: "Встроенный дисплей".into(),
            kind: "internal",
            brightness: level,
            supported: true,
        });
    }
    Ok(out)
}

fn wmi_set(svc: &IWbemServices, instance: Option<&str>, percent: u8) -> AppResult<()> {
    unsafe {
        let mut class: Option<IWbemClassObject> = None;
        svc.GetObject(
            &BSTR::from("WmiMonitorBrightnessMethods"),
            WBEM_GENERIC_FLAG_TYPE(0),
            None,
            Some(&mut class),
            None,
        )?;
        let class = class.ok_or_else(|| AppError::unsupported("WMI-класс яркости недоступен"))?;
        let mut in_sig: Option<IWbemClassObject> = None;
        let method = super::wide("WmiSetBrightness");
        class.GetMethod(PCWSTR(method.as_ptr()), 0, &mut in_sig, std::ptr::null_mut())?;
        let in_params = in_sig
            .ok_or_else(|| AppError::unsupported("Метод WmiSetBrightness недоступен"))?
            .SpawnInstance(0)?;
        // CIM uint32 is marshalled as VT_I4, uint8 as VT_UI1.
        let timeout = super::wide("Timeout");
        let brightness = super::wide("Brightness");
        in_params.Put(PCWSTR(timeout.as_ptr()), 0, &VARIANT::from(0i32), 0)?;
        in_params.Put(PCWSTR(brightness.as_ptr()), 0, &VARIANT::from(percent), 0)?;

        let targets = query(svc, "SELECT __PATH, InstanceName FROM WmiMonitorBrightnessMethods WHERE Active=TRUE")?;
        if targets.is_empty() {
            return Err(AppError::unsupported("Встроенный дисплей не поддерживает управление яркостью"));
        }
        for t in targets {
            let name = prop(&t, "InstanceName")?.to_string();
            if instance.is_some_and(|want| want != name) {
                continue;
            }
            let path = prop(&t, "__PATH")?.to_string();
            svc.ExecMethod(
                &BSTR::from(path),
                &BSTR::from("WmiSetBrightness"),
                WBEM_GENERIC_FLAG_TYPE(0),
                None,
                &in_params,
                None,
                None,
            )?;
        }
    }
    Ok(())
}

// ---------- DDC/CI (external monitors) ----------

struct Physical {
    handle: PHYSICAL_MONITOR,
    hmon: isize,
    index: usize,
}

fn physical_monitors() -> Vec<Physical> {
    unsafe extern "system" fn cb(hmon: HMONITOR, _: HDC, _: *mut RECT, data: LPARAM) -> windows::core::BOOL {
        let list = unsafe { &mut *(data.0 as *mut Vec<HMONITOR>) };
        list.push(hmon);
        true.into()
    }
    let mut hmons: Vec<HMONITOR> = Vec::new();
    unsafe {
        let _ = EnumDisplayMonitors(None, None, Some(cb), LPARAM(&mut hmons as *mut _ as isize));
    }
    let mut out = Vec::new();
    for hmon in hmons {
        unsafe {
            let mut n = 0u32;
            if GetNumberOfPhysicalMonitorsFromHMONITOR(hmon, &mut n).is_err() || n == 0 {
                continue;
            }
            let mut mons = vec![PHYSICAL_MONITOR::default(); n as usize];
            if GetPhysicalMonitorsFromHMONITOR(hmon, &mut mons).is_err() {
                continue;
            }
            for (index, handle) in mons.into_iter().enumerate() {
                out.push(Physical { handle, hmon: hmon.0 as isize, index });
            }
        }
    }
    out
}

fn release(list: Vec<Physical>) {
    let handles: Vec<PHYSICAL_MONITOR> = list.into_iter().map(|p| p.handle).collect();
    if !handles.is_empty() {
        unsafe {
            let _ = DestroyPhysicalMonitors(&handles);
        }
    }
}

fn ddc_read(p: &Physical) -> Option<(u32, u32, u32)> {
    let (mut min, mut cur, mut max) = (0u32, 0u32, 0u32);
    let ok = unsafe { GetMonitorBrightness(p.handle.hPhysicalMonitor, &mut min, &mut cur, &mut max) };
    (ok != 0 && max > min).then_some((min, cur, max))
}

fn ddc_entries(list: &[Physical]) -> Vec<DisplayBrightness> {
    list.iter()
        .filter_map(|p| {
            let (min, cur, max) = ddc_read(p)?;
            let pct = ((cur - min) as f64 / (max - min) as f64 * 100.0).round() as u8;
            let desc_buf = p.handle.szPhysicalMonitorDescription;
            let desc = from_wide(&desc_buf);
            Some(DisplayBrightness {
                id: format!("ddc:{}#{}", p.hmon, p.index),
                name: if desc.is_empty() { "Внешний монитор".into() } else { desc },
                kind: "external",
                brightness: Some(pct),
                supported: true,
            })
        })
        .collect()
}

// ---------- public API ----------

pub fn list() -> AppResult<Vec<DisplayBrightness>> {
    let mut out = match wmi().and_then(|svc| wmi_panels(&svc)) {
        Ok(v) => v,
        Err(e) => {
            tracing::debug!("WMI brightness unavailable: {e}");
            Vec::new()
        }
    };
    let phys = physical_monitors();
    out.extend(ddc_entries(&phys));
    release(phys);
    Ok(out)
}

/// Set brightness on one display (`target` = id from [`list`]) or on every
/// supported display. Returns the verified state read back afterwards.
pub fn set(percent: u8, target: Option<&str>) -> AppResult<Vec<DisplayBrightness>> {
    if percent > 100 {
        return Err(AppError::invalid("Яркость должна быть от 0 до 100%"));
    }
    let mut touched = 0;
    let mut errors = Vec::new();

    let want_wmi = target.is_none_or(|t| t.starts_with("wmi:"));
    if want_wmi {
        match wmi().and_then(|svc| wmi_set(&svc, target.and_then(|t| t.strip_prefix("wmi:")), percent)) {
            Ok(()) => touched += 1,
            Err(e) => errors.push(e.to_string()),
        }
    }

    let want_ddc = target.is_none_or(|t| t.starts_with("ddc:"));
    if want_ddc {
        let phys = physical_monitors();
        for p in &phys {
            let id = format!("ddc:{}#{}", p.hmon, p.index);
            if target.is_some_and(|t| t != id) {
                continue;
            }
            if let Some((min, _, max)) = ddc_read(p) {
                let value = min + ((max - min) as f64 * percent as f64 / 100.0).round() as u32;
                if unsafe { SetMonitorBrightness(p.handle.hPhysicalMonitor, value) } != 0 {
                    touched += 1;
                } else {
                    errors.push(format!("DDC/CI отклонил команду для {id}"));
                }
            }
        }
        release(phys);
    }

    if touched == 0 {
        return Err(AppError::unsupported(if errors.is_empty() {
            "Ни один дисплей не поддерживает программное управление яркостью".to_string()
        } else {
            errors.join("; ")
        }));
    }
    // Panels apply the change asynchronously; give them a moment before
    // reading back the verified values.
    std::thread::sleep(Duration::from_millis(150));
    list()
}
