use tauri::{Runtime, Window};

use super::model::PersistedWindowState;

pub fn read_window_state<R: Runtime>(window: &Window<R>) -> Option<PersistedWindowState> {
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

pub fn apply_window_state<R: Runtime>(window: &Window<R>, state: &PersistedWindowState) {
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
    if state.width == 0 || state.height == 0 {
        return false;
    }
    let Ok(monitors) = window.available_monitors() else {
        return false;
    };
    monitors.into_iter().any(|monitor| {
        let work_area = monitor.work_area();
        let window_right = i64::from(state.x) + i64::from(state.width);
        let window_bottom = i64::from(state.y) + i64::from(state.height);
        let area_right = i64::from(work_area.position.x) + i64::from(work_area.size.width);
        let area_bottom = i64::from(work_area.position.y) + i64::from(work_area.size.height);
        i64::from(state.x) < area_right
            && window_right > i64::from(work_area.position.x)
            && i64::from(state.y) < area_bottom
            && window_bottom > i64::from(work_area.position.y)
    })
}
