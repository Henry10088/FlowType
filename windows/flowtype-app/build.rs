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
    const SIZE: usize = 32;
    let mut bytes = Vec::with_capacity(22 + 40 + SIZE * SIZE * 4 + 128);

    bytes.extend_from_slice(&0_u16.to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&[SIZE as u8, SIZE as u8, 0, 0]);
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&32_u16.to_le_bytes());
    bytes.extend_from_slice(&((40 + SIZE * SIZE * 4 + 128) as u32).to_le_bytes());
    bytes.extend_from_slice(&22_u32.to_le_bytes());

    bytes.extend_from_slice(&40_u32.to_le_bytes());
    bytes.extend_from_slice(&(SIZE as i32).to_le_bytes());
    bytes.extend_from_slice(&((SIZE * 2) as i32).to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&32_u16.to_le_bytes());
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    bytes.extend_from_slice(&((SIZE * SIZE * 4) as u32).to_le_bytes());
    bytes.extend_from_slice(&0_i32.to_le_bytes());
    bytes.extend_from_slice(&0_i32.to_le_bytes());
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    bytes.extend_from_slice(&0_u32.to_le_bytes());

    for y in (0..SIZE).rev() {
        for x in 0..SIZE {
            let xf = x as f32;
            let yf = y as f32;
            let pen = [
                ((9.0, 23.0), (10.5, 17.0)),
                ((10.5, 17.0), (21.0, 7.0)),
                ((21.0, 7.0), (26.0, 12.0)),
                ((26.0, 12.0), (15.0, 22.0)),
                ((15.0, 22.0), (9.0, 23.0)),
                ((19.0, 9.0), (24.0, 14.0)),
            ]
            .iter()
            .any(|&((x1, y1), (x2, y2))| near_segment(xf, yf, x1, y1, x2, y2, 1.3));
            let dx = xf - 27.0;
            let dy = yf - 22.0;
            let dot = dx * dx + dy * dy <= 3.0 * 3.0;
            let (r, g, b) = if pen {
                (244, 246, 247)
            } else if dot {
                (0, 186, 184)
            } else {
                (16, 18, 20)
            };
            bytes.extend_from_slice(&[b, g, r, 255]);
        }
    }
    bytes.extend_from_slice(&[0_u8; 128]);
    fs::write(path, bytes).expect("failed to write FlowType icon");
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
