//! Portable, read-only native tools. No filesystem, network, or process access.
use spine_protocol::body::{ERR_INVALID_ARGS, ERR_PARSE_ERROR, ERR_UNSUPPORTED_TOOL};
use spine_protocol::{ToolCall, ToolOutcome};

pub fn capabilities() -> Vec<spine_protocol::Capability> {
    [("spine.html/parse","Extract the structured semantic representation of supplied HTML"),
     ("spine.content/hash","Return the SHA-256 hash of supplied UTF-8 text")]
        .into_iter().map(|(uri,description)|spine_protocol::Capability {
            uri:uri.into(),description:description.into(),embedding:None,
            input_schema:serde_json::json!({"type":"object","required":["text"],
                "properties":{"text":{"type":"string","maxLength":1048576}},
                "x-spine-max-utf8-bytes":1048576,"x-spine-version":1,"x-spine-permission":"tool.read"}),
            output_schema:serde_json::json!({"type":"object"}),
        }).collect()
}

pub fn invoke(call: ToolCall) -> ToolOutcome {
    let result = (|| -> Result<serde_json::Value, (&str, &str)> {
        let text = call
            .args
            .get("text")
            .and_then(serde_json::Value::as_str)
            .ok_or((ERR_INVALID_ARGS, "text must be a string"))?;
        if text.len() > 1024 * 1024 {
            return Err((ERR_INVALID_ARGS, "tool text exceeds 1 MiB"));
        }
        match call.name.as_str() {
            "spine.content/hash" => {
                let hash = spine_name::content_hash(text.as_bytes());
                let hex: String = hash.iter().map(|b| format!("{b:02x}")).collect();
                Ok(serde_json::json!({"sha256": hex, "bytes":text.len()}))
            }
            "spine.html/parse" => {
                let parsed = spine_parser::parse_html(text)
                    .map_err(|_| (ERR_PARSE_ERROR, "HTML parsing failed"))?;
                serde_json::to_value(parsed)
                    .map_err(|_| (spine_protocol::body::ERR_INTERNAL, "result encoding failed"))
            }
            _ => Err((ERR_UNSUPPORTED_TOOL, "tool is not available on this origin")),
        }
    })();
    match result {
        Ok(content) => ToolOutcome::Ok { content },
        Err((code, message)) => ToolOutcome::Err {
            code: code.into(),
            message: message.into(),
            details: None,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tools_reject_unbounded_or_unsupported_work() {
        let call = |name: &str, text: &str| ToolCall {
            name: name.into(),
            args: serde_json::json!({"text":text}),
            trace: None,
        };
        let result = invoke(call("spine.content/hash", "abc"));
        let ToolOutcome::Ok { content } = result else {
            panic!("hash failed")
        };
        assert_eq!(
            content["sha256"],
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert!(matches!(
            invoke(call("shell", "abc")),
            ToolOutcome::Err { ref code, .. } if code == ERR_UNSUPPORTED_TOOL
        ));
        assert!(matches!(
            invoke(call("spine.html/parse", &"a".repeat(1024 * 1024 + 1))),
            ToolOutcome::Err { ref code, .. } if code == ERR_INVALID_ARGS
        ));
    }
}
