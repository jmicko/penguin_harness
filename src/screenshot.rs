use crate::types::WindowId;
use anyhow::{Context, Result, anyhow};
use image::{Rgb, RgbImage};
use std::path::Path;
use x11rb::connection::Connection;
use x11rb::image::{Image, PixelLayout};
use x11rb::protocol::xproto::Visualtype;

#[derive(Debug)]
pub struct CaptureInfo {
    pub width: u32,
    pub height: u32,
}

pub fn capture_window(
    window_id: WindowId,
    width: u16,
    height: u16,
    path: &Path,
) -> Result<CaptureInfo> {
    let (conn, _) = x11rb::connect(None).context("connect to X11 display")?;
    capture_drawable(&conn, window_id.0, width, height, path)
}

pub fn capture_screen(path: &Path) -> Result<CaptureInfo> {
    let (conn, screen_num) = x11rb::connect(None).context("connect to X11 display")?;
    let screen = &conn.setup().roots[screen_num];
    capture_drawable(
        &conn,
        screen.root,
        screen.width_in_pixels,
        screen.height_in_pixels,
        path,
    )
}

fn capture_drawable(
    conn: &impl Connection,
    drawable: u32,
    width: u16,
    height: u16,
    path: &Path,
) -> Result<CaptureInfo> {
    let (image, visual_id) = Image::get(conn, drawable, 0, 0, width, height)
        .with_context(|| format!("capture X11 drawable 0x{drawable:x}"))?;
    let visual = find_visual(conn, visual_id)
        .ok_or_else(|| anyhow!("could not resolve visual 0x{visual_id:x} for screenshot"))?;
    let layout = PixelLayout::from_visual_type(visual)
        .map_err(|error| anyhow!("unsupported visual for screenshot: {error}"))?;

    let mut png = RgbImage::new(u32::from(width), u32::from(height));
    for y in 0..height {
        for x in 0..width {
            let (red, green, blue) = layout.decode(image.get_pixel(x, y));
            png.put_pixel(
                u32::from(x),
                u32::from(y),
                Rgb([(red >> 8) as u8, (green >> 8) as u8, (blue >> 8) as u8]),
            );
        }
    }
    png.save(path)
        .with_context(|| format!("write PNG screenshot {}", path.display()))?;

    Ok(CaptureInfo {
        width: u32::from(width),
        height: u32::from(height),
    })
}

fn find_visual(conn: &impl Connection, visual_id: u32) -> Option<Visualtype> {
    conn.setup()
        .roots
        .iter()
        .flat_map(|screen| &screen.allowed_depths)
        .flat_map(|depth| &depth.visuals)
        .find(|visual| visual.visual_id == visual_id)
        .copied()
}
