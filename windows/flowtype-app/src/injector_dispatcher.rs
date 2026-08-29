use std::sync::{Arc, Mutex};

use flowtype_core::ipc::{InjectorRequest, InjectorResponse};
use tokio::sync::Semaphore;

use crate::injector::InjectorClient;

pub struct InjectorDispatcher {
    client: Arc<Mutex<Option<InjectorClient>>>,
    slot: Arc<Semaphore>,
}

impl InjectorDispatcher {
    pub fn new(client: Option<InjectorClient>) -> Self {
        Self {
            client: Arc::new(Mutex::new(client)),
            slot: Arc::new(Semaphore::new(1)),
        }
    }

    pub fn is_ready(&self) -> bool {
        self.client
            .lock()
            .map(|client| client.is_some())
            .unwrap_or(false)
    }

    pub fn repair(&self) -> std::io::Result<()> {
        let repaired = InjectorClient::connect()?;
        *self
            .client
            .lock()
            .map_err(|_| std::io::Error::other("input service state unavailable"))? =
            Some(repaired);
        Ok(())
    }

    pub async fn request(
        &self,
        request: InjectorRequest,
    ) -> Result<InjectorResponse, InjectorRequestFailure> {
        let permit = Arc::clone(&self.slot)
            .acquire_owned()
            .await
            .map_err(|_| InjectorRequestFailure::Unavailable)?;
        let client = Arc::clone(&self.client);
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            request_blocking(&client, request)
        })
        .await
        .map_err(|_| InjectorRequestFailure::Unavailable)?
    }
}

#[derive(Clone, Copy)]
pub enum InjectorRequestFailure {
    Unavailable,
    RecoveryRequired,
}

fn request_blocking(
    client: &Mutex<Option<InjectorClient>>,
    request: InjectorRequest,
) -> Result<InjectorResponse, InjectorRequestFailure> {
    let mut injector = client
        .lock()
        .map_err(|_| InjectorRequestFailure::Unavailable)?;
    if injector.is_none() {
        *injector = InjectorClient::connect().ok();
    }
    let previous_instance = injector
        .as_ref()
        .map(|client| client.instance_id().to_owned())
        .ok_or(InjectorRequestFailure::Unavailable)?;
    if let Ok(response) = injector
        .as_mut()
        .ok_or(InjectorRequestFailure::Unavailable)?
        .request(request.clone())
    {
        return Ok(response);
    }

    *injector = None;
    let mut recovered =
        InjectorClient::connect().map_err(|_| InjectorRequestFailure::Unavailable)?;
    let same_instance = recovered.instance_id() == previous_instance;
    match reconcile_injector_request(&mut recovered, same_instance, &request) {
        Ok(response) => {
            *injector = Some(recovered);
            Ok(response)
        }
        Err(ReconcileFailure::Unavailable) => Err(InjectorRequestFailure::Unavailable),
        Err(ReconcileFailure::Unconfirmed) => {
            *injector = Some(recovered);
            Err(InjectorRequestFailure::RecoveryRequired)
        }
    }
}

#[derive(Clone, Copy)]
enum ReconcileFailure {
    Unavailable,
    Unconfirmed,
}

fn reconcile_injector_request(
    client: &mut InjectorClient,
    same_instance: bool,
    request: &InjectorRequest,
) -> Result<InjectorResponse, ReconcileFailure> {
    match request {
        InjectorRequest::Hello
        | InjectorRequest::BeginSession { .. }
        | InjectorRequest::ProbeTarget
        | InjectorRequest::CancelInvalidSession { .. } => client
            .request(request.clone())
            .map_err(|_| ReconcileFailure::Unavailable),
        InjectorRequest::ApplyState {
            session_id,
            sequence,
            full_text,
        } if same_instance => {
            let state = client
                .request(InjectorRequest::QuerySession {
                    session_id: session_id.clone(),
                })
                .map_err(|_| ReconcileFailure::Unavailable)?;
            match classify_apply_recovery(session_id, *sequence, full_text, &state) {
                ReconcileDecision::Applied => Ok(InjectorResponse::Applied {
                    sequence: *sequence,
                }),
                ReconcileDecision::Retry => client
                    .request(request.clone())
                    .map_err(|_| ReconcileFailure::Unavailable),
                ReconcileDecision::Finished => Ok(InjectorResponse::Finished {
                    sequence: *sequence,
                }),
                ReconcileDecision::Unknown => Err(ReconcileFailure::Unconfirmed),
            }
        }
        InjectorRequest::FinishSession {
            session_id,
            sequence,
        } if same_instance => {
            let state = client
                .request(InjectorRequest::QuerySession {
                    session_id: session_id.clone(),
                })
                .map_err(|_| ReconcileFailure::Unavailable)?;
            match classify_finish_recovery(session_id, *sequence, &state) {
                ReconcileDecision::Finished => Ok(InjectorResponse::Finished {
                    sequence: *sequence,
                }),
                ReconcileDecision::Retry => client
                    .request(request.clone())
                    .map_err(|_| ReconcileFailure::Unavailable),
                ReconcileDecision::Applied | ReconcileDecision::Unknown => {
                    Err(ReconcileFailure::Unconfirmed)
                }
            }
        }
        InjectorRequest::QuerySession { .. } if same_instance => client
            .request(request.clone())
            .map_err(|_| ReconcileFailure::Unavailable),
        _ => Err(ReconcileFailure::Unconfirmed),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReconcileDecision {
    Applied,
    Finished,
    Retry,
    Unknown,
}

pub(crate) fn classify_apply_recovery(
    session_id: &str,
    sequence: i64,
    full_text: &str,
    state: &InjectorResponse,
) -> ReconcileDecision {
    match state {
        InjectorResponse::SessionFinished {
            session_id: finished_session,
            sequence: finished_sequence,
            full_text: finished_text,
        } if finished_session == session_id
            && *finished_sequence == sequence
            && finished_text == full_text =>
        {
            ReconcileDecision::Finished
        }
        InjectorResponse::SessionActive {
            session_id: active_session,
            sequence: active_sequence,
            full_text: active_text,
        } if active_session == session_id
            && *active_sequence == sequence
            && active_text == full_text =>
        {
            ReconcileDecision::Applied
        }
        InjectorResponse::SessionActive {
            session_id: active_session,
            sequence: active_sequence,
            ..
        } if active_session == session_id && *active_sequence < sequence => {
            ReconcileDecision::Retry
        }
        _ => ReconcileDecision::Unknown,
    }
}

pub(crate) fn classify_finish_recovery(
    session_id: &str,
    sequence: i64,
    state: &InjectorResponse,
) -> ReconcileDecision {
    match state {
        InjectorResponse::SessionFinished {
            session_id: finished_session,
            sequence: finished_sequence,
            ..
        } if finished_session == session_id && *finished_sequence == sequence => {
            ReconcileDecision::Finished
        }
        InjectorResponse::SessionActive {
            session_id: active_session,
            sequence: active_sequence,
            ..
        } if active_session == session_id && *active_sequence == sequence => {
            ReconcileDecision::Retry
        }
        _ => ReconcileDecision::Unknown,
    }
}
