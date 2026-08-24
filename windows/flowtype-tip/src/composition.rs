use std::cell::{Cell, RefCell};
use std::mem::ManuallyDrop;
use std::rc::{Rc, Weak};

use windows::Win32::UI::TextServices::{
    ITfComposition, ITfCompositionSink, ITfCompositionSink_Impl, ITfContext, ITfContextComposition,
    ITfEditRecord, ITfEditSession, ITfEditSession_Impl, ITfInsertAtSelection, ITfSource,
    ITfTextEditSink, ITfTextEditSink_Impl, TF_AE_END, TF_ANCHOR_END, TF_DEFAULT_SELECTION,
    TF_ES_ASYNC, TF_ES_READWRITE, TF_ES_SYNC, TF_IAS_QUERYONLY, TF_SELECTION, TF_SELECTIONSTYLE,
};
use windows::core::{ComObject, Interface, Result, implement};

use crate::lifetime::ObjectGuard;

pub struct CompositionState {
    session_id: RefCell<Option<String>>,
    sequence: Cell<i64>,
    text: RefCell<String>,
    context: RefCell<Option<ITfContext>>,
    composition: RefCell<Option<ITfComposition>>,
    edit_sink: RefCell<Option<EditSinkSubscription>>,
    applying_remote_edit: Cell<bool>,
    terminated: Cell<bool>,
}

impl Default for CompositionState {
    fn default() -> Self {
        Self {
            session_id: RefCell::new(None),
            sequence: Cell::new(0),
            text: RefCell::new(String::new()),
            context: RefCell::new(None),
            composition: RefCell::new(None),
            edit_sink: RefCell::new(None),
            applying_remote_edit: Cell::new(false),
            terminated: Cell::new(false),
        }
    }
}

impl CompositionState {
    pub fn session_id(&self) -> Option<String> {
        self.session_id.borrow().clone()
    }

    pub fn sequence(&self) -> i64 {
        self.sequence.get()
    }

    pub fn text_matches(&self, text: &str) -> bool {
        *self.text.borrow() == text
    }

    pub fn is_terminated(&self) -> bool {
        self.terminated.get()
    }

    pub fn context(&self) -> Option<ITfContext> {
        self.context.borrow().clone()
    }

    pub fn start_session(
        self: &Rc<Self>,
        session_id: String,
        context: ITfContext,
        client_id: u32,
    ) -> Result<()> {
        let source: ITfSource = context.cast()?;
        let sink = ComObject::new(TextEditSink {
            _guard: ObjectGuard::new(),
            state: Rc::downgrade(self),
            client_id,
        })
        .into_interface::<ITfTextEditSink>();
        let cookie = unsafe { source.AdviseSink(&ITfTextEditSink::IID, &sink)? };
        *self.session_id.borrow_mut() = Some(session_id);
        self.sequence.set(0);
        self.text.borrow_mut().clear();
        *self.context.borrow_mut() = Some(context);
        *self.edit_sink.borrow_mut() = Some(EditSinkSubscription {
            source,
            cookie,
            _sink: sink,
        });
        self.terminated.set(false);
        Ok(())
    }

    pub fn applied(&self, sequence: i64, text: String) {
        self.sequence.set(sequence);
        *self.text.borrow_mut() = text;
    }

    pub fn clear(&self) {
        if let Some(subscription) = self.edit_sink.borrow_mut().take() {
            let _ = unsafe { subscription.source.UnadviseSink(subscription.cookie) };
        }
        self.session_id.borrow_mut().take();
        self.sequence.set(0);
        self.text.borrow_mut().clear();
        self.context.borrow_mut().take();
        self.composition.borrow_mut().take();
        self.applying_remote_edit.set(false);
        self.terminated.set(false);
    }

    fn composition(&self) -> Result<ITfComposition> {
        self.composition
            .borrow()
            .clone()
            .ok_or_else(|| windows::core::Error::from_hresult(windows::Win32::Foundation::E_FAIL))
    }

    fn set_composition(&self, composition: ITfComposition) {
        *self.composition.borrow_mut() = Some(composition);
    }

    pub fn has_composition(&self) -> bool {
        self.composition.borrow().is_some()
    }

    fn is_applying_remote_edit(&self) -> bool {
        self.applying_remote_edit.get()
    }

    fn matches_expected_edit_state(&self, context: &ITfContext, ec: u32) -> Result<bool> {
        let composition = self.composition()?;
        let composition_range = unsafe { composition.GetRange()? };
        let expected: Vec<u16> = self.text.borrow().encode_utf16().collect();
        let mut actual = vec![0_u16; expected.len().saturating_add(1)];
        let mut actual_len = 0_u32;
        unsafe { composition_range.GetText(ec, 0, &mut actual, &mut actual_len)? };
        let actual_len = actual_len as usize;
        if actual_len != expected.len() || actual[..actual_len] != expected {
            return Ok(false);
        }

        let mut selection = [TF_SELECTION::default()];
        let mut fetched = 0_u32;
        let result = (|| {
            unsafe {
                context.GetSelection(ec, TF_DEFAULT_SELECTION, &mut selection, &mut fetched)?
            };
            let selected_range = selection
                .first()
                .and_then(|selected| selected.range.as_ref())
                .filter(|_| fetched == 1)
                .ok_or_else(|| {
                    windows::core::Error::from_hresult(windows::Win32::Foundation::E_FAIL)
                })?;
            if !unsafe { selected_range.IsEmpty(ec)? }.as_bool() {
                return Ok(false);
            }
            Ok(unsafe { selected_range.CompareStart(ec, &composition_range, TF_ANCHOR_END)? == 0 })
        })();
        unsafe { ManuallyDrop::drop(&mut selection[0].range) };
        result
    }

    fn mark_external_edit(&self) {
        self.terminated.set(true);
    }

    fn mark_terminated(&self) {
        self.composition.borrow_mut().take();
        self.terminated.set(true);
    }
}

struct EditSinkSubscription {
    source: ITfSource,
    cookie: u32,
    _sink: ITfTextEditSink,
}

pub enum EditAction {
    Begin,
    Update(String),
    Finish,
}

pub fn request_edit(
    context: &ITfContext,
    client_id: u32,
    state: Rc<CompositionState>,
    action: EditAction,
) -> Result<()> {
    let _guard = RemoteEditGuard::new(&state);
    let session = ComObject::new(EditSession {
        context: context.clone(),
        state: state.clone(),
        action,
    })
    .into_interface::<ITfEditSession>();
    let result =
        unsafe { context.RequestEditSession(client_id, &session, TF_ES_SYNC | TF_ES_READWRITE)? };
    result.ok()
}

fn request_async_finish(
    context: &ITfContext,
    client_id: u32,
    state: Rc<CompositionState>,
) -> Result<()> {
    let session = ComObject::new(EditSession {
        context: context.clone(),
        state,
        action: EditAction::Finish,
    })
    .into_interface::<ITfEditSession>();
    let result =
        unsafe { context.RequestEditSession(client_id, &session, TF_ES_ASYNC | TF_ES_READWRITE)? };
    result.ok()
}

struct RemoteEditGuard<'a>(&'a CompositionState);

impl<'a> RemoteEditGuard<'a> {
    fn new(state: &'a CompositionState) -> Self {
        state.applying_remote_edit.set(true);
        Self(state)
    }
}

impl Drop for RemoteEditGuard<'_> {
    fn drop(&mut self) {
        self.0.applying_remote_edit.set(false);
    }
}

#[implement(ITfEditSession)]
struct EditSession {
    context: ITfContext,
    state: Rc<CompositionState>,
    action: EditAction,
}

impl ITfEditSession_Impl for EditSession_Impl {
    fn DoEditSession(&self, ec: u32) -> Result<()> {
        match &self.action {
            EditAction::Begin => self.begin(ec),
            EditAction::Update(text) => self.update(ec, text),
            EditAction::Finish => self.finish(ec),
        }
    }
}

impl EditSession_Impl {
    fn begin(&self, ec: u32) -> Result<()> {
        let insert: ITfInsertAtSelection = self.context.cast()?;
        let range = unsafe { insert.InsertTextAtSelection(ec, TF_IAS_QUERYONLY, &[])? };
        let context_composition: ITfContextComposition = self.context.cast()?;
        let sink = ComObject::new(CompositionSink {
            _guard: ObjectGuard::new(),
            state: self.state.clone(),
        })
        .into_interface::<ITfCompositionSink>();
        self.state.terminated.set(false);
        let composition = unsafe { context_composition.StartComposition(ec, &range, &sink)? };
        if self.state.is_terminated() {
            return Err(windows::core::Error::from_hresult(
                windows::Win32::Foundation::E_FAIL,
            ));
        }
        self.state.set_composition(composition);
        set_caret_to_composition_end(&self.context, ec, &range)
    }

    fn update(&self, ec: u32, text: &str) -> Result<()> {
        let composition = self.state.composition()?;
        if !self.state.matches_expected_edit_state(&self.context, ec)? {
            self.state.mark_external_edit();
            let _ = unsafe { composition.EndComposition(ec) };
            return Err(windows::core::Error::from_hresult(
                windows::Win32::Foundation::E_FAIL,
            ));
        }
        let range = unsafe { composition.GetRange()? };
        let utf16: Vec<u16> = text.encode_utf16().collect();
        unsafe { range.SetText(ec, 0, &utf16)? };
        if self.state.is_terminated() {
            return Err(windows::core::Error::from_hresult(
                windows::Win32::Foundation::E_FAIL,
            ));
        }
        set_caret_to_composition_end(&self.context, ec, &range)
    }

    fn finish(&self, ec: u32) -> Result<()> {
        let composition = self.state.composition()?;
        unsafe { composition.EndComposition(ec) }
    }
}

fn set_caret_to_composition_end(
    context: &ITfContext,
    ec: u32,
    range: &windows::Win32::UI::TextServices::ITfRange,
) -> Result<()> {
    let caret = range.clone();
    unsafe { caret.Collapse(ec, TF_ANCHOR_END)? };
    let mut selection = TF_SELECTION {
        range: ManuallyDrop::new(Some(caret)),
        style: TF_SELECTIONSTYLE {
            ase: TF_AE_END,
            fInterimChar: false.into(),
        },
    };
    let result = unsafe { context.SetSelection(ec, std::slice::from_ref(&selection)) };
    unsafe { ManuallyDrop::drop(&mut selection.range) };
    result
}

#[implement(ITfCompositionSink)]
struct CompositionSink {
    _guard: ObjectGuard,
    state: Rc<CompositionState>,
}

impl ITfCompositionSink_Impl for CompositionSink_Impl {
    fn OnCompositionTerminated(
        &self,
        _ecwrite: u32,
        _composition: windows::core::Ref<ITfComposition>,
    ) -> Result<()> {
        self.state.mark_terminated();
        Ok(())
    }
}

#[implement(ITfTextEditSink)]
struct TextEditSink {
    _guard: ObjectGuard,
    state: Weak<CompositionState>,
    client_id: u32,
}

impl ITfTextEditSink_Impl for TextEditSink_Impl {
    fn OnEndEdit(
        &self,
        context: windows::core::Ref<ITfContext>,
        ec_read_only: u32,
        _edit_record: windows::core::Ref<ITfEditRecord>,
    ) -> Result<()> {
        let Some(state) = self.state.upgrade() else {
            return Ok(());
        };
        if state.session_id().is_none() || state.is_applying_remote_edit() || state.is_terminated()
        {
            return Ok(());
        }
        let context = context.ok()?.clone();
        if state
            .matches_expected_edit_state(&context, ec_read_only)
            .unwrap_or(true)
        {
            return Ok(());
        }
        state.mark_external_edit();
        if state.has_composition() {
            let _ = request_async_finish(&context, self.client_id, state);
        }
        Ok(())
    }
}
