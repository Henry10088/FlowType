use std::cell::{Cell, RefCell};
use std::mem::ManuallyDrop;
use std::rc::{Rc, Weak};

use windows::Win32::UI::TextServices::{
    ITfComposition, ITfCompositionSink, ITfCompositionSink_Impl, ITfContext, ITfContextComposition,
    ITfEditRecord, ITfEditSession, ITfEditSession_Impl, ITfInsertAtSelection, ITfRange,
    ITfRangeACP, ITfSource, ITfTextEditSink, ITfTextEditSink_Impl, TF_AE_END, TF_ANCHOR_END,
    TF_DEFAULT_SELECTION, TF_ES_ASYNC, TF_ES_READWRITE, TF_ES_SYNC, TF_GRAVITY_BACKWARD,
    TF_GRAVITY_FORWARD, TF_IAS_QUERYONLY, TF_SELECTION, TF_SELECTIONSTYLE,
};
use windows::core::{ComObject, Interface, Result, implement};

use crate::diagnostics;
use crate::host;
use crate::lifetime::ObjectGuard;

pub struct CompositionState {
    session_id: RefCell<Option<String>>,
    sequence: Cell<i64>,
    text: RefCell<String>,
    context: RefCell<Option<ITfContext>>,
    composition: RefCell<Option<ITfComposition>>,
    range: RefCell<Option<ITfRange>>,
    edit_sink: RefCell<Option<EditSinkSubscription>>,
    applying_remote_edit: Cell<bool>,
    terminated: Cell<bool>,
    target_modified: Cell<bool>,
}

impl Default for CompositionState {
    fn default() -> Self {
        Self {
            session_id: RefCell::new(None),
            sequence: Cell::new(0),
            text: RefCell::new(String::new()),
            context: RefCell::new(None),
            composition: RefCell::new(None),
            range: RefCell::new(None),
            edit_sink: RefCell::new(None),
            applying_remote_edit: Cell::new(false),
            terminated: Cell::new(false),
            target_modified: Cell::new(false),
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

    pub fn is_target_modified(&self) -> bool {
        self.target_modified.get()
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
        if self.edit_sink.borrow().is_none() {
            let source: ITfSource = context.cast()?;
            let sink = ComObject::new(TextEditSink {
                _guard: ObjectGuard::new(),
                state: Rc::downgrade(self),
                client_id,
            })
            .into_interface::<ITfTextEditSink>();
            let cookie = unsafe { source.AdviseSink(&ITfTextEditSink::IID, &sink)? };
            *self.edit_sink.borrow_mut() = Some(EditSinkSubscription {
                source,
                cookie,
                _sink: sink,
            });
        }
        *self.session_id.borrow_mut() = Some(session_id);
        self.sequence.set(0);
        self.text.borrow_mut().clear();
        *self.context.borrow_mut() = Some(context);
        self.target_modified.set(false);
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
        self.range.borrow_mut().take();
        self.applying_remote_edit.set(false);
        self.terminated.set(false);
        self.target_modified.set(false);
    }

    fn composition(&self) -> Result<ITfComposition> {
        self.composition
            .borrow()
            .clone()
            .ok_or_else(|| windows::core::Error::from_hresult(windows::Win32::Foundation::E_FAIL))
    }

    fn set_composition(&self, composition: ITfComposition, range: ITfRange) {
        *self.composition.borrow_mut() = Some(composition);
        *self.range.borrow_mut() = Some(range);
        self.terminated.set(false);
    }

    pub fn has_composition(&self) -> bool {
        !self.is_terminated() && self.composition.borrow().is_some()
    }

    fn range(&self) -> Result<ITfRange> {
        self.range
            .borrow()
            .clone()
            .ok_or_else(|| windows::core::Error::from_hresult(windows::Win32::Foundation::E_FAIL))
    }

    fn is_applying_remote_edit(&self) -> bool {
        self.applying_remote_edit.get()
    }

    fn matches_expected_range(
        &self,
        context: &ITfContext,
        range: &windows::Win32::UI::TextServices::ITfRange,
        ec: u32,
    ) -> Result<bool> {
        let expected: Vec<u16> = self.text.borrow().encode_utf16().collect();
        let mut actual = vec![0_u16; expected.len().saturating_add(1)];
        let mut actual_len = 0_u32;
        unsafe { range.GetText(ec, 0, &mut actual, &mut actual_len)? };
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
            Ok(unsafe { selected_range.CompareStart(ec, range, TF_ANCHOR_END)? == 0 })
        })();
        unsafe { ManuallyDrop::drop(&mut selection[0].range) };
        result
    }

    fn matches_expected_edit_state(&self, context: &ITfContext, ec: u32) -> Result<bool> {
        let range = self.range()?;
        self.matches_expected_range(context, &range, ec)
    }

    fn mark_external_edit(&self) {
        self.target_modified.set(true);
    }

    fn mark_terminated(&self) {
        self.terminated.set(true);
        self.composition.borrow_mut().take();
        if let Some(subscription) = self.edit_sink.borrow_mut().take() {
            let _ = unsafe { subscription.source.UnadviseSink(subscription.cookie) };
        }
    }

    fn composition_terminated(&self) {
        if !self.is_applying_remote_edit() {
            self.mark_external_edit();
        }
        self.mark_terminated();
    }
}

struct EditSinkSubscription {
    source: ITfSource,
    cookie: u32,
    _sink: ITfTextEditSink,
}

pub enum EditAction {
    Begin {
        initial_text: String,
        attach_existing: bool,
    },
    Update(String),
    Finish,
    ForceFinish,
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

fn request_async_force_finish(
    context: &ITfContext,
    client_id: u32,
    state: Rc<CompositionState>,
) -> Result<()> {
    let session = ComObject::new(EditSession {
        context: context.clone(),
        state,
        action: EditAction::ForceFinish,
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
            EditAction::Begin {
                initial_text,
                attach_existing,
            } => self.begin(ec, initial_text, *attach_existing),
            EditAction::Update(text) => self.update(ec, text),
            EditAction::Finish => self.finish(ec, FinishCause::Phone),
            EditAction::ForceFinish => self.finish(ec, FinishCause::Forced),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum FinishCause {
    Phone,
    Forced,
}

impl FinishCause {
    fn validates_snapshot(self) -> bool {
        self == Self::Phone
    }
}

impl EditSession_Impl {
    fn begin(&self, ec: u32, initial_text: &str, attach_existing: bool) -> Result<()> {
        let insert: ITfInsertAtSelection = self.context.cast()?;
        let range = if attach_existing {
            current_selection_range(&self.context, ec)
                .or_else(|_| unsafe { insert.InsertTextAtSelection(ec, TF_IAS_QUERYONLY, &[]) })?
        } else {
            unsafe { insert.InsertTextAtSelection(ec, TF_IAS_QUERYONLY, &[])? }
        };
        let attached_range = attach_existing
            .then(|| range_with_exact_suffix(&range, ec, initial_text))
            .transpose()?
            .flatten();
        let mut host_replacement = if attach_existing && attached_range.is_none() {
            host::replace_exact_suffix(initial_text)
        } else {
            None
        };
        let (tracked_range, text_already_present) = if let Some(existing) = attached_range {
            (existing, true)
        } else if host_replacement.is_some() {
            diagnostics::log(format!(
                "attach range=scintilla units={}",
                initial_text.encode_utf16().count()
            ));
            (
                unsafe { insert.InsertTextAtSelection(ec, TF_IAS_QUERYONLY, &[])? },
                false,
            )
        } else {
            (unsafe { range.Clone()? }, false)
        };
        unsafe {
            tracked_range.SetGravity(ec, TF_GRAVITY_BACKWARD, TF_GRAVITY_FORWARD)?;
        }
        let context_composition: ITfContextComposition = self.context.cast()?;
        let sink = ComObject::new(CompositionSink {
            _guard: ObjectGuard::new(),
            state: self.state.clone(),
        })
        .into_interface::<ITfCompositionSink>();
        self.state.terminated.set(false);
        let composition =
            unsafe { context_composition.StartComposition(ec, &tracked_range, &sink)? };
        if self.state.is_terminated() {
            return Err(windows::core::Error::from_hresult(
                windows::Win32::Foundation::E_FAIL,
            ));
        }
        self.state
            .set_composition(composition, unsafe { tracked_range.Clone()? });
        if !text_already_present {
            let utf16: Vec<u16> = initial_text.encode_utf16().collect();
            unsafe { tracked_range.SetText(ec, 0, &utf16)? };
        }
        if let Some(replacement) = host_replacement.take() {
            replacement.commit();
            diagnostics::log(format!(
                "attach result=matched units={}",
                initial_text.encode_utf16().count()
            ));
        }
        set_caret_to_composition_end(&self.context, ec, &tracked_range)?;
        Ok(())
    }

    fn update(&self, ec: u32, text: &str) -> Result<()> {
        if self.state.is_target_modified() {
            return Err(windows::core::Error::from_hresult(
                windows::Win32::Foundation::E_FAIL,
            ));
        }
        if !self.state.matches_expected_edit_state(&self.context, ec)? {
            self.state.mark_external_edit();
            if let Ok(composition) = self.state.composition() {
                let _ = unsafe { composition.EndComposition(ec) };
            }
            return Err(windows::core::Error::from_hresult(
                windows::Win32::Foundation::E_FAIL,
            ));
        }
        let range = self.state.range()?;
        let utf16: Vec<u16> = text.encode_utf16().collect();
        unsafe { range.SetText(ec, 0, &utf16)? };
        if self.state.is_target_modified() {
            return Err(windows::core::Error::from_hresult(
                windows::Win32::Foundation::E_FAIL,
            ));
        }
        set_caret_to_composition_end(&self.context, ec, &range)
    }

    fn finish(&self, ec: u32, cause: FinishCause) -> Result<()> {
        if cause.validates_snapshot()
            && (self.state.is_target_modified()
                || !self.state.matches_expected_edit_state(&self.context, ec)?)
        {
            return Err(windows::core::Error::from_hresult(
                windows::Win32::Foundation::E_FAIL,
            ));
        }
        let result = if let Ok(composition) = self.state.composition() {
            unsafe { composition.EndComposition(ec) }
        } else {
            Ok(())
        };
        if cause == FinishCause::Forced {
            self.state.mark_terminated();
        }
        result
    }
}

fn current_selection_range(context: &ITfContext, ec: u32) -> Result<ITfRange> {
    let mut selection = [TF_SELECTION::default()];
    let mut fetched = 0_u32;
    let result = (|| {
        unsafe { context.GetSelection(ec, TF_DEFAULT_SELECTION, &mut selection, &mut fetched)? };
        let range = selection
            .first()
            .and_then(|selected| selected.range.as_ref())
            .filter(|_| fetched == 1)
            .ok_or_else(|| {
                windows::core::Error::from_hresult(windows::Win32::Foundation::E_FAIL)
            })?;
        unsafe { range.Clone() }
    })();
    unsafe { ManuallyDrop::drop(&mut selection[0].range) };
    result
}

fn range_with_exact_suffix(range: &ITfRange, ec: u32, text: &str) -> Result<Option<ITfRange>> {
    let expected: Vec<u16> = text.encode_utf16().collect();
    let Ok(length) = i32::try_from(expected.len()) else {
        diagnostics::log("attach result=length_overflow");
        return Ok(None);
    };
    if length == 0 {
        diagnostics::log("attach result=empty_text");
        return Ok(None);
    }
    if !unsafe { range.IsEmpty(ec) }.is_ok_and(|empty| empty.as_bool()) {
        diagnostics::log(format!(
            "attach result=selection_not_collapsed units={length}"
        ));
        return Ok(None);
    }

    let candidate = unsafe { range.Clone()? };
    if let Ok(acp_range) = candidate.cast::<ITfRangeACP>() {
        let mut anchor = 0_i32;
        let mut current_length = 0_i32;
        if unsafe { acp_range.GetExtent(&mut anchor, &mut current_length) }.is_ok()
            && current_length == 0
        {
            if anchor < length {
                diagnostics::log(format!(
                    "attach result=acp_short units={length} anchor={anchor}"
                ));
                return Ok(None);
            }
            if unsafe { acp_range.SetExtent(anchor - length, length) }.is_ok() {
                diagnostics::log(format!("attach range=acp units={length}"));
                return match_exact_suffix(candidate, ec, &expected, length);
            }
            diagnostics::log(format!("attach range=acp_set_failed units={length}"));
        } else {
            diagnostics::log(format!("attach range=acp_extent_failed units={length}"));
        }
    }

    let requested = -length;
    let mut shifted = 0_i32;
    if unsafe { candidate.ShiftStart(ec, requested, &mut shifted, std::ptr::null()) }.is_err() {
        diagnostics::log(format!("attach result=shift_failed units={length}"));
        return Ok(None);
    }
    if shifted != requested {
        diagnostics::log(format!(
            "attach result=shift_short units={length} shifted={shifted}"
        ));
        return Ok(None);
    }

    diagnostics::log(format!("attach range=shift units={length}"));
    match_exact_suffix(candidate, ec, &expected, length)
}

fn match_exact_suffix(
    candidate: ITfRange,
    ec: u32,
    expected: &[u16],
    length: i32,
) -> Result<Option<ITfRange>> {
    let mut actual = vec![0_u16; expected.len().saturating_add(1)];
    let mut actual_len = 0_u32;
    if unsafe { candidate.GetText(ec, 0, &mut actual, &mut actual_len) }.is_err() {
        diagnostics::log(format!("attach result=read_failed units={length}"));
        return Ok(None);
    }
    if !utf16_text_matches(expected, &actual, actual_len as usize) {
        diagnostics::log(format!(
            "attach result=text_mismatch expected_units={length} actual_units={actual_len}"
        ));
        return Ok(None);
    }
    diagnostics::log(format!("attach result=matched units={length}"));
    Ok(Some(candidate))
}

fn utf16_text_matches(expected: &[u16], actual: &[u16], actual_len: usize) -> bool {
    actual_len == expected.len()
        && actual
            .get(..actual_len)
            .is_some_and(|value| value == expected)
}

#[cfg(test)]
mod tests {
    use super::{CompositionState, FinishCause, RemoteEditGuard, utf16_text_matches};

    #[test]
    fn external_edit_can_end_a_modified_composition() {
        assert!(FinishCause::Phone.validates_snapshot());
        assert!(!FinishCause::Forced.validates_snapshot());
    }

    #[test]
    fn only_external_composition_termination_marks_the_target_modified() {
        let external = CompositionState::default();
        external.composition_terminated();
        assert!(external.is_target_modified());

        let remote = CompositionState::default();
        {
            let _guard = RemoteEditGuard::new(&remote);
            remote.composition_terminated();
        }
        assert!(!remote.is_target_modified());
    }

    #[test]
    fn exact_suffix_matching_uses_the_complete_utf16_text() {
        let expected: Vec<u16> = "通天塔\n😀".encode_utf16().collect();
        let mut actual = expected.clone();
        actual.push('x' as u16);

        assert!(utf16_text_matches(&expected, &actual, expected.len()));
        assert!(!utf16_text_matches(&expected, &actual, expected.len() - 1));
        assert!(!utf16_text_matches(&expected, &actual, actual.len()));
    }
}

fn set_caret_to_composition_end(
    context: &ITfContext,
    ec: u32,
    range: &windows::Win32::UI::TextServices::ITfRange,
) -> Result<()> {
    let caret = unsafe { range.Clone()? };
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
        self.state.composition_terminated();
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
        if state.session_id().is_none()
            || state.is_applying_remote_edit()
            || state.is_terminated()
            || state.is_target_modified()
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
            let _ = request_async_force_finish(&context, self.client_id, state);
        }
        Ok(())
    }
}
