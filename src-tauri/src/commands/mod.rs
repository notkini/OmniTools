// Tauri commands — thin IPC shells over pure logic (no business logic here).
// One module per domain as the backend grows: hash.rs, image.rs, rename.rs, pdf.rs.

#[tauri::command]
pub fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}
