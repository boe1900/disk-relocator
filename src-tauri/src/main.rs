#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod bootstrap;
mod commands;
mod db;
mod health;
mod migration;
mod models;
mod profiles;
mod reconcile;
mod recovery;

use tauri::Manager;

pub struct AppState {
    pub db: db::Database,
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let app_data_dir = app.path().app_data_dir()?;
            let database = db::Database::init(app_data_dir)?;
            let recovery_summary = recovery::recover_unfinished_relocations(&database)
                .map_err(|err| format!("startup recovery failed: {err}"))?;
            health::spawn_health_monitor(database.clone());
            reconcile::spawn_reconcile_monitor(database.clone());

            app.manage(AppState { db: database });

            if recovery_summary.total > 0 {
                println!(
                    "[recovery] total={}, healthy={}, rolled_back={}, failed={}",
                    recovery_summary.total,
                    recovery_summary.healthy,
                    recovery_summary.rolled_back,
                    recovery_summary.failed
                );
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::scan_apps,
            commands::get_disk_status,
            commands::get_system_disk_status,
            commands::migrate_app,
            commands::rollback_relocation,
            commands::list_operation_logs,
            commands::list_relocations,
            commands::list_health_events,
            commands::reconcile_relocations,
            commands::check_health,
        ])
        .run(tauri::generate_context!())
        .expect("failed to run Disk Relocator app");
}
