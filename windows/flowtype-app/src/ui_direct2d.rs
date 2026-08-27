use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Direct2D::Common::{
    D2D_RECT_F, D2D_SIZE_U, D2D1_ALPHA_MODE_UNKNOWN, D2D1_COLOR_F,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1_DRAW_TEXT_OPTIONS_CLIP, D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1_FEATURE_LEVEL_DEFAULT,
    D2D1_HWND_RENDER_TARGET_PROPERTIES, D2D1_PRESENT_OPTIONS_NONE, D2D1_RENDER_TARGET_PROPERTIES,
    D2D1_RENDER_TARGET_TYPE_DEFAULT, D2D1_RENDER_TARGET_USAGE_NONE, D2D1CreateFactory,
    ID2D1Factory, ID2D1HwndRenderTarget, ID2D1SolidColorBrush,
};
use windows::Win32::Graphics::DirectWrite::{
    DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL,
    DWRITE_FONT_WEIGHT_NORMAL, DWRITE_MEASURING_MODE_NATURAL, DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
    DWRITE_TEXT_ALIGNMENT_LEADING, DWRITE_WORD_WRAPPING_NO_WRAP, DWriteCreateFactory,
    IDWriteFactory, IDWriteFontCollection, IDWriteTextFormat,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_UNKNOWN;
use windows::core::w;

use super::ui_layout::{PhonesLayout, Rect};

pub(super) struct PhonePaintRow {
    pub connected: bool,
}

pub(super) struct Direct2dPainter {
    target: ID2D1HwndRenderTarget,
    icon: IDWriteTextFormat,
}

impl Direct2dPainter {
    pub(super) fn new(
        hwnd: windows_sys::Win32::Foundation::HWND,
        width: u32,
        height: u32,
    ) -> windows::core::Result<Self> {
        unsafe {
            let factory: ID2D1Factory = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
            let properties = D2D1_RENDER_TARGET_PROPERTIES {
                r#type: D2D1_RENDER_TARGET_TYPE_DEFAULT,
                pixelFormat: windows::Win32::Graphics::Direct2D::Common::D2D1_PIXEL_FORMAT {
                    format: DXGI_FORMAT_UNKNOWN,
                    alphaMode: D2D1_ALPHA_MODE_UNKNOWN,
                },
                dpiX: 0.0,
                dpiY: 0.0,
                usage: D2D1_RENDER_TARGET_USAGE_NONE,
                minLevel: D2D1_FEATURE_LEVEL_DEFAULT,
            };
            let hwnd_properties = D2D1_HWND_RENDER_TARGET_PROPERTIES {
                hwnd: HWND(hwnd),
                pixelSize: D2D_SIZE_U { width, height },
                presentOptions: D2D1_PRESENT_OPTIONS_NONE,
            };
            let target = factory.CreateHwndRenderTarget(&properties, &hwnd_properties)?;
            let write: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;
            let icon = text_format(
                &write,
                w!("Segoe Fluent Icons"),
                DWRITE_FONT_WEIGHT_NORMAL,
                20.0,
            )?;
            Ok(Self { target, icon })
        }
    }

    pub(super) fn resize(&self, width: u32, height: u32) {
        unsafe {
            let _ = self.target.Resize(&D2D_SIZE_U { width, height });
        }
    }

    pub(super) fn paint_phones(
        &self,
        client_height: f32,
        layout: &PhonesLayout,
        rows: &[PhonePaintRow],
    ) -> windows::core::Result<()> {
        unsafe {
            let sidebar = self.brush(rgb(251, 251, 251))?;
            let line = self.brush(rgb(223, 223, 223))?;
            let text = self.brush(rgb(31, 31, 31))?;
            let teal = self.brush(rgb(0, 186, 184))?;
            let offline = self.brush(rgb(204, 204, 204))?;

            self.target.BeginDraw();
            self.target.Clear(Some(&rgb(255, 255, 255)));
            self.target.FillRectangle(
                &D2D_RECT_F {
                    left: 0.0,
                    top: 0.0,
                    right: 180.0,
                    bottom: client_height,
                },
                &sidebar,
            );
            self.target.FillRectangle(
                &D2D_RECT_F {
                    left: 179.5,
                    top: 0.0,
                    right: 180.5,
                    bottom: client_height,
                },
                &line,
            );
            for (row, row_layout) in rows.iter().zip(&layout.rows) {
                self.target.FillRectangle(
                    &D2D_RECT_F {
                        left: row_layout.bounds.left,
                        top: row_layout.bounds.bottom - 1.0,
                        right: row_layout.bounds.right,
                        bottom: row_layout.bounds.bottom,
                    },
                    &line,
                );
                self.draw_text("\u{e8ea}", row_layout.icon, &self.icon, &text);
                let dot = row_layout.status_dot;
                self.target.FillEllipse(
                    &windows::Win32::Graphics::Direct2D::D2D1_ELLIPSE {
                        point: windows_numerics::Vector2 {
                            X: (dot.left + dot.right) / 2.0,
                            Y: (dot.top + dot.bottom) / 2.0,
                        },
                        radiusX: dot.width() / 2.0,
                        radiusY: (dot.bottom - dot.top) / 2.0,
                    },
                    if row.connected { &teal } else { &offline },
                );
            }
            self.target.EndDraw(None, None)
        }
    }

    unsafe fn brush(&self, color: D2D1_COLOR_F) -> windows::core::Result<ID2D1SolidColorBrush> {
        unsafe { self.target.CreateSolidColorBrush(&color, None) }
    }

    unsafe fn draw_text(
        &self,
        value: &str,
        rect: Rect,
        format: &IDWriteTextFormat,
        brush: &ID2D1SolidColorBrush,
    ) {
        let value: Vec<u16> = value.encode_utf16().collect();
        unsafe {
            self.target.DrawText(
                &value,
                format,
                &D2D_RECT_F {
                    left: rect.left,
                    top: rect.top,
                    right: rect.right,
                    bottom: rect.bottom,
                },
                brush,
                D2D1_DRAW_TEXT_OPTIONS_CLIP,
                DWRITE_MEASURING_MODE_NATURAL,
            );
        }
    }
}

unsafe fn text_format(
    factory: &IDWriteFactory,
    family: windows::core::PCWSTR,
    weight: windows::Win32::Graphics::DirectWrite::DWRITE_FONT_WEIGHT,
    size: f32,
) -> windows::core::Result<IDWriteTextFormat> {
    let format = unsafe {
        factory.CreateTextFormat(
            family,
            None::<&IDWriteFontCollection>,
            weight,
            DWRITE_FONT_STYLE_NORMAL,
            DWRITE_FONT_STRETCH_NORMAL,
            size,
            w!("zh-CN"),
        )?
    };
    unsafe {
        format.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_LEADING)?;
        format.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER)?;
        format.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP)?;
    }
    Ok(format)
}

const fn rgb(red: u8, green: u8, blue: u8) -> D2D1_COLOR_F {
    D2D1_COLOR_F {
        r: red as f32 / 255.0,
        g: green as f32 / 255.0,
        b: blue as f32 / 255.0,
        a: 1.0,
    }
}
