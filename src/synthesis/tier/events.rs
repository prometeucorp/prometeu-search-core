//! Event-bus globs and directional socket channels.
use super::*;
pub(super) struct Dispatch<'a> {
    from: &'a Node,
    line: u32,
    column: u32,
    callee: String,
    event: String,
    shape: &'static str,
    server: bool,
}
pub(super) struct Handler<'a> {
    node: &'a Node,
    file: String,
    line: u32,
    pattern: String,
    kind: &'static str,
    server: bool,
}
fn shape(receiver: &str) -> Result<Option<&'static str>> {
    let bare = regex(r"\([^()]*\)")?.replace_all(receiver, "");
    let compact = scan::compact(&bare);
    let segments: Vec<_> = compact.split('.').filter(|part| *part != "this").collect();
    for (pattern, shape) in [
        (
            r"^(?:socket|io|ws|wss|client|server|namespace|nsp|conn|connection|gateway|broadcast|to|in|of|except|volatile|local|sockets|socketServer|wsServer|room|channel|pusher|ably|ioClient|socketClient|sock)$",
            "socket",
        ),
        (
            r"^(?:eventEmitter|emitter|events|eventBus|bus|dispatcher|pubsub|publisher|eventPublisher|ee|hub|mediator|broker|messageBus|appEvents|domainEvents|eventsService|eventService)$",
            "bus",
        ),
    ] {
        let regex = regex(pattern)?;
        for part in &segments {
            if regex.is_match(part)? {
                return Ok(Some(shape));
            }
        }
    }
    Ok(None)
}
fn generic(name: &str) -> Result<bool> {
    regex(r"^(?:error|connect|connect_error|connect_failed|connection|disconnect|disconnecting|reconnect|reconnect_attempt|reconnecting|reconnect_error|reconnect_failed|close|open|end|data|ready|drain|finish|pipe|unpipe|listening|timeout|ping|pong|upgrade|newListener|removeListener)$")?.is_match(name).map_err(Into::into)
}
pub(super) fn collect<'a>(
    session: &mut Session<'_, 'a>,
    facts: &Facts,
    dispatches: &mut Vec<Dispatch<'a>>,
    handlers: &mut Vec<Handler<'a>>,
) -> Result<()> {
    let safe = &facts.safe;
    let file = &facts.file;
    for found in regex(r#"(?<![\w$])((?:[\w$]+(?:\([^()]*\))?\s*\.\s*)*[\w$]+)\s*\.\s*(emit|emitAsync)\s*\(\s*(['"`])([^'"`\n]+)\3"#)?.captures_iter(safe){
        let found=found?;let Some(shape)=shape(&found[1])? else{continue};if generic(&found[4])?{continue}let at=found.get(0).unwrap().start();let line=session.ctx.line_at(file,at);let Some(from)=session.ctx.enclosing(file,line)? else{continue};
        dispatches.push(Dispatch{from,line,column:column(session.ctx,file,at),callee:format!("{}.{}",scan::compact(&found[1]),&found[2]),event:found[4].into(),shape,server:facts.socket_server});
    }
    for name in ["OnEvent", "SubscribeMessage"] {
        for decorator in scan::decorators(session.ctx, safe, name)? {
            let Some(node) = decorated(session.ctx, facts, decorator.end, None)? else {
                continue;
            };
            let mut patterns = scan::literals(decorator.args)?;
            if name != "OnEvent"
                || !decorator
                    .args
                    .trim_start_matches(crate::search::query_whitespace)
                    .starts_with('[')
            {
                patterns.truncate(1);
            }
            for pattern in patterns {
                handlers.push(Handler {
                    node,
                    file: file.clone(),
                    line: session.ctx.line_at(file, decorator.start),
                    pattern,
                    kind: if name == "OnEvent" { "bus" } else { "socket" },
                    server: name == "SubscribeMessage" || facts.socket_server,
                });
            }
        }
    }
    for found in regex(r#"(?<![\w$])((?:[\w$]+(?:\([^()]*\))?\s*\.\s*)*[\w$]+)\s*\.\s*(?:on|once)\s*\(\s*(['"`])([^'"`\n]+)\2\s*,\s*"#)?.captures_iter(safe){
        let found=found?;if shape(&found[1])?!=Some("socket")||generic(&found[3])?{continue}let text=found.get(0).unwrap();let line=session.ctx.line_at(file,text.start());
        let Some(node)=session.handler(facts,scan::prefix(&safe[text.end()..],200),line)? else{continue};handlers.push(Handler{node,file:file.clone(),line,pattern:found[3].into(),kind:"socket",server:facts.socket_server});
    }
    Ok(())
}
pub(super) fn matches(pattern: &str, event: &str) -> Result<bool> {
    if pattern == event {
        return Ok(true);
    }
    if !pattern.contains('*') {
        return Ok(false);
    }
    let pattern = format!(
        "^{}$",
        pattern
            .split("**")
            .map(|part| part
                .split('*')
                .map(regex::escape)
                .collect::<Vec<_>>()
                .join("[^.]+"))
            .collect::<Vec<_>>()
            .join(".*")
    );
    regex(&pattern)?.is_match(event).map_err(Into::into)
}
pub(super) fn pair(
    ctx: &Context<'_>,
    dispatches: &[Dispatch<'_>],
    handlers: &[Handler<'_>],
    out: &mut Vec<Edge>,
    seen: &mut HashSet<(String, String)>,
) -> Result<()> {
    let mut groups: Vec<(&str, &str, Vec<&Dispatch<'_>>)> = Vec::new();
    let mut positions = HashMap::new();
    for dispatch in dispatches {
        let key = (dispatch.shape, dispatch.event.as_str());
        let at = *positions.entry(key).or_insert_with(|| {
            groups.push((key.0, key.1, Vec::new()));
            groups.len() - 1
        });
        groups[at].2.push(dispatch);
    }
    for (_, _, group) in groups {
        if group.len() > FANOUT {
            continue;
        }
        for dispatch in group {
            let mut matched = Vec::new();
            for handler in handlers {
                ctx.step()?;
                if (dispatch.shape == "bus"
                    && handler.kind == "bus"
                    && matches(&handler.pattern, &dispatch.event)?)
                    || (dispatch.shape == "socket"
                        && handler.kind == "socket"
                        && handler.server != dispatch.server
                        && handler.pattern == dispatch.event)
                {
                    matched.push(handler);
                }
            }
            if matched.len() > FANOUT {
                continue;
            }
            for handler in matched {
                if handler.node.id == dispatch.from.id {
                    continue;
                }
                let mut metadata = serde_json::json!({"synthesizedBy":"event-bus","channel":if dispatch.shape=="bus"{"event"}else{"socket"},"callee":dispatch.callee,"event":dispatch.event,"registeredAt":format!("{}:{}",handler.file,handler.line)});
                if dispatch.shape == "socket" {
                    metadata["tier"] = if dispatch.server {
                        "server→client"
                    } else {
                        "client→server"
                    }
                    .into();
                }
                emit(
                    ctx,
                    out,
                    seen,
                    dispatch.from,
                    handler.node,
                    (dispatch.line, dispatch.column),
                    metadata,
                )?;
            }
        }
    }
    Ok(())
}
