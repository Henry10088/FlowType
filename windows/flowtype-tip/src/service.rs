use std::cell::{Cell, RefCell};
use std::ffi::c_void;
use std::rc::Rc;

use flowtype_core::tip::{TipCommand, TipResponse};
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::Threading::{GetCurrentProcessId, GetCurrentThreadId};
use windows::Win32::UI::TextServices::{
    ITfContext, ITfTextInputProcessor_Impl, ITfTextInputProcessorEx, ITfTextInputProcessorEx_Impl,
    ITfThreadMgr,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CREATESTRUCTW, CreateWindowExW, DefWindowProcW, DestroyWindow, GWLP_USERDATA,
    GetWindowLongPtrW, HWND_MESSAGE, RegisterClassW, SetWindowLongPtrW, WINDOW_EX_STYLE,
    WINDOW_STYLE, WM_APP, WM_NCCREATE, WM_NCDESTROY, WNDCLASSW,
};
use windows::core::{PCWSTR, Ref, Result, implement};

use crate::composition::{CompositionState, EditAction, request_edit};
use crate::ipc::{PendingCommands, Worker};
use crate::lifetime::ObjectGuard;
use crate::module_instance;

pub const WM_TIP_COMMAND: u32 = WM_APP + 0x37;
const WINDOW_CLASS: &[u16] = &[
    b'F' as u16,
    b'l' as u16,
    b'o' as u16,
    b'w' as u16,
    b'T' as u16,
    b'y' as u16,
    b'p' as u16,
    b'e' as u16,
    b'T' as u16,
    b'i' as u16,
    b'p' as u16,
    b'M' as u16,
    b's' as u16,
    b'g' as u16,
    0,
];

#[implement(ITfTextInputProcessorEx)]
pub struct TextService {
    _guard: ObjectGuard,
    controller: Rc<Controller>,
}

impl TextService {
    pub fn new() -> Self {
        Self {
            _guard: ObjectGuard::new(),
            controller: Rc::new(Controller::default()),
        }
    }
}

impl Drop for TextService {
    fn drop(&mut self) {
        self.controller.deactivate();
    }
}

impl ITfTextInputProcessor_Impl for TextService_Impl {
    fn Activate(&self, thread_manager: Ref<ITfThreadMgr>, client_id: u32) -> Result<()> {
        self.controller
            .activate(thread_manager.ok()?.clone(), client_id)
    }

    fn Deactivate(&self) -> Result<()> {
        self.controller.deactivate();
        Ok(())
    }
}

impl ITfTextInputProcessorEx_Impl for TextService_Impl {
    fn ActivateEx(
        &self,
        thread_manager: Ref<ITfThreadMgr>,
        client_id: u32,
        _flags: u32,
    ) -> Result<()> {
        self.controller
            .activate(thread_manager.ok()?.clone(), client_id)
    }
}

#[derive(Default)]
struct Controller {
    thread_manager: RefCell<Option<ITfThreadMgr>>,
    client_id: Cell<u32>,
    composition: Rc<CompositionState>,
    window: Cell<Option<HWND>>,
    pending_commands: PendingCommands,
    worker: RefCell<Option<Worker>>,
}

impl Controller {
    fn activate(self: &Rc<Self>, thread_manager: ITfThreadMgr, client_id: u32) -> Result<()> {
        self.deactivate();
        *self.thread_manager.borrow_mut() = Some(thread_manager);
        self.client_id.set(client_id);
        let window = create_message_window(self)?;
        self.window.set(Some(window));
        let worker = Worker::start(
            window,
            unsafe { GetCurrentProcessId() },
            unsafe { GetCurrentThreadId() },
            self.pending_commands.clone(),
        );
        *self.worker.borrow_mut() = Some(worker);
        Ok(())
    }

    fn deactivate(&self) {
        if let Some(worker) = self.worker.borrow_mut().take() {
            worker.stop();
        }
        if let Some(window) = self.window.take() {
            let _ = unsafe { DestroyWindow(window) };
        }
        if self.composition.has_composition()
            && let Some(context) = self.composition.context()
        {
            let _ = request_edit(
                &context,
                self.client_id.get(),
                self.composition.clone(),
                EditAction::ForceFinish,
            );
        }
        self.composition.clear();
        self.thread_manager.borrow_mut().take();
        self.client_id.set(0);
    }

    fn handle_command(&self, command: TipCommand) -> TipResponse {
        match command {
            TipCommand::Ping => TipResponse::Ready,
            TipCommand::Begin {
                session_id,
                sequence,
                full_text,
                attach_existing,
            } => self.begin(session_id, sequence, full_text, attach_existing),
            TipCommand::Update {
                session_id,
                sequence,
                full_text,
            } => self.update(session_id, sequence, full_text),
            TipCommand::Finish {
                session_id,
                sequence,
            } => self.finish(session_id, sequence),
            TipCommand::Cancel { session_id } => self.cancel(session_id),
            TipCommand::Query { session_id } => self.query(session_id),
        }
    }

    fn begin(
        &self,
        session_id: String,
        sequence: i64,
        full_text: String,
        attach_existing: bool,
    ) -> TipResponse {
        if self.composition.session_id().as_deref() == Some(&session_id) {
            return if self.composition.is_target_modified() {
                TipResponse::CompositionTerminated
            } else {
                TipResponse::Begun { session_id }
            };
        }
        if self.composition.session_id().is_some() {
            // Cancel/finish must close the prior session before a different
            // session can select a new insertion range. Silently replacing it
            // would append the phone's full snapshot a second time.
            return TipResponse::RebindRejected;
        }
        let Some(context) = self.focused_context() else {
            return TipResponse::NoFocus;
        };
        if self
            .composition
            .start_session(session_id.clone(), context.clone(), self.client_id.get())
            .is_err()
        {
            let _ = request_edit(
                &context,
                self.client_id.get(),
                self.composition.clone(),
                EditAction::ForceFinish,
            );
            self.composition.clear();
            return TipResponse::EditRejected;
        }
        if request_edit(
            &context,
            self.client_id.get(),
            self.composition.clone(),
            EditAction::Begin {
                initial_text: full_text.clone(),
                attach_existing,
            },
        )
        .is_err()
        {
            let _ = request_edit(
                &context,
                self.client_id.get(),
                self.composition.clone(),
                EditAction::ForceFinish,
            );
            self.composition.clear();
            return TipResponse::EditRejected;
        }
        self.composition.applied(sequence, full_text);
        TipResponse::Begun { session_id }
    }

    fn update(&self, session_id: String, sequence: i64, full_text: String) -> TipResponse {
        if self.composition.session_id().as_deref() != Some(&session_id) {
            return TipResponse::SessionMismatch;
        }
        if self.composition.is_target_modified() {
            return TipResponse::CompositionTerminated;
        }
        let current_sequence = self.composition.sequence();
        if sequence < current_sequence {
            return TipResponse::Applied {
                session_id,
                sequence: current_sequence,
            };
        }
        if sequence == current_sequence {
            return if self.composition.text_matches(&full_text) {
                TipResponse::Applied {
                    session_id,
                    sequence,
                }
            } else {
                TipResponse::SequenceConflict
            };
        }
        let Some(context) = self.active_focused_context() else {
            return TipResponse::NoFocus;
        };
        if request_edit(
            &context,
            self.client_id.get(),
            self.composition.clone(),
            EditAction::Update(full_text.clone()),
        )
        .is_err()
        {
            return if self.composition.is_target_modified() {
                TipResponse::CompositionTerminated
            } else {
                TipResponse::EditRejected
            };
        }
        self.composition.applied(sequence, full_text);
        TipResponse::Applied {
            session_id,
            sequence,
        }
    }

    fn finish(&self, session_id: String, sequence: i64) -> TipResponse {
        if self.composition.session_id().as_deref() != Some(&session_id)
            || self.composition.sequence() != sequence
        {
            return TipResponse::SessionMismatch;
        }
        if self.composition.is_target_modified() {
            return TipResponse::CompositionTerminated;
        }
        let Some(context) = self.composition.context() else {
            return TipResponse::CompositionTerminated;
        };
        if request_edit(
            &context,
            self.client_id.get(),
            self.composition.clone(),
            EditAction::Finish,
        )
        .is_err()
        {
            return TipResponse::EditRejected;
        }
        self.composition.clear();
        TipResponse::Finished {
            session_id,
            sequence,
        }
    }

    fn cancel(&self, session_id: String) -> TipResponse {
        if self.composition.session_id().as_deref() != Some(&session_id) {
            return TipResponse::SessionMismatch;
        }
        if self.composition.has_composition()
            && let Some(context) = self.composition.context()
        {
            let _ = request_edit(
                &context,
                self.client_id.get(),
                self.composition.clone(),
                EditAction::ForceFinish,
            );
        }
        self.composition.clear();
        TipResponse::Cancelled { session_id }
    }

    fn query(&self, session_id: String) -> TipResponse {
        if self.controller_session_matches(&session_id) {
            if self.composition.is_target_modified() {
                TipResponse::CompositionTerminated
            } else {
                TipResponse::SessionActive {
                    session_id,
                    sequence: self.composition.sequence(),
                }
            }
        } else {
            TipResponse::SessionMismatch
        }
    }

    fn controller_session_matches(&self, session_id: &str) -> bool {
        self.composition.session_id().as_deref() == Some(session_id)
    }

    fn focused_context(&self) -> Option<ITfContext> {
        let manager = self.thread_manager.borrow().clone()?;
        if !unsafe { manager.IsThreadFocus().ok()? }.as_bool() {
            return None;
        }
        let document = unsafe { manager.GetFocus().ok()? };
        unsafe { document.GetTop().ok() }
    }

    fn active_focused_context(&self) -> Option<ITfContext> {
        let focused = self.focused_context()?;
        let active = self.composition.context()?;
        (focused == active).then_some(active)
    }
}

fn create_message_window(controller: &Rc<Controller>) -> Result<HWND> {
    let instance = HINSTANCE(module_instance().0);
    let class = WNDCLASSW {
        lpfnWndProc: Some(window_proc),
        hInstance: instance,
        lpszClassName: PCWSTR(WINDOW_CLASS.as_ptr()),
        ..Default::default()
    };
    unsafe { RegisterClassW(&class) };
    unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            PCWSTR(WINDOW_CLASS.as_ptr()),
            PCWSTR::null(),
            WINDOW_STYLE(0),
            0,
            0,
            0,
            0,
            Some(HWND_MESSAGE),
            None,
            Some(instance),
            Some(Rc::as_ptr(controller).cast::<c_void>().cast_mut()),
        )
    }
}

unsafe extern "system" fn window_proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if message == WM_NCCREATE {
        if unsafe { GetWindowLongPtrW(window, GWLP_USERDATA) } != 0 {
            return LRESULT(0);
        }
        let create = unsafe { &*(lparam.0 as *const CREATESTRUCTW) };
        unsafe { SetWindowLongPtrW(window, GWLP_USERDATA, create.lpCreateParams as _) };
        return LRESULT(1);
    }
    let controller = unsafe { GetWindowLongPtrW(window, GWLP_USERDATA) } as *const Controller;
    if message == WM_TIP_COMMAND && !controller.is_null() {
        let Some((command, response_sender)) =
            (unsafe { &*controller }).pending_commands.take(wparam.0)
        else {
            return LRESULT(0);
        };
        let response = unsafe { &*controller }.handle_command(command);
        let _ = response_sender.send(response);
        return LRESULT(0);
    }
    if message == WM_NCDESTROY && !controller.is_null() {
        unsafe {
            SetWindowLongPtrW(window, GWLP_USERDATA, 0);
        }
    }
    unsafe { DefWindowProcW(window, message, wparam, lparam) }
}
