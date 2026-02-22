use tauri::{
	App,
	menu::{MenuBuilder, MenuItemBuilder},
	tray::TrayIconBuilder,
};

pub fn setup<R: tauri::Runtime>(app: &mut App<R>) -> tauri::Result<()> {
	let handle = app.handle();

	let capture_now = MenuItemBuilder::with_id("capture-now", "Capture Now").build(handle)?;
	let settings = MenuItemBuilder::with_id("settings", "Settings").build(handle)?;
	let quit = MenuItemBuilder::with_id("quit", "Quit").build(handle)?;

	let menu = MenuBuilder::new(handle).items(&[&capture_now, &settings, &quit]).build()?;

	let mut tray = TrayIconBuilder::new().menu(&menu);
	if let Some(icon) = app.default_window_icon() {
		tray = tray.icon(icon.clone());
	}

	tray.build(handle)?;
	Ok(())
}
