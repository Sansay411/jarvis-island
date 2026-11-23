pub use super::data_display::*;
use super::{capture, native};
use crate::platform::HWND;
use core_graphics::{
    display::CGDisplay,
    event::CGEvent,
    event_source::{CGEventSource, CGEventSourceStateID},
};
use serde_json::json;

// macOS screen coordinates are global Quartz points (top-left origin), matching winmgr.
pub fn primary_monitor() -> Option<MonitorInfo> {
    list_monitors().into_iter().find(|m| m.primary)
}
pub fn monitor_for_window(hwnd: HWND) -> Option<MonitorInfo> {
    let monitors = list_monitors();
    let Ok(rect) = capture::window_frame(hwnd) else {
        return monitors.into_iter().find(|m| m.primary);
    };
    monitor_for_rect(monitors, rect)
}
pub fn monitor_for_point(x: i32, y: i32) -> Option<MonitorInfo> {
    nearest(list_monitors(), x as f64, y as f64)
}
pub fn monitor_for_cursor() -> Option<MonitorInfo> {
    let source = CGEventSource::new(CGEventSourceStateID::CombinedSessionState).ok()?;
    let point = CGEvent::new(source).ok()?.location();
    monitor_for_point(point.x.round() as i32, point.y.round() as i32)
}
pub fn list_monitors() -> Vec<MonitorInfo> {
    let ids = CGDisplay::active_displays().unwrap_or_default();
    if ids.is_empty() {
        return Vec::new();
    }
    // NSScreen is main-thread-only; the helper process owns its own AppKit main thread.
    let metadata = native::jxa(r#"
        const screens = $.NSScreen.screens, result = [];
        for (let i = 0; i < screens.count; i++) {
            const s = screens.objectAtIndex(i), v = s.visibleFrame;
            result.push({id: Number(ObjC.unwrap(s.deviceDescription.objectForKey('NSScreenNumber'))),
                name: ObjC.unwrap(s.localizedName), scale: s.backingScaleFactor,
                work: {x: Math.round(v.origin.x), y: Math.round(a.height - v.origin.y - v.size.height),
                    width: Math.round(v.size.width), height: Math.round(v.size.height)}});
        }
        return result;
    "#, json!({"height": CGDisplay::main().bounds().size.height})).ok();
    ids.into_iter()
        .map(|id| {
            let display = CGDisplay::new(id);
            let r = display.bounds();
            let bounds = Rect {
                x: r.origin.x.round() as i32,
                y: r.origin.y.round() as i32,
                width: r.size.width.round() as i32,
                height: r.size.height.round() as i32,
            };
            let meta = metadata
                .as_ref()
                .and_then(|v| v.as_array())
                .and_then(|list| list.iter().find(|m| m["id"].as_u64() == Some(id as u64)));
            MonitorInfo {
                id: id as isize,
                name: meta
                    .and_then(|m| m["name"].as_str())
                    .map(str::to_owned)
                    .unwrap_or_else(|| format!("Display {id}")),
                primary: display.is_main(),
                bounds,
                work_area: meta
                    .and_then(|m| serde_json::from_value(m["work"].clone()).ok())
                    .unwrap_or(bounds),
                scale: meta.and_then(|m| m["scale"].as_f64()).unwrap_or_else(|| {
                    display
                        .display_mode()
                        .map(|m| m.pixel_width() as f64 / m.width().max(1) as f64)
                        .unwrap_or(1.0)
                }),
            }
        })
        .collect()
}

fn nearest(monitors: Vec<MonitorInfo>, x: f64, y: f64) -> Option<MonitorInfo> {
    if let Some(monitor) = monitors.iter().find(|m| {
        let r = m.bounds;
        x >= r.x as f64
            && y >= r.y as f64
            && x < r.x as f64 + r.width as f64
            && y < r.y as f64 + r.height as f64
    }) {
        return Some(monitor.clone());
    }
    monitors.into_iter().min_by(|a, b| {
        let distance = |m: &MonitorInfo| {
            let r = m.bounds;
            let dx = (r.x as f64 - x)
                .max(0.0)
                .max(x - (r.x as f64 + r.width as f64));
            let dy = (r.y as f64 - y)
                .max(0.0)
                .max(y - (r.y as f64 + r.height as f64));
            dx * dx + dy * dy
        };
        distance(a).total_cmp(&distance(b))
    })
}

fn monitor_for_rect(monitors: Vec<MonitorInfo>, rect: Rect) -> Option<MonitorInfo> {
    let overlap = |m: &MonitorInfo| {
        let r = m.bounds;
        let width = ((r.x as i64 + r.width as i64).min(rect.x as i64 + rect.width as i64)
            - (r.x as i64).max(rect.x as i64))
        .max(0);
        let height = ((r.y as i64 + r.height as i64).min(rect.y as i64 + rect.height as i64)
            - (r.y as i64).max(rect.y as i64))
        .max(0);
        width * height
    };
    if let Some(m) = monitors
        .iter()
        .filter(|m| overlap(m) > 0)
        .max_by_key(|m| overlap(m))
    {
        return Some(m.clone());
    }
    nearest(
        monitors,
        rect.x as f64 + rect.width as f64 / 2.0,
        rect.y as f64 + rect.height as f64 / 2.0,
    )
}
pub fn foreground_shape(
    _own_hwnd: isize,
    _island_monitor: isize,
    _island_center_x: i32,
    _island_bottom_y: i32,
) -> ForegroundShape {
    ForegroundShape::default()
}

#[cfg(test)]
mod tests {
    use super::*;
    fn monitors() -> Vec<MonitorInfo> {
        [-100, 0]
            .into_iter()
            .map(|x| MonitorInfo {
                id: x as isize,
                name: x.to_string(),
                primary: x == 0,
                bounds: Rect {
                    x,
                    y: 0,
                    width: 100,
                    height: 100,
                },
                work_area: Rect {
                    x,
                    y: 10,
                    width: 100,
                    height: 90,
                },
                scale: 2.0,
            })
            .collect()
    }
    #[test]
    fn chooses_containing_or_nearest_monitor() {
        assert_eq!(nearest(monitors(), -50.0, 10.0).unwrap().id, -100);
        assert_eq!(nearest(monitors(), 0.0, 10.0).unwrap().id, 0);
        assert_eq!(nearest(monitors(), 500.0, -10.0).unwrap().id, 0);
        assert!(nearest(Vec::new(), 0.0, 0.0).is_none());
        assert_eq!(
            monitor_for_rect(
                monitors(),
                Rect {
                    x: -30,
                    y: 10,
                    width: 90,
                    height: 30
                }
            )
            .unwrap()
            .id,
            0
        );
        assert_eq!(
            monitor_for_rect(
                monitors(),
                Rect {
                    x: -300,
                    y: 10,
                    width: 10,
                    height: 30
                }
            )
            .unwrap()
            .id,
            -100
        );
    }
}
