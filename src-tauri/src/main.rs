use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};

mod capture;
mod tray;

fn handle_capture_now<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
	match capture::capture_primary_display_to_cache(app) {
		Ok(path) => println!("Capture saved to {}", path.display()),
		Err(err) => eprintln!("Capture failed: {err}"),
	}
}

fn main() {
	let default_shortcut = Shortcut::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::KeyS);

	tauri::Builder::default()
		.plugin(tauri_plugin_global_shortcut::Builder::new().build())
		.setup(move |app| {
			tray::setup(app)?;
			app.global_shortcut().on_shortcut(default_shortcut, move |_app, _, event| {
				if event.state == ShortcutState::Pressed {
					handle_capture_now(_app);
				}
			})?;
			Ok(())
		})
		.on_menu_event(|app, event| match event.id().as_ref() {
			"capture-now" => handle_capture_now(app),
			"settings" => println!("Settings selected from tray"),
			"quit" => app.exit(0),
			_ => {},
		})
		.run(tauri::generate_context!())
		.expect("error while running tauri application");
}
