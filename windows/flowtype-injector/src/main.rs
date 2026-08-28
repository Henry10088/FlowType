#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod diagnostics;
mod inject;
mod input_monitor;
mod speech;
mod target;
mod tip_broker;

use std::fs::File;
use std::io;
use std::mem::size_of;
use std::os::windows::io::FromRawHandle;
use std::sync::Arc;
use std::time::Duration;

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
    GetTokenInformation, PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES, TOKEN_QUERY, TOKEN_USER,
    TokenUser,
};
use windows_sys::Win32::Storage::FileSystem::PIPE_ACCESS_DUPLEX;
use windows_sys::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, GetNamedPipeClientProcessId, PIPE_READMODE_BYTE,
    PIPE_TYPE_BYTE, PIPE_WAIT,
};
use windows_sys::Win32::System::Threading::{
    GetCurrentProcess, OpenProcess, OpenProcessToken, PROCESS_QUERY_LIMITED_INFORMATION,
    QueryFullProcessImageNameW,
};

use crate::target::TargetWindow;
use crate::tip_broker::{TipKey, TipRegistry};

struct ActiveSession {
    id: String,
    sequence: i64,
    text: String,
    target: TargetWindow,
    mode: InputMode,
    input_epoch: u64,
}

enum InputMode {
    Tsf(TipKey),
    Legacy,
}

fn main() -> io::Result<()> {
    diagnostics::log("startup");
    diagnostics::log(format!("input_monitor ready={}", input_monitor::start()));
    speech::initialize_com().map_err(io::Error::other)?;
    let tips = TipRegistry::start();
    speech::ensure_flowtype_active().map_err(io::Error::other)?;
    let pipe_sddl = pipe_security_sddl()?;
    let mut session = None;
    loop {
        let mut pipe = match accept_pipe(&pipe_sddl) {
            Ok(pipe) => pipe,
            Err(error) if error.kind() == io::ErrorKind::PermissionDenied => continue,
            Err(error) => return Err(error),
        };
        while let Ok(request) = read_message(&mut pipe) {
            let response = handle_request(request, &mut session, &tips);
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
    tips: &Arc<TipRegistry>,
) -> InjectorResponse {
    match request {
        InjectorRequest::BeginSession { session_id } => {
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
            let mode = match tips.begin_for_target(
                target.process_id(),
                target.thread_id(),
                &session_id,
                Duration::from_millis(250),
            ) {
                Ok(tip) => {
                    diagnostics::log(format!(
                        "begin mode=tsf pid={} thread={}",
                        tip.process_id, tip.thread_id
                    ));
                    InputMode::Tsf(tip)
                }
                Err(error) => {
                    diagnostics::log(format!("begin mode=legacy_unicode_input reason={error}"));
                    InputMode::Legacy
                }
            };
            let target_name = target.title();
            *session = Some(ActiveSession {
                id: session_id,
                sequence: 0,
                text: String::new(),
                target,
                mode,
                input_epoch: input_monitor::epoch(),
            });
            diagnostics::log("begin accepted");
            InjectorResponse::SessionBegun { target_name }
        }
        InjectorRequest::ApplyState {
            session_id,
            sequence,
            full_text,
        } => apply_state(session, tips, &session_id, sequence, full_text),
        InjectorRequest::FinishSession {
            session_id,
            sequence,
        } => finish_session(session, tips, &session_id, sequence),
        InjectorRequest::QueryStatus => InjectorResponse::Ready,
        InjectorRequest::QueryIdentity => InjectorResponse::Identity {
            protocol_version: flowtype_core::PROTOCOL_VERSION,
            executable_path: std::env::current_exe()
                .map(|path| path.to_string_lossy().into_owned())
                .unwrap_or_default(),
        },
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
                && let InputMode::Tsf(tip) = active.mode
            {
                let _ = tips.send(
                    tip,
                    TipCommand::Cancel {
                        session_id: session_id.clone(),
                    },
                );
            }
            InjectorResponse::Cancelled
        }
    }
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
    if matches!(&active.mode, InputMode::Legacy) {
        if !active.target.activate_for_input() {
            let target_name = active.target.title();
            diagnostics::log(format!(
                "update pid={} seq={} text_len={} mode=legacy result=target_activation_failed",
                active.target.process_id(),
                sequence,
                text_len
            ));
            end_failed_session(session, tips);
            return InjectorResponse::TargetNotForeground { target_name };
        }
        let result = inject::replace_text(&active.text, &full_text);
        diagnostics::log(format!(
            "update pid={} seq={} text_len={} mode=legacy result={:?}",
            active.target.process_id(),
            sequence,
            text_len,
            result
                .as_ref()
                .map(|_| "ok")
                .map_err(|error| error.to_string())
        ));
        if result.is_err() {
            end_failed_session(session, tips);
            return InjectorResponse::InjectionUnknown;
        }
        active.sequence = sequence;
        active.text = full_text;
        return InjectorResponse::Applied { sequence };
    }
    let InputMode::Tsf(tip) = &active.mode else {
        unreachable!("legacy mode returned above");
    };
    let response = tips.send(
        *tip,
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
    if matches!(&active.mode, InputMode::Legacy) {
        let target_pid = active.target.process_id();
        session.take();
        diagnostics::log(format!(
            "finish pid={} seq={} mode=legacy result=ok",
            target_pid, sequence
        ));
        return InjectorResponse::Finished { sequence };
    }
    let InputMode::Tsf(tip) = &active.mode else {
        unreachable!("legacy mode returned above");
    };
    let response = tips.send(
        *tip,
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
    if let Some(active) = session.take()
        && let InputMode::Tsf(tip) = active.mode
    {
        let _ = tips.send(
            tip,
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

    use super::{speech, target::TargetWindow, tip_broker::TipRegistry};

    #[test]
    #[ignore = "requires a registered FlowType TIP and a focused Notepad edit control"]
    fn notepad_composition_smoke() {
        speech::initialize_com().unwrap();
        let target = TargetWindow::capture_foreground().expect("no foreground window");
        assert!(
            target.title().to_ascii_lowercase().contains("notepad"),
            "foreground window must be Notepad"
        );
        let tips = TipRegistry::start();
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
