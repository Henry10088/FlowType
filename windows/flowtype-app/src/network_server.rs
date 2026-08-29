use super::*;

pub(super) async fn run_network(
    state: Arc<AppState>,
    endpoint_host: IpAddr,
) -> Result<(), Box<dyn std::error::Error>> {
    let tls = tls_acceptor(&state.identity)?;
    let _mdns = advertise(&state.identity, endpoint_host)?;
    let listener = TcpListener::bind((Ipv4Addr::UNSPECIFIED, PORT)).await?;
    let connection_slots = Arc::new(Semaphore::new(MAX_CONNECTIONS));
    let connection_limiter = ConnectionLimiter::new();
    loop {
        let (stream, peer) = listener.accept().await?;
        let Ok(connection_slot) = Arc::clone(&connection_slots).try_acquire_owned() else {
            continue;
        };
        let Some(peer_slot) = connection_limiter.try_acquire(peer.ip()) else {
            continue;
        };
        let state = Arc::clone(&state);
        let tls = tls.clone();
        tokio::spawn(async move {
            let _connection_slot = connection_slot;
            let _peer_slot = peer_slot;
            if let Err(error) = serve_connection(stream, tls, state).await {
                eprintln!("connection ended: {error}");
            }
        });
    }
}

fn advertise(
    identity: &PcIdentity,
    address: IpAddr,
) -> Result<ServiceDaemon, Box<dyn std::error::Error>> {
    let daemon = ServiceDaemon::new()?;
    let short_id = identity.pc_id.chars().take(8).collect::<String>();
    let host = format!("flowtype-{short_id}.local.");
    let properties = [
        ("pc_id", identity.pc_id.as_str()),
        ("protocol_version", "1"),
    ];
    let service = ServiceInfo::new(
        "_flowtype._tcp.local.",
        &identity.pc_id,
        &host,
        address,
        PORT,
        &properties[..],
    )?;
    daemon.register(service)?;
    Ok(daemon)
}

async fn serve_connection(
    stream: TcpStream,
    tls: TlsAcceptor,
    state: Arc<AppState>,
) -> Result<(), Box<dyn std::error::Error>> {
    let stream = tokio::time::timeout(CONNECTION_TIMEOUT, tls.accept(stream))
        .await
        .map_err(|_| "TLS handshake timed out")??;
    let websocket_config = WebSocketConfig::default()
        .read_buffer_size(16 * 1024)
        .write_buffer_size(16 * 1024)
        .max_write_buffer_size(2 * flowtype_core::MAX_MESSAGE_BYTES)
        .max_message_size(Some(MAX_IMAGE_BYTES))
        .max_frame_size(Some(MAX_IMAGE_BYTES));
    let mut websocket = tokio::time::timeout(
        CONNECTION_TIMEOUT,
        tokio_tungstenite::accept_async_with_config(stream, Some(websocket_config)),
    )
    .await
    .map_err(|_| "WebSocket handshake timed out")??;
    let nonce = random_token();
    let auth = tokio::time::timeout(CONNECTION_TIMEOUT, async {
        send_json(
            &mut websocket,
            &ChallengeMessage {
                protocol_version: flowtype_core::PROTOCOL_VERSION,
                message_type: "challenge",
                pc_id: &state.identity.pc_id,
                nonce: &nonce,
            },
        )
        .await?;
        next_text(&mut websocket).await
    })
    .await
    .map_err(|_| "authentication timed out")??;
    if auth.len() > MAX_AUTH_MESSAGE_BYTES {
        return Err("authentication message too large".into());
    }
    let auth: AuthMessage = serde_json::from_str(&auth)?;
    if authenticate_phone_async(&state, auth.clone(), nonce.clone())
        .await
        .is_err()
    {
        send_json(
            &mut websocket,
            &ServerMessage::Error(ProtocolError {
                protocol_version: flowtype_core::PROTOCOL_VERSION,
                code: ErrorCode::AuthFailed,
                session_id: None,
            }),
        )
        .await?;
        return Err("authentication failed".into());
    }
    let connection_id = state.next_connection_id.fetch_add(1, Ordering::Relaxed);
    let pc_name = state
        .pc_name
        .lock()
        .map_err(|_| "computer name unavailable")?
        .clone();
    send_json(
        &mut websocket,
        &ReadyMessage {
            protocol_version: flowtype_core::PROTOCOL_VERSION,
            message_type: "ready",
            pc_id: &state.identity.pc_id,
            pc_name: &pc_name,
            candidate_endpoints: endpoint_urls(None),
            capabilities: &["health_check", "switch_ack"],
        },
    )
    .await?;
    let is_control = auth.connection_mode.as_deref() == Some("control");
    let _online_lease = (!is_control).then(|| {
        state.mark_online_connection(&auth.phone_id, &auth.phone_name, connection_id);
        OnlineConnectionLease {
            state: Arc::clone(&state),
            connection_id,
        }
    });
    let (switch_tx, mut switch_rx) = channel(4);
    state.register_switch_channel(
        connection_id,
        switch_tx,
        is_control,
        auth.capabilities.iter().any(|value| value == "switch_ack"),
    );
    let _switch_lease = SwitchChannelLease {
        state: Arc::clone(&state),
        connection_id,
    };
    // Authentication is also used by short-lived target probes. Do not claim
    // the single input connection until the client sends a real input message.
    let mut active_lease: Option<ActiveConnectionLease> = None;
    let mut pending_image: Option<ImageStart> = None;
    let mut monitored_session: Option<String> = None;
    let mut target_poll = tokio::time::interval(Duration::from_millis(120));
    target_poll.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            _ = target_poll.tick(), if monitored_session.is_some() && active_lease.is_some() => {
                let Some(session_id) = monitored_session.clone() else { continue; };
                match injector_request_async(&state, InjectorRequest::QuerySession { session_id: session_id.clone() }).await {
                    Ok(InjectorResponse::SessionActive { .. }) => {}
                    Ok(response) => {
                        monitored_session = None;
                        send_injector_state(&mut websocket, &state, &session_id, response).await?;
                    }
                    Err(failure) => {
                        monitored_session = None;
                        send_injector_failure(&mut websocket, &session_id, failure).await?;
                    }
                }
            }
            Some(request) = switch_rx.recv() => {
                send_json(
                    &mut websocket,
                    &ServerMessage::SwitchComputer(SwitchComputer {
                        protocol_version: flowtype_core::PROTOCOL_VERSION,
                        pc_id: request.pc_id,
                        pc_name: request.pc_name,
                        request_id: request.request_id,
                    }),
                ).await?;
            }
            inbound = websocket.next() => {
                let Some(message) = inbound else { break; };
                match message? {
            Message::Text(text) => {
                if text.len() > flowtype_core::MAX_MESSAGE_BYTES {
                    return Err("message too large".into());
                }
                let value: serde_json::Value = serde_json::from_str(&text)?;
                let message_type = value.get("type").and_then(serde_json::Value::as_str);
                let is_probe = message_type == Some("probe");
                let is_control_message = matches!(
                    message_type,
                    Some("probe" | "health_check" | "switch_ack")
                );
                if is_probe {
                    state.mark_probe_connection(connection_id);
                }
                if !is_control_message && active_lease.is_none() {
                    active_lease = Some(claim_active_connection(
                        &state,
                        &auth.phone_id,
                        &auth.phone_name,
                        connection_id,
                    )?);
                }
                if active_lease.is_some()
                    && !is_active_connection(&state, &auth.phone_id, connection_id)
                {
                    return Err("connection superseded".into());
                }
                if value.get("type").and_then(serde_json::Value::as_str) == Some("image_start") {
                    let image: ImageStart = serde_json::from_value(value)?;
                    image.validate(&auth.phone_id)?;
                    if pending_image.is_some() {
                        send_image_reply(
                            &mut websocket,
                            &image.transfer_id,
                            false,
                            "transfer_busy",
                        )
                        .await?;
                    } else {
                        pending_image = Some(image);
                    }
                } else {
                    let message: ClientMessage = serde_json::from_value(value)?;
                    monitored_session = match &message {
                        ClientMessage::Start(snapshot) | ClientMessage::Update(snapshot)
                            if !snapshot.session_id.is_empty() => Some(snapshot.session_id.clone()),
                        ClientMessage::Resume(resume)
                            if resume.session_state == ClientSessionState::Active
                                && !resume.session_id.is_empty() => Some(resume.session_id.clone()),
                        ClientMessage::Finish(_) | ClientMessage::Resume(_) => None,
                        _ => monitored_session,
                    };
                    handle_client_message(&mut websocket, &state, &auth.phone_id, message).await?;
                }
            }
            Message::Binary(bytes) => {
                if active_lease.is_none() {
                    active_lease = Some(claim_active_connection(
                        &state,
                        &auth.phone_id,
                        &auth.phone_name,
                        connection_id,
                    )?);
                }
                if !is_active_connection(&state, &auth.phone_id, connection_id) {
                    return Err("connection superseded".into());
                }
                let Some(image) = pending_image.take() else {
                    continue;
                };
                if bytes.len() != image.byte_length
                    || format!("{:x}", Sha256::digest(&bytes)) != image.sha256.to_ascii_lowercase()
                {
                    send_image_reply(
                        &mut websocket,
                        &image.transfer_id,
                        false,
                        "integrity_failed",
                    )
                    .await?;
                    continue;
                }
                let mime_type = image.mime_type.clone();
                let image_bytes = bytes.to_vec();
                let stored = tokio::task::spawn_blocking(move || {
                    clipboard::set_image(&image_bytes, &mime_type)
                })
                .await
                .map_err(|_| "image worker failed")?;
                if stored.is_ok() {
                    state.update_status(|status| {
                        status.summary = tr("图片已保存到剪贴板", "Image copied to clipboard").to_owned();
                        status.last_error = None;
                    });
                    send_image_reply(&mut websocket, &image.transfer_id, true, "").await?;
                } else {
                    state.update_status(|status| {
                        status.last_error = Some(tr("无法写入 Windows 剪贴板", "Could not copy the image to the Windows clipboard").to_owned());
                    });
                    send_image_reply(
                        &mut websocket,
                        &image.transfer_id,
                        false,
                        "clipboard_failed",
                    )
                    .await?;
                }
            }
            Message::Ping(payload) => websocket.send(Message::Pong(payload)).await?,
            Message::Close(_) => break,
            _ => {}
                }
            }
        }
    }
    Ok(())
}

fn authenticate_phone(
    state: &AppState,
    auth: &AuthMessage,
    nonce: &str,
) -> Result<(), &'static str> {
    if auth.protocol_version != flowtype_core::PROTOCOL_VERSION {
        return Err("unsupported protocol");
    }
    if auth.message_type == "pair" {
        let supplied = auth
            .pairing_token
            .as_deref()
            .ok_or("pairing token required")?;
        let mut token = state
            .pairing_token
            .lock()
            .map_err(|_| "pairing unavailable")?;
        if token.as_deref() != Some(supplied) {
            return Err("invalid pairing token");
        }
        let public_key = auth
            .public_key_spki
            .as_deref()
            .ok_or("public key required")?;
        verify_signature(&state.identity.pc_id, auth, nonce, public_key)?;
        *token = None;
        drop(token);
        if state
            .paired_phones
            .upsert(&auth.phone_id, &auth.phone_name, public_key, unix_time())
            .is_err()
        {
            if let Ok(mut token) = state.pairing_token.lock()
                && token.is_none()
            {
                *token = Some(supplied.to_owned());
            }
            return Err("cannot save phone");
        }
    } else if auth.message_type == "authenticate" {
        let public_key = state
            .paired_phones
            .public_key(&auth.phone_id)
            .ok_or("phone is not paired")?;
        verify_signature(&state.identity.pc_id, auth, nonce, &public_key)?;
        state
            .paired_phones
            .mark_connected(&auth.phone_id, &auth.phone_name, unix_time())
            .map_err(|_| "cannot save phone")?;
    } else {
        return Err("invalid auth type");
    }
    Ok(())
}

async fn authenticate_phone_async(
    state: &Arc<AppState>,
    auth: AuthMessage,
    nonce: String,
) -> Result<(), &'static str> {
    let permit = Arc::clone(&state.pairing_slot)
        .acquire_owned()
        .await
        .map_err(|_| "phone store unavailable")?;
    let state = Arc::clone(state);
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        authenticate_phone(&state, &auth, &nonce)
    })
    .await
    .map_err(|_| "phone authentication stopped")?
}

fn unix_time() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

pub(crate) fn verify_signature(
    pc_id: &str,
    auth: &AuthMessage,
    nonce: &str,
    public_key_spki: &str,
) -> Result<(), &'static str> {
    let public_key = STANDARD
        .decode(public_key_spki)
        .map_err(|_| "invalid public key")?;
    let verifying_key =
        VerifyingKey::from_public_key_der(&public_key).map_err(|_| "invalid public key")?;
    let signature = STANDARD
        .decode(&auth.signature)
        .map_err(|_| "invalid signature")?;
    let signature = Signature::from_der(&signature).map_err(|_| "invalid signature")?;
    verifying_key
        .verify(&auth_payload(pc_id, &auth.phone_id, nonce), &signature)
        .map_err(|_| "signature verification failed")
}

pub(crate) fn auth_payload(pc_id: &str, phone_id: &str, nonce: &str) -> Vec<u8> {
    format!("flowtype-auth-v1\0{pc_id}\0{phone_id}\0{nonce}").into_bytes()
}

fn is_active_connection(state: &AppState, phone_id: &str, connection_id: u64) -> bool {
    state
        .active_connection
        .lock()
        .ok()
        .and_then(|active| {
            active
                .as_ref()
                .map(|active| active.phone_id == phone_id && active.connection_id == connection_id)
        })
        .unwrap_or(false)
}

fn claim_active_connection(
    state: &Arc<AppState>,
    phone_id: &str,
    phone_name: &str,
    connection_id: u64,
) -> Result<ActiveConnectionLease, &'static str> {
    *state
        .active_connection
        .lock()
        .map_err(|_| "connection state unavailable")? = Some(ActiveConnection {
        phone_id: phone_id.to_owned(),
        connection_id,
    });
    state.update_status(|status| {
        status.summary = format!("{}{phone_name}", tr("已连接：", "Connected: "));
        status.connected_phone = Some(phone_name.to_owned());
        status.target_name = None;
        status.last_error = None;
    });
    Ok(ActiveConnectionLease {
        state: Arc::clone(state),
        phone_id: phone_id.to_owned(),
        connection_id,
    })
}

struct ActiveConnectionLease {
    state: Arc<AppState>,
    phone_id: String,
    connection_id: u64,
}

async fn injector_request_async(
    state: &Arc<AppState>,
    request: InjectorRequest,
) -> Result<InjectorResponse, InjectorRequestFailure> {
    let result = state.injector.request(request).await;
    match result {
        Ok(response) => {
            state.update_status(|status| status.last_error = None);
            Ok(response)
        }
        Err(InjectorRequestFailure::Unavailable) => {
            mark_injector_unavailable(state);
            Err(InjectorRequestFailure::Unavailable)
        }
        Err(InjectorRequestFailure::RecoveryRequired) => {
            state.update_status(|status| {
                status.summary = tr("输入服务已恢复", "Input service recovered").to_owned();
                status.target_name = None;
                status.last_error = Some(
                    tr(
                        "Windows 无法确认中断前的输入结果，请在手机上同步全文",
                        "Windows could not confirm the interrupted input. Sync the full text from your phone.",
                    )
                    .to_owned(),
                );
            });
            Err(InjectorRequestFailure::RecoveryRequired)
        }
    }
}

fn mark_injector_unavailable(state: &AppState) {
    state.update_status(|status| {
        status.summary = tr("Windows 输入服务不可用", "Windows input is unavailable").to_owned();
        status.target_name = None;
        status.last_error =
            Some(tr("请在设置中修复输入服务", "Repair input from Settings").to_owned());
    });
}

impl Drop for ActiveConnectionLease {
    fn drop(&mut self) {
        if let Ok(mut active) = self.state.active_connection.lock()
            && active.as_ref().is_some_and(|active| {
                active.phone_id == self.phone_id && active.connection_id == self.connection_id
            })
        {
            *active = None;
            self.state.update_status(|status| {
                status.summary = tr("等待手机连接", "Waiting for phone").to_owned();
                if let Some(phone) = status.connected_phone.as_deref() {
                    status.summary = format!("{}{phone}", tr("已连接：", "Connected: "));
                }
                status.target_name = None;
            });
        }
    }
}

struct SwitchChannelLease {
    state: Arc<AppState>,
    connection_id: u64,
}

struct OnlineConnectionLease {
    state: Arc<AppState>,
    connection_id: u64,
}

impl Drop for OnlineConnectionLease {
    fn drop(&mut self) {
        self.state.clear_online_connection(self.connection_id);
    }
}

impl Drop for SwitchChannelLease {
    fn drop(&mut self) {
        self.state.clear_switch_channel(self.connection_id);
    }
}

async fn handle_client_message<S>(
    websocket: &mut WebSocketStream<S>,
    state: &Arc<AppState>,
    authenticated_phone_id: &str,
    message: ClientMessage,
) -> Result<(), Box<dyn std::error::Error>>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    if let ClientMessage::HealthCheck(health) = &message {
        health.validate().map_err(|_| "invalid health check")?;
        if health.phone_id != authenticated_phone_id {
            return Err("health check phone does not match".into());
        }
        send_json(
            websocket,
            &ServerMessage::HealthAck(HealthAck {
                protocol_version: flowtype_core::PROTOCOL_VERSION,
            }),
        )
        .await?;
        return Ok(());
    }

    if let ClientMessage::SwitchAck(ack) = &message {
        ack.validate()
            .map_err(|_| "invalid switch acknowledgement")?;
        if ack.pc_id != state.identity.pc_id {
            return Err("switch acknowledgement computer does not match".into());
        }
        state.acknowledge_switch(ack);
        return Ok(());
    }

    if let ClientMessage::Probe(probe) = &message {
        probe.validate().map_err(|_| "invalid probe")?;
        if probe.phone_id != authenticated_phone_id {
            send_json(
                websocket,
                &ServerMessage::Error(ProtocolError {
                    protocol_version: flowtype_core::PROTOCOL_VERSION,
                    code: ErrorCode::InvalidMessage,
                    session_id: None,
                }),
            )
            .await?;
            return Ok(());
        }
        let result = match injector_request_async(state, InjectorRequest::ProbeTarget).await {
            Ok(InjectorResponse::TargetReady {
                target_name,
                activity_age_ms,
            }) => ProbeResult {
                protocol_version: flowtype_core::PROTOCOL_VERSION,
                target_state: ProbeState::Ready,
                target_name: Some(target_name),
                activity_age_ms: Some(activity_age_ms),
            },
            Ok(InjectorResponse::TargetUnsupported) => ProbeResult {
                protocol_version: flowtype_core::PROTOCOL_VERSION,
                target_state: ProbeState::Unsupported,
                target_name: None,
                activity_age_ms: None,
            },
            Ok(InjectorResponse::TargetInvalid) => ProbeResult {
                protocol_version: flowtype_core::PROTOCOL_VERSION,
                target_state: ProbeState::Invalid,
                target_name: None,
                activity_age_ms: None,
            },
            Ok(_) | Err(_) => ProbeResult {
                protocol_version: flowtype_core::PROTOCOL_VERSION,
                target_state: ProbeState::Unsupported,
                target_name: None,
                activity_age_ms: None,
            },
        };
        send_json(websocket, &ServerMessage::ProbeResult(result)).await?;
        return Ok(());
    }

    let (kind, snapshot) = match message {
        ClientMessage::Start(value) => ("start", value),
        ClientMessage::Update(value) => ("update", value),
        ClientMessage::Finish(value) => ("finish", value),
        ClientMessage::Resume(value) => {
            return handle_resume(websocket, state, authenticated_phone_id, value).await;
        }
        ClientMessage::Cancel(value) => {
            return handle_cancel(state, authenticated_phone_id, value).await;
        }
        ClientMessage::Probe(_) | ClientMessage::HealthCheck(_) | ClientMessage::SwitchAck(_) => {
            unreachable!()
        }
    };
    snapshot.validate().map_err(|_| "invalid snapshot")?;
    if snapshot.phone_id != authenticated_phone_id {
        return send_json(
            websocket,
            &ServerMessage::Error(ProtocolError {
                protocol_version: flowtype_core::PROTOCOL_VERSION,
                code: ErrorCode::InvalidMessage,
                session_id: Some(snapshot.session_id),
            }),
        )
        .await;
    }

    if kind == "start" {
        let response = match injector_request_async(
            state,
            InjectorRequest::BeginSession {
                session_id: snapshot.session_id.clone(),
            },
        )
        .await
        {
            Ok(response) => response,
            Err(error) => {
                return send_injector_failure(websocket, &snapshot.session_id, error).await;
            }
        };
        match response {
            InjectorResponse::SessionBegun { target_name } => {
                state.update_status(|status| {
                    status.summary = format!("{}{target_name}", tr("正在输入到：", "Typing in: "));
                    status.target_name = Some(target_name.clone());
                    status.last_error = None;
                });
                send_json(
                    websocket,
                    &ServerMessage::Target(Target {
                        protocol_version: flowtype_core::PROTOCOL_VERSION,
                        session_id: snapshot.session_id.clone(),
                        target_state: TargetState::Active,
                        target_name: Some(target_name),
                    }),
                )
                .await?;
            }
            InjectorResponse::SessionFinished {
                session_id,
                sequence,
                full_text,
            } if session_id == snapshot.session_id
                && sequence == snapshot.sequence
                && full_text == snapshot.full_text =>
            {
                state.mark_input_finished();
                return send_ack(websocket, &snapshot.session_id, sequence, true).await;
            }
            other => {
                return send_injector_state(websocket, state, &snapshot.session_id, other).await;
            }
        }
    }

    let applied = match injector_request_async(
        state,
        InjectorRequest::ApplyState {
            session_id: snapshot.session_id.clone(),
            sequence: snapshot.sequence,
            full_text: snapshot.full_text,
        },
    )
    .await
    {
        Ok(response) => response,
        Err(error) => return send_injector_failure(websocket, &snapshot.session_id, error).await,
    };
    match applied {
        InjectorResponse::Applied { sequence } if kind == "finish" => {
            let finished = match injector_request_async(
                state,
                InjectorRequest::FinishSession {
                    session_id: snapshot.session_id.clone(),
                    sequence,
                },
            )
            .await
            {
                Ok(response) => response,
                Err(error) => {
                    return send_injector_failure(websocket, &snapshot.session_id, error).await;
                }
            };
            match finished {
                InjectorResponse::Finished { sequence } => {
                    state.mark_input_finished();
                    send_ack(websocket, &snapshot.session_id, sequence, true).await
                }
                other => send_injector_state(websocket, state, &snapshot.session_id, other).await,
            }
        }
        InjectorResponse::Applied { sequence } => {
            send_ack(websocket, &snapshot.session_id, sequence, false).await
        }
        other => send_injector_state(websocket, state, &snapshot.session_id, other).await,
    }
}

async fn handle_cancel(
    state: &Arc<AppState>,
    authenticated_phone_id: &str,
    cancel: Cancel,
) -> Result<(), Box<dyn std::error::Error>> {
    validate_cancel_for_phone(&cancel, authenticated_phone_id)?;
    let _ = injector_request_async(
        state,
        InjectorRequest::CancelInvalidSession {
            session_id: cancel.session_id,
        },
    )
    .await;
    state.mark_input_finished();
    Ok(())
}

async fn handle_resume<S>(
    websocket: &mut WebSocketStream<S>,
    state: &Arc<AppState>,
    authenticated_phone_id: &str,
    resume: Resume,
) -> Result<(), Box<dyn std::error::Error>>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    validate_resume_for_phone(&resume, authenticated_phone_id)?;
    let applied = match injector_request_async(
        state,
        InjectorRequest::ApplyState {
            session_id: resume.session_id.clone(),
            sequence: resume.sequence,
            full_text: resume.full_text,
        },
    )
    .await
    {
        Ok(response) => response,
        Err(error) => return send_injector_failure(websocket, &resume.session_id, error).await,
    };
    match applied {
        InjectorResponse::Finished { sequence } => {
            state.mark_input_finished();
            send_ack(websocket, &resume.session_id, sequence, true).await
        }
        InjectorResponse::Applied { sequence }
            if resume.session_state == ClientSessionState::Finishing =>
        {
            let finished = match injector_request_async(
                state,
                InjectorRequest::FinishSession {
                    session_id: resume.session_id.clone(),
                    sequence,
                },
            )
            .await
            {
                Ok(response) => response,
                Err(error) => {
                    return send_injector_failure(websocket, &resume.session_id, error).await;
                }
            };
            match finished {
                InjectorResponse::Finished { sequence } => {
                    state.mark_input_finished();
                    send_ack(websocket, &resume.session_id, sequence, true).await
                }
                other => send_injector_state(websocket, state, &resume.session_id, other).await,
            }
        }
        InjectorResponse::Applied { sequence } => {
            send_ack(websocket, &resume.session_id, sequence, false).await
        }
        other => send_injector_state(websocket, state, &resume.session_id, other).await,
    }
}

pub(crate) fn validate_cancel_for_phone(
    cancel: &Cancel,
    authenticated_phone_id: &str,
) -> Result<(), &'static str> {
    cancel.validate().map_err(|_| "invalid cancel")?;
    if cancel.phone_id != authenticated_phone_id {
        return Err("cancel phone does not match");
    }
    Ok(())
}

pub(crate) fn validate_resume_for_phone(
    resume: &Resume,
    authenticated_phone_id: &str,
) -> Result<(), &'static str> {
    resume.validate().map_err(|_| "invalid resume")?;
    if resume.phone_id != authenticated_phone_id {
        return Err("resume phone does not match");
    }
    Ok(())
}

async fn send_ack<S>(
    websocket: &mut WebSocketStream<S>,
    session_id: &str,
    sequence: i64,
    finished: bool,
) -> Result<(), Box<dyn std::error::Error>>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    send_json(
        websocket,
        &ServerMessage::Ack(Ack {
            protocol_version: flowtype_core::PROTOCOL_VERSION,
            session_id: session_id.to_owned(),
            applied_sequence: sequence,
            session_state: if finished {
                ServerSessionState::Finished
            } else {
                ServerSessionState::Active
            },
        }),
    )
    .await
}

async fn send_injector_state<S>(
    websocket: &mut WebSocketStream<S>,
    state: &AppState,
    session_id: &str,
    response: InjectorResponse,
) -> Result<(), Box<dyn std::error::Error>>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let (target_state, target_name) = match response {
        InjectorResponse::TargetNotForeground { target_name } => {
            state.update_status(|status| {
                status.summary = format!("{}{target_name}", tr("请回到：", "Return to: "));
                status.target_name = Some(target_name.clone());
            });
            (TargetState::NotForeground, Some(target_name))
        }
        InjectorResponse::TargetInvalid => {
            state.update_status(|status| {
                status.summary = tr("原输入窗口已关闭", "The input window was closed").to_owned();
                status.target_name = None;
                status.last_error = Some(
                    tr(
                        "请在电脑上重新放置光标，再从手机同步全文",
                        "Place the cursor again, then sync from your phone",
                    )
                    .to_owned(),
                );
            });
            (TargetState::Invalid, None)
        }
        InjectorResponse::TargetModified => {
            state.update_status(|status| {
                status.summary = tr("电脑端已编辑", "Text edited on the computer").to_owned();
                status.last_error = Some(
                    tr(
                        "本次同步已停止，手机正文仍保留",
                        "Syncing stopped. The text remains on your phone.",
                    )
                    .to_owned(),
                );
            });
            return send_json(
                websocket,
                &ServerMessage::Error(ProtocolError {
                    protocol_version: flowtype_core::PROTOCOL_VERSION,
                    code: ErrorCode::TargetModified,
                    session_id: Some(session_id.to_owned()),
                }),
            )
            .await;
        }
        InjectorResponse::TargetSubmitted => {
            state.update_status(|status| {
                status.summary = tr("输入已提交", "Input submitted").to_owned();
                status.target_name = None;
                status.last_error = None;
            });
            state.mark_input_finished();
            return send_json(
                websocket,
                &ServerMessage::Error(ProtocolError {
                    protocol_version: flowtype_core::PROTOCOL_VERSION,
                    code: ErrorCode::TargetSubmitted,
                    session_id: Some(session_id.to_owned()),
                }),
            )
            .await;
        }
        InjectorResponse::TargetUnsupported => {
            state.update_status(|status| {
                status.summary = tr(
                    "当前应用不支持实时输入",
                    "This app does not support live input",
                )
                .to_owned();
                status.target_name = None;
                status.last_error = Some(
                    tr(
                        "请将光标移到其他输入框后重试",
                        "Move the cursor to another text field and try again",
                    )
                    .to_owned(),
                );
            });
            return send_json(
                websocket,
                &ServerMessage::Error(ProtocolError {
                    protocol_version: flowtype_core::PROTOCOL_VERSION,
                    code: ErrorCode::TargetUnavailable,
                    session_id: Some(session_id.to_owned()),
                }),
            )
            .await;
        }
        InjectorResponse::TsfUnavailable => {
            mark_injector_unavailable(state);
            return send_json(
                websocket,
                &ServerMessage::Error(ProtocolError {
                    protocol_version: flowtype_core::PROTOCOL_VERSION,
                    code: ErrorCode::InjectorUnavailable,
                    session_id: Some(session_id.to_owned()),
                }),
            )
            .await;
        }
        InjectorResponse::InjectionUnknown | InjectorResponse::InvalidRequest => {
            let code = if response == InjectorResponse::InjectionUnknown {
                ErrorCode::InjectionUnknown
            } else {
                ErrorCode::SequenceConflict
            };
            state.update_status(|status| {
                status.summary = tr("输入已停止", "Input stopped").to_owned();
                status.last_error = Some(if code == ErrorCode::InjectionUnknown {
                    tr(
                        "Windows 无法确认本次输入结果",
                        "Windows could not confirm this input",
                    )
                    .to_owned()
                } else {
                    tr("输入状态不一致", "Input state mismatch").to_owned()
                });
            });
            return send_json(
                websocket,
                &ServerMessage::Error(ProtocolError {
                    protocol_version: flowtype_core::PROTOCOL_VERSION,
                    code,
                    session_id: Some(session_id.to_owned()),
                }),
            )
            .await;
        }
        _ => (TargetState::Invalid, None),
    };
    send_json(
        websocket,
        &ServerMessage::Target(Target {
            protocol_version: flowtype_core::PROTOCOL_VERSION,
            session_id: session_id.to_owned(),
            target_state,
            target_name,
        }),
    )
    .await
}

async fn send_injector_failure<S>(
    websocket: &mut WebSocketStream<S>,
    session_id: &str,
    failure: InjectorRequestFailure,
) -> Result<(), Box<dyn std::error::Error>>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    send_json(
        websocket,
        &ServerMessage::Error(ProtocolError {
            protocol_version: flowtype_core::PROTOCOL_VERSION,
            code: match failure {
                InjectorRequestFailure::Unavailable => ErrorCode::InjectorUnavailable,
                InjectorRequestFailure::RecoveryRequired => ErrorCode::RecoveryRequired,
            },
            session_id: Some(session_id.to_owned()),
        }),
    )
    .await
}

async fn send_json<S, T>(
    websocket: &mut WebSocketStream<S>,
    value: &T,
) -> Result<(), Box<dyn std::error::Error>>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
    T: Serialize,
{
    websocket
        .send(Message::Text(serde_json::to_string(value)?.into()))
        .await?;
    Ok(())
}

async fn send_image_reply<S>(
    websocket: &mut WebSocketStream<S>,
    transfer_id: &str,
    success: bool,
    error_code: &'static str,
) -> Result<(), Box<dyn std::error::Error>>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    send_json(
        websocket,
        &ImageReply {
            protocol_version: flowtype_core::PROTOCOL_VERSION,
            message_type: if success { "image_ack" } else { "image_error" },
            transfer_id,
            code: (!success).then_some(error_code),
        },
    )
    .await
}

async fn next_text<S>(
    websocket: &mut WebSocketStream<S>,
) -> Result<String, Box<dyn std::error::Error>>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    match websocket.next().await.ok_or("connection closed")?? {
        Message::Text(value) => Ok(value.to_string()),
        _ => Err("expected text message".into()),
    }
}

fn tls_acceptor(identity: &PcIdentity) -> Result<TlsAcceptor, Box<dyn std::error::Error>> {
    let provider = rustls::crypto::ring::default_provider();
    let config = ServerConfig::builder_with_provider(Arc::new(provider))
        .with_protocol_versions(&[&rustls::version::TLS13])?
        .with_no_client_auth()
        .with_single_cert(
            vec![CertificateDer::from(identity.cert_der.clone())],
            PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(identity.key_der.clone())),
        )?;
    Ok(TlsAcceptor::from(Arc::new(config)))
}
