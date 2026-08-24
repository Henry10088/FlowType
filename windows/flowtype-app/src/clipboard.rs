use std::ptr::{copy_nonoverlapping, null_mut};
use std::thread;
use std::time::Duration;

use image::GenericImageView;
use windows_sys::Win32::Foundation::GlobalFree;
use windows_sys::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, OpenClipboard, RegisterClipboardFormatW, SetClipboardData,
};
use windows_sys::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock};

const MAX_PIXELS: u64 = 40_000_000;
const CF_DIB: u32 = 8;

pub fn set_image(bytes: &[u8], mime_type: &str) -> Result<(), String> {
    let format = match mime_type {
        "image/jpeg" => image::ImageFormat::Jpeg,
        "image/png" => image::ImageFormat::Png,
        _ => return Err("unsupported image format".to_owned()),
    };
    let image = image::load_from_memory_with_format(bytes, format)
        .map_err(|_| "cannot decode image".to_owned())?;
    let (width, height) = image.dimensions();
    if width == 0 || height == 0 || u64::from(width) * u64::from(height) > MAX_PIXELS {
        return Err("invalid image dimensions".to_owned());
    }
    let dib = make_dib(&image.to_rgba8(), width, height);

    open_clipboard()?;
    let _clipboard = ClipboardGuard;
    unsafe {
        if EmptyClipboard() == 0 {
            Err("cannot clear clipboard".to_owned())
        } else {
            set_clipboard_bytes(CF_DIB, &dib)?;
            if mime_type == "image/png" {
                let name: Vec<u16> = "PNG\0".encode_utf16().collect();
                let png_format = RegisterClipboardFormatW(name.as_ptr());
                if png_format != 0 {
                    let _ = set_clipboard_bytes(png_format, bytes);
                }
            }
            Ok(())
        }
    }
}

struct ClipboardGuard;

impl Drop for ClipboardGuard {
    fn drop(&mut self) {
        unsafe { CloseClipboard() };
    }
}

fn open_clipboard() -> Result<(), String> {
    for _ in 0..6 {
        if unsafe { OpenClipboard(null_mut()) } != 0 {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(15));
    }
    Err("clipboard is busy".to_owned())
}

unsafe fn set_clipboard_bytes(format: u32, bytes: &[u8]) -> Result<(), String> {
    let memory = unsafe { GlobalAlloc(GMEM_MOVEABLE, bytes.len()) };
    if memory.is_null() {
        return Err("cannot allocate clipboard memory".to_owned());
    }
    let target = unsafe { GlobalLock(memory) }.cast::<u8>();
    if target.is_null() {
        unsafe { GlobalFree(memory) };
        return Err("cannot lock clipboard memory".to_owned());
    }
    unsafe {
        copy_nonoverlapping(bytes.as_ptr(), target, bytes.len());
        GlobalUnlock(memory);
    }
    if unsafe { SetClipboardData(format, memory) }.is_null() {
        unsafe { GlobalFree(memory) };
        return Err("cannot set clipboard data".to_owned());
    }
    Ok(())
}

fn make_dib(rgba: &[u8], width: u32, height: u32) -> Vec<u8> {
    let image_size = width as usize * height as usize * 4;
    let mut dib = Vec::with_capacity(40 + image_size);
    dib.extend_from_slice(&40_u32.to_le_bytes());
    dib.extend_from_slice(&(width as i32).to_le_bytes());
    dib.extend_from_slice(&(height as i32).to_le_bytes());
    dib.extend_from_slice(&1_u16.to_le_bytes());
    dib.extend_from_slice(&32_u16.to_le_bytes());
    dib.extend_from_slice(&0_u32.to_le_bytes());
    dib.extend_from_slice(&(image_size as u32).to_le_bytes());
    dib.extend_from_slice(&0_i32.to_le_bytes());
    dib.extend_from_slice(&0_i32.to_le_bytes());
    dib.extend_from_slice(&0_u32.to_le_bytes());
    dib.extend_from_slice(&0_u32.to_le_bytes());
    for row in (0..height as usize).rev() {
        for pixel in rgba[row * width as usize * 4..(row + 1) * width as usize * 4].chunks_exact(4)
        {
            dib.extend_from_slice(&[pixel[2], pixel[1], pixel[0], pixel[3]]);
        }
    }
    dib
}

#[cfg(test)]
mod tests {
    use super::make_dib;

    #[test]
    fn creates_bottom_up_bgra_dib() {
        let rgba = [255, 0, 0, 255, 0, 255, 0, 128];
        let dib = make_dib(&rgba, 1, 2);
        assert_eq!(&dib[0..4], &40_u32.to_le_bytes());
        assert_eq!(&dib[40..44], &[0, 255, 0, 128]);
        assert_eq!(&dib[44..48], &[0, 0, 255, 255]);
    }
}
