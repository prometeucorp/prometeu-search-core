//! Ordered reading of guarded call sites, with explicit decision arms and loops.
//! SPDX-License-Identifier: MIT. See crates/prometeu-extract/UPSTREAM.md.
use super::*;
use branches::{Guard, Loop};

#[derive(Clone, Deserialize)]
pub(super) struct Position {
    pub line: usize,
    pub column: usize,
    pub end: Point,
}
#[derive(Clone, Deserialize)]
pub(super) struct Point {
    pub line: usize,
    pub column: usize,
}
#[derive(Clone, Deserialize)]
pub(super) struct Site {
    pub step: Option<String>,
    pub link: Option<String>,
    pub into: Option<String>,
    pub at: Position,
    pub within: Option<String>,
    pub guards: Vec<Guard>,
    #[serde(default)]
    pub loops: Vec<Loop>,
    pub trigger: Option<Value>,
}
#[derive(Clone, Deserialize)]
pub(super) struct Step {
    pub reply: bool,
    pub into: Option<String>,
}
struct Input<'a> {
    sites: &'a BTreeMap<String, Vec<Site>>,
    nodes: &'a BTreeMap<String, Value>,
    steps: &'a BTreeMap<String, Step>,
}
struct State {
    read: HashSet<String>,
    items: usize,
    truncated: usize,
}
#[derive(Clone)]
enum Scope {
    Guard(Guard),
    Loop(Loop),
}
impl Scope {
    fn branch(&self) -> &str {
        match self {
            Self::Guard(guard) => &guard.branch,
            Self::Loop(loop_) => &loop_.branch,
        }
    }
    fn same_arm(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Guard(a), Self::Guard(b)) => a.text == b.text && a.negated == b.negated,
            (Self::Loop(_), Self::Loop(_)) => true,
            _ => false,
        }
    }
}
struct Prepared {
    site: Site,
    scopes: Vec<Scope>,
}
pub(super) fn build(
    root: Option<&str>,
    sites: &BTreeMap<String, Vec<Site>>,
    nodes: &BTreeMap<String, Value>,
    steps: &BTreeMap<String, Step>,
) -> Option<Value> {
    let root = root?;
    let input = Input {
        sites,
        nodes,
        steps,
    };
    let mut state = State {
        read: HashSet::from([root.into()]),
        items: 0,
        truncated: 0,
    };
    let body = block(&input, root, &[root.into()], &mut state);
    (!body.is_empty()).then(|| json!({"root":body,"truncated":state.truncated}))
}
fn at(branch: &str) -> usize {
    let (line, column) = branch.split_once(':').unwrap_or(("0", "0"));
    line.parse::<usize>().unwrap_or(0) * 10000 + column.parse::<usize>().unwrap_or(0)
}
fn inside(a: &Position, b: &Position) -> bool {
    (a.line, a.column) > (b.line, b.column) && (a.line, a.column) < (b.end.line, b.end.column)
}
fn block(input: &Input<'_>, id: &str, path: &[String], state: &mut State) -> Vec<Value> {
    let mut sites = input.sites.get(id).cloned().unwrap_or_default();
    sites.sort_by(|a, b| {
        if inside(&a.at, &b.at) {
            std::cmp::Ordering::Less
        } else if inside(&b.at, &a.at) {
            std::cmp::Ordering::Greater
        } else {
            (a.at.line, a.at.column).cmp(&(b.at.line, b.at.column))
        }
    });
    let prepared = sites
        .into_iter()
        .map(|site| {
            let mut scopes = site
                .guards
                .iter()
                .cloned()
                .map(Scope::Guard)
                .collect::<Vec<_>>();
            if !site.loops.is_empty() {
                scopes.extend(site.loops.iter().cloned().map(Scope::Loop));
                scopes.sort_by(|a, b| {
                    at(a.branch())
                        .cmp(&at(b.branch()))
                        .then_with(|| a.branch().cmp(b.branch()))
                });
            }
            Prepared { site, scopes }
        })
        .collect::<Vec<_>>();
    level(input, &prepared, 0, path, state)
}
fn level(
    input: &Input<'_>,
    sites: &[Prepared],
    depth: usize,
    path: &[String],
    state: &mut State,
) -> Vec<Value> {
    let mut body = vec![];
    let mut position = 0;
    if depth > 128 {
        state.truncated += 1;
        return vec![json!({"kind":"cut","why":"depth"})];
    }
    while position < sites.len() {
        let current = &sites[position];
        if current.scopes.len() <= depth {
            if let Some(item) = item(input, &current.site, path, state) {
                state.items += 1;
                place(&mut body, item, &current.site);
            }
            position += 1;
            continue;
        }
        let scope = &current.scopes[depth];
        let mut end = position + 1;
        while end < sites.len()
            && sites[end].scopes.get(depth).is_some_and(|next| {
                next.branch() == scope.branch()
                    && matches!(
                        (scope, next),
                        (Scope::Guard(_), Scope::Guard(_)) | (Scope::Loop(_), Scope::Loop(_))
                    )
            })
        {
            end += 1;
        }
        match scope {
            Scope::Loop(loop_)=>body.push(json!({"kind":"block","block":"loop","by":loop_.text,"loop":loop_.kind,"body":level(input,&sites[position..end],depth+1,path,state)})),
            Scope::Guard(guard)=>{
                let mut positive=guard.clone();positive.negated=false;
                let form=match guard.form.as_str(){"case"=>"switch","ternary"=>"ternary","catch"=>"try",_=>"if"};
                let mut arms=vec![];
                if guard.form=="guard" && guard.negated {arms.push(json!({"when":branches::label(&[positive.clone()]),"ends":guard.exit.as_deref().unwrap_or("return"),"body":[]}));}
                let mut arm_start=position;
                while arm_start<end {
                    let arm_scope=&sites[arm_start].scopes[depth];let Scope::Guard(arm_guard)=arm_scope else{unreachable!()};
                    let mut arm_end=arm_start+1;while arm_end<end && arm_scope.same_arm(&sites[arm_end].scopes[depth]){arm_end+=1;}
                    let arm_body=level(input,&sites[arm_start..arm_end],depth+1,path,state);let when=branches::label(std::slice::from_ref(arm_guard));
                    let ends=if replies_last(input,&arm_body){Some("reply")}else{arm_guard.arm_exit.as_deref()};
                    if let Some(existing)=arms.iter_mut().find(|arm|arm["when"]==when){existing["body"].as_array_mut().unwrap().extend(arm_body);if ends.is_some(){existing["ends"]=json!(ends);}}
                    else {let mut arm=json!({"when":when,"ends":ends,"body":arm_body});if arm_guard.negated {arm["not"]=json!(true);}arms.push(arm);}
                    arm_start=arm_end;
                }
                let on=if form=="switch"{subject(&arms)}else{branches::label(&[positive])};
                body.push(json!({"kind":"fork","on":on,"form":form,"arms":arms}));
            }
        }
        position = end;
    }
    body
}
fn open(id: &str, path: &[String], state: &mut State) -> bool {
    if path.iter().any(|current| current == id) || state.read.contains(id) {
        return false;
    }
    if path.len() >= 8 || state.items >= 1200 {
        state.truncated += 1;
        return false;
    }
    state.read.insert(id.into());
    true
}
fn item(input: &Input<'_>, site: &Site, path: &[String], state: &mut State) -> Option<Value> {
    if let Some(id) = &site.into {
        let mut item = json!({"kind":"block","block":"inline","body":[]});
        if let Some(node) = input.nodes.get(id) {
            item["via"] = node.clone();
        }
        if let Some(within) = &site.within {
            item["within"] = json!(within);
        }
        if !open(id, path, state) {
            item["again"] = json!(true);
            return Some(item);
        }
        let mut path = path.to_vec();
        path.push(id.clone());
        let body = block(input, id, &path, state);
        if body.is_empty() {
            return None;
        }
        item["body"] = json!(body);
        return Some(item);
    }
    let id = site.step.as_ref()?;
    let mut item = json!({"kind":"step","step":id});
    if let Some(link) = &site.link {
        item["link"] = json!(link);
    }
    if let Some(within) = &site.within {
        item["within"] = json!(within);
    }
    if let Some(next) = input.steps.get(id).and_then(|step| step.into.as_deref()) {
        if open(next, path, state) {
            let mut path = path.to_vec();
            path.push(next.into());
            let body = block(input, next, &path, state);
            if !body.is_empty() {
                item["body"] = json!(body);
            }
        } else if input.sites.contains_key(next) {
            item["again"] = json!(true);
        }
    }
    Some(item)
}
fn place(body: &mut Vec<Value>, item: Value, site: &Site) {
    let run = site
        .trigger
        .as_ref()
        .filter(|trigger| trigger["kind"] == "callback")
        .and_then(|trigger| trigger["name"].as_str())
        .filter(|name| {
            matches!(
                *name,
                "then"
                    | "catch"
                    | "finally"
                    | "setTimeout"
                    | "setInterval"
                    | "setImmediate"
                    | "queueMicrotask"
                    | "requestAnimationFrame"
                    | "useEffect"
                    | "useLayoutEffect"
                    | "addListener"
                    | "addEventListener"
                    | "on"
                    | "once"
                    | "subscribe"
                    | "nextTick"
                    | "process.nextTick"
            )
        })
        .map(|by| ("later", by))
        .or_else(|| {
            site.within
                .as_deref()
                .filter(|within| {
                    matches!(
                        *within,
                        "Promise.all"
                            | "Promise.allSettled"
                            | "Promise.any"
                            | "Promise.race"
                            | "asyncio.gather"
                            | "Task.WhenAll"
                            | "Task.WhenAny"
                    )
                })
                .map(|by| ("together", by))
        });
    if let Some((kind, by)) = run {
        if let Some(last) = body
            .last_mut()
            .filter(|last| last["kind"] == "block" && last["block"] == kind && last["by"] == by)
        {
            last["body"].as_array_mut().unwrap().push(item);
        } else {
            body.push(json!({"kind":"block","block":kind,"by":by,"body":[item]}));
        }
    } else {
        body.push(item);
    }
}
fn replies_last(input: &Input<'_>, body: &[Value]) -> bool {
    let Some(last) = body.last() else {
        return false;
    };
    if last["kind"] == "step" {
        return last["step"]
            .as_str()
            .and_then(|id| input.steps.get(id))
            .is_some_and(|step| step.reply);
    }
    if last["kind"] == "block" {
        return last["body"]
            .as_array()
            .is_some_and(|body| replies_last(input, body));
    }
    false
}
fn subject(arms: &[Value]) -> String {
    if arms.len() < 2 {
        return String::new();
    }
    let mut common = arms[0]["when"].as_str().unwrap_or("").to_owned();
    for arm in &arms[1..] {
        common = common
            .chars()
            .zip(arm["when"].as_str().unwrap_or("").chars())
            .take_while(|(a, b)| a == b)
            .map(|(ch, _)| ch)
            .collect();
    }
    common
        .trim()
        .chars()
        .take_while(|ch| ch.is_alphanumeric() || "_$ .?[]'\"".contains(*ch) && *ch != ' ')
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ordered_programs_match_independent_fixtures() {
        let cases: Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/upstream/viewer/programs.json"
        ))
        .unwrap();
        for case in cases.as_array().unwrap() {
            let sites = serde_json::from_value(case["sites"].clone()).unwrap();
            let nodes = serde_json::from_value(case["nodes"].clone()).unwrap();
            let steps = serde_json::from_value(case["steps"].clone()).unwrap();
            assert_eq!(
                json!(build(case["root"].as_str(), &sites, &nodes, &steps)),
                case["expected"],
                "{}",
                case["name"]
            );
        }
    }
}
