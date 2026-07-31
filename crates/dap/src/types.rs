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

/// debugpy's `launch` arguments.
///
/// A separate struct from codelldb's [`LaunchArgs`] rather than one type with
/// everything optional. The two adapters agree on the first six fields and on
/// nothing after them, and a shared struct would have to make `terminal`,
/// `console`, `justMyCode` and `subProcess` all optional — at which point
/// nothing stops a codelldb launch from being built with debugpy's fields set,
/// and the compiler has stopped helping. What each adapter sends is part of
/// what that adapter *is*.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PythonLaunchArgs {
    #[serde(rename = "type")]
    pub adapter_type: String,
    pub request: String,
    pub program: String,
    pub args: Vec<String>,
    pub cwd: String,
    pub stop_on_entry: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub env: Option<std::collections::BTreeMap<String, String>>,
    /// Where the debuggee's console goes. `"internalConsole"` is the only
    /// value lazydap can use: every other one makes debugpy ask for a terminal
    /// with a `runInTerminal` reverse request we do not advertise, and the
    /// debuggee's stdout would arrive in that terminal rather than as DAP
    /// `output` events. codelldb spells the same idea `terminal: "console"`.
    pub console: String,
    /// Whether to step over code outside the user's own project.
    ///
    /// debugpy defaults this to `true`, which hides library and stdlib frames.
    /// lazydap sends `false`: its first-class caller is an agent debugging a
    /// failure that is as likely to be in a dependency as in the project, and
    /// a stack that silently omits where the program actually is makes that
    /// failure unfindable.
    pub just_my_code: bool,
    /// Whether debugpy should follow the debuggee's subprocesses.
    ///
    /// `false`, because following one means debugpy asking us to open a second
    /// debug session with a `startDebugging` reverse request, and lazydap runs
    /// one session at a time (D007).
    pub sub_process: bool,
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

/// The response to `continue`. codelldb answers with a body; some adapters
/// send none at all, which is why every field is defaulted.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ContinueResponse {
    pub all_threads_continued: bool,
}

/// `next`, `stepIn` and `stepOut` all take the same arguments. One struct,
/// three commands — the command name is what differs, not the shape.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StepArgs {
    pub thread_id: i64,
    /// `statement`, `line` or `instruction`. Omitted means the adapter's
    /// default, which is what a source-level debugger wants.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub granularity: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PauseArgs {
    pub thread_id: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StackTraceArgs {
    pub thread_id: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_frame: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub levels: Option<u32>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct StackTraceResponse {
    pub stack_frames: Vec<DapStackFrame>,
    pub total_frames: Option<u32>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DapStackFrame {
    pub id: i64,
    pub name: String,
    #[serde(default)]
    pub source: Option<DapSource>,
    #[serde(default)]
    pub line: u32,
    #[serde(default)]
    pub column: u32,
}

/// A DAP source. `path` is absent for code the adapter can only serve by
/// reference — disassembly, inlined or generated code.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct DapSource {
    pub name: Option<String>,
    pub path: Option<String>,
    /// The spec uses `0` to mean "not a reference", which is not the same as
    /// absent; both end up as `None` here.
    pub source_reference: Option<i64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScopesArgs {
    pub frame_id: i64,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ScopesResponse {
    pub scopes: Vec<DapScope>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DapScope {
    pub name: String,
    pub variables_reference: i64,
    #[serde(default)]
    pub expensive: bool,
    #[serde(default)]
    pub named_variables: Option<u32>,
    #[serde(default)]
    pub indexed_variables: Option<u32>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VariablesArgs {
    pub variables_reference: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filter: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub count: Option<u32>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct VariablesResponse {
    pub variables: Vec<DapVariable>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DapVariable {
    pub name: String,
    pub value: String,
    #[serde(rename = "type", default)]
    pub type_name: Option<String>,
    #[serde(default)]
    pub variables_reference: i64,
    #[serde(default)]
    pub named_variables: Option<u32>,
    #[serde(default)]
    pub indexed_variables: Option<u32>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EvaluateArgs {
    pub expression: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub frame_id: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct EvaluateResponse {
    /// DAP calls the value `result` here and `value` on a variable, for the
    /// same thing.
    pub result: String,
    #[serde(rename = "type")]
    pub type_name: Option<String>,
    pub variables_reference: i64,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ThreadsResponse {
    pub threads: Vec<DapThread>,
}

#[derive(Debug, Deserialize)]
pub struct DapThread {
    pub id: i64,
    #[serde(default)]
    pub name: String,
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

/// A request sent by the *adapter* to us — DAP calls these reverse requests.
///
/// There are two in the wild: `runInTerminal`, which asks the client to start
/// the debuggee in a terminal it owns, and `startDebugging`, which asks it to
/// open a second debug session for a subprocess. lazydap advertises support
/// for neither and every launch it builds is configured to avoid provoking
/// them, so one arriving means an adapter asked anyway. It is answered with a
/// refusal rather than ignored: see [`crate::DapWriter::refuse`].
#[derive(Debug, Deserialize)]
pub struct DapRequest {
    pub seq: i64,
    pub command: String,
    pub arguments: Option<serde_json::Value>,
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
    fn deserialises_a_stack_trace_with_a_frame_that_has_no_source_on_disk() {
        // Every stack bottoms out in something without a source file — libc,
        // a thread trampoline — and a stack that failed to parse because of it
        // would be no stack at all.
        let json = r#"{"stackFrames":[
            {"id":1,"name":"main","source":{"name":"main.c","path":"/tmp/main.c"},
             "line":19,"column":5},
            {"id":2,"name":"start","line":0,"column":0}
        ],"totalFrames":2}"#;
        let response: StackTraceResponse = serde_json::from_str(json).expect("deserialise");

        assert_eq!(response.total_frames, Some(2));
        assert_eq!(response.stack_frames[0].name, "main");
        assert!(response.stack_frames[1].source.is_none());
    }

    #[test]
    fn deserialises_a_variable_whose_type_field_dap_spells_differently() {
        let json = r#"{"variables":[
            {"name":"x","value":"5","type":"int","variablesReference":0}
        ]}"#;
        let response: VariablesResponse = serde_json::from_str(json).expect("deserialise");
        let variable = &response.variables[0];

        assert_eq!(variable.type_name.as_deref(), Some("int"));
        assert_eq!(variable.variables_reference, 0, "a scalar has no children");
    }

    #[test]
    fn deserialises_an_evaluate_response_that_carries_no_type() {
        let json = r#"{"result":"12","variablesReference":0}"#;
        let response: EvaluateResponse = serde_json::from_str(json).expect("deserialise");
        assert_eq!(response.result, "12");
        assert!(response.type_name.is_none());
    }

    #[test]
    fn a_continue_response_with_no_body_at_all_still_reads() {
        // Some adapters answer `continue` with a bare success and no body.
        let response: ContinueResponse =
            serde_json::from_value(serde_json::json!({})).expect("deserialise");
        assert!(!response.all_threads_continued);
    }

    #[test]
    fn serialises_step_args_without_a_granularity_we_did_not_ask_for() {
        let json = serde_json::to_string(&StepArgs {
            thread_id: 1,
            granularity: None,
        })
        .expect("serialise");
        assert_eq!(json, r#"{"threadId":1}"#, "got: {json}");
    }

    #[test]
    fn serialises_variables_args_with_only_the_fields_that_were_set() {
        let json = serde_json::to_string(&VariablesArgs {
            variables_reference: 1001,
            filter: Some("indexed".into()),
            start: Some(0),
            count: None,
        })
        .expect("serialise");
        assert!(json.contains(r#""variablesReference":1001"#), "got: {json}");
        assert!(json.contains(r#""filter":"indexed""#), "got: {json}");
        assert!(!json.contains("count"), "got: {json}");
    }

    #[test]
    fn deserialises_an_event_with_no_body() {
        let json = r#"{"seq":4,"type":"event","event":"initialized"}"#;
        let event: DapEvent = serde_json::from_str(json).expect("deserialise");
        assert_eq!(event.event, "initialized");
        assert!(event.body.is_none());
    }
}
