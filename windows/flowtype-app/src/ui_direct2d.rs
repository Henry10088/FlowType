use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Direct2D::Common::{
    D2D_RECT_F, D2D_SIZE_U, D2D1_ALPHA_MODE_UNKNOWN, D2D1_COLOR_F,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1_DRAW_TEXT_OPTIONS_CLIP, D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1_FEATURE_LEVEL_DEFAULT,
    D2D1_HWND_RENDER_TARGET_PROPERTIES, D2D1_PRESENT_OPTIONS_NONE, D2D1_RENDER_TARGET_PROPERTIES,
    D2D1_RENDER_TARGET_TYPE_DEFAULT, D2D1_RENDER_TARGET_USAGE_NONE, D2D1_ROUNDED_RECT,
    D2D1CreateFactory, ID2D1Factory, ID2D1HwndRenderTarget, ID2D1SolidColorBrush,
};
use windows::Win32::Graphics::DirectWrite::{
    DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL,
    DWRITE_FONT_WEIGHT_NORMAL, DWRITE_MEASURING_MODE_NATURAL, DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
    DWRITE_TEXT_ALIGNMENT_LEADING, DWRITE_WORD_WRAPPING_NO_WRAP, DWriteCreateFactory,
    IDWriteFactory, IDWriteFontCollection, IDWriteTextFormat,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_UNKNOWN;
use windows::core::w;

use super::SaveNotice;
use super::ui_layout::{PairingLayout, PhonesLayout, Rect, SettingsLayout, StatusLayout};

pub(super) struct PhonePaintRow {
    pub connected: bool,
}

pub(super) struct Direct2dPainter {
    target: ID2D1HwndRenderTarget,
    icon: IDWriteTextFormat,
    body: IDWriteTextFormat,
}

impl Direct2dPainter {
    pub(super) fn new(
        hwnd: windows_sys::Win32::Foundation::HWND,
        width: u32,
        height: u32,
        dpi: f32,
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
            target.SetDpi(dpi, dpi);
            let write: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;
            let icon = text_format(
                &write,
                w!("Segoe MDL2 Assets"),
                DWRITE_FONT_WEIGHT_NORMAL,
                20.0,
            )?;
            let body = text_format(&write, w!("Segoe UI"), DWRITE_FONT_WEIGHT_NORMAL, 14.0)?;
            Ok(Self { target, icon, body })
        }
    }

    pub(super) fn resize(&self, width: u32, height: u32) -> windows::core::Result<()> {
        unsafe { self.target.Resize(&D2D_SIZE_U { width, height }) }
    }

    pub(super) fn set_dpi(&self, dpi: f32) {
        unsafe { self.target.SetDpi(dpi, dpi) };
    }

    pub(super) fn paint_phones(
        &self,
        client_height: f32,
        layout: &PhonesLayout,
        rows: &[PhonePaintRow],
        panels: &[Rect],
        notice: Option<&SaveNotice>,
    ) -> windows::core::Result<()> {
        unsafe {
            let text = self.brush(rgb(31, 31, 31))?;
            let teal = self.brush(rgb(0, 186, 184))?;
            let offline = self.brush(rgb(204, 204, 204))?;
            let notice_brushes = self.notice_brushes(notice)?;
            self.begin_shell(client_height, panels)?;

            for (row, row_layout) in rows.iter().zip(&layout.rows) {
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
            self.draw_notice(notice, notice_brushes.as_ref());
            self.target.EndDraw(None, None)
        }
    }

    pub(super) fn paint_status(
        &self,
        client_height: f32,
        layout: &StatusLayout,
        panels: &[Rect],
        notice: Option<&SaveNotice>,
    ) -> windows::core::Result<()> {
        unsafe {
            let teal = self.brush(rgb(0, 186, 184))?;
            let white = self.brush(rgb(255, 255, 255))?;
            let notice_brushes = self.notice_brushes(notice)?;
            let line = self.begin_shell(client_height, panels)?;
            for y in layout.separators {
                self.target.FillRectangle(
                    &D2D_RECT_F {
                        left: layout.shell.content_left,
                        top: y - 0.5,
                        right: layout.shell.content_right - 34.0,
                        bottom: y + 0.5,
                    },
                    &line,
                );
            }
            let icon = layout.status_icon;
            self.target.FillEllipse(&ellipse(icon), &teal);
            self.draw_text("\u{e73e}", icon, &self.icon, &white);
            self.draw_notice(notice, notice_brushes.as_ref());
            self.target.EndDraw(None, None)
        }
    }

    pub(super) fn paint_files(
        &self,
        client_height: f32,
        panels: &[Rect],
        notice: Option<&SaveNotice>,
    ) -> windows::core::Result<()> {
        unsafe {
            let notice_brushes = self.notice_brushes(notice)?;
            self.begin_shell(client_height, panels)?;
            self.draw_notice(notice, notice_brushes.as_ref());
            self.target.EndDraw(None, None)
        }
    }

    pub(super) fn paint_settings(
        &self,
        client_height: f32,
        layout: &SettingsLayout,
        service_ready: bool,
        name_focused: bool,
        panels: &[Rect],
        notice: Option<&SaveNotice>,
    ) -> windows::core::Result<()> {
        unsafe {
            let color = if service_ready {
                rgb(0, 186, 184)
            } else {
                rgb(209, 63, 63)
            };
            let dot = self.brush(color)?;
            let field_border = self.brush(if name_focused {
                rgb(0, 186, 184)
            } else {
                rgb(226, 229, 233)
            })?;
            let notice_brushes = self.notice_brushes(notice)?;
            self.begin_shell(client_height, panels)?;
            let field = layout.name_frame;
            self.target.DrawRoundedRectangle(
                &D2D1_ROUNDED_RECT {
                    rect: D2D_RECT_F {
                        left: field.left,
                        top: field.top,
                        right: field.right,
                        bottom: field.bottom,
                    },
                    radiusX: 6.0,
                    radiusY: 6.0,
                },
                &field_border,
                1.0,
                None,
            );
            let status = layout.service_status_dot;
            self.target.FillEllipse(&ellipse(status), &dot);
            self.draw_notice(notice, notice_brushes.as_ref());
            self.target.EndDraw(None, None)
        }
    }

    pub(super) fn paint_pairing(
        &self,
        client_height: f32,
        layout: &PairingLayout,
        qr: Option<(usize, &[bool])>,
        panels: &[Rect],
        notice: Option<&SaveNotice>,
    ) -> windows::core::Result<()> {
        unsafe {
            let teal = self.brush(rgb(0, 186, 184))?;
            let black = if qr.is_some() {
                Some(self.brush(rgb(0, 0, 0))?)
            } else {
                None
            };
            let notice_brushes = self.notice_brushes(notice)?;
            self.begin_shell(client_height, panels)?;
            self.target.FillEllipse(&ellipse(layout.waiting_dot), &teal);

            if let Some((width, modules)) = qr {
                let black = black.as_ref().expect("QR brush exists");
                let module = (220.0 / (width as f32 + 8.0)).floor().max(2.0);
                let quiet = 4.0;
                for row in 0..width {
                    for column in 0..width {
                        if modules[row * width + column] {
                            let left = layout.qr_origin.0 + (column as f32 + quiet) * module;
                            let top = layout.qr_origin.1 + (row as f32 + quiet) * module;
                            self.target.FillRectangle(
                                &D2D_RECT_F {
                                    left,
                                    top,
                                    right: left + module,
                                    bottom: top + module,
                                },
                                black,
                            );
                        }
                    }
                }
            }
            self.draw_notice(notice, notice_brushes.as_ref());
            self.target.EndDraw(None, None)
        }
    }

    unsafe fn begin_shell(
        &self,
        client_height: f32,
        panels: &[Rect],
    ) -> windows::core::Result<ID2D1SolidColorBrush> {
        let sidebar = unsafe { self.brush(rgb(247, 248, 250))? };
        let line = unsafe { self.brush(rgb(226, 229, 233))? };
        let surface = unsafe { self.brush(rgb(255, 255, 255))? };
        unsafe {
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
            for panel in panels {
                let rounded = D2D1_ROUNDED_RECT {
                    rect: D2D_RECT_F {
                        left: panel.left,
                        top: panel.top,
                        right: panel.right,
                        bottom: panel.bottom,
                    },
                    radiusX: 8.0,
                    radiusY: 8.0,
                };
                self.target.FillRoundedRectangle(&rounded, &surface);
                self.target.DrawRoundedRectangle(&rounded, &line, 1.0, None);
            }
        }
        Ok(line)
    }

    unsafe fn brush(&self, color: D2D1_COLOR_F) -> windows::core::Result<ID2D1SolidColorBrush> {
        unsafe { self.target.CreateSolidColorBrush(&color, None) }
    }

    unsafe fn notice_brushes(
        &self,
        notice: Option<&SaveNotice>,
    ) -> windows::core::Result<Option<NoticeBrushes>> {
        let Some(notice) = notice else {
            return Ok(None);
        };
        Ok(Some(NoticeBrushes {
            background: unsafe {
                self.brush(if notice.success {
                    rgb(234, 249, 248)
                } else {
                    rgb(241, 228, 228)
                })?
            },
            accent: unsafe {
                self.brush(if notice.success {
                    rgb(0, 186, 184)
                } else {
                    rgb(209, 63, 63)
                })?
            },
            text: unsafe { self.brush(rgb(31, 31, 31))? },
            white: unsafe { self.brush(rgb(255, 255, 255))? },
        }))
    }

    unsafe fn draw_notice(&self, notice: Option<&SaveNotice>, brushes: Option<&NoticeBrushes>) {
        let (Some(notice), Some(brushes)) = (notice, brushes) else {
            return;
        };
        let right = unsafe { self.target.GetSize().width } - 18.0;
        let toast = D2D1_ROUNDED_RECT {
            rect: D2D_RECT_F {
                left: right - 222.0,
                top: 76.0,
                right,
                bottom: 120.0,
            },
            radiusX: 8.0,
            radiusY: 8.0,
        };
        unsafe {
            self.target
                .FillRoundedRectangle(&toast, &brushes.background);
            let icon = Rect::from_xywh(right - 212.0, 86.0, 24.0, 24.0);
            self.target.FillEllipse(&ellipse(icon), &brushes.accent);
            self.draw_text(
                if notice.success { "\u{e73e}" } else { "!" },
                icon,
                &self.icon,
                &brushes.white,
            );
            self.draw_text(
                &notice.text,
                Rect::from_xywh(right - 176.0, 76.0, 166.0, 44.0),
                &self.body,
                &brushes.text,
            );
        }
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

struct NoticeBrushes {
    background: ID2D1SolidColorBrush,
    accent: ID2D1SolidColorBrush,
    text: ID2D1SolidColorBrush,
    white: ID2D1SolidColorBrush,
}

fn ellipse(rect: Rect) -> windows::Win32::Graphics::Direct2D::D2D1_ELLIPSE {
    windows::Win32::Graphics::Direct2D::D2D1_ELLIPSE {
        point: windows_numerics::Vector2 {
            X: (rect.left + rect.right) / 2.0,
            Y: (rect.top + rect.bottom) / 2.0,
        },
        radiusX: rect.width() / 2.0,
        radiusY: (rect.bottom - rect.top) / 2.0,
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
