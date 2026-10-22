// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use clap::Parser;
use handy_app_lib::CliArgs;

fn main() {
    let cli_args = CliArgs::parse();

    #[cfg(target_os = "linux")]
    {
        let gdk_wayland = select_gdk_backend();

        // DMABUF renderer causes crashes on various GPU/display server configurations
        // See: https://github.com/tauri-apps/tauri/issues/9394
        // On Wayland it stays on: without it the transparent overlay only
        // repaints its damaged regions (seen on KDE), so the card vanishes
        // around the waveform. An explicit value always wins.
        if !gdk_wayland && std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_none() {
            std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
        }
    }

    #[cfg(target_os = "windows")]
    {
        // Avoid overlay/capture layer crashes (#2049). Set before backend
        // initialization, preserving user overrides.
        if std::env::var_os("VK_LOADER_LAYERS_DISABLE").is_none()
            && !handy_app_lib::env_flag_enabled("HANDY_KEEP_VULKAN_IMPLICIT_LAYERS")
        {
            std::env::set_var("VK_LOADER_LAYERS_DISABLE", "~implicit~");
        }
    }

    handy_app_lib::run(cli_args)
}

/// Prefers GTK's native Wayland backend on Wayland sessions, so the overlay is
/// a layer-shell surface placed by the compositor. The AppImage's GTK hook
/// exports `GDK_BACKEND=x11` unconditionally, so inside an AppImage that value
/// is treated as the hook's rather than the user's; `HANDY_GDK_BACKEND` sets the
/// backend explicitly (e.g. `x11` to opt out). Must run before GTK initializes.
/// Returns whether GTK will use Wayland.
#[cfg(target_os = "linux")]
fn select_gdk_backend() -> bool {
    if let Some(backend) = std::env::var_os("HANDY_GDK_BACKEND") {
        std::env::set_var("GDK_BACKEND", backend);
    } else if std::env::var_os("APPIMAGE").is_some()
        && std::env::var_os("WAYLAND_DISPLAY").is_some()
        && std::env::var("GDK_BACKEND").as_deref() == Ok("x11")
    {
        std::env::set_var("GDK_BACKEND", "wayland");
    }

    std::env::var_os("WAYLAND_DISPLAY").is_some()
        && std::env::var("GDK_BACKEND").map_or(true, |backend| backend.starts_with("wayland"))
}
