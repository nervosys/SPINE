//! Model calls through IronGate, when one is configured.
//!
//! The stack is modular: a component *"can choose to or choose not to
//! connect"* to another. This is SPINE's choice to offer, with the contract
//! Rodeo (`rodeo-core/src/gateway_route.rs`) and IronBridge adopted, so the
//! same three variables mean the same thing across the stack. `ironstack
//! coverage` reported SPINE naming provider hosts with no way to route through
//! IronGate at all; this is that way.
//!
//! # How
//!
//! Set `IRONGATE_URL` to the gateway's base (`http://localhost:7700`) and,
//! when the gateway requires one, `IRONGATE_TOKEN`. Every remote dispatcher
//! then sends its calls to `{IRONGATE_URL}/v1/chat/completions` -- IronGate's
//! OpenAI-compatible API, the only one it serves -- with the model name passed
//! through for the gateway to route by, and no provider key: the gateway holds
//! them. [`crate::AnthropicDispatcher`] speaks Anthropic's API directly, so
//! through the gateway its request is translated into the chat shape; the text
//! the model is shown is the same either way.
//!
//! # What stays direct
//!
//! A dispatcher whose base URL is a loopback address -- an `OpenAiConfig`
//! pointed at a local vLLM, llama.cpp or Ollama server. That never leaves the
//! machine, and the gateway would add a hop without adding a boundary.
//!
//! # Without a gateway
//!
//! Unset, calls go direct exactly as before -- connecting is a choice. A
//! deployment that must not reach a provider directly sets
//! `SPINE_REQUIRE_GATEWAY=1`, and then a remote call without `IRONGATE_URL` is
//! an error naming both variables, rather than a direct call.

/// Where a gateway is, read once from the environment.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Settings {
    /// `IRONGATE_URL`, without a trailing slash. `None` when unset or empty.
    pub url: Option<String>,
    /// `IRONGATE_TOKEN`, sent as a bearer token to the gateway when present.
    pub token: Option<String>,
    /// `SPINE_REQUIRE_GATEWAY=1`: refuse a remote call that would go direct.
    pub required: bool,
}

impl Settings {
    /// Read the three variables from the process environment.
    pub fn from_env() -> Self {
        Self::from_vars(|k| std::env::var(k).ok())
    }

    /// Read them through `var`, so a test states the environment instead of
    /// mutating the process's.
    pub fn from_vars(var: impl Fn(&str) -> Option<String>) -> Self {
        let nonempty = |k: &str| {
            var(k)
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty())
        };
        Settings {
            url: nonempty("IRONGATE_URL").map(|u| u.trim_end_matches('/').to_string()),
            token: nonempty("IRONGATE_TOKEN"),
            required: var("SPINE_REQUIRE_GATEWAY").is_some_and(|v| v.trim() == "1"),
        }
    }

    /// Where a call to a provider at `direct_base` goes.
    ///
    /// `provider` names the dispatcher in the refusal, so an operator reading
    /// it knows which call was stopped.
    pub fn route(&self, direct_base: &str, provider: &str) -> Result<Route, String> {
        if is_loopback(direct_base) {
            return Ok(Route::Direct);
        }
        match &self.url {
            // Trimmed here as well as on read: `Settings` is public, and a
            // base built in code with a trailing slash would otherwise send
            // `//v1/...`, which the integration test caught.
            Some(base) => Ok(Route::Gateway {
                endpoint: format!("{}/v1/chat/completions", base.trim_end_matches('/')),
                token: self.token.clone(),
            }),
            None if self.required => Err(format!(
                "SPINE_REQUIRE_GATEWAY is set and IRONGATE_URL is not, so the {provider} \
                 dispatcher would reach {direct_base} directly. Set IRONGATE_URL to route \
                 through IronGate, or unset SPINE_REQUIRE_GATEWAY to allow direct calls."
            )),
            None => Ok(Route::Direct),
        }
    }
}

/// Where one call goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Route {
    /// To the provider, as before.
    Direct,
    /// To IronGate's chat endpoint, with its bearer token when one is set.
    Gateway {
        endpoint: String,
        token: Option<String>,
    },
}

/// Whether a base URL names this machine: `localhost`, `127.x.x.x` or `[::1]`.
pub fn is_loopback(base: &str) -> bool {
    let rest = base.split_once("://").map(|(_, r)| r).unwrap_or(base);
    let authority = rest.split('/').next().unwrap_or("");
    let authority = authority
        .rsplit_once('@')
        .map(|(_, h)| h)
        .unwrap_or(authority);
    let host = if let Some(v6) = authority.strip_prefix('[') {
        v6.split(']').next().unwrap_or("")
    } else {
        authority.split(':').next().unwrap_or("")
    };
    host.eq_ignore_ascii_case("localhost")
        || host == "::1"
        || host
            .parse::<std::net::Ipv4Addr>()
            .is_ok_and(|ip| ip.is_loopback())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(vars: &[(&str, &str)]) -> Settings {
        let vars: Vec<(String, String)> = vars
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        Settings::from_vars(|k| vars.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone()))
    }

    const OPENAI: &str = "https://api.example-provider.test/v1";

    #[test]
    fn unset_goes_direct_exactly_as_before() {
        let s = settings(&[]);
        assert_eq!(s, Settings::default());
        assert_eq!(s.route(OPENAI, "openai"), Ok(Route::Direct));
    }

    #[test]
    fn a_gateway_url_routes_every_remote_call_to_its_chat_endpoint() {
        let s = settings(&[
            ("IRONGATE_URL", "http://gate.internal:7700/"),
            ("IRONGATE_TOKEN", "t0k"),
        ]);
        assert_eq!(
            s.route(OPENAI, "openai"),
            Ok(Route::Gateway {
                endpoint: "http://gate.internal:7700/v1/chat/completions".into(),
                token: Some("t0k".into()),
            }),
            "a trailing slash is trimmed and the token carried"
        );
    }

    #[test]
    fn a_local_model_server_stays_direct_even_with_a_gateway() {
        let s = settings(&[("IRONGATE_URL", "http://gate:7700")]);
        for local in [
            "http://localhost:8000/v1",
            "http://127.0.0.1:11434/v1",
            "http://[::1]:8080/v1",
            "http://user@127.0.0.2/v1",
        ] {
            assert_eq!(s.route(local, "openai"), Ok(Route::Direct), "{local}");
        }
        assert!(!is_loopback("http://localhost.example.com/v1"));
        assert!(!is_loopback("http://10.0.0.1/v1"));
    }

    #[test]
    fn requiring_the_gateway_without_one_refuses_and_names_both_variables() {
        let s = settings(&[("SPINE_REQUIRE_GATEWAY", "1")]);
        let err = s.route(OPENAI, "openai").unwrap_err();
        assert!(err.contains("SPINE_REQUIRE_GATEWAY"), "{err}");
        assert!(err.contains("IRONGATE_URL"), "{err}");
        assert!(err.contains("openai"), "{err}");
        // A local server needs no gateway, required or not.
        assert_eq!(
            s.route("http://localhost:8000/v1", "openai"),
            Ok(Route::Direct)
        );
        // Anything but "1" does not require it.
        let s = settings(&[("SPINE_REQUIRE_GATEWAY", "yes")]);
        assert_eq!(s.route(OPENAI, "openai"), Ok(Route::Direct));
    }

    #[test]
    fn an_empty_variable_is_unset() {
        let s = settings(&[("IRONGATE_URL", "  "), ("IRONGATE_TOKEN", "")]);
        assert_eq!(s.url, None);
        assert_eq!(s.token, None);
    }
}
