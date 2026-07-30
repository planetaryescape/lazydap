use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Capabilities {
    pub supports_configuration_done_request: bool,
    pub supports_function_breakpoints: bool,
    pub supports_conditional_breakpoints: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InitializeArgs {
    #[serde(rename = "clientID", skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_name: Option<String>,
    #[serde(rename = "adapterID", skip_serializing_if = "Option::is_none")]
    pub adapter_id: Option<String>,
    pub lines_start_at1: bool,
    pub columns_start_at1: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path_format: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub locale: Option<String>,
}

impl Default for InitializeArgs {
    /// DAP-conventional defaults: 1-based lines and columns. A derived
    /// `Default` would send `false` for both, which means 0-based indexing and
    /// silently off-by-one breakpoints and frames.
    fn default() -> Self {
        Self {
            client_id: Some("lazydap".into()),
            client_name: Some("lazydap".into()),
            adapter_id: None,
            lines_start_at1: true,
            columns_start_at1: true,
            path_format: Some("path".into()),
            locale: None,
        }
    }
}

impl InitializeArgs {
    /// DAP-conventional defaults with the adapter identity filled in
    /// (`"lldb"` for codelldb, `"debugpy"` for debugpy, ...).
    pub fn new(adapter_id: impl Into<String>) -> Self {
        Self {
            adapter_id: Some(adapter_id.into()),
            ..Self::default()
        }
    }
}

/// codelldb's `launch` arguments: the `launch.json` entry minus the outer
/// `name`. Optional fields are omitted rather than sent as `null`.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchArgs {
    #[serde(rename = "type")]
    pub adapter_type: String,
    pub request: String,
    pub program: String,
    pub args: Vec<String>,
    pub cwd: String,
    pub stop_on_entry: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub env: Option<std::collections::BTreeMap<String, String>>,
    /// codelldb's console selector: `"console"` (the default) keeps the
    /// debuggee attached to the adapter, so its stdout arrives as DAP `output`
    /// events rather than in a separate terminal.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub terminal: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SetBreakpointsArgs {
    pub source: Source,
    pub breakpoints: Vec<SourceBreakpoint>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_modified: Option<bool>,
}

#[derive(Debug, Serialize)]
pub struct Source {
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceBreakpoint {
    pub line: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub column: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub condition: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hit_condition: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub log_message: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetBreakpointsResponse {
    pub breakpoints: Vec<Breakpoint>,
}

/// One breakpoint as the adapter sees it. codelldb verifies lazily: the
/// `setBreakpoints` response can carry `verified: false` and a later
/// `breakpoint` event (reason `"changed"`) flips it, sometimes moving `line`.
/// Match on `id` when reconciling those updates.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Breakpoint {
    pub id: Option<i64>,
    pub verified: bool,
    pub message: Option<String>,
    pub line: Option<u32>,
    pub column: Option<u32>,
}

#[derive(Debug, Default, Serialize)]
pub struct ConfigurationDoneArgs {}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContinueArgs {
    pub thread_id: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DisconnectArgs {
    pub terminate_debuggee: bool,
}

/// An adapter-initiated message. `body` stays untyped here: each event name
/// carries its own shape, and the daemon (M5) decides which ones to model.
#[derive(Debug, Deserialize)]
pub struct DapEvent {
    pub seq: i64,
    #[serde(rename = "type")]
    pub message_type: String,
    pub event: String,
    pub body: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
pub struct DapResponse<R> {
    pub seq: i64,
    pub request_seq: i64,
    #[serde(rename = "type")]
    pub message_type: String,
    pub command: String,
    pub success: bool,
    pub message: Option<String>,
    pub body: Option<R>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserialises_a_capabilities_body() {
        let json = r#"{
            "supportsConfigurationDoneRequest": true,
            "supportsFunctionBreakpoints": true,
            "supportsConditionalBreakpoints": true
        }"#;
        let caps: Capabilities = serde_json::from_str(json).expect("deserialise");
        assert!(caps.supports_configuration_done_request);
    }

    #[test]
    fn serialises_a_initialize_args_body() {
        let init_args: String = serde_json::to_string(&InitializeArgs {
            client_id: Some(String::from("1234")),
            client_name: Some(String::from("lazydap")),
            adapter_id: Some(String::from("lazydap-adapter")),
            lines_start_at1: true,
            columns_start_at1: true,
            path_format: Some(String::from("something")),
            locale: Some(String::from("en")),
        })
        .expect("serialise");
        assert!(
            init_args.contains(r#""clientID":"1234""#),
            "got: {init_args}"
        );
        assert!(
            init_args.contains(r#""adapterID":"lazydap-adapter""#),
            "got: {init_args}"
        );
        assert!(
            init_args.contains(r#""linesStartAt1":true"#),
            "got: {init_args}"
        );
        assert!(
            init_args.contains(r#""columnsStartAt1":true"#),
            "got: {init_args}"
        );
        assert!(
            init_args.contains(r#""pathFormat":"something""#),
            "got: {init_args}"
        );
        assert!(init_args.contains(r#""locale":"en""#), "got: {init_args}");
        assert!(!init_args.contains("client_id"));
        assert!(!init_args.contains(r#""clientId""#));
    }

    #[test]
    fn deserialises_a_full_initialize_response() {
        let json = r#"{
            "seq": 1,
            "request_seq": 1,
            "type": "response",
            "command": "initialize",
            "success": true,
            "body": {
                "supportsConfigurationDoneRequest": true,
                "supportsFunctionBreakpoints": true,
                "supportsConditionalBreakpoints": false
            }
        }"#;
        let resp: DapResponse<Capabilities> = serde_json::from_str(json).expect("deserialise");

        assert_eq!(resp.command, "initialize");
        assert!(resp.success);
        assert!(resp.message.is_none());

        let body = resp.body.expect("body present on success");
        assert!(body.supports_configuration_done_request);
        assert!(body.supports_function_breakpoints);
        assert!(!body.supports_conditional_breakpoints);
    }

    #[test]
    fn initialize_args_default_to_one_based_lines_and_columns() {
        let json = serde_json::to_string(&InitializeArgs::new("lldb")).expect("serialise");
        assert!(json.contains(r#""linesStartAt1":true"#), "got: {json}");
        assert!(json.contains(r#""columnsStartAt1":true"#), "got: {json}");
        assert!(json.contains(r#""adapterID":"lldb""#), "got: {json}");
        assert!(
            !json.contains("null"),
            "None fields must be omitted, got: {json}"
        );
    }

    #[test]
    fn serialises_launch_args_in_codelldb_shape() {
        let json = serde_json::to_string(&LaunchArgs {
            adapter_type: "lldb".into(),
            request: "launch".into(),
            program: "/tmp/hello".into(),
            args: vec![],
            cwd: "/tmp".into(),
            stop_on_entry: false,
            env: None,
            terminal: None,
        })
        .expect("serialise");
        assert!(json.contains(r#""type":"lldb""#), "got: {json}");
        assert!(json.contains(r#""request":"launch""#), "got: {json}");
        assert!(json.contains(r#""stopOnEntry":false"#), "got: {json}");
        assert!(
            !json.contains("env"),
            "None env must be omitted, got: {json}"
        );
        assert!(
            !json.contains("terminal"),
            "None terminal must be omitted, got: {json}"
        );
    }

    #[test]
    fn serialises_set_breakpoints_args_without_null_padding() {
        let json = serde_json::to_string(&SetBreakpointsArgs {
            source: Source {
                path: "/tmp/main.c".into(),
                name: Some("main.c".into()),
            },
            breakpoints: vec![SourceBreakpoint {
                line: 19,
                column: None,
                condition: None,
                hit_condition: None,
                log_message: None,
            }],
            source_modified: None,
        })
        .expect("serialise");
        assert!(json.contains(r#""line":19"#), "got: {json}");
        assert!(json.contains(r#""name":"main.c""#), "got: {json}");
        assert!(
            !json.contains("null"),
            "None fields must be omitted, got: {json}"
        );
    }

    #[test]
    fn deserialises_an_unverified_breakpoint_response() {
        let json = r#"{"breakpoints":[{"id":1,"verified":false}]}"#;
        let resp: SetBreakpointsResponse = serde_json::from_str(json).expect("deserialise");
        let bp = resp.breakpoints.first().expect("one breakpoint");
        assert_eq!(bp.id, Some(1));
        assert!(!bp.verified);
        assert_eq!(bp.line, None);
    }

    #[test]
    fn serialises_continue_args_as_camel_case() {
        let json = serde_json::to_string(&ContinueArgs { thread_id: 1 }).expect("serialise");
        assert_eq!(json, r#"{"threadId":1}"#, "got: {json}");
    }

    #[test]
    fn deserialises_an_event_with_no_body() {
        let json = r#"{"seq":4,"type":"event","event":"initialized"}"#;
        let event: DapEvent = serde_json::from_str(json).expect("deserialise");
        assert_eq!(event.event, "initialized");
        assert!(event.body.is_none());
    }
}
