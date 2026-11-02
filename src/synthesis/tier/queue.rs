//! Named queues and job-specific consumers.
use super::*;
pub(super) struct Producer<'a> {
    from: &'a Node,
    line: u32,
    column: u32,
    callee: String,
    queue: Option<String>,
    job: String,
}
pub(super) struct Consumer<'a> {
    node: &'a Node,
    file: String,
    line: u32,
    queue: Option<String>,
    job: Option<String>,
}
fn shaped(receiver: &str) -> Result<bool> {
    let name = scan::compact(receiver);
    regex(r"(?i)queue|jobs?$|worker|bull|flow|producer")?
        .is_match(name.rsplit('.').next().unwrap())
        .map_err(Into::into)
}
pub(super) fn collect<'a>(
    session: &mut Session<'_, 'a>,
    facts: &Facts,
    producers: &mut Vec<Producer<'a>>,
    consumers: &mut Vec<Consumer<'a>>,
) -> Result<()> {
    let file = &facts.file;
    let safe = &facts.safe;
    for found in regex(&format!(r#"{CHAIN}((?:this\s*\.\s*)?[A-Za-z_$][\w$]*(?:\s*\.\s*[A-Za-z_$][\w$]*)*)\s*\.\s*add\s*\(\s*(['"`])([^'"`]+)\2"#))?.captures_iter(safe) {
        let found=found?;let text=found.get(0).unwrap();if !chain_start(safe,text.start()){continue}let receiver=&found[1];let queue=session.queue(facts,receiver)?;
        if queue.is_none()&&!shaped(receiver)?{continue}let line=session.ctx.line_at(file,text.start());let Some(from)=session.ctx.enclosing(file,line)? else{continue};
        producers.push(Producer{from,line,column:column(session.ctx,file,text.start()),callee:format!("{}.add",scan::compact(receiver)),queue,job:found[3].into()});
    }
    for processor in scan::decorators(session.ctx, safe, "Processor")? {
        let queue = scan::literals(processor.args)?.into_iter().next();
        let Some(found) =
            regex(r"\bclass\s+([A-Za-z_$][\w$]*)")?.captures_from_pos(safe, processor.end)?
        else {
            continue;
        };
        let line = session.ctx.line_at(file, found.get(0).unwrap().start());
        let Some(class) = session.ctx.in_file(file).find(|node| {
            node.kind == "class"
                && node.name == found[1]
                && node.start_line >= line.saturating_sub(2)
        }) else {
            continue;
        };
        let mut any = false;
        for job in scan::decorators(session.ctx, safe, "Process")? {
            let line = session.ctx.line_at(file, job.start);
            if line < class.start_line || line > class.end_line {
                continue;
            }
            let Some(node) = decorated(session.ctx, facts, job.end, Some(class))? else {
                continue;
            };
            any = true;
            consumers.push(Consumer {
                node,
                file: file.clone(),
                line,
                queue: queue.clone(),
                job: scan::literals(job.args)?.into_iter().next(),
            });
        }
        if !any
            && let Some(node) = session.ctx.in_file(file).find(|node| {
                node.kind == "method"
                    && node.name == "process"
                    && node.start_line >= class.start_line
                    && node.end_line <= class.end_line
            })
        {
            consumers.push(Consumer {
                node,
                file: file.clone(),
                line: node.start_line,
                queue,
                job: None,
            });
        }
    }
    for found in regex(r#"\bnew\s+Worker\s*(?:<[^>]*>)?\s*\(\s*(['"`])([^'"`]+)\1\s*,\s*"#)?
        .captures_iter(safe)
    {
        let found = found?;
        let text = found.get(0).unwrap();
        let line = session.ctx.line_at(file, text.start());
        let Some(node) = session.handler(facts, scan::prefix(&safe[text.end()..], 200), line)?
        else {
            continue;
        };
        consumers.push(Consumer {
            node,
            file: file.clone(),
            line,
            queue: Some(found[2].into()),
            job: None,
        });
    }
    for found in regex(&format!(r#"{CHAIN}((?:this\s*\.\s*)?[A-Za-z_$][\w$]*(?:\s*\.\s*[A-Za-z_$][\w$]*)*)\s*\.\s*process\s*\(\s*(?:(['"`])([^'"`]+)\2\s*,\s*)?(?:\d+\s*,\s*)?"#))?.captures_iter(safe){
        let found=found?;let text=found.get(0).unwrap();if !chain_start(safe,text.start()){continue}let receiver=&found[1];let queue=session.queue(facts,receiver)?;
        if queue.is_none()&&!shaped(receiver)?{continue}let line=session.ctx.line_at(file,text.start());
        let Some(node)=session.handler(facts,scan::prefix(&safe[text.end()..],200),line)? else{continue};consumers.push(Consumer{node,file:file.clone(),line,queue,job:found.get(3).map(|part|part.as_str().into())});
    }
    Ok(())
}
pub(super) fn pair(
    ctx: &Context<'_>,
    producers: &[Producer<'_>],
    consumers: &[Consumer<'_>],
    out: &mut Vec<Edge>,
    seen: &mut HashSet<(String, String)>,
) -> Result<()> {
    for producer in producers {
        ctx.steps(consumers.len())?;
        let mut candidates: Vec<_> = consumers
            .iter()
            .filter(|consumer| {
                (producer.queue.is_none()
                    || consumer.queue.is_none()
                    || consumer.queue == producer.queue)
                    && (consumer.job.is_none() || consumer.job.as_ref() == Some(&producer.job))
            })
            .collect();
        if candidates.iter().any(|consumer| {
            consumer.job.as_ref() == Some(&producer.job)
                && consumer.queue == producer.queue
                && producer.queue.is_some()
        }) {
            candidates.retain(|consumer| {
                consumer.job.as_ref() == Some(&producer.job) && consumer.queue == producer.queue
            });
        } else if candidates
            .iter()
            .any(|consumer| consumer.job.as_ref() == Some(&producer.job))
        {
            candidates.retain(|consumer| consumer.job.as_ref() == Some(&producer.job));
        } else if producer.queue.is_none() {
            continue;
        } else {
            candidates.retain(|consumer| consumer.queue == producer.queue);
        }
        if candidates.len() > FANOUT {
            continue;
        }
        for consumer in candidates {
            if consumer.node.id == producer.from.id {
                continue;
            }
            let mut metadata = serde_json::json!({"synthesizedBy":"queue-job","channel":"queue","callee":producer.callee,"event":producer.job,"registeredAt":format!("{}:{}",consumer.file,consumer.line)});
            if let Some(queue) = producer
                .queue
                .as_ref()
                .or(consumer.queue.as_ref())
                .filter(|queue| !queue.is_empty())
            {
                metadata["queue"] = queue.clone().into();
            }
            emit(
                ctx,
                out,
                seen,
                producer.from,
                consumer.node,
                (producer.line, producer.column),
                metadata,
            )?;
        }
    }
    Ok(())
}
