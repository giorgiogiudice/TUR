// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! Live sessions: the conformance of each family, from messages shaped as
//! the providers' documentation shows them (read 16 November 2026), the
//! scripted session, and one whole OpenAI-Realtime session over a WebSocket
//! on the loopback interface. No test talks to a real provider.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use serde_json::{Value, json};
use tur_pax_providers::live::{
    LivePrices, audio_from_base64, audio_to_base64, resample, websocket_accept,
};
use tur_pax_providers::live_gemini;
use tur_pax_providers::live_openai::{self, ReaderState};
use tur_pax_providers::{
    ApiKey, ErrorKind, LiveConfig, LiveEvent, LiveFamily, LiveSession, LiveUsage, ScriptStep,
    ScriptedLive, ToolDefinition, ToolResult, connect_live,
};

/// The key every test uses. No output may contain it.
const TEST_KEY: &str = "sk-live-TEST-9c41e7a2b8d05f36";

fn key() -> ApiKey {
    ApiKey::new(TEST_KEY.to_owned()).expect("a usable key")
}

fn tool() -> ToolDefinition {
    ToolDefinition::new(
        "list_volumes",
        "List the storage volumes.",
        json!({ "type": "object", "properties": {} }),
    )
    .expect("a valid tool")
}

fn config(family: LiveFamily) -> LiveConfig {
    LiveConfig {
        transcription_model: Some("gpt-4o-mini-transcribe".to_owned()),
        family,
        model: match family {
            LiveFamily::GeminiLive => "gemini-3.8-live".to_owned(),
            _ => "gpt-realtime-2.1".to_owned(),
        },
        base_url: None,
        key_header: None,
        voice: Some("marin".to_owned()),
        instructions: "You are P@x.".to_owned(),
        tools: vec![tool()],
        prices: None,
    }
}

fn parse(text: &str) -> Value {
    serde_json::from_str(text).expect("JSON")
}

// ------------------------------------------------------------ OpenAI

#[test]
fn openai_the_session_is_set_up_as_documented() {
    let config = config(LiveFamily::OpenAiRealtime);
    assert_eq!(
        live_openai::session_url(&config),
        "wss://api.openai.com/v1/realtime?model=gpt-realtime-2.1"
    );
    let update = live_openai::session_update(&config);
    assert_eq!(update["type"], "session.update");
    let session = &update["session"];
    assert_eq!(session["type"], "realtime");
    assert_eq!(session["model"], "gpt-realtime-2.1");
    assert_eq!(session["instructions"], "You are P@x.");
    assert_eq!(session["output_modalities"], json!(["audio"]));
    assert_eq!(session["audio"]["input"]["format"], json!({ "type": "audio/pcm", "rate": 24000 }));
    assert_eq!(session["audio"]["output"]["format"], json!({ "type": "audio/pcm", "rate": 24000 }));
    assert_eq!(session["audio"]["output"]["voice"], "marin");
    assert_eq!(session["audio"]["input"]["turn_detection"]["type"], "server_vad");
    assert_eq!(session["audio"]["input"]["turn_detection"]["interrupt_response"], true);
    assert!(session["audio"]["input"]["transcription"]["model"].is_string());
    assert_eq!(session["tools"][0]["type"], "function");
    assert_eq!(session["tools"][0]["name"], "list_volumes");
    assert_eq!(session["tools"][0]["parameters"]["type"], "object");
    assert_eq!(session["tool_choice"], "auto");

    // Azure: another address, the model is the deployment.
    let mut azure = config.clone();
    azure.base_url = Some("wss://contoso.openai.azure.com/openai/v1/realtime".to_owned());
    azure.key_header = Some("api-key".to_owned());
    azure.model = "my-realtime".to_owned();
    assert!(azure.validate().is_ok());
    assert_eq!(
        live_openai::session_url(&azure),
        "wss://contoso.openai.azure.com/openai/v1/realtime?model=my-realtime"
    );
}

#[test]
fn openai_messages_out_have_the_documented_shapes() {
    let samples: Vec<i16> = (0..480).map(|i| (i * 7) as i16).collect();
    let append = live_openai::audio_message(&samples);
    assert_eq!(append["type"], "input_audio_buffer.append");
    assert_eq!(
        audio_from_base64(append["audio"].as_str().expect("text")),
        Some(samples)
    );

    let text = live_openai::text_messages("What is on my disk?");
    assert_eq!(text[0]["type"], "conversation.item.create");
    assert_eq!(text[0]["item"]["role"], "user");
    assert_eq!(text[0]["item"]["content"][0]["type"], "input_text");
    assert_eq!(text[1]["type"], "response.create");

    let done = live_openai::tool_result_messages(&ToolResult::text("call_1", "list_volumes", "vol1"));
    assert_eq!(done[0]["item"]["type"], "function_call_output");
    assert_eq!(done[0]["item"]["call_id"], "call_1");
    assert_eq!(done[0]["item"]["output"], "vol1");
    assert_eq!(done[1]["type"], "response.create");
    let failed = live_openai::tool_result_messages(&ToolResult::error("call_2", "list_volumes", "refused"));
    assert_eq!(parse(failed[0]["item"]["output"].as_str().expect("text")), json!({ "error": "refused" }));
}

#[test]
fn openai_server_events_become_live_events() {
    let mut state = ReaderState::default();
    let sound = audio_to_base64(&[1, 2, 3, 4]);
    let script = [
        json!({ "type": "session.created", "session": {} }).to_string(),
        json!({ "type": "input_audio_buffer.speech_started", "audio_start_ms": 100, "item_id": "in_1" }).to_string(),
        json!({ "type": "conversation.item.input_audio_transcription.delta", "item_id": "in_1", "content_index": 0, "delta": "What is" }).to_string(),
        json!({ "type": "conversation.item.input_audio_transcription.completed", "item_id": "in_1", "content_index": 0, "transcript": "What is on my disk?" }).to_string(),
        json!({ "type": "response.created", "response": { "id": "resp_1" } }).to_string(),
        json!({ "type": "response.output_audio.delta", "response_id": "resp_1", "item_id": "out_1", "output_index": 0, "content_index": 0, "delta": sound }).to_string(),
        json!({ "type": "response.output_audio_transcript.delta", "item_id": "out_1", "delta": "Let me look" }).to_string(),
        json!({ "type": "response.output_audio_transcript.done", "item_id": "out_1", "transcript": "Let me look." }).to_string(),
        json!({ "type": "response.function_call_arguments.done", "item_id": "fc_1", "call_id": "call_1", "name": "list_volumes", "arguments": "{}" }).to_string(),
    ];
    let mut events = Vec::new();
    for message in &script {
        events.extend(live_openai::parse_server_event(message, &mut state));
    }
    assert_eq!(
        events,
        vec![
            LiveEvent::SpeechStarted,
            LiveEvent::Heard { text: "What is".to_owned(), final_text: false },
            LiveEvent::Heard { text: "What is on my disk?".to_owned(), final_text: true },
            LiveEvent::Audio { samples: vec![1, 2, 3, 4], rate: 24_000 },
            LiveEvent::Said { text: "Let me look".to_owned(), final_text: false },
            LiveEvent::Said { text: "Let me look.".to_owned(), final_text: true },
            match &events[6] {
                LiveEvent::ToolCall { call, arguments_text } => {
                    assert_eq!(call.id, "call_1");
                    assert_eq!(call.name, "list_volumes");
                    assert!(call.arguments.is_empty());
                    assert_eq!(arguments_text, "{}");
                    events[6].clone()
                }
                other => panic!("{other:?}"),
            },
        ]
    );
    assert!(state.responding);
    assert_eq!(state.audio_item.as_deref(), Some("out_1"));

    // Barge-in: cancel the response, cut the item at what was played.
    let barge = live_openai::interrupt_messages(&state, 640);
    assert_eq!(barge[0], json!({ "type": "response.cancel" }));
    assert_eq!(
        barge[1],
        json!({ "type": "conversation.item.truncate", "item_id": "out_1", "content_index": 0, "audio_end_ms": 640 })
    );

    // The end of a response: its usage, then the turn is done.
    let done = json!({
        "type": "response.done",
        "response": {
            "id": "resp_1",
            "status": "completed",
            "usage": {
                "total_tokens": 1900,
                "input_tokens": 700,
                "output_tokens": 1200,
                "input_token_details": {
                    "text_tokens": 400, "audio_tokens": 300, "cached_tokens": 100,
                    "cached_tokens_details": { "text_tokens": 100, "audio_tokens": 0 }
                },
                "output_token_details": { "text_tokens": 50, "audio_tokens": 1150 }
            }
        }
    })
    .to_string();
    let end = live_openai::parse_server_event(&done, &mut state);
    let usage = LiveUsage {
        audio_input_tokens: 300,
        audio_output_tokens: 1150,
        text_input_tokens: 300,
        text_output_tokens: 50,
        cached_input_tokens: 100,
    };
    assert_eq!(end, vec![LiveEvent::Usage(usage), LiveEvent::TurnDone]);
    assert!(!state.responding);
    assert!(live_openai::interrupt_messages(&state, 0).is_empty(), "nothing to cancel");

    // Priced as the model list would price gpt-realtime-2.1.
    let prices = LivePrices {
        currency: "USD".to_owned(),
        audio_input_per_million: 32.0,
        audio_output_per_million: 64.0,
        text_input_per_million: 4.0,
        text_output_per_million: 24.0,
        cached_input_per_million: Some(0.4),
    };
    let expected = (300.0 * 32.0 + 1150.0 * 64.0 + 300.0 * 4.0 + 50.0 * 24.0 + 100.0 * 0.4) / 1e6;
    assert!((prices.cost(&usage) - expected).abs() < 1e-12);
}

#[test]
fn openai_errors_end_the_session_with_their_kind_except_a_late_cancel() {
    let mut state = ReaderState::default();
    let error = |code: &str, kind: &str| {
        json!({ "type": "error", "error": { "type": kind, "code": code, "message": "no" } }).to_string()
    };
    let kind_of = |events: Vec<LiveEvent>| match events.as_slice() {
        [LiveEvent::Closed { error: Some(error) }] => error.kind(),
        other => panic!("{other:?}"),
    };
    assert_eq!(
        kind_of(live_openai::parse_server_event(&error("invalid_api_key", "invalid_request_error"), &mut state)),
        ErrorKind::AuthenticationFailed
    );
    assert_eq!(
        kind_of(live_openai::parse_server_event(&error("insufficient_quota", "invalid_request_error"), &mut state)),
        ErrorKind::QuotaExhausted
    );
    assert_eq!(
        kind_of(live_openai::parse_server_event(&error("rate_limit_exceeded", "rate_limit_error"), &mut state)),
        ErrorKind::RateLimited
    );
    assert!(live_openai::parse_server_event(&error("response_cancel_not_active", "invalid_request_error"), &mut state)
        .is_empty());
}

#[test]
fn a_repeated_key_anywhere_ends_the_session_as_malformed() {
    let mut state = ReaderState::default();
    let in_arguments = json!({
        "type": "response.function_call_arguments.done",
        "call_id": "call_9", "name": "delete_file",
        "arguments": "{\"path\": \"/tmp/a\", \"path\": \"/home\"}"
    })
    .to_string();
    let in_message = r#"{"type": "response.output_audio_transcript.delta", "delta": "a", "delta": "b"}"#;
    let gemini_args = r#"{"toolCall": {"functionCalls": [{"id": "c", "name": "delete_file", "args": {"path": "/tmp/a", "path": "/home"}}]}}"#;
    for events in [
        live_openai::parse_server_event(&in_arguments, &mut state),
        live_openai::parse_server_event(in_message, &mut state),
        live_gemini::parse_server_message(gemini_args),
        live_openai::parse_server_event("not json", &mut state),
    ] {
        match events.as_slice() {
            [LiveEvent::Closed { error: Some(error) }] => {
                assert_eq!(error.kind(), ErrorKind::MalformedResponse);
            }
            other => panic!("{other:?}"),
        }
    }
}

// ------------------------------------------------------------ Gemini

#[test]
fn gemini_the_session_is_set_up_as_documented() {
    let config = config(LiveFamily::GeminiLive);
    assert_eq!(live_gemini::model_name("gemini-3.8-live"), "models/gemini-3.8-live");
    assert_eq!(live_gemini::model_name("models/x"), "models/x");
    assert_eq!(
        live_gemini::session_url(&config, Some("K")),
        format!("{}?key=K", live_gemini::DEFAULT_URL)
    );
    assert_eq!(live_gemini::session_url(&config, None), live_gemini::DEFAULT_URL);
    let setup = live_gemini::setup_message(&config);
    let setup = &setup["setup"];
    assert_eq!(setup["model"], "models/gemini-3.8-live");
    assert_eq!(setup["generationConfig"]["responseModalities"], json!(["AUDIO"]));
    assert_eq!(
        setup["generationConfig"]["speechConfig"]["voiceConfig"]["prebuiltVoiceConfig"]["voiceName"],
        "marin"
    );
    assert_eq!(setup["systemInstruction"]["parts"][0]["text"], "You are P@x.");
    assert_eq!(setup["tools"][0]["functionDeclarations"][0]["name"], "list_volumes");
    assert_eq!(setup["realtimeInputConfig"]["automaticActivityDetection"]["disabled"], false);
    assert!(setup["inputAudioTranscription"].is_object());
    assert!(setup["outputAudioTranscription"].is_object());

    let audio = live_gemini::audio_message(&[5, -5]);
    assert_eq!(audio["realtimeInput"]["audio"]["mimeType"], "audio/pcm;rate=16000");
    assert_eq!(
        audio_from_base64(audio["realtimeInput"]["audio"]["data"].as_str().expect("text")),
        Some(vec![5, -5])
    );
    assert_eq!(live_gemini::text_message("hi")["realtimeInput"]["text"], "hi");
    let answer = live_gemini::tool_result_message(&ToolResult::text("c1", "list_volumes", "vol1"));
    assert_eq!(
        answer,
        json!({ "toolResponse": { "functionResponses": [{ "id": "c1", "name": "list_volumes", "response": { "output": "vol1" } }] } })
    );
    let failed = live_gemini::tool_result_message(&ToolResult::error("c2", "list_volumes", "refused"));
    assert_eq!(failed["toolResponse"]["functionResponses"][0]["response"], json!({ "error": "refused" }));
}

#[test]
fn gemini_server_messages_become_live_events() {
    let sound = audio_to_base64(&[7, 8]);
    let content = json!({
        "serverContent": {
            "inputTranscription": { "text": "Che cosa c'è sul disco?" },
            "modelTurn": { "parts": [
                { "inlineData": { "mimeType": "audio/pcm;rate=24000", "data": sound } }
            ] },
            "outputTranscription": { "text": "Guardo subito." }
        }
    })
    .to_string();
    assert_eq!(
        live_gemini::parse_server_message(&content),
        vec![
            LiveEvent::Heard { text: "Che cosa c'è sul disco?".to_owned(), final_text: false },
            LiveEvent::Audio { samples: vec![7, 8], rate: 24_000 },
            LiveEvent::Said { text: "Guardo subito.".to_owned(), final_text: false },
        ]
    );
    let interrupted = json!({ "serverContent": { "interrupted": true } }).to_string();
    assert_eq!(live_gemini::parse_server_message(&interrupted), vec![LiveEvent::SpeechStarted]);
    let complete = json!({
        "serverContent": { "turnComplete": true },
        "usageMetadata": {
            "promptTokenCount": 96, "responseTokenCount": 64, "totalTokenCount": 160,
            "promptTokensDetails": [{ "modality": "AUDIO", "tokenCount": 64 }, { "modality": "TEXT", "tokenCount": 32 }],
            "responseTokensDetails": [{ "modality": "AUDIO", "tokenCount": 64 }]
        }
    })
    .to_string();
    assert_eq!(
        live_gemini::parse_server_message(&complete),
        vec![
            LiveEvent::TurnDone,
            LiveEvent::Usage(LiveUsage {
                audio_input_tokens: 64,
                audio_output_tokens: 64,
                text_input_tokens: 32,
                text_output_tokens: 0,
                cached_input_tokens: 0,
            }),
        ]
    );
    let call = json!({ "toolCall": { "functionCalls": [{ "id": "fc_1", "name": "list_volumes", "args": { "all": true } }] } })
        .to_string();
    match live_gemini::parse_server_message(&call).as_slice() {
        [LiveEvent::ToolCall { call, arguments_text }] => {
            assert_eq!(call.id, "fc_1");
            assert_eq!(call.arguments["all"], true);
            assert_eq!(parse(arguments_text), json!({ "all": true }));
        }
        other => panic!("{other:?}"),
    }
    assert!(live_gemini::parse_server_message(r#"{"setupComplete": {}}"#).is_empty());
    assert!(live_gemini::parse_server_message(r#"{"goAway": {"timeLeft": "10s"}}"#).is_empty());
}

// ------------------------------------------------------------ the interface

#[test]
fn a_scripted_session_plays_audio_in_events_and_tool_calls_out() {
    let call = tur_pax_providers::ToolCall {
        id: "call_1".to_owned(),
        name: "list_volumes".to_owned(),
        arguments: serde_json::Map::new(),
    };
    let scripted = ScriptedLive::new(vec![
        ScriptStep::AwaitAudio { at_least_samples: 3200 },
        ScriptStep::Emit(LiveEvent::Heard { text: "What is on my disk?".to_owned(), final_text: true }),
        ScriptStep::Emit(LiveEvent::ToolCall { call, arguments_text: "{}".to_owned() }),
        ScriptStep::AwaitToolResult { call_id: "call_1".to_owned() },
        ScriptStep::Emit(LiveEvent::Audio { samples: vec![0; 2400], rate: 24_000 }),
        ScriptStep::Emit(LiveEvent::Said { text: "You have one volume.".to_owned(), final_text: true }),
        ScriptStep::AwaitInterrupt,
        ScriptStep::Emit(LiveEvent::Closed { error: None }),
    ]);
    let record = scripted.received();
    let mut session: Box<dyn LiveSession> = Box::new(scripted);
    assert_eq!(session.family(), LiveFamily::Scripted);
    let wait = Duration::from_millis(1);
    assert_eq!(session.next_event(wait), None, "it waits for the person's sound");
    session.send_audio(&[0; 1600], 16_000).expect("sent");
    assert_eq!(session.next_event(wait), None, "not enough yet");
    session.send_audio(&[0; 1600], 16_000).expect("sent");
    assert!(matches!(session.next_event(wait), Some(LiveEvent::Heard { .. })));
    let Some(LiveEvent::ToolCall { call, .. }) = session.next_event(wait) else {
        panic!("a tool call");
    };
    assert_eq!(session.next_event(wait), None, "it waits for the result");
    session
        .send_tool_result(&ToolResult::text(call.id.clone(), call.name.clone(), "vol1"))
        .expect("sent");
    assert!(matches!(session.next_event(wait), Some(LiveEvent::Audio { .. })));
    assert!(matches!(session.next_event(wait), Some(LiveEvent::Said { .. })));
    session.interrupt(120).expect("sent");
    assert_eq!(session.next_event(wait), Some(LiveEvent::Closed { error: None }));
    assert_eq!(session.next_event(wait), None, "nothing after the end");
    let received = record.lock().expect("the record").clone();
    assert_eq!(received.audio_samples, 3200);
    assert_eq!(received.texts, Vec::<String>::new());
    assert_eq!(received.tool_results.len(), 1);
    assert_eq!(received.interrupts, vec![120]);
    assert!(session.send_text("too late").is_err());
}

#[test]
fn connect_refuses_what_it_must_and_never_shows_the_key() {
    // The scripted family is for tests.
    assert!(connect_live(&config(LiveFamily::Scripted), key()).is_err());
    // Not wss, and not this computer.
    let mut plain = config(LiveFamily::OpenAiRealtime);
    plain.base_url = Some("ws://api.example.org/v1/realtime".to_owned());
    assert_eq!(connect_live(&plain, key()).err().map(|e| e.kind()), Some(ErrorKind::ConfigInvalid));
    // A model name that cannot be one.
    let mut odd = config(LiveFamily::OpenAiRealtime);
    odd.model = "gpt realtime&x=1".to_owned();
    assert_eq!(connect_live(&odd, key()).err().map(|e| e.kind()), Some(ErrorKind::ConfigInvalid));

    // Nothing listens: the error names neither key nor query, in either family.
    let closed = TcpListener::bind("127.0.0.1:0").expect("a port");
    let port = closed.local_addr().expect("address").port();
    drop(closed);
    for family in [LiveFamily::OpenAiRealtime, LiveFamily::GeminiLive] {
        let mut local = config(family);
        local.base_url = Some(format!("ws://127.0.0.1:{port}/live"));
        let error = connect_live(&local, key()).err().expect("refused");
        assert_eq!(error.kind(), ErrorKind::Network);
        let shown = format!("{error} {error:?}");
        assert!(!shown.contains(TEST_KEY), "{shown}");
    }
    assert!(!format!("{:?}", key()).contains(TEST_KEY));
}

#[test]
fn resampling_for_each_family() {
    assert_eq!(resample(&[0; 1600], 16_000, LiveFamily::OpenAiRealtime.input_rate()).len(), 2400);
    assert_eq!(resample(&[0; 2400], 24_000, LiveFamily::GeminiLive.input_rate()).len(), 1600);
}

// ------------------------------------------------- a session over a socket

/// Read one client frame: masked, as a client must send it.
fn read_client_frame(stream: &mut TcpStream) -> (u8, Vec<u8>) {
    let mut head = [0u8; 2];
    stream.read_exact(&mut head).expect("a frame");
    assert_ne!(head[1] & 0x80, 0, "a client frame is masked");
    let length = match head[1] & 0x7F {
        126 => {
            let mut bytes = [0u8; 2];
            stream.read_exact(&mut bytes).expect("length");
            u64::from(u16::from_be_bytes(bytes))
        }
        127 => {
            let mut bytes = [0u8; 8];
            stream.read_exact(&mut bytes).expect("length");
            u64::from_be_bytes(bytes)
        }
        short => u64::from(short),
    };
    let mut mask = [0u8; 4];
    stream.read_exact(&mut mask).expect("mask");
    let mut payload = vec![0u8; length as usize];
    stream.read_exact(&mut payload).expect("payload");
    for (index, byte) in payload.iter_mut().enumerate() {
        *byte ^= mask[index % 4];
    }
    (head[0] & 0x0F, payload)
}

fn server_text(stream: &mut TcpStream, text: &str) {
    let payload = text.as_bytes();
    let mut frame = vec![0x81];
    if payload.len() < 126 {
        frame.push(payload.len() as u8);
    } else {
        frame.push(126);
        frame.extend_from_slice(&(payload.len() as u16).to_be_bytes());
    }
    frame.extend_from_slice(payload);
    stream.write_all(&frame).expect("sent");
}

#[test]
fn an_openai_realtime_session_over_a_loopback_websocket() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("a port");
    let port = listener.local_addr().expect("address").port();
    let (seen_tx, seen_rx) = mpsc::channel::<(String, Vec<Value>)>();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("a connection");
        stream.set_read_timeout(Some(Duration::from_secs(10))).expect("timeout");
        let mut head = Vec::new();
        let mut byte = [0u8; 1];
        while !head.ends_with(b"\r\n\r\n") {
            stream.read_exact(&mut byte).expect("the request");
            head.push(byte[0]);
        }
        let head = String::from_utf8(head).expect("text");
        let key = head
            .lines()
            .find_map(|line| line.strip_prefix("Sec-WebSocket-Key: "))
            .expect("a key")
            .trim()
            .to_owned();
        let answer = format!(
            "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {}\r\n\r\n",
            websocket_accept(&key)
        );
        stream.write_all(answer.as_bytes()).expect("answer");
        server_text(&mut stream, &json!({ "type": "session.created", "session": {} }).to_string());
        let mut messages = Vec::new();
        // The setup, then a piece of sound.
        for _ in 0..2 {
            let (opcode, payload) = read_client_frame(&mut stream);
            assert_eq!(opcode, 1);
            messages.push(serde_json::from_slice::<Value>(&payload).expect("JSON"));
        }
        server_text(&mut stream, &json!({ "type": "input_audio_buffer.speech_started", "item_id": "in_1" }).to_string());
        server_text(
            &mut stream,
            &json!({ "type": "response.function_call_arguments.done", "call_id": "call_1", "name": "list_volumes", "arguments": "{}" })
                .to_string(),
        );
        // The tool's result and the request to go on.
        for _ in 0..2 {
            let (_, payload) = read_client_frame(&mut stream);
            messages.push(serde_json::from_slice::<Value>(&payload).expect("JSON"));
        }
        server_text(
            &mut stream,
            &json!({ "type": "response.output_audio.delta", "item_id": "out_1", "delta": audio_to_base64(&[9, 9, 9]) })
                .to_string(),
        );
        server_text(&mut stream, &json!({ "type": "response.done", "response": { "status": "completed" } }).to_string());
        // A normal close.
        stream.write_all(&[0x88, 2, 0x03, 0xE8]).expect("close");
        let _ = read_client_frame(&mut stream);
        seen_tx.send((head, messages)).expect("reported");
    });

    let mut local = config(LiveFamily::OpenAiRealtime);
    local.base_url = Some(format!("ws://127.0.0.1:{port}/v1/realtime"));
    let mut session = connect_live(&local, key()).expect("connected");
    session.send_audio(&[100; 1600], 16_000).expect("sent");
    let wait = Duration::from_secs(5);
    let mut events = Vec::new();
    loop {
        let Some(event) = session.next_event(wait) else {
            panic!("no event in time after {events:?}");
        };
        if let LiveEvent::ToolCall { call, .. } = &event {
            session
                .send_tool_result(&ToolResult::text(call.id.clone(), call.name.clone(), "vol1"))
                .expect("sent");
        }
        let end = matches!(event, LiveEvent::Closed { .. });
        events.push(event);
        if end {
            break;
        }
    }
    let (head, messages) = seen_rx.recv_timeout(Duration::from_secs(10)).expect("the server's record");
    server.join().expect("the server ended");

    assert!(head.starts_with("GET /v1/realtime?model=gpt-realtime-2.1 HTTP/1.1\r\n"), "{head}");
    assert!(head.contains(&format!("Authorization: Bearer {TEST_KEY}\r\n")));
    assert!(!head.contains("OpenAI-Beta"));
    assert_eq!(messages[0]["type"], "session.update");
    assert_eq!(messages[1]["type"], "input_audio_buffer.append");
    let sent = audio_from_base64(messages[1]["audio"].as_str().expect("text")).expect("base64");
    assert_eq!(sent.len(), 2400, "16 kHz became 24 kHz");
    assert_eq!(messages[2]["item"]["type"], "function_call_output");
    assert_eq!(messages[2]["item"]["output"], "vol1");
    assert_eq!(messages[3]["type"], "response.create");

    assert_eq!(events[0], LiveEvent::SpeechStarted);
    assert!(matches!(&events[1], LiveEvent::ToolCall { call, .. } if call.name == "list_volumes"));
    assert_eq!(events[2], LiveEvent::Audio { samples: vec![9, 9, 9], rate: 24_000 });
    assert_eq!(events[3], LiveEvent::TurnDone);
    assert_eq!(events[4], LiveEvent::Closed { error: None });
}
