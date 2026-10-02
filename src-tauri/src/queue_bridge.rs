use crate::queue_state::{ProjectId, ProjectStateInput, QueueMutation, QueueRuntime};
use std::io::Read;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::thread::{self, JoinHandle};
use std::time::Duration;
use tauri::{AppHandle, Emitter};
use tiny_http::{Header, Method, Response, Server, StatusCode};

const BRIDGE_ADDRESS: &str = "127.0.0.1:52780";
const MAX_REQUEST_BODY_BYTES: u64 = 8 * 1024;

#[derive(Clone, Default)]
pub(crate) struct QueueBridge {
    shutdown: Arc<AtomicBool>,
}

impl QueueBridge {
    pub(crate) fn start(
        &self,
        app: AppHandle,
        runtime: QueueRuntime,
    ) -> Result<JoinHandle<()>, String> {
        let server = Server::http(BRIDGE_ADDRESS).map_err(|error| error.to_string())?;
        self.shutdown.store(false, Ordering::SeqCst);
        let shutdown = Arc::clone(&self.shutdown);
        Ok(thread::spawn(move || {
            while !shutdown.load(Ordering::SeqCst) {
                match server.recv_timeout(Duration::from_millis(200)) {
                    Ok(Some(request)) => handle_request(request, &app, &runtime),
                    Ok(None) => {}
                    Err(error) => eprintln!("queue bridge receive error: {error}"),
                }
            }
        }))
    }

    pub(crate) fn shutdown(&self) {
        self.shutdown.store(true, Ordering::SeqCst);
    }
}

pub(crate) fn emit_mutation(app: &AppHandle, mutation: QueueMutation) {
    if let Err(error) = app.emit("queue-state-updated", mutation.projection) {
        eprintln!("failed to emit queue update: {error}");
    }
    if let Some(completed) = mutation.completed {
        if let Err(error) = app.emit("queue-project-completed", completed) {
            eprintln!("failed to emit queue completion: {error}");
        }
    }
}

fn handle_request(mut request: tiny_http::Request, app: &AppHandle, runtime: &QueueRuntime) {
    let method = request.method().clone();
    let path = request.url().split('?').next().unwrap_or(request.url());
    let response = match (method, path) {
        (Method::Get, "/health") => json_response(200, &serde_json::json!({ "ok": true })),
        (Method::Get, "/state") => json_response(200, &runtime.projection()),
        (Method::Put, path) => match project_from_path(path) {
            Some(project) => match read_state_input(&mut request) {
                Ok(input) => match runtime.replace(project, input) {
                    Ok(mutation) => {
                        let projection = mutation.projection.clone();
                        emit_mutation(app, mutation);
                        json_response(200, &projection)
                    }
                    Err(error) => error_response(400, &error),
                },
                Err(error) => error_response(400, &error),
            },
            None => route_error(path),
        },
        (Method::Delete, path) => match project_from_path(path) {
            Some(project) => {
                let mutation = runtime.clear(project);
                let projection = mutation.projection.clone();
                emit_mutation(app, mutation);
                json_response(200, &projection)
            }
            None => route_error(path),
        },
        (_, "/health") | (_, "/state") | (_, "/state/noctua") | (_, "/state/fgo") => {
            error_response(405, "method not allowed")
        }
        _ => error_response(404, "not found"),
    };
    let _ = request.respond(response);
}

fn project_from_path(path: &str) -> Option<ProjectId> {
    ProjectId::parse(path.strip_prefix("/state/")?)
}

fn route_error(path: &str) -> Response<std::io::Cursor<Vec<u8>>> {
    if path.starts_with("/state/") {
        error_response(404, "unknown project")
    } else {
        error_response(404, "not found")
    }
}

fn read_state_input(request: &mut tiny_http::Request) -> Result<ProjectStateInput, String> {
    let is_json = request.headers().iter().any(|header| {
        header.field.equiv("content-type")
            && header
                .value
                .as_str()
                .split(';')
                .next()
                .is_some_and(|value| value.trim().eq_ignore_ascii_case("application/json"))
    });
    if !is_json {
        return Err("Content-Type must be application/json".to_string());
    }
    let mut bytes = Vec::new();
    request
        .as_reader()
        .take(MAX_REQUEST_BODY_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() as u64 > MAX_REQUEST_BODY_BYTES {
        return Err("request body exceeds 8 KiB limit".to_string());
    }
    serde_json::from_slice(&bytes).map_err(|error| format!("invalid JSON: {error}"))
}

fn json_response(status: u16, value: &impl serde::Serialize) -> Response<std::io::Cursor<Vec<u8>>> {
    let body = serde_json::to_vec(value)
        .unwrap_or_else(|_| b"{\"error\":\"serialization failure\"}".to_vec());
    Response::from_data(body)
        .with_status_code(StatusCode(status))
        .with_header(Header::from_bytes("Content-Type", "application/json; charset=utf-8").unwrap())
}

fn error_response(status: u16, message: &str) -> Response<std::io::Cursor<Vec<u8>>> {
    json_response(status, &serde_json::json!({ "error": message }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_paths_are_fixed() {
        assert_eq!(project_from_path("/state/noctua"), Some(ProjectId::Noctua));
        assert_eq!(project_from_path("/state/fgo"), Some(ProjectId::Fgo));
        assert_eq!(project_from_path("/state/other"), None);
    }
}
