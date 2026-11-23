use objc2::MainThreadMarker;
use objc2_app_kit::NSScreen;
use objc2_foundation::{NSPoint, NSRect, NSSize};
use serde::Serialize;

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize)]
pub struct NotchGeometry {
    pub has_notch: bool,
    pub notch_width: f64,
    pub notch_height: f64,
    pub menu_bar_height: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScreenGeometry {
    pub frame: NSRect,
    pub center_x: f64,
    pub notch_rect: NSRect,
    pub notch: NotchGeometry,
}

pub fn parse_override(value: &str) -> Option<(f64, f64)> {
    let (w, h) = value.split_once('x')?;
    let (w, h): (f64, f64) = (w.parse().ok()?, h.parse().ok()?);
    (w.is_finite() && h.is_finite() && w > 0.0 && h > 0.0).then_some((w, h))
}

pub fn geometry(
    frame: NSRect,
    visible_top: f64,
    safe_top: f64,
    left_width: f64,
    right_width: f64,
    fake: Option<(f64, f64)>,
) -> ScreenGeometry {
    let menu_bar_height = (frame.origin.y + frame.size.height - visible_top).max(0.0);
    let (width, height) = fake.unwrap_or_else(|| {
        if safe_top > 0.0 {
            (
                (frame.size.width - left_width - right_width).max(0.0),
                safe_top,
            )
        } else {
            (0.0, 0.0)
        }
    });
    let width = width.min(frame.size.width);
    let height = height.min(frame.size.height);
    let center_x = frame.origin.x
        + if fake.is_none() && height > 0.0 {
            left_width + width / 2.0
        } else {
            frame.size.width / 2.0
        };
    ScreenGeometry {
        frame,
        center_x,
        notch_rect: NSRect::new(
            NSPoint::new(
                center_x - width / 2.0,
                frame.origin.y + frame.size.height - height,
            ),
            NSSize::new(width, height),
        ),
        notch: NotchGeometry {
            has_notch: width > 0.0 && height > 0.0,
            notch_width: width,
            notch_height: height,
            menu_bar_height,
        },
    }
}

pub fn read(mtm: MainThreadMarker) -> Option<ScreenGeometry> {
    let screens = NSScreen::screens(mtm);
    let screen = screens
        .iter()
        .find(|s| s.safeAreaInsets().top > 0.0)
        .or_else(|| screens.firstObject())?;
    let frame = screen.frame();
    let visible = screen.visibleFrame();
    Some(geometry(
        frame,
        visible.origin.y + visible.size.height,
        screen.safeAreaInsets().top,
        screen.auxiliaryTopLeftArea().size.width,
        screen.auxiliaryTopRightArea().size.width,
        std::env::var("JARVIS_FAKE_NOTCH")
            .ok()
            .as_deref()
            .and_then(parse_override),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame() -> NSRect {
        NSRect::new(NSPoint::new(-100.0, 50.0), NSSize::new(1500.0, 1000.0))
    }

    #[test]
    fn notch_uses_auxiliary_areas_and_screen_origin() {
        let g = geometry(frame(), 1018.0, 32.0, 640.0, 660.0, None);
        assert!(g.notch.has_notch);
        assert_eq!((g.notch.notch_width, g.notch.notch_height), (200.0, 32.0));
        assert_eq!(g.center_x, 640.0);
        assert_eq!(
            g.notch_rect,
            NSRect::new(NSPoint::new(540.0, 1018.0), NSSize::new(200.0, 32.0))
        );
        assert_eq!(g.notch.menu_bar_height, 32.0);
    }

    #[test]
    fn notchless_and_invalid_auxiliary_areas_are_harmless() {
        let g = geometry(frame(), 1026.0, 0.0, 0.0, 0.0, None);
        assert!(!g.notch.has_notch);
        assert_eq!(g.notch.notch_width, 0.0);
        assert_eq!(g.notch.menu_bar_height, 24.0);
        assert_eq!(g.center_x, 650.0);
        assert!(
            !geometry(frame(), 1050.0, 32.0, 900.0, 900.0, None)
                .notch
                .has_notch
        );
    }

    #[test]
    fn override_is_centered_and_bounded() {
        let g = geometry(frame(), 1026.0, 0.0, 0.0, 0.0, parse_override("200x32"));
        assert_eq!(g.center_x, 650.0);
        assert_eq!((g.notch.notch_width, g.notch.notch_height), (200.0, 32.0));
        assert_eq!(
            geometry(frame(), 1050.0, 0.0, 0.0, 0.0, Some((2000.0, 2000.0)))
                .notch
                .notch_width,
            1500.0
        );
    }

    #[test]
    fn rejects_empty_malformed_and_nonpositive_overrides() {
        for value in [
            "", "200", "200X32", "200x", "0x32", "-1x32", "NaNx32", "200xinf", "200x32x4",
        ] {
            assert_eq!(parse_override(value), None, "{value}");
        }
        assert_eq!(parse_override("200x32"), Some((200.0, 32.0)));
    }
}
