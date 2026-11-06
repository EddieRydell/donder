use tauri::{Runtime, Window};

use super::model::PersistedWindowState;

pub(crate) fn read_window_state<R: Runtime>(window: &Window<R>) -> Option<PersistedWindowState> {
    if window.is_minimized().ok()? {
        return None;
    }
    let position = window.outer_position().ok()?;
    let size = window.inner_size().ok()?;
    Some(PersistedWindowState {
        x: position.x,
        y: position.y,
        width: size.width,
        height: size.height,
        maximized: window.is_maximized().unwrap_or(false),
    })
}

pub(crate) fn apply_window_state<R: Runtime>(window: &Window<R>, state: &PersistedWindowState) {
    if !state_is_visible_on_monitor(window, state) {
        return;
    }
    use tauri::{PhysicalPosition, PhysicalSize};
    let _ = window.set_size(PhysicalSize::new(state.width, state.height));
    let _ = window.set_position(PhysicalPosition::new(state.x, state.y));
    if state.maximized {
        let _ = window.maximize();
    }
}

fn state_is_visible_on_monitor<R: Runtime>(
    window: &Window<R>,
    state: &PersistedWindowState,
) -> bool {
    let Ok(monitors) = window.available_monitors() else {
        return false;
    };
    overlaps_any_area(
        state,
        monitors.into_iter().map(|monitor| {
            let area = monitor.work_area();
            (
                area.position.x,
                area.position.y,
                area.size.width,
                area.size.height,
            )
        }),
    )
}

/// Whether a saved window, with some size, overlaps one of the `(x, y,
/// width, height)` screen areas, so restoring it shows it.
pub(crate) fn overlaps_any_area(
    state: &PersistedWindowState,
    areas: impl IntoIterator<Item = (i32, i32, u32, u32)>,
) -> bool {
    state.width > 0
        && state.height > 0
        && areas.into_iter().any(|(x, y, width, height)| {
            i64::from(state.x) < i64::from(x) + i64::from(width)
                && i64::from(state.x) + i64::from(state.width) > i64::from(x)
                && i64::from(state.y) < i64::from(y) + i64::from(height)
                && i64::from(state.y) + i64::from(state.height) > i64::from(y)
        })
}
