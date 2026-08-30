//! Menu-bar tray: icon rendering (CoreGraphics → template image) and menu.
//!
//! Glyphs are pure black on transparent so macOS treats them as template
//! images and tints them for light/dark menu bars automatically. Phase is
//! borrowed from `flow` — one vocabulary, no parallel enum.
use core_graphics::base::{kCGImageAlphaPremultipliedLast, kCGBitmapByteOrder32Big};
use core_graphics::color_space::CGColorSpace;
use core_graphics::context::CGContext;
use core_graphics::geometry::{CGRect, CGPoint, CGSize};
use tauri::image::Image;
use tauri::menu::{Menu, MenuBuilder, MenuItem, MenuItemBuilder};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager};

use crate::flow::PhaseWire;

pub const TRAY_ID: &str = "main";

// Menu item ids (also used by the tray menu handler).
pub const ID_TOGGLE: &str = "toggle-record";
pub const ID_PANEL: &str = "open-panel";
pub const ID_QUIT: &str = "quit";

/// Held as managed state so the toggle label can be updated without an
/// id lookup (muda's Manager has no menu_item_by_id).
pub struct TrayMenu {
    pub toggle: MenuItem<tauri::Wry>,
}
impl PhaseWire {
    pub fn tooltip(self) -> &'static str {
        match self {
            PhaseWire::Idle => "Ready — press hotkey to dictate",
            PhaseWire::Recording => "Recording… press hotkey to stop",
            PhaseWire::Processing => "Transcribing…",
            PhaseWire::Error => "Error — open panel for details",
            PhaseWire::ServerStarting => "ASR server starting (cold boot can take minutes)",
        }
    }
}

const SIZE: usize = 36; // logical px; template icons scale cleanly in the menu bar

/// Render the glyph for a phase into an owned RGBA image (top-down rows).
pub fn icon_for(phase: PhaseWire) -> Image<'static> {
    let mut ctx = CGContext::create_bitmap_context(
        None,
        SIZE,
        SIZE,
        8,
        SIZE * 4,
        &CGColorSpace::create_device_rgb(),
        kCGImageAlphaPremultipliedLast | kCGBitmapByteOrder32Big,
    );

    ctx.set_gray_fill_color(0.0, 1.0); // pure black; macOS tints templates
    let c = SIZE as f64 / 2.0;

    match phase {
        PhaseWire::Idle => {
            // Hollow ring: ready.
            ring(&mut ctx, c, c, 13.0, 2.5);
        }
        PhaseWire::Recording => {
            // Filled dot inside a thin ring: live capture.
            ring(&mut ctx, c, c, 14.0, 2.0);
            disc(&mut ctx, c, c, 7.5);
        }
        PhaseWire::Processing => {
            // Tick ring: spinner-ish (static frame).
            tick_ring(&mut ctx, c, c, 13.0, 8, 2.0);
        }
        PhaseWire::Error => {
            ring(&mut ctx, c, c, 13.0, 2.5);
            // Exclamation: stem + dot (CG origin is bottom-left).
            let stem = CGRect::new(&CGPoint::new(c - 1.4, 8.0), &CGSize::new(2.8, 11.5));
            ctx.fill_rect(stem);
            disc(&mut ctx, c, c - 9.0, 1.9);
        }
        PhaseWire::ServerStarting => {
            // Sparse tick ring + center dot: waiting on the stack.
            tick_ring(&mut ctx, c, c, 13.0, 6, 1.7);
            disc(&mut ctx, c, c, 2.6);
        }
    }

    let rgba = copy_flipped_rgba(ctx.data());
    Image::new_owned(rgba, SIZE as u32, SIZE as u32)
}

/// Stroked circle — CGContextStrokeEllipseInRect honors the current line width.
fn ring(ctx: &mut CGContext, x: f64, y: f64, r: f64, line_width: f64) {
    ctx.set_line_width(line_width);
    let rect = CGRect::new(&CGPoint::new(x - r, y - r), &CGSize::new(r * 2.0, r * 2.0));
    ctx.stroke_ellipse_in_rect(rect);
}

fn disc(ctx: &mut CGContext, x: f64, y: f64, r: f64) {
    let rect = CGRect::new(&CGPoint::new(x - r, y - r), &CGSize::new(r * 2.0, r * 2.0));
    ctx.fill_ellipse_in_rect(rect);
}

fn tick_ring(ctx: &mut CGContext, x: f64, y: f64, r: f64, count: u32, dot: f64) {
    for i in 0..count {
        let a = std::f64::consts::PI * 2.0 * i as f64 / count as f64;
        disc(ctx, x + r * a.cos(), y + r * a.sin(), dot);
    }
}

/// CGContext rows are bottom-up; tauri Image rows are top-down. Flip while copying.
fn copy_flipped_rgba(data: &[u8]) -> Vec<u8> {
    let row = SIZE * 4;
    let mut out = vec![0u8; data.len()];
    for y in 0..SIZE {
        out[y * row..(y + 1) * row].copy_from_slice(&data[(SIZE - 1 - y) * row..(SIZE - y) * row]);
    }
    out
}

/// NSStatusItem mutations must happen on the main thread; flow calls these
/// from tokio workers, so hop explicitly. Off-main set_icon silently removes
/// the status item (observed: tray vanishes after the first cycle).
pub fn set_phase(app: &AppHandle, phase: PhaseWire) {
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        if let Some(tray) = handle.tray_by_id(TRAY_ID) {
            let _ = tray.set_icon(Some(icon_for(phase)));
            let _ = tray.set_tooltip(Some(phase.tooltip()));
        }
    });
}

/// Create the status item and its right-click menu. Call from setup (main thread).
pub fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    let toggle = MenuItemBuilder::with_id(ID_TOGGLE, "Start Recording").build(app)?;
    let panel = MenuItemBuilder::with_id(ID_PANEL, "Open Panel").build(app)?;
    let quit = MenuItemBuilder::with_id(ID_QUIT, "Quit ASR").build(app)?;

    let menu: Menu<tauri::Wry> = MenuBuilder::new(app)
        .item(&toggle)
        .item(&panel)
        .separator()
        .item(&quit)
        .build()?;

    let _tray = TrayIconBuilder::with_id(TRAY_ID)
        .menu(&menu)
        // Left click toggles the panel; the menu opens on right-click only.
        .show_menu_on_left_click(false)
        .icon(icon_for(PhaseWire::Idle))
        .icon_as_template(true)
        .tooltip(PhaseWire::Idle.tooltip())
        .build(app)?;

    app.manage(TrayMenu { toggle });
    Ok(())
}

/// Keep the "Start/Stop Recording" menu label in sync with phase.
pub fn sync_toggle_label(app: &AppHandle, recording: bool) {
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        if let Some(menu) = handle.try_state::<TrayMenu>() {
            let _ = menu.toggle.set_text(if recording {
                "Stop Recording"
            } else {
                "Start Recording"
            });
        }
    });
}
