//! The router, driven from the outside.
//!
//! Every test here runs against a local mock HTTP server (a
//! `std::net::TcpListener` in `common`) and a temp secrets file. Nothing
//! touches the network, and nothing reads the real
//! `~/.config/last-mile/secrets.env`.

mod common;

use common::{missing_secrets_file, secrets_file, two_pool_config, MockResponse, MockServer};

use lm_router::{
    build_call, parse_response, ChatMessage, ChatReq, ChatResp, Pool, ProviderKind, Role, Router,
    RouterConfig, RouterError, Secret, Secrets, SkipReason, Usage,
};

/// An OpenAI-shaped answer, with the tokens it cost.
const OPENAI_ANSWER: &str = r#"{
    "model": "glm-test",
    "choices": [{"message": {"role": "assistant", "content": "hold the beacon"},
                 "finish_reason": "stop"}],
    "usage": {"prompt_tokens": 10, "completion_tokens": 5, "total_tokens": 15}
}"#;

/// What a provider says when the plan is spent.
const RATE_LIMIT_ANSWER: &str = r#"{"error": {"message": "rate limit reached, try later"}}"#;

fn ask() -> ChatReq {
    ChatReq::user("what now?")
}

/// The allotment of a pool is spent: the router stops asking it and falls
/// back to the next pool in the preference list.
#[test]
fn allotment_enforced() {
    let zai = MockServer::start(vec![
        MockResponse::ok(OPENAI_ANSWER),
        MockResponse::ok(OPENAI_ANSWER),
    ]);
    let minimax = MockServer::start(vec![MockResponse::ok(OPENAI_ANSWER)]);
    let secrets = secrets_file(
        "allotment",
        &[
            ("ZAI_API_KEY", "sk-zai-test"),
            ("MINIMAX_API_KEY", "sk-minimax-test"),
        ],
    );
    let config = RouterConfig::from_toml_str(&two_pool_config(
        &secrets,
        &zai.base_url(),
        &minimax.base_url(),
        1,
    ))
    .expect("the test config is valid");
    let mut router = Router::new(
        config,
        Secrets::from_file(&secrets).expect("the temp secrets read"),
    );

    // The first call goes to the preferred pool.
    let first = router
        .ask(&[Pool::Zai, Pool::MiniMax], ask())
        .expect("the first pool answers");
    assert_eq!(first.pool, Pool::Zai);

    // Its allotment of one request is spent now, so the second call goes
    // past it, in the order the caller gave.
    let second = router
        .ask(&[Pool::Zai, Pool::MiniMax], ask())
        .expect("the fallback pool answers");
    assert_eq!(second.pool, Pool::MiniMax);
    assert_eq!(zai.count(), 1, "the spent pool was not asked again");
    assert_eq!(minimax.count(), 1);

    // The spend was counted by observation: the tokens the answer reported.
    let usage = router.usage(Pool::Zai);
    assert_eq!(usage.requests, 1);
    assert_eq!(usage.tokens, 15);
    assert_eq!(usage.milli_credits, 30, "15 tokens at 2 credits per 1k");
}

/// A pool that is rate limited goes cold, and the next pool in the order
/// answers instead.
#[test]
fn cold_pool_falls_back() {
    let zai = MockServer::start(vec![MockResponse::status(429, RATE_LIMIT_ANSWER)]);
    let minimax = MockServer::start(vec![
        MockResponse::ok(OPENAI_ANSWER),
        MockResponse::ok(OPENAI_ANSWER),
    ]);
    let secrets = secrets_file(
        "cold",
        &[
            ("ZAI_API_KEY", "sk-zai-test"),
            ("MINIMAX_API_KEY", "sk-minimax-test"),
        ],
    );
    let config = RouterConfig::from_toml_str(&two_pool_config(
        &secrets,
        &zai.base_url(),
        &minimax.base_url(),
        100,
    ))
    .expect("the test config is valid");
    let mut router = Router::new(
        config,
        Secrets::from_file(&secrets).expect("the temp secrets read"),
    );

    let first = router
        .ask(&[Pool::Zai, Pool::MiniMax], ask())
        .expect("the fallback pool answers");
    assert_eq!(first.pool, Pool::MiniMax);
    assert_eq!(zai.count(), 1);
    assert!(router.is_cold(Pool::Zai), "the rate limited pool went cold");

    // The cold pool is not tried again: the walk starts at the next one.
    let second = router
        .ask(&[Pool::Zai, Pool::MiniMax], ask())
        .expect("the fallback pool answers again");
    assert_eq!(second.pool, Pool::MiniMax);
    assert_eq!(zai.count(), 1, "the cold pool was not asked again");
    assert_eq!(minimax.count(), 2);
}

/// Every pool cold: the review is skipped, with a reason to log.
#[test]
fn all_cold_skips_review() {
    let zai = MockServer::start(vec![MockResponse::status(429, RATE_LIMIT_ANSWER)]);
    let minimax = MockServer::start(vec![MockResponse::status(429, RATE_LIMIT_ANSWER)]);
    let secrets = secrets_file(
        "allcold",
        &[
            ("ZAI_API_KEY", "sk-zai-test"),
            ("MINIMAX_API_KEY", "sk-minimax-test"),
        ],
    );
    let config = RouterConfig::from_toml_str(&two_pool_config(
        &secrets,
        &zai.base_url(),
        &minimax.base_url(),
        100,
    ))
    .expect("the test config is valid");
    let mut router = Router::new(
        config,
        Secrets::from_file(&secrets).expect("the temp secrets read"),
    );

    let err = router
        .ask(&[Pool::Zai, Pool::MiniMax], ask())
        .expect_err("no pool can answer");

    match err {
        RouterError::AllCold { pools, causes } => {
            assert_eq!(pools, vec![Pool::Zai, Pool::MiniMax]);
            assert!(causes.iter().any(|c| c.contains("429")), "{causes:?}");
        }
        other => panic!("expected every pool to be cold, got {other:?}"),
    }
    // The reason says why, so a skipped review can say so in the log.
    let text = router
        .ask(&[Pool::Zai, Pool::MiniMax], ask())
        .expect_err("no pool can answer")
        .to_string();
    assert!(text.contains("cold"));
    assert!(text.contains("zai"));
}

/// No type that can hold a key prints one.
#[test]
fn keys_never_logged() {
    let secret = "sk-do-not-print-me-0123456789";
    let zai = MockServer::start(vec![MockResponse::status(429, RATE_LIMIT_ANSWER)]);
    let minimax = MockServer::start(vec![MockResponse::ok(OPENAI_ANSWER)]);
    let secrets = secrets_file(
        "keys",
        &[("ZAI_API_KEY", secret), ("MINIMAX_API_KEY", secret)],
    );
    let config = RouterConfig::from_toml_str(&two_pool_config(
        &secrets,
        &zai.base_url(),
        &minimax.base_url(),
        100,
    ))
    .expect("the test config is valid");
    let mut router = Router::new(
        config,
        Secrets::from_file(&secrets).expect("the temp secrets read"),
    );

    let req = ask();
    let pool_cfg = router
        .config()
        .pool(Pool::Zai)
        .expect("zai is configured")
        .clone();
    let call = build_call(Pool::Zai, &pool_cfg, &req, Some(secret));
    let resp: ChatResp = parse_response(
        Pool::Zai,
        ProviderKind::OpenAi,
        "glm-test",
        &serde_json::from_str(OPENAI_ANSWER).expect("the fixture is JSON"),
    )
    .expect("the fixture parses");
    let err = router
        .ask(&[Pool::Zai], ChatReq::user("what now?"))
        .expect_err("the pool is rate limited");

    let mut printed: Vec<String> = Vec::new();
    let mut push = |text: String| printed.push(text);

    // Every type the router holds, in both forms.
    push(format!("{:?}", router));
    push(format!("{}", router));
    push(format!("{:?}", router.config()));
    push(format!("{}", router.config()));
    push(format!("{:?}", router.config().router));
    push(format!("{}", router.config().router));
    push(format!("{:?}", pool_cfg));
    push(format!("{}", pool_cfg));
    push(format!("{:?}", pool_cfg.kind));
    push(format!("{}", pool_cfg.kind));
    push(format!("{:?}", router.secrets()));
    push(format!("{}", router.secrets()));
    push(format!(
        "{:?}",
        router
            .secrets()
            .get("ZAI_API_KEY")
            .expect("the key is there")
    ));
    push(format!(
        "{}",
        router
            .secrets()
            .get("ZAI_API_KEY")
            .expect("the key is there")
    ));
    push(format!("{:?}", Secret::new(secret)));
    push(format!("{}", Secret::new(secret)));
    push(format!("{:?}", router.meter()));
    push(format!("{}", router.meter()));
    push(format!("{:?}", router.usage(Pool::Zai)));
    push(format!("{}", router.usage(Pool::Zai)));
    push(format!("{:?}", router.config().exchange));
    push(format!("{}", router.config().exchange));
    push(format!("{:?}", req));
    push(format!("{}", req));
    push(format!("{:?}", req.messages));
    push(format!("{:?}", req.messages[0]));
    push(format!("{}", req.messages[0]));
    push(format!("{:?}", req.messages[0].role));
    push(format!("{}", req.messages[0].role));
    push(format!("{:?}", call));
    push(format!("{}", call));
    push(format!("{:?}", resp));
    push(format!("{}", resp));
    push(format!("{:?}", resp.usage));
    push(format!("{}", resp.usage));
    push(format!("{:?}", err));
    push(format!("{}", err));
    push(format!("{:?}", Pool::Zai));
    push(format!("{}", Pool::Zai));
    push(format!("{:?}", SkipReason::NoKey));
    push(format!("{}", SkipReason::NoKey));
    push(format!("{:?}", ChatMessage::user("what now?")));

    for text in &printed {
        assert!(!text.contains(secret), "a key leaked into:\n{text}");
    }
    // The key did travel: it is in the auth header and nowhere else.
    let sent = zai.requests();
    assert_eq!(sent.len(), 1);
    assert_eq!(
        sent[0].header("authorization"),
        Some(format!("Bearer {secret}").as_str())
    );
    assert!(!sent[0].body.contains(secret));
}

/// The OpenAI shape is read: the text, the model and the usage.
#[test]
fn parses_openai_shape() {
    let body: serde_json::Value = serde_json::from_str(OPENAI_ANSWER).expect("the fixture is JSON");
    let resp = parse_response(Pool::Zai, ProviderKind::OpenAi, "glm-test", &body)
        .expect("the fixture parses");
    assert_eq!(resp.pool, Pool::Zai);
    assert_eq!(resp.text, "hold the beacon");
    assert_eq!(resp.model, "glm-test");
    assert_eq!(
        resp.usage,
        Usage {
            input_tokens: 10,
            output_tokens: 5
        }
    );
    assert_eq!(resp.usage.total(), 15);
    assert_eq!(resp.finish_reason.as_deref(), Some("stop"));

    // And the same shape, over the wire.
    let server = MockServer::start(vec![MockResponse::ok(OPENAI_ANSWER)]);
    let secrets = secrets_file("shape", &[("ZAI_API_KEY", "sk-zai-test")]);
    let config = RouterConfig::from_toml_str(&two_pool_config(
        &secrets,
        &server.base_url(),
        &server.base_url(),
        100,
    ))
    .expect("the test config is valid");
    let mut router = Router::new(
        config,
        Secrets::from_file(&secrets).expect("the temp secrets read"),
    );
    let resp = router.ask(&[Pool::Zai], ask()).expect("the pool answers");
    assert_eq!(resp.text, "hold the beacon");
    assert_eq!(resp.usage.total(), 15);

    // The request was the OpenAI shape.
    let sent = server.requests();
    assert_eq!(sent[0].method, "POST");
    assert_eq!(sent[0].path, "/chat/completions");
    let body = sent[0].json();
    assert_eq!(body["model"], "glm-test");
    assert_eq!(body["messages"][0]["role"], "user");
    assert_eq!(body["messages"][0]["content"], "what now?");
}

/// A pool that does not answer in time is an error, not a hang.
#[test]
fn timeout_is_error() {
    let zai = MockServer::start(vec![MockResponse::slow(OPENAI_ANSWER, 1500)]);
    let secrets = secrets_file("timeout", &[("ZAI_API_KEY", "sk-zai-test")]);
    let mut config = RouterConfig::from_toml_str(&two_pool_config(
        &secrets,
        &zai.base_url(),
        &zai.base_url(),
        100,
    ))
    .expect("the test config is valid");
    // A short timeout, so the test does not wait for the mock.
    if let Some(pool) = config.pools.get_mut("zai") {
        pool.timeout_ms = 100;
    }
    let mut router = Router::new(
        config,
        Secrets::from_file(&secrets).expect("the temp secrets read"),
    );

    let err = router
        .ask(&[Pool::Zai], ask())
        .expect_err("the pool did not answer in time");
    match err {
        RouterError::Timeout { pool, timeout_ms } => {
            assert_eq!(pool, Pool::Zai);
            assert_eq!(timeout_ms, 100);
        }
        other => panic!("expected a timeout, got {other:?}"),
    }
}

/// A pool without a key is unavailable, not an error at start-up.
#[test]
fn a_missing_key_makes_the_pool_unavailable() {
    let zai = MockServer::start(vec![MockResponse::ok(OPENAI_ANSWER)]);
    let secrets = secrets_file("nokey", &[("MINIMAX_API_KEY", "sk-minimax-test")]);
    let config = RouterConfig::from_toml_str(&two_pool_config(
        &secrets,
        &zai.base_url(),
        &zai.base_url(),
        100,
    ))
    .expect("the test config is valid");
    let mut router = Router::new(
        config,
        Secrets::from_file(&secrets).expect("the temp secrets read"),
    );

    let err = router
        .ask(&[Pool::Zai], ask())
        .expect_err("the pool has no key");
    match err {
        RouterError::NoUsablePool { reasons } => {
            assert_eq!(reasons, vec![(Pool::Zai, SkipReason::NoKey)]);
        }
        other => panic!("expected no usable pool, got {other:?}"),
    }
    assert_eq!(zai.count(), 0, "the pool was never called");

    // The other pool has its key, so the walk finds it.
    let resp = router
        .ask(&[Pool::Zai, Pool::MiniMax], ask())
        .expect("the pool with a key answers");
    assert_eq!(resp.pool, Pool::MiniMax);
}

/// An error that says "limit" makes the pool cold, even without a 429.
#[test]
fn a_limit_error_makes_the_pool_cold() {
    let zai = MockServer::start(vec![MockResponse::status(
        500,
        "your plan limit is reached",
    )]);
    let minimax = MockServer::start(vec![MockResponse::ok(OPENAI_ANSWER)]);
    let secrets = secrets_file(
        "limit",
        &[
            ("ZAI_API_KEY", "sk-zai-test"),
            ("MINIMAX_API_KEY", "sk-minimax-test"),
        ],
    );
    let config = RouterConfig::from_toml_str(&two_pool_config(
        &secrets,
        &zai.base_url(),
        &minimax.base_url(),
        100,
    ))
    .expect("the test config is valid");
    let mut router = Router::new(
        config,
        Secrets::from_file(&secrets).expect("the temp secrets read"),
    );

    let resp = router
        .ask(&[Pool::Zai, Pool::MiniMax], ask())
        .expect("the fallback pool answers");
    assert_eq!(resp.pool, Pool::MiniMax);
    assert!(router.is_cold(Pool::Zai));
}

/// The stub answers without a network, and costs nothing.
#[test]
fn the_stub_answers_without_a_network() {
    let config = RouterConfig::from_toml_str(
        r#"
[pools.stub]
kind = "stub"
model = "stub"
stub_text = "stub: nothing to say"

[pools.stub.allotment]
tokens = 0
requests = 0
"#,
    )
    .expect("the test config is valid");
    let mut router = Router::new(config, Secrets::default());

    let resp = router
        .ask(&[Pool::Stub], ChatReq::user("what now?"))
        .expect("the stub always answers");
    assert_eq!(resp.pool, Pool::Stub);
    assert_eq!(resp.text, "stub: nothing to say");
    assert_eq!(resp.usage.total(), 0);
    assert_eq!(router.usage(Pool::Stub).tokens, 0);
}

/// The keys file is read at runtime: a missing one means no pool has a key,
/// which is not an error.
#[test]
fn a_missing_secrets_file_is_not_an_error() {
    let path = missing_secrets_file("nofile");
    let secrets = Secrets::from_file(&path).expect("a missing secrets file is not an error");
    assert!(secrets.is_empty());
}

/// The spend is metered by observation, and priced by the exchange table.
#[test]
fn credits_are_counted_by_observation() {
    let zai = MockServer::start(vec![MockResponse::ok(OPENAI_ANSWER)]);
    let secrets = secrets_file("credits", &[("ZAI_API_KEY", "sk-zai-test")]);
    let config = RouterConfig::from_toml_str(&two_pool_config(
        &secrets,
        &zai.base_url(),
        &zai.base_url(),
        100,
    ))
    .expect("the test config is valid");
    let mut router = Router::new(
        config,
        Secrets::from_file(&secrets).expect("the temp secrets read"),
    );

    router.ask(&[Pool::Zai], ask()).expect("the pool answers");
    let usage = router.usage(Pool::Zai);
    assert_eq!(usage.tokens, 15);
    assert_eq!(usage.requests, 1);
    // 15 tokens at 2 credits per 1000 tokens is 30 thousandths of a credit.
    assert_eq!(usage.milli_credits, 30);
}

/// The request that is sent says what the caller meant.
#[test]
fn the_request_carries_the_conversation() {
    let zai = MockServer::start(vec![MockResponse::ok(OPENAI_ANSWER)]);
    let secrets = secrets_file("request", &[("ZAI_API_KEY", "sk-zai-test")]);
    let config = RouterConfig::from_toml_str(&two_pool_config(
        &secrets,
        &zai.base_url(),
        &zai.base_url(),
        100,
    ))
    .expect("the test config is valid");
    let mut router = Router::new(
        config,
        Secrets::from_file(&secrets).expect("the temp secrets read"),
    );

    let req = ChatReq::system_and_user("be brief", "what now?")
        .with_model("glm-other")
        .with_max_tokens(64);
    router.ask(&[Pool::Zai], req).expect("the pool answers");

    let sent = zai.requests();
    let body = sent[0].json();
    assert_eq!(body["model"], "glm-other");
    assert_eq!(body["max_tokens"], 64);
    assert_eq!(body["messages"][0]["role"], "system");
    assert_eq!(body["messages"][1]["role"], "user");
    assert_eq!(body["messages"][1]["content"], "what now?");
    assert_eq!(sent[0].header("content-type"), Some("application/json"));
}

/// A provider that echoes the key back does not get it into the error.
#[test]
fn a_provider_that_echoes_the_key_is_scrubbed() {
    let secret = "sk-echoed-by-the-provider";
    let zai = MockServer::start(vec![MockResponse::status(
        500,
        format!("the request with key {secret} was rejected"),
    )]);
    let secrets = secrets_file("echo", &[("ZAI_API_KEY", secret)]);
    let config = RouterConfig::from_toml_str(&two_pool_config(
        &secrets,
        &zai.base_url(),
        &zai.base_url(),
        100,
    ))
    .expect("the test config is valid");
    let mut router = Router::new(
        config,
        Secrets::from_file(&secrets).expect("the temp secrets read"),
    );

    let err = router
        .ask(&[Pool::Zai], ask())
        .expect_err("the pool answered with an error");
    assert!(!err.to_string().contains(secret), "{err}");
    assert!(!format!("{err:?}").contains(secret), "{err:?}");
}

/// An empty preference list is a mistake, not a call.
#[test]
fn no_pools_is_an_error() {
    let mut router = Router::default();
    let err = router
        .ask(&[], ChatReq::user("what now?"))
        .expect_err("there is nowhere to send the call");
    assert!(matches!(err, RouterError::NoPools));
}

/// The roles of a conversation are named the way the providers name them.
#[test]
fn the_roles_are_named() {
    assert_eq!(Role::System.as_str(), "system");
    assert_eq!(Role::User.as_str(), "user");
    assert_eq!(Role::Assistant.as_str(), "assistant");
}
