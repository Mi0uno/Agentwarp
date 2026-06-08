use std::collections::HashMap;
use std::env;
use std::fs::{create_dir_all, OpenOptions};
use std::io::Write;
use std::net::SocketAddr;
use std::path::Path;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use axum::body::{Body, Bytes};
use axum::extract::{OriginalUri, State};
use axum::http::header::{self, HeaderName};
use axum::http::{HeaderMap, Method, StatusCode, Uri};
use axum::response::Response;
use axum::{routing::any, Router};
use serde::{Deserialize, Serialize};
use tokio::net::TcpListener;
use tokio::process::Command;

#[derive(Clone, Debug, Deserialize)]
struct ApiProfile {
    #[serde(default)]
    id: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    agent: String,
    #[serde(default)]
    base_url: String,
    #[serde(default)]
    full_url_mode: bool,
    #[serde(default)]
    api_key: String,
    #[serde(default)]
    model: String,
    #[serde(default)]
    model_mappings: Vec<ModelMapping>,
    #[serde(default)]
    extra_env: HashMap<String, String>,
    #[serde(default)]
    input_cost_per_million_tokens: f64,
    #[serde(default)]
    output_cost_per_million_tokens: f64,
    #[serde(default = "default_enabled")]
    enabled: bool,
}

#[derive(Clone, Debug, Deserialize)]
struct ModelMapping {
    #[serde(default)]
    role: String,
    #[serde(default)]
    display_name: String,
    #[serde(default)]
    model: String,
}

#[derive(Clone)]
struct ProxyState {
    client: reqwest::Client,
    profiles: Arc<Vec<ApiProfile>>,
    usage_log_path: Option<Arc<String>>,
}

#[derive(Debug, Serialize)]
struct UsageEvent {
    timestamp_epoch_ms: i64,
    profile_id: String,
    profile_name: String,
    agent: String,
    method: String,
    path: String,
    status: u16,
    success: bool,
    retryable: bool,
    final_attempt: bool,
    attempt: usize,
    latency_ms: u64,
    request_bytes: usize,
    response_bytes: usize,
    prompt_tokens: u64,
    completion_tokens: u64,
    total_tokens: u64,
    estimated_cost_usd: f64,
    error: String,
}

#[derive(Clone, Copy, Debug, Default)]
struct TokenUsage {
    prompt_tokens: u64,
    completion_tokens: u64,
    total_tokens: u64,
}

fn default_enabled() -> bool {
    true
}

fn command_args() -> Vec<String> {
    let args = env::args().skip(1).collect::<Vec<_>>();
    if matches!(args.first().map(String::as_str), Some("--")) {
        args.into_iter().skip(1).collect()
    } else {
        args
    }
}

fn fallback_profiles_from_env() -> Vec<ApiProfile> {
    env::var("AGENTWARP_AGENT_API_FALLBACKS")
        .ok()
        .and_then(|value| serde_json::from_str::<Vec<ApiProfile>>(&value).ok())
        .unwrap_or_default()
        .into_iter()
        .filter(|profile| profile.enabled && !profile.base_url.trim().is_empty())
        .collect()
}

fn usage_log_path_from_env() -> Option<Arc<String>> {
    env::var("AGENTWARP_AGENT_API_USAGE_LOG")
        .ok()
        .map(|path| path.trim().to_owned())
        .filter(|path| !path.is_empty())
        .map(Arc::new)
}

fn now_epoch_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(i64::MAX as u128) as i64)
        .unwrap_or_default()
}

fn append_usage_event(log_path: &Option<Arc<String>>, event: UsageEvent) {
    let Some(log_path) = log_path else {
        return;
    };
    let path = Path::new(log_path.as_str());
    if let Some(parent) = path.parent() {
        if let Err(error) = create_dir_all(parent) {
            eprintln!("agentwarp-agent-api-proxy: failed to create usage log dir: {error}");
            return;
        }
    }

    let mut file = match OpenOptions::new().create(true).append(true).open(path) {
        Ok(file) => file,
        Err(error) => {
            eprintln!("agentwarp-agent-api-proxy: failed to open usage log: {error}");
            return;
        }
    };
    if serde_json::to_writer(&mut file, &event).is_ok() {
        let _ = writeln!(file);
    }
}

fn token_usage_value(value: &serde_json::Value, keys: &[&str]) -> u64 {
    keys.iter()
        .find_map(|key| value.get(*key).and_then(serde_json::Value::as_u64))
        .unwrap_or_default()
}

fn response_token_usage(response_bytes: &Bytes) -> TokenUsage {
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(response_bytes) else {
        return TokenUsage::default();
    };
    let usage = value
        .get("usage")
        .or_else(|| value.get("usageMetadata"))
        .unwrap_or(&value);
    let prompt_tokens = token_usage_value(
        usage,
        &["prompt_tokens", "input_tokens", "promptTokenCount"],
    );
    let completion_tokens = token_usage_value(
        usage,
        &["completion_tokens", "output_tokens", "candidatesTokenCount"],
    );
    let total_tokens =
        token_usage_value(usage, &["total_tokens", "totalTokens", "totalTokenCount"]);
    TokenUsage {
        prompt_tokens,
        completion_tokens,
        total_tokens: if total_tokens == 0 {
            prompt_tokens.saturating_add(completion_tokens)
        } else {
            total_tokens
        },
    }
}

fn profile_estimated_cost_usd(profile: &ApiProfile, usage: TokenUsage) -> f64 {
    let input_cost = if profile.input_cost_per_million_tokens.is_finite()
        && profile.input_cost_per_million_tokens > 0.0
    {
        profile.input_cost_per_million_tokens
    } else {
        0.0
    };
    let output_cost = if profile.output_cost_per_million_tokens.is_finite()
        && profile.output_cost_per_million_tokens > 0.0
    {
        profile.output_cost_per_million_tokens
    } else {
        0.0
    };
    let cost = (usage.prompt_tokens as f64 * input_cost
        + usage.completion_tokens as f64 * output_cost)
        / 1_000_000.0;
    if cost.is_finite() && cost > 0.0 {
        cost
    } else {
        0.0
    }
}

fn should_retry(status: StatusCode) -> bool {
    status == StatusCode::TOO_MANY_REQUESTS
        || status == StatusCode::REQUEST_TIMEOUT
        || status == StatusCode::CONFLICT
        || status.is_server_error()
}

fn is_hop_by_hop_header(name: &HeaderName) -> bool {
    matches!(
        name.as_str().to_ascii_lowercase().as_str(),
        "connection"
            | "keep-alive"
            | "proxy-authenticate"
            | "proxy-authorization"
            | "te"
            | "trailer"
            | "transfer-encoding"
            | "upgrade"
    )
}

fn should_forward_request_header(name: &HeaderName) -> bool {
    !is_hop_by_hop_header(name)
        && name != header::HOST
        && name != header::CONTENT_LENGTH
        && name != header::AUTHORIZATION
        && name.as_str().to_ascii_lowercase() != "x-api-key"
        && name.as_str().to_ascii_lowercase() != "x-goog-api-key"
}

fn should_forward_response_header(name: &HeaderName) -> bool {
    !is_hop_by_hop_header(name) && name != header::CONTENT_LENGTH
}

fn target_url(profile: &ApiProfile, uri: &Uri) -> String {
    let base_url = profile.base_url.trim().trim_end_matches('/');
    if profile.full_url_mode {
        return base_url.to_owned();
    }
    let path_and_query = uri
        .path_and_query()
        .map(|value| value.as_str())
        .unwrap_or("/");
    if base_url.ends_with("/v1") && path_and_query.starts_with("/v1/") {
        format!("{}{}", base_url.trim_end_matches("/v1"), path_and_query)
    } else {
        format!("{base_url}{path_and_query}")
    }
}

fn preferred_model(profile: &ApiProfile) -> Option<String> {
    if !profile.model.trim().is_empty() {
        return Some(profile.model.trim().to_owned());
    }
    profile
        .model_mappings
        .iter()
        .find(|mapping| mapping.role.eq_ignore_ascii_case("sonnet"))
        .or_else(|| {
            profile
                .model_mappings
                .iter()
                .find(|mapping| mapping.role.eq_ignore_ascii_case("default"))
        })
        .or_else(|| {
            profile
                .model_mappings
                .iter()
                .find(|mapping| !mapping.model.trim().is_empty())
        })
        .map(|mapping| mapping.model.trim().to_owned())
}

fn model_mapping_matches(mapping: &ModelMapping, requested_model: &str) -> bool {
    let requested_model = requested_model.trim().to_ascii_lowercase();
    if requested_model.is_empty() {
        return false;
    }
    for candidate in [&mapping.role, &mapping.display_name, &mapping.model] {
        let candidate = candidate.trim().to_ascii_lowercase();
        if candidate.is_empty() {
            continue;
        }
        if requested_model == candidate || requested_model.contains(&candidate) {
            return true;
        }
    }
    false
}

fn mapped_model(profile: &ApiProfile, requested_model: &str) -> Option<String> {
    profile
        .model_mappings
        .iter()
        .find(|mapping| {
            !mapping.model.trim().is_empty() && model_mapping_matches(mapping, requested_model)
        })
        .map(|mapping| mapping.model.trim().to_owned())
}

fn rewrite_request_body(profile: &ApiProfile, body: &Bytes) -> Bytes {
    let Ok(mut value) = serde_json::from_slice::<serde_json::Value>(body) else {
        return body.clone();
    };
    let Some(object) = value.as_object_mut() else {
        return body.clone();
    };

    let current_model = object
        .get("model")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    let next_model = if current_model.trim().is_empty() {
        preferred_model(profile)
    } else {
        mapped_model(profile, current_model)
    };
    let Some(next_model) = next_model.filter(|model| !model.trim().is_empty()) else {
        return body.clone();
    };
    object.insert("model".to_owned(), serde_json::Value::String(next_model));
    serde_json::to_vec(&value)
        .map(Bytes::from)
        .unwrap_or_else(|_| body.clone())
}

fn add_profile_auth(
    request: reqwest::RequestBuilder,
    profile: &ApiProfile,
) -> reqwest::RequestBuilder {
    let api_key = profile.api_key.trim();
    if api_key.is_empty() {
        return request;
    }

    let agent = profile.agent.to_ascii_lowercase();
    if agent.contains("claude") {
        request.header("x-api-key", api_key)
    } else if agent.contains("gemini") {
        request.header("x-goog-api-key", api_key)
    } else {
        request.bearer_auth(api_key)
    }
}

fn error_response(status: StatusCode, message: impl Into<String>) -> Response {
    let body = serde_json::json!({
        "error": {
            "message": message.into(),
            "type": "agentwarp_agent_api_proxy_error"
        }
    });
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_string()))
        .unwrap_or_else(|_| Response::new(Body::empty()))
}

async fn proxy_request(
    State(state): State<ProxyState>,
    method: Method,
    OriginalUri(original_uri): OriginalUri,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let mut last_error = None;
    let request_path = original_uri
        .path_and_query()
        .map(|value| value.as_str().to_owned())
        .unwrap_or_else(|| "/".to_owned());
    let profile_count = state.profiles.len();

    for (attempt_index, profile) in state.profiles.iter().enumerate() {
        let attempt = attempt_index + 1;
        let final_attempt = attempt == profile_count;
        let request_started = Instant::now();
        let url = target_url(profile, &original_uri);
        let request_body = rewrite_request_body(profile, &body);
        let request_bytes = request_body.len();
        let mut request = state.client.request(method.clone(), url).body(request_body);
        for (name, value) in headers.iter() {
            if should_forward_request_header(name) {
                request = request.header(name, value);
            }
        }
        for (name, value) in &profile.extra_env {
            if let Some(header_name) = name.strip_prefix("header:") {
                if !header_name.trim().is_empty() && !value.trim().is_empty() {
                    request = request.header(header_name.trim(), value.trim());
                }
            }
        }
        request = add_profile_auth(request, profile);

        match request.send().await {
            Ok(response) => {
                let status = response.status();
                let response_headers = response.headers().clone();
                let response_bytes = match response.bytes().await {
                    Ok(bytes) => bytes,
                    Err(error) => {
                        let latency_ms =
                            request_started.elapsed().as_millis().min(u64::MAX as u128) as u64;
                        append_usage_event(
                            &state.usage_log_path,
                            UsageEvent {
                                timestamp_epoch_ms: now_epoch_ms(),
                                profile_id: profile.id.clone(),
                                profile_name: profile.name.clone(),
                                agent: profile.agent.clone(),
                                method: method.to_string(),
                                path: request_path.clone(),
                                status: status.as_u16(),
                                success: false,
                                retryable: true,
                                final_attempt,
                                attempt,
                                latency_ms,
                                request_bytes,
                                response_bytes: 0,
                                prompt_tokens: 0,
                                completion_tokens: 0,
                                total_tokens: 0,
                                estimated_cost_usd: 0.0,
                                error: format!("failed while reading response body: {error}"),
                            },
                        );
                        last_error = Some(format!(
                            "{} failed while reading response body: {error}",
                            profile.name
                        ));
                        continue;
                    }
                };
                let token_usage = response_token_usage(&response_bytes);
                let estimated_cost_usd = profile_estimated_cost_usd(profile, token_usage);

                if should_retry(status) {
                    let latency_ms =
                        request_started.elapsed().as_millis().min(u64::MAX as u128) as u64;
                    append_usage_event(
                        &state.usage_log_path,
                        UsageEvent {
                            timestamp_epoch_ms: now_epoch_ms(),
                            profile_id: profile.id.clone(),
                            profile_name: profile.name.clone(),
                            agent: profile.agent.clone(),
                            method: method.to_string(),
                            path: request_path.clone(),
                            status: status.as_u16(),
                            success: false,
                            retryable: true,
                            final_attempt,
                            attempt,
                            latency_ms,
                            request_bytes,
                            response_bytes: response_bytes.len(),
                            prompt_tokens: token_usage.prompt_tokens,
                            completion_tokens: token_usage.completion_tokens,
                            total_tokens: token_usage.total_tokens,
                            estimated_cost_usd,
                            error: format!("retryable status {status}"),
                        },
                    );
                    last_error = Some(format!(
                        "{} returned retryable status {status}",
                        profile.name
                    ));
                    continue;
                }

                let mut builder = Response::builder().status(status);
                for (name, value) in response_headers.iter() {
                    if should_forward_response_header(name) {
                        builder = builder.header(name, value);
                    }
                }
                let latency_ms = request_started.elapsed().as_millis().min(u64::MAX as u128) as u64;
                append_usage_event(
                    &state.usage_log_path,
                    UsageEvent {
                        timestamp_epoch_ms: now_epoch_ms(),
                        profile_id: profile.id.clone(),
                        profile_name: profile.name.clone(),
                        agent: profile.agent.clone(),
                        method: method.to_string(),
                        path: request_path.clone(),
                        status: status.as_u16(),
                        success: status.is_success(),
                        retryable: false,
                        final_attempt: true,
                        attempt,
                        latency_ms,
                        request_bytes,
                        response_bytes: response_bytes.len(),
                        prompt_tokens: token_usage.prompt_tokens,
                        completion_tokens: token_usage.completion_tokens,
                        total_tokens: token_usage.total_tokens,
                        estimated_cost_usd,
                        error: String::new(),
                    },
                );
                return builder
                    .body(Body::from(response_bytes))
                    .unwrap_or_else(|_| Response::new(Body::empty()));
            }
            Err(error) => {
                let latency_ms = request_started.elapsed().as_millis().min(u64::MAX as u128) as u64;
                let profile_label = if profile.name.trim().is_empty() {
                    profile.id.as_str()
                } else {
                    profile.name.as_str()
                };
                append_usage_event(
                    &state.usage_log_path,
                    UsageEvent {
                        timestamp_epoch_ms: now_epoch_ms(),
                        profile_id: profile.id.clone(),
                        profile_name: profile.name.clone(),
                        agent: profile.agent.clone(),
                        method: method.to_string(),
                        path: request_path.clone(),
                        status: 0,
                        success: false,
                        retryable: true,
                        final_attempt,
                        attempt,
                        latency_ms,
                        request_bytes,
                        response_bytes: 0,
                        prompt_tokens: 0,
                        completion_tokens: 0,
                        total_tokens: 0,
                        estimated_cost_usd: 0.0,
                        error: error.to_string(),
                    },
                );
                last_error = Some(format!("{profile_label} request failed: {error}"));
            }
        }
    }

    error_response(
        StatusCode::BAD_GATEWAY,
        last_error.unwrap_or_else(|| "No usable Agent API profile is configured".to_owned()),
    )
}

fn proxy_env_vars(agent: &str, proxy_url: &str) -> Vec<(&'static str, String)> {
    let agent = agent.to_ascii_lowercase();
    let mut vars = vec![
        ("AGENTWARP_AGENT_API_PROXY_URL", proxy_url.to_owned()),
        ("AGENTWARP_AGENT_API_PROXY_ACTIVE", "1".to_owned()),
    ];

    if agent.contains("claude") {
        vars.push(("ANTHROPIC_BASE_URL", proxy_url.to_owned()));
    } else if agent.contains("gemini") {
        vars.push(("GOOGLE_GEMINI_BASE_URL", proxy_url.to_owned()));
    } else {
        vars.push(("OPENAI_BASE_URL", proxy_url.to_owned()));
    }

    vars
}

#[tokio::main]
async fn main() -> ExitCode {
    let args = command_args();
    if args.is_empty() {
        eprintln!("usage: agentwarp-agent-api-proxy -- <agent-command> [args...]");
        return ExitCode::from(2);
    }

    let profiles = fallback_profiles_from_env();
    if profiles.is_empty() {
        eprintln!("agentwarp-agent-api-proxy: no usable fallback profiles; launching directly");
        return launch_agent(args, None).await;
    }

    let listener = match TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0))).await {
        Ok(listener) => listener,
        Err(error) => {
            eprintln!("agentwarp-agent-api-proxy: failed to bind local proxy: {error}");
            return launch_agent(args, None).await;
        }
    };
    let proxy_url = match listener.local_addr() {
        Ok(addr) => format!("http://{addr}"),
        Err(error) => {
            eprintln!("agentwarp-agent-api-proxy: failed to read local proxy address: {error}");
            return launch_agent(args, None).await;
        }
    };

    let state = ProxyState {
        client: reqwest::Client::new(),
        profiles: Arc::new(profiles),
        usage_log_path: usage_log_path_from_env(),
    };
    let router = Router::new().fallback(any(proxy_request)).with_state(state);
    let server = tokio::spawn(async move {
        if let Err(error) = axum::serve(listener, router).await {
            eprintln!("agentwarp-agent-api-proxy: local proxy stopped: {error}");
        }
    });

    let exit_code = launch_agent(args, Some(proxy_url)).await;
    server.abort();
    exit_code
}

async fn launch_agent(args: Vec<String>, proxy_url: Option<String>) -> ExitCode {
    let mut command = Command::new(&args[0]);
    command.args(&args[1..]);

    if let Some(proxy_url) = proxy_url {
        let agent = env::var("AGENTWARP_AGENT_API_AGENT").unwrap_or_default();
        for (key, value) in proxy_env_vars(&agent, &proxy_url) {
            command.env(key, value);
        }
    }

    match command.status().await {
        Ok(status) => ExitCode::from(status.code().unwrap_or(1) as u8),
        Err(error) => {
            eprintln!(
                "agentwarp-agent-api-proxy: failed to launch {}: {error}",
                args[0]
            );
            ExitCode::from(127)
        }
    }
}
