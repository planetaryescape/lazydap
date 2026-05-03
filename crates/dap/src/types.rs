use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Capabilities {
    pub supports_configuration_done_request: bool,
    pub supports_function_breakpoints: bool,
    pub supports_conditional_breakpoints: bool,
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InitializeArgs {
    #[serde(rename = "clientID")]
    pub client_id: Option<String>,
    pub client_name: Option<String>,
    #[serde(rename = "adapterID")]
    pub adapter_id: Option<String>,
    pub lines_start_at1: bool,
    pub columns_start_at1: bool,
    pub path_format: Option<String>,
    pub locale: Option<String>,
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
        }).expect("serialise");
        assert!(init_args.contains(r#""clientID":"1234""#), "got: {init_args}");
        assert!(init_args.contains(r#""adapterID":"lazydap-adapter""#), "got: {init_args}");
        assert!(init_args.contains(r#""linesStartAt1":true"#), "got: {init_args}");
        assert!(init_args.contains(r#""columnsStartAt1":true"#), "got: {init_args}");
        assert!(init_args.contains(r#""pathFormat":"something""#), "got: {init_args}");
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
        let resp: DapResponse<Capabilities> =
            serde_json::from_str(json).expect("deserialise");

        assert_eq!(resp.command, "initialize");
        assert!(resp.success);
        assert!(resp.message.is_none());

        let body = resp.body.expect("body present on success");
        assert!(body.supports_configuration_done_request);
        assert!(body.supports_function_breakpoints);
        assert!(!body.supports_conditional_breakpoints);
    }

}
