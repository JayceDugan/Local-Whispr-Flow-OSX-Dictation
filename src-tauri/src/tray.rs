//! Menu-bar tray: icon rendering (CoreGraphics → template image) and menu.
//!
//! Glyphs are pure black on transparent so macOS treats them as template
//! images and tints them for light/dark menu bars automatically. Phase is
//! borrowed from `flow` — one vocabulary, no parallel enum.
use core_graphics::base::{kCGImageAlphaPremultipliedLast, kCGBitmapByteOrder32Big};
use core_graphics::color_space::CGColorSpace;
use core_graphics::context::{CGContext, CGLineCap, CGLineJoin};
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
    ctx.set_rgb_stroke_color(0.0, 0.0, 0.0, 1.0); // no gray stroke variant exists
    ctx.set_line_cap(CGLineCap::CGLineCapRound);
    ctx.set_line_join(CGLineJoin::CGLineJoinRound);
    let c = SIZE as f64 / 2.0;

    match phase {
        PhaseWire::Idle => {
            // Microphone: the hero glyph (no surrounding circle).
            mic(&mut ctx, false);
        }
        PhaseWire::Recording => {
            // Same microphone plus radiating sound-wave arcs: live capture.
            mic(&mut ctx, true);
        }
        PhaseWire::Processing => {
            // Tick ring: spinner-ish (static frame).
            tick_ring(&mut ctx, c, c, 13.0, 8, 2.0);
        }
        PhaseWire::Error => {
            // Bold exclamation mark alone (CG origin is bottom-left).
            capsule(&mut ctx, 18.0, 14.0, 18.0, 25.0, 3.5);
            disc(&mut ctx, 18.0, 9.0, 2.0);
        }
        PhaseWire::ServerStarting => {
            // Sparse tick ring + center dot: waiting on the stack.
            tick_ring(&mut ctx, c, c, 13.0, 6, 1.7);
            disc(&mut ctx, c, c, 2.6);
        }
    }

    let rgba = copy_rgba(ctx.data());
    Image::new_owned(rgba, SIZE as u32, SIZE as u32)
}

/// Microphone motif: capsule body, cradle "U", stem, base bar; `waves` adds
/// radiating sound-wave arcs on both sides (recording). CG origin is bottom-left.
fn mic(ctx: &mut CGContext, waves: bool) {
    // Capsule body: thick round-capped stroke rising out of the cradle.
    capsule(ctx, 18.0, 20.0, 18.0, 28.0, 6.0);

    // Cradle "U": open at the top, wraps the lower capsule; collinear quad
    // tangents at (18,14) keep the bottom smooth.
    ctx.set_line_width(2.0);
    ctx.begin_path();
    ctx.move_to_point(11.0, 24.0);
    ctx.add_quad_curve_to_point(11.0, 14.0, 18.0, 14.0);
    ctx.add_quad_curve_to_point(25.0, 14.0, 25.0, 24.0);
    ctx.stroke_path();

    // Stem down to the base bar, then the bar itself.
    capsule(ctx, 18.0, 9.0, 18.0, 14.0, 2.0);
    capsule(ctx, 14.0, 9.0, 22.0, 9.0, 2.0);

    if waves {
        // Sound-wave arcs flanking the mic (quad-curve arc approximations).
        ctx.begin_path();
        ctx.move_to_point(7.0, 15.0);
        ctx.add_quad_curve_to_point(4.0, 19.0, 7.0, 23.0);
        ctx.stroke_path();
        ctx.begin_path();
        ctx.move_to_point(29.0, 15.0);
        ctx.add_quad_curve_to_point(32.0, 19.0, 29.0, 23.0);
        ctx.stroke_path();
    }
}

/// Thick round-capped stroked line — reads as a capsule at either orientation.
fn capsule(ctx: &mut CGContext, x0: f64, y0: f64, x1: f64, y1: f64, width: f64) {
    ctx.set_line_width(width);
    ctx.begin_path();
    ctx.move_to_point(x0, y0);
    ctx.add_line_to_point(x1, y1);
    ctx.stroke_path();
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

/// Copy the bitmap context's RGBA buffer verbatim. A CGBitmapContext stores
/// rows top-down (row 0 = image top) even though its coordinate origin is
/// bottom-left, and tauri Image also wants top-down rows — so no flip. (An
/// earlier version flipped here; it was harmless for the old vertically
/// symmetric glyphs but inverted asymmetric ones like the microphone.)
fn copy_rgba(data: &[u8]) -> Vec<u8> {
    data.to_vec()
}

/// NSStatusItem mutations must happen on the main thread; flow calls these
/// from tokio workers, so hop explicitly. Off-main set_icon silently removes
/// the status item (observed: tray vanishes after the first cycle). Use
/// `set_icon_with_as_template(..., true)`: plain `set_icon` hardcodes the
/// template flag to false on macOS, re-rendering the glyph as a non-tinted
/// black icon that is invisible on dark menu bars.
pub fn set_phase(app: &AppHandle, phase: PhaseWire) {
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        if let Some(tray) = handle.tray_by_id(TRAY_ID) {
            let _ = tray.set_icon_with_as_template(Some(icon_for(phase)), true);
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
