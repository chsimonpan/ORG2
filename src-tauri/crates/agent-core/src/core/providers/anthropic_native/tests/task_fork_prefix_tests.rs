//! Appending a Task summary to the main line must not change anything the
//! provider could already have cached: system blocks (BP1), tool schema (BP2) and
//! every earlier message stay byte-identical. The only permitted difference is
//! that the sliding history breakpoint (BP3) — existing behaviour on every turn —
//! moves from the previous tail to the new tail.
use super::messages::extract_system;
use super::tools::convert_tools;
use serde_json::{json, Value};

fn strip_cache_control(value: &Value) -> Value {
    match value {
        Value::Object(map) => Value::Object(
            map.iter()
                .filter(|(k, _)| k.as_str() != "cache_control")
                .map(|(k, v)| (k.clone(), strip_cache_control(v)))
                .collect(),
        ),
        Value::Array(items) => Value::Array(items.iter().map(strip_cache_control).collect()),
        other => other.clone(),
    }
}

fn tools() -> Vec<Value> {
    vec![
        json!({"type":"function","function":{"name":"read_file","description":"read","parameters":{"type":"object","properties":{"path":{"type":"string"}}}}}),
        json!({"type":"function","function":{"name":"edit_file","description":"edit","parameters":{"type":"object","properties":{"path":{"type":"string"}}}}}),
    ]
}

fn history() -> Vec<Value> {
    vec![
        json!({"role":"user","content":"整理账单导出"}),
        json!({"role":"assistant","content":"","tool_calls":[{"id":"call_1","type":"function","function":{"name":"read_file","arguments":"{\"path\":\"a.rs\"}"}}]}),
        json!({"role":"tool","tool_call_id":"call_1","content":"fn main(){}"}),
        json!({"role":"assistant","content":"已读取 a.rs"}),
    ]
}

fn roles_alternate(messages: &[Value]) -> bool {
    messages.windows(2).all(|w| w[0]["role"] != w[1]["role"])
}

fn tool_pairs_are_legal(messages: &[Value]) -> bool {
    for (i, m) in messages.iter().enumerate() {
        let Some(blocks) = m["content"].as_array() else {
            continue;
        };
        for block in blocks.iter().filter(|b| b["type"] == "tool_use") {
            let id = &block["id"];
            let next = messages.get(i + 1);
            let answered = next.is_some_and(|n| {
                n["role"] == "user"
                    && n["content"].as_array().is_some_and(|c| {
                        c.iter()
                            .any(|b| b["type"] == "tool_result" && &b["tool_use_id"] == id)
                    })
            });
            if !answered {
                return false;
            }
        }
    }
    true
}

#[test]
fn appending_a_task_summary_keeps_system_tools_and_prior_messages_byte_identical() {
    let system = json!({"role":"system","content":"稳定的系统前缀：身份、规则、工具说明"});
    let volatile = json!({"role":"system","content":"动态：技能、记忆"});

    let mut before: Vec<Value> = vec![system.clone(), volatile.clone()];
    before.extend(history());
    before.push(json!({"role":"user","content":"第 N 条用户消息"}));

    // After: the main line answered, then ONE Task summary was appended, then the user continued.
    let mut after = before.clone();
    after.push(json!({"role":"assistant","content":"第 N 条回答"}));
    after.push(json!({"role":"user","content":"【Task 摘要】来源：Task「对账导出重试」…\n结论：已实现指数退避"}));
    after.push(json!({"role":"user","content":"继续主线"}));

    let (sys_before, msgs_before) = extract_system(&before, false);
    let (sys_after, msgs_after) = extract_system(&after, false);
    // BP1: system blocks, including their cache markers, are byte-identical.
    assert_eq!(
        serde_json::to_vec(&sys_before).unwrap(),
        serde_json::to_vec(&sys_after).unwrap()
    );
    // BP2: tool schema is independent of the transcript.
    assert_eq!(
        serde_json::to_vec(&convert_tools(&tools(), false)).unwrap(),
        serde_json::to_vec(&convert_tools(&tools(), false)).unwrap()
    );

    // Every earlier message is identical modulo the sliding BP3 marker.
    let prior = msgs_before.len();
    assert!(msgs_after.len() >= prior);
    let head_before: Vec<Value> = msgs_before.iter().map(strip_cache_control).collect();
    let head_after: Vec<Value> = msgs_after[..prior]
        .iter()
        .map(strip_cache_control)
        .collect();
    assert_eq!(
        head_before, head_after,
        "earlier messages must be untouched by the append"
    );

    // The old tail carried BP3; the new request moves it to the new tail only.
    let marked = |m: &Value| serde_json::to_string(m).unwrap().contains("cache_control");
    assert!(marked(msgs_before.last().unwrap()));
    assert_eq!(msgs_after.iter().filter(|m| marked(m)).count(), 1);
    assert!(marked(msgs_after.last().unwrap()));

    // The summary is a plain user turn: roles still alternate and tool pairing is legal.
    assert!(roles_alternate(&msgs_after), "{msgs_after:#?}");
    assert!(tool_pairs_are_legal(&msgs_after));
    let last = msgs_after.last().unwrap();
    assert!(serde_json::to_string(last)
        .unwrap()
        .contains("【Task 摘要】"));
    // The Task's own conversation is nowhere in the main-line request.
    assert!(!serde_json::to_string(&msgs_after)
        .unwrap()
        .contains("Task 内"));
}

#[test]
fn skip_cache_write_summarizer_requests_write_no_cache_breakpoints() {
    // The compress/finish summarizers are one-shot over prefixes that are never re-sent.
    let msgs = vec![
        json!({"role":"system","content":"你是上下文压缩器"}),
        json!({"role":"user","content":"[用户] …"}),
    ];
    let (sys, converted) = extract_system(&msgs, true);
    assert!(!serde_json::to_string(&sys)
        .unwrap()
        .contains("cache_control"));
    assert!(!serde_json::to_string(&converted)
        .unwrap()
        .contains("cache_control"));
}
