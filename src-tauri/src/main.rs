mod capture;
mod commands;
mod export;
mod tray;

use tauri::{AppHandle, Manager, Runtime};
use tauri_plugin_global_shortcut::GlobalShortcutExt;

fn handle_capture_now<R>(app: &AppHandle<R>)
where
	R: Runtime,
{
	if let Err(err) = commands::capture_now_with_app(app) {
		eprintln!("Capture failed: {err}");
	}
}

fn main() {
	let default_shortcut = tauri_plugin_global_shortcut::Shortcut::new(
		Some(
			tauri_plugin_global_shortcut::Modifiers::CONTROL
				| tauri_plugin_global_shortcut::Modifiers::SHIFT,
		),
		tauri_plugin_global_shortcut::Code::KeyS,
	);

	tauri::Builder::default()
		.plugin(tauri_plugin_global_shortcut::Builder::new().build())
		.invoke_handler(tauri::generate_handler![
			commands::capture_now,
			commands::get_last_capture_base64,
			commands::save_png_base64,
			commands::copy_png_base64,
			commands::open_pin_window,
		])
		.setup(move |app| {
			tray::setup(app)?;

			app.global_shortcut().on_shortcut(default_shortcut, move |_app, _, event| {
				if event.state == tauri_plugin_global_shortcut::ShortcutState::Pressed {
					handle_capture_now(_app);
				}
			})?;

			Ok(())
		})
		.on_menu_event(|app, event| match event.id().as_ref() {
			"capture-now" => handle_capture_now(app.app_handle()),
			"settings" => println!("Settings selected from tray"),
			"quit" => app.exit(0),
			_ => {},
		})
		.run(tauri::generate_context!())
		.unwrap_or_else(|err| eprintln!("Error while running tauri application: {err}"));
}
