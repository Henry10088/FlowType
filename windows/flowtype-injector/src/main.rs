#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod diagnostics;
mod input_monitor;
mod speech;
mod target;
mod tip_broker;

use std::fs::File;
use std::io;
use std::mem::size_of;
use std::os::windows::io::FromRawHandle;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use flowtype_core::ipc::{
    InjectorRequest, InjectorResponse, PIPE_NAME, read_message, write_message,
};
use flowtype_core::tip::{TipCommand, TipResponse};
use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_PIPE_CONNECTED, GetLastError, HANDLE, INVALID_HANDLE_VALUE, LocalFree,
};
use windows_sys::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows_sys::Win32::Security::{
    GetTokenInformation, PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES, TOKEN_ELEVATION, TOKEN_QUERY,
    TOKEN_USER, TokenElevation, TokenUser,
};
use windows_sys::Win32::Storage::FileSystem::PIPE_ACCESS_DUPLEX;
use windows_sys::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, GetNamedPipeClientProcessId, PIPE_READMODE_BYTE,
    PIPE_TYPE_BYTE, PIPE_WAIT,
};
use windows_sys::Win32::System::Threading::{
    GetCurrentProcess, GetCurrentProcessId, OpenProcess, OpenProcessToken,
    PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};

use crate::target::TargetWindow;
use crate::tip_broker::{TipBeginError, TipKey, TipRegistry};

struct ActiveSession {
    id: String,
    sequence: i64,
    text: String,
    target: TargetWindow,
    tip: TipKey,
    input_epoch: u64,
}

struct CompletedSession {
    id: String,
    sequence: i64,
    text: String,
}

fn main() -> io::Result<()> {
    diagnostics::log("startup");
    diagnostics::log(format!("input_monitor ready={}", input_monitor::start()));
    if let Err(error) = speech::initialize_com() {
        diagnostics::log(format!("speech com init failed: {error:?}"));
        return Err(io::Error::other(error));
    }
    let pipe_sddl = pipe_security_sddl()?;
    let tips = TipRegistry::start(pipe_sddl.clone());
    if let Err(error) = speech::ensure_flowtype_active() {
        diagnostics::log(format!("speech profile activation failed: {error:?}"));
    }
    let instance_id = service_instance_id();
    let elevated = is_elevated()?;
    diagnostics::log(format!(
        "service instance={instance_id} elevated={elevated} ipc_version={}",
        flowtype_core::INJECTOR_IPC_VERSION
    ));
    let mut session = None;
    let mut completed = None;
    loop {
        let mut pipe = match accept_pipe(&pipe_sddl) {
            Ok(pipe) => pipe,
            Err(error) if error.kind() == io::ErrorKind::PermissionDenied => continue,
            Err(error) => return Err(error),
        };
        while let Ok(request) = read_message(&mut pipe) {
            let response = handle_request(
                request,
                &mut session,
                &mut completed,
                &tips,
                &instance_id,
                elevated,
            );
            if write_message(&mut pipe, &response).is_err() {
                break;
            }
        }
    }
}

fn accept_pipe(sddl: &[u16]) -> io::Result<File> {
    let name: Vec<u16> = PIPE_NAME.encode_utf16().chain(Some(0)).collect();
    let mut descriptor: PSECURITY_DESCRIPTOR = std::ptr::null_mut();
    let converted = unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            sddl.as_ptr(),
            SDDL_REVISION_1,
            &mut descriptor,
            std::ptr::null_mut(),
        )
    };
    if converted == 0 {
        return Err(io::Error::last_os_error());
    }
    let attributes = SECURITY_ATTRIBUTES {
        nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: descriptor,
        bInheritHandle: 0,
    };
    let handle = unsafe {
        CreateNamedPipeW(
            name.as_ptr(),
            PIPE_ACCESS_DUPLEX,
            PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT,
            1,
            (flowtype_core::MAX_MESSAGE_BYTES + 4) as u32,
            (flowtype_core::MAX_MESSAGE_BYTES + 4) as u32,
            0,
            &attributes,
        )
    };
    unsafe { LocalFree(descriptor) };
    if handle == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    let connected = unsafe { ConnectNamedPipe(handle, std::ptr::null_mut()) };
    if connected == 0 && unsafe { GetLastError() } != ERROR_PIPE_CONNECTED {
        unsafe { CloseHandle(handle) };
        return Err(io::Error::last_os_error());
    }
    if !is_expected_client(handle)? {
        unsafe { CloseHandle(handle) };
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "unexpected pipe client",
        ));
    }
    Ok(unsafe { File::from_raw_handle(handle) })
}

fn pipe_security_sddl() -> io::Result<Vec<u16>> {
    let mut token = std::ptr::null_mut();
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
        return Err(io::Error::last_os_error());
    }

    let result = (|| {
        let mut length = 0_u32;
        unsafe {
            GetTokenInformation(token, TokenUser, std::ptr::null_mut(), 0, &mut length);
        }
        if length < size_of::<TOKEN_USER>() as u32 {
            return Err(io::Error::last_os_error());
        }

        let mut buffer = vec![0_u8; length as usize];
        if unsafe {
            GetTokenInformation(
                token,
                TokenUser,
                buffer.as_mut_ptr().cast(),
                length,
                &mut length,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        let user = unsafe { std::ptr::read_unaligned(buffer.as_ptr().cast::<TOKEN_USER>()) };
        let mut sid_text = std::ptr::null_mut();
        if unsafe { ConvertSidToStringSidW(user.User.Sid, &mut sid_text) } == 0 {
            return Err(io::Error::last_os_error());
        }
        let sid = unsafe {
            let mut length = 0;
            while *sid_text.add(length) != 0 {
                length += 1;
            }
            String::from_utf16_lossy(std::slice::from_raw_parts(sid_text, length))
        };
        unsafe { LocalFree(sid_text.cast()) };
        Ok(format!("D:P(A;;GA;;;{sid})(A;;GA;;;SY)(A;;GA;;;BA)")
            .encode_utf16()
            .chain(Some(0))
            .collect())
    })();
    unsafe { CloseHandle(token) };
    result
}

fn is_expected_client(pipe: HANDLE) -> io::Result<bool> {
    let mut process_id = 0_u32;
    if unsafe { GetNamedPipeClientProcessId(pipe, &mut process_id) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, process_id) };
    if process.is_null() {
        return Err(io::Error::last_os_error());
    }
    let mut path = vec![0_u16; 32_768];
    let mut length = path.len() as u32;
    let queried = unsafe { QueryFullProcessImageNameW(process, 0, path.as_mut_ptr(), &mut length) };
    unsafe { CloseHandle(process) };
    if queried == 0 {
        return Err(io::Error::last_os_error());
    }
    let actual = String::from_utf16_lossy(&path[..length as usize]);
    let expected = std::env::current_exe()?
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "input service path has no parent"))?
        .join("flowtype.exe");
    Ok(actual.eq_ignore_ascii_case(&expected.to_string_lossy()))
}

fn handle_request(
    request: InjectorRequest,
    session: &mut Option<ActiveSession>,
    completed: &mut Option<CompletedSession>,
    tips: &Arc<TipRegistry>,
    instance_id: &str,
    elevated: bool,
) -> InjectorResponse {
    match request {
        InjectorRequest::Hello => InjectorResponse::Hello {
            ipc_version: flowtype_core::INJECTOR_IPC_VERSION,
            instance_id: instance_id.to_owned(),
            executable_path: std::env::current_exe()
                .map(|path| path.to_string_lossy().into_owned())
                .unwrap_or_default(),
            elevated,
        },
        InjectorRequest::BeginSession { session_id } => {
            if let Some(active) = session.as_ref().filter(|active| active.id == session_id) {
                diagnostics::log(format!(
                    "begin existing=active pid={} seq={}",
                    active.target.process_id(),
                    active.sequence
                ));
                return InjectorResponse::SessionBegun {
                    target_name: active.target.title(),
                };
            }
            if let Some(finished) = completed
                .as_ref()
                .filter(|finished| finished.id == session_id)
            {
                diagnostics::log(format!("begin existing=finished seq={}", finished.sequence));
                return InjectorResponse::SessionFinished {
                    session_id,
                    sequence: finished.sequence,
                    full_text: finished.text.clone(),
                };
            }
            if session.is_some() {
                // A new START is the recovery boundary after a lost socket or
                // a rejected resume. Do not let an orphaned injector session
                // make every later attempt fail with session_busy.
                diagnostics::log("begin replacing=stale_session");
                end_failed_session(session, tips);
            }
            let Some(target) = TargetWindow::capture_foreground() else {
                diagnostics::log("begin rejected=target_invalid");
                return InjectorResponse::TargetInvalid;
            };
            if target.is_remote_desktop_client() {
                diagnostics::log("begin rejected=remote_desktop_client");
                return InjectorResponse::TargetUnsupported;
            }
            if target.is_flowtype_window() {
                diagnostics::log("begin rejected=flowtype_window");
                return InjectorResponse::TargetUnsupported;
            }
            if !target.activate_for_input() {
                diagnostics::log(format!(
                    "begin rejected=target_activation_failed pid={} thread={}",
                    target.process_id(),
                    target.thread_id()
                ));
                return InjectorResponse::TargetNotForeground {
                    target_name: target.title(),
                };
            }
            diagnostics::log(format!(
                "begin target_pid={} target_thread={} title_len={}",
                target.process_id(),
                target.thread_id(),
                target.title().chars().count()
            ));
            let tip = match tips.begin_for_target(
                target.process_id(),
                target.thread_id(),
                &session_id,
                Duration::from_millis(1_500),
            ) {
                Ok(tip) => {
                    diagnostics::log(format!(
                        "begin mode=tsf pid={} thread={}",
                        tip.process_id, tip.thread_id
                    ));
                    tip
                }
                Err(TipBeginError::Unsupported) => {
                    diagnostics::log("begin rejected=target_tsf_unsupported");
                    return InjectorResponse::TargetUnsupported;
                }
                Err(TipBeginError::RebindRejected) => {
                    diagnostics::log("begin rejected=unsafe_rebind");
                    return InjectorResponse::TargetModified;
                }
                Err(TipBeginError::Unavailable) => {
                    diagnostics::log("begin rejected=tsf_unavailable");
                    return InjectorResponse::TsfUnavailable;
                }
            };
            let target_name = target.title();
            *completed = None;
            *session = Some(ActiveSession {
                id: session_id,
                sequence: 0,
                text: String::new(),
                target,
                tip,
                input_epoch: input_monitor::epoch(),
            });
            diagnostics::log("begin accepted");
            InjectorResponse::SessionBegun { target_name }
        }
        InjectorRequest::ApplyState {
            session_id,
            sequence,
            full_text,
        } => completed_apply_response(completed.as_ref(), &session_id, sequence, &full_text)
            .unwrap_or_else(|| apply_state(session, tips, &session_id, sequence, full_text)),
        InjectorRequest::FinishSession {
            session_id,
            sequence,
        } => {
            let finished_text = session
                .as_ref()
                .filter(|active| active.id == session_id && active.sequence == sequence)
                .map(|active| active.text.clone());
            let response = finish_session(session, tips, &session_id, sequence);
            if let (InjectorResponse::Finished { sequence }, Some(text)) =
                (&response, finished_text)
            {
                *completed = Some(CompletedSession {
                    id: session_id,
                    sequence: *sequence,
                    text,
                });
            }
            response
        }
        InjectorRequest::QuerySession { session_id } => {
            if let Some(active) = session.as_ref().filter(|active| active.id == session_id) {
                let tip = active.tip;
                let input_epoch = active.input_epoch;
                let sequence = active.sequence;
                let full_text = active.text.clone();
                query_active_session(
                    session,
                    tips,
                    tip,
                    input_epoch,
                    sequence,
                    full_text,
                    &session_id,
                )
            } else if let Some(finished) = completed
                .as_ref()
                .filter(|finished| finished.id == session_id)
            {
                InjectorResponse::SessionFinished {
                    session_id,
                    sequence: finished.sequence,
                    full_text: finished.text.clone(),
                }
            } else {
                InjectorResponse::SessionMissing
            }
        }
        InjectorRequest::ProbeTarget => {
            let Some(target) = TargetWindow::capture_foreground() else {
                return InjectorResponse::TargetInvalid;
            };
            if !target.is_valid() {
                return InjectorResponse::TargetInvalid;
            }
            if target.is_remote_desktop_client() {
                return InjectorResponse::TargetUnsupported;
            }
            if target.is_flowtype_window() {
                return InjectorResponse::TargetUnsupported;
            }
            InjectorResponse::TargetReady {
                target_name: target.title(),
                activity_age_ms: target.activity_age_ms(),
            }
        }
        InjectorRequest::CancelInvalidSession { session_id } => {
            if session
                .as_ref()
                .is_some_and(|active| active.id == session_id)
                && let Some(active) = session.take()
            {
                let _ = tips.send(
                    active.tip,
                    TipCommand::Cancel {
                        session_id: session_id.clone(),
                    },
                );
            }
            InjectorResponse::Cancelled
        }
    }
}

fn query_active_session(
    session: &mut Option<ActiveSession>,
    tips: &TipRegistry,
    tip: TipKey,
    input_epoch: u64,
    sequence: i64,
    full_text: String,
    session_id: &str,
) -> InjectorResponse {
    // A physical key/mouse event can terminate a TSF composition without a
    // phone packet arriving. Query the TIP so the desktop can notify the
    // phone immediately instead of replaying the old full text later.
    if input_epoch != input_monitor::epoch() {
        let submitted = is_submitted_candidate(
            full_text.as_str(),
            input_epoch,
            input_monitor::epoch(),
            input_monitor::last_event_was_return(),
        );
        if submitted {
            end_failed_session(session, tips);
        } else {
            detach_session(session);
        }
        return if submitted {
            InjectorResponse::TargetSubmitted
        } else {
            InjectorResponse::TargetModified
        };
    }
    match tips.send(
        tip,
        TipCommand::Query {
            session_id: session_id.to_owned(),
        },
    ) {
        Ok(TipResponse::SessionActive { .. }) => InjectorResponse::SessionActive {
            session_id: session_id.to_owned(),
            sequence,
            full_text,
        },
        Ok(TipResponse::CompositionTerminated) => {
            let submitted = is_submitted_candidate(
                full_text.as_str(),
                input_epoch,
                input_monitor::epoch(),
                input_monitor::last_event_was_return(),
            );
            if submitted {
                end_failed_session(session, tips);
            } else {
                detach_session(session);
            }
            if submitted {
                InjectorResponse::TargetSubmitted
            } else {
                InjectorResponse::TargetModified
            }
        }
        _ => {
            end_failed_session(session, tips);
            InjectorResponse::TargetModified
        }
    }
}

fn is_submitted_candidate(
    full_text: &str,
    session_input_epoch: u64,
    current_input_epoch: u64,
    last_event_was_return: bool,
) -> bool {
    full_text.ends_with('\n') && current_input_epoch != session_input_epoch && last_event_was_return
}

fn detach_session(session: &mut Option<ActiveSession>) {
    // Keep the TIP's terminated composition range available for a verified
    // rebind. The injector session itself is no longer accepted for updates.
    session.take();
}

fn completed_apply_response(
    completed: Option<&CompletedSession>,
    session_id: &str,
    sequence: i64,
    full_text: &str,
) -> Option<InjectorResponse> {
    let finished = completed.filter(|finished| finished.id == session_id)?;
    Some(
        if finished.sequence == sequence && finished.text == full_text {
            InjectorResponse::Finished { sequence }
        } else {
            InjectorResponse::InvalidRequest
        },
    )
}

fn service_instance_id() -> String {
    let started = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    format!("{}-{started}", unsafe { GetCurrentProcessId() })
}

fn is_elevated() -> io::Result<bool> {
    let mut token = std::ptr::null_mut();
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let result = (|| {
        let mut elevation = TOKEN_ELEVATION::default();
        let mut length = 0_u32;
        if unsafe {
            GetTokenInformation(
                token,
                TokenElevation,
                (&mut elevation as *mut TOKEN_ELEVATION).cast(),
                size_of::<TOKEN_ELEVATION>() as u32,
                &mut length,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(elevation.TokenIsElevated != 0)
    })();
    unsafe { CloseHandle(token) };
    result
}

fn apply_state(
    session: &mut Option<ActiveSession>,
    tips: &TipRegistry,
    session_id: &str,
    sequence: i64,
    full_text: String,
) -> InjectorResponse {
    let Some(active) = session.as_mut().filter(|active| active.id == session_id) else {
        return InjectorResponse::InvalidRequest;
    };
    if !active.target.is_valid() {
        return InjectorResponse::TargetInvalid;
    }
    if active.input_epoch != input_monitor::epoch() {
        diagnostics::log(format!(
            "update pid={} seq={} rejected=physical_input",
            active.target.process_id(),
            sequence
        ));
        end_failed_session(session, tips);
        return InjectorResponse::TargetModified;
    }
    if !active.target.is_foreground() {
        return InjectorResponse::TargetNotForeground {
            target_name: active.target.title(),
        };
    }
    if sequence < active.sequence {
        return InjectorResponse::Applied {
            sequence: active.sequence,
        };
    }
    if sequence == active.sequence {
        return if full_text == active.text {
            InjectorResponse::Applied { sequence }
        } else {
            diagnostics::log(format!(
                "update pid={} seq={} rejected=sequence_conflict",
                active.target.process_id(),
                sequence
            ));
            end_failed_session(session, tips);
            InjectorResponse::InvalidRequest
        };
    }

    let text_len = full_text.chars().count();
    let tip = active.tip;
    let response = tips.send(
        tip,
        TipCommand::Update {
            session_id: session_id.to_owned(),
            sequence,
            full_text: full_text.clone(),
        },
    );
    diagnostics::log(format!(
        "update pid={} thread={} seq={} text_len={} result={:?}",
        tip.process_id,
        tip.thread_id,
        sequence,
        text_len,
        response.as_ref().map_err(|error| error.to_string())
    ));
    match response {
        Ok(TipResponse::Applied {
            session_id: applied_session,
            sequence: applied_sequence,
        }) if applied_session == session_id => {
            if applied_sequence == sequence {
                active.sequence = sequence;
                active.text = full_text;
            }
            InjectorResponse::Applied {
                sequence: applied_sequence,
            }
        }
        Ok(TipResponse::NoFocus) => InjectorResponse::TargetNotForeground {
            target_name: active.target.title(),
        },
        Ok(TipResponse::CompositionTerminated) => {
            end_failed_session(session, tips);
            InjectorResponse::TargetModified
        }
        Ok(TipResponse::SessionMismatch | TipResponse::SequenceConflict) => {
            // The TIP and injector no longer agree on the active snapshot.
            // Retaining the local session would make the next START collide
            // with the same stale state, so force both layers back to idle.
            end_failed_session(session, tips);
            InjectorResponse::InvalidRequest
        }
        _ => {
            end_failed_session(session, tips);
            InjectorResponse::InjectionUnknown
        }
    }
}

fn finish_session(
    session: &mut Option<ActiveSession>,
    tips: &TipRegistry,
    session_id: &str,
    sequence: i64,
) -> InjectorResponse {
    let Some(active) = session.as_ref() else {
        return InjectorResponse::InvalidRequest;
    };
    if active.id != session_id || active.sequence != sequence {
        return InjectorResponse::InvalidRequest;
    }
    let tip = active.tip;
    let response = tips.send(
        tip,
        TipCommand::Finish {
            session_id: session_id.to_owned(),
            sequence,
        },
    );
    diagnostics::log(format!(
        "finish pid={} thread={} seq={} result={:?}",
        tip.process_id,
        tip.thread_id,
        sequence,
        response.as_ref().map_err(|error| error.to_string())
    ));
    match response {
        Ok(TipResponse::Finished {
            session_id: finished_session,
            sequence: finished_sequence,
        }) if finished_session == session_id && finished_sequence == sequence => {
            session.take();
            InjectorResponse::Finished { sequence }
        }
        Ok(TipResponse::SessionMismatch | TipResponse::SequenceConflict) => {
            end_failed_session(session, tips);
            InjectorResponse::InvalidRequest
        }
        _ => {
            end_failed_session(session, tips);
            InjectorResponse::InjectionUnknown
        }
    }
}

fn end_failed_session(session: &mut Option<ActiveSession>, tips: &TipRegistry) {
    if let Some(active) = session.take() {
        let _ = tips.send(
            active.tip,
            TipCommand::Cancel {
                session_id: active.id,
            },
        );
    }
}

#[cfg(test)]
mod integration_tests {
    use std::time::Duration;

    use flowtype_core::tip::{TipCommand, TipResponse};

    use super::{
        CompletedSession, completed_apply_response, is_submitted_candidate, pipe_security_sddl,
        speech, target::TargetWindow, tip_broker::TipRegistry,
    };

    #[test]
    fn only_a_newline_snapshot_after_a_real_return_is_submitted() {
        assert!(is_submitted_candidate("已完成\n", 10, 11, true));
        assert!(!is_submitted_candidate("已完成", 10, 11, true));
        assert!(!is_submitted_candidate("已完成\n", 10, 11, false));
        assert!(!is_submitted_candidate("已完成\n", 10, 10, true));
    }

    #[test]
    fn completed_session_replays_only_the_exact_final_snapshot() {
        let completed = CompletedSession {
            id: "voice".to_owned(),
            sequence: 8,
            text: "最终正文".to_owned(),
        };

        assert_eq!(
            completed_apply_response(Some(&completed), "voice", 8, "最终正文"),
            Some(flowtype_core::ipc::InjectorResponse::Finished { sequence: 8 }),
        );
        assert_eq!(
            completed_apply_response(Some(&completed), "voice", 8, "不同正文"),
            Some(flowtype_core::ipc::InjectorResponse::InvalidRequest),
        );
        assert_eq!(
            completed_apply_response(Some(&completed), "other", 8, "最终正文"),
            None,
        );
    }

    #[test]
    #[ignore = "requires a registered FlowType TIP and a focused Notepad edit control"]
    fn notepad_composition_smoke() {
        speech::initialize_com().unwrap();
        let target = TargetWindow::capture_foreground().expect("no foreground window");
        assert!(
            target.title().to_ascii_lowercase().contains("notepad"),
            "foreground window must be Notepad"
        );
        let tips = TipRegistry::start(pipe_security_sddl().unwrap());
        let keyboard_before = speech::active_keyboard_profile().unwrap();
        let target_keyboard_before = speech::thread_keyboard_layout(target.thread_id());
        speech::ensure_flowtype_active().unwrap();
        assert_eq!(speech::active_keyboard_profile().unwrap(), keyboard_before);
        assert_eq!(
            speech::thread_keyboard_layout(target.thread_id()),
            target_keyboard_before
        );
        for round in 1..=3 {
            let session_id = format!("flowtype-notepad-smoke-{round}");
            let tip = tips
                .begin_for_target(
                    target.process_id(),
                    target.thread_id(),
                    &session_id,
                    Duration::from_secs(5),
                )
                .unwrap();

            for (sequence, text) in [
                (1, format!("voice draft {round}")),
                (2, format!("voice corrected {round}")),
                (3, format!("TSF 中文最终稿 {round}")),
            ] {
                assert_eq!(
                    tips.send(
                        tip,
                        TipCommand::Update {
                            session_id: session_id.clone(),
                            sequence,
                            full_text: text,
                        },
                    )
                    .unwrap(),
                    TipResponse::Applied {
                        session_id: session_id.clone(),
                        sequence,
                    }
                );
            }
            assert_eq!(
                tips.send(
                    tip,
                    TipCommand::Finish {
                        session_id: session_id.clone(),
                        sequence: 3,
                    },
                )
                .unwrap(),
                TipResponse::Finished {
                    session_id,
                    sequence: 3,
                }
            );
            assert_eq!(speech::active_keyboard_profile().unwrap(), keyboard_before);
            assert_eq!(
                speech::thread_keyboard_layout(target.thread_id()),
                target_keyboard_before
            );
        }
    }

    #[test]
    #[ignore = "inspects the interactive Windows input profile"]
    fn speech_startup_is_idempotent_and_preserves_the_keyboard_profile() {
        speech::initialize_com().unwrap();
        let keyboard_before = speech::active_keyboard_profile().unwrap();
        speech::ensure_flowtype_active().unwrap();
        assert_eq!(speech::active_keyboard_profile().unwrap(), keyboard_before);
        speech::ensure_flowtype_active().unwrap();
        assert_eq!(speech::active_keyboard_profile().unwrap(), keyboard_before);
    }
}
