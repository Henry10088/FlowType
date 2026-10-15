use std::env;
use std::fs;
use std::path::Path;

fn main() {
    println!("cargo:rerun-if-env-changed=FLOWTYPE_WINDOWS_CERT_SHA256");
    if std::env::var_os("CARGO_CFG_WINDOWS").is_some() {
        let icon_path = Path::new(&env::var_os("OUT_DIR").expect("OUT_DIR")).join("flowtype.ico");
        write_icon(&icon_path);
        winresource::WindowsResource::new()
            .set_icon(icon_path.to_str().expect("icon path is valid UTF-8"))
            .set("FileDescription", "FlowType Windows client")
            .set("ProductName", "FlowType")
            .set_manifest(
                r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <dependency><dependentAssembly><assemblyIdentity type="win32" name="Microsoft.Windows.Common-Controls" version="6.0.0.0" processorArchitecture="*" publicKeyToken="6595b64144ccf1df" language="*" /></dependentAssembly></dependency>
  <application xmlns="urn:schemas-microsoft-com:asm.v3"><windowsSettings><dpiAwareness xmlns="http://schemas.microsoft.com/SMI/2016/WindowsSettings">PerMonitorV2</dpiAwareness><longPathAware xmlns="http://schemas.microsoft.com/SMI/2016/WindowsSettings">true</longPathAware></windowsSettings></application>
</assembly>"#,
            )
            .compile()
            .expect("failed to compile Windows resources");
    }
}

fn write_icon(path: &Path) {
    // Three pixel-perfect sizes keep the titlebar, taskbar and Explorer icon crisp.
    const SIZES: [usize; 3] = [16, 32, 48];
    let header_size = 6 + SIZES.len() * 16;
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&0_u16.to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&(SIZES.len() as u16).to_le_bytes());
    let mut offset = header_size;
    for size in SIZES {
        let mask_bytes = size.div_ceil(32) * 4 * size;
        let image_bytes = 40 + size * size * 4 + mask_bytes;
        bytes.extend_from_slice(&[size as u8, size as u8, 0, 0]);
        bytes.extend_from_slice(&1_u16.to_le_bytes());
        bytes.extend_from_slice(&32_u16.to_le_bytes());
        bytes.extend_from_slice(&(image_bytes as u32).to_le_bytes());
        bytes.extend_from_slice(&(offset as u32).to_le_bytes());
        offset += image_bytes;
    }
    for size in SIZES {
        bytes.extend_from_slice(&40_u32.to_le_bytes());
        bytes.extend_from_slice(&(size as i32).to_le_bytes());
        bytes.extend_from_slice(&((size * 2) as i32).to_le_bytes());
        bytes.extend_from_slice(&1_u16.to_le_bytes());
        bytes.extend_from_slice(&32_u16.to_le_bytes());
        bytes.extend_from_slice(&0_u32.to_le_bytes());
        bytes.extend_from_slice(&((size * size * 4) as u32).to_le_bytes());
        bytes.extend_from_slice(&[0_u8; 16]);
        for y in (0..size).rev() {
            for x in 0..size {
                let mut teal = 0;
                let mut white = 0;
                // Supersampling avoids jagged edges at 16px without a raster asset.
                for sy in 0..4 {
                    for sx in 0..4 {
                        let px = (x as f32 + (sx as f32 + 0.5) / 4.0) * 32.0 / size as f32;
                        let py = (y as f32 + (sy as f32 + 0.5) / 4.0) * 32.0 / size as f32;
                        let inside = rounded_square(px, py, 5.2, 26.8, 6.2);
                        let pen = near_segment(px, py, 11.0, 21.2, 20.8, 10.5, 2.0)
                            || near_segment(px, py, 9.8, 23.1, 12.2, 17.9, 1.25)
                            || near_segment(px, py, 19.1, 8.7, 23.1, 12.6, 1.8);
                        teal += i32::from(inside);
                        white += i32::from(inside && pen);
                    }
                }
                let a = teal * 255 / 16;
                let white = white * 255 / 16;
                // Premultiplication is not used by the ICO DIB format.
                let blend = |base: i32| ((base * (255 - white) + 255 * white) / 255) as u8;
                bytes.extend_from_slice(&[blend(184), blend(186), blend(0), a as u8]);
            }
        }
        bytes.extend(std::iter::repeat_n(0_u8, size.div_ceil(32) * 4 * size));
    }
    fs::write(path, bytes).expect("failed to write FlowType icon");
}

fn rounded_square(x: f32, y: f32, start: f32, end: f32, radius: f32) -> bool {
    let cx = x.clamp(start + radius, end - radius);
    let cy = y.clamp(start + radius, end - radius);
    (x - cx).powi(2) + (y - cy).powi(2) <= radius.powi(2)
}

fn near_segment(px: f32, py: f32, x1: f32, y1: f32, x2: f32, y2: f32, radius: f32) -> bool {
    let dx = x2 - x1;
    let dy = y2 - y1;
    let length_squared = dx * dx + dy * dy;
    let projection = if length_squared == 0.0 {
        0.0
    } else {
        ((px - x1) * dx + (py - y1) * dy) / length_squared
    };
    let t = projection.clamp(0.0, 1.0);
    let closest_x = x1 + t * dx;
    let closest_y = y1 + t * dy;
    let distance_x = px - closest_x;
    let distance_y = py - closest_y;
    distance_x * distance_x + distance_y * distance_y <= radius * radius
}
