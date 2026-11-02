//! Function-like macro visibility with unknown build flags and bounded include walks.
use super::*;
type Truth = Option<bool>;
fn and(a: Truth, b: Truth) -> Truth {
    if a == Some(false) || b == Some(false) {
        Some(false)
    } else if a == Some(true) && b == Some(true) {
        Some(true)
    } else {
        None
    }
}
fn or(a: Truth, b: Truth) -> Truth {
    if a == Some(true) || b == Some(true) {
        Some(true)
    } else if a == Some(false) && b == Some(false) {
        Some(false)
    } else {
        None
    }
}
fn not(a: Truth) -> Truth {
    a.map(|value| !value)
}
#[derive(Clone)]
enum Event {
    Define {
        line: u32,
        name: String,
        defining: bool,
        function: bool,
        value: String,
        wrapper: bool,
    },
    Include {
        line: u32,
        spec: String,
    },
    Branch {
        line: u32,
        op: String,
        expression: String,
        guard: bool,
    },
    Once {
        line: u32,
    },
}
impl Event {
    fn line(&self) -> u32 {
        match self {
            Self::Define { line, .. }
            | Self::Include { line, .. }
            | Self::Branch { line, .. }
            | Self::Once { line } => *line,
        }
    }
}
#[derive(Clone, Copy, Default, PartialEq)]
struct Definition {
    defined: Truth,
    value: Truth,
    mac: Truth,
}
#[derive(Default)]
struct Timeline {
    events: HashMap<String, Vec<(u32, Truth)>>,
    cutoff: Option<u32>,
}
pub(super) struct Macros<'a> {
    events: HashMap<String, Vec<Event>>,
    roots: RefCell<HashMap<String, Timeline>>,
    resolve: &'a dyn Fn(&str, &str) -> Option<String>,
}
fn condition(text: &str, definitions: &HashMap<String, Definition>) -> Result<Truth> {
    let text = text.trim();
    if regex(r"(?i)^(?:0x[\da-f]+|\d+)[uUlL]*$")?.is_match(text)? {
        let number = text.trim_end_matches(['u', 'U', 'l', 'L']);
        let number = number
            .strip_prefix("0x")
            .or_else(|| number.strip_prefix("0X"))
            .map_or_else(
                || number.parse::<u64>(),
                |number| u64::from_str_radix(number, 16),
            );
        return Ok(number.ok().map(|number| number != 0));
    }
    if let Some(found) = regex(r"^(!)?\s*defined\s*(?:\(\s*(\w+)\s*\)|(\w+))$")?.captures(text)? {
        let name = found.get(2).or_else(|| found.get(3)).unwrap().as_str();
        let known = definitions.get(name).and_then(|value| value.defined);
        return Ok(if found.get(1).is_some() {
            not(known)
        } else {
            known
        });
    }
    Ok(definitions.get(text).and_then(|value| value.value))
}
fn lines(source: &str) -> Result<Vec<String>> {
    let mut masked = source.as_bytes().to_vec();
    for found in regex(r#"R"([^ ()\\\t\r\n]{0,16})\([\s\S]*?\)\1""#)?.find_iter(source) {
        let found = found?;
        for byte in &mut masked[found.start()..found.end()] {
            if !matches!(*byte, b'\n' | b'\r') {
                *byte = b' ';
            }
        }
    }
    let source = String::from_utf8(masked).expect("raw literal masking preserves UTF-8");
    Ok(crate::synthesis::comments::strip_for_regex(&source, "cpp")
        .lines()
        .map(str::to_string)
        .collect())
}
impl<'a> Macros<'a> {
    pub fn possible(&self, at: &UnresolvedReference) -> Result<bool> {
        self.visible(at)?;
        let roots = self.roots.borrow();
        let timeline = &roots[at.file_path.as_deref().unwrap_or("")];
        Ok(timeline.cutoff.is_none_or(|cutoff| at.line < cutoff)
            && timeline
                .events
                .get(&at.reference_name)
                .and_then(|events| events.iter().rev().find(|(line, _)| *line <= at.line))
                .is_some_and(|(_, value)| *value != Some(false)))
    }
    pub fn new(
        sources: &BTreeMap<&str, &str>,
        resolve: &'a dyn Fn(&str, &str) -> Option<String>,
    ) -> Result<Self> {
        let mut files = HashMap::new();
        let mut count = 0;
        for (&file, &source) in sources {
            let lines = lines(source)?;
            let mut events = Vec::new();
            for (i, line) in lines.iter().enumerate() {
                if let Some(found) =
                    regex(r"^\s*#\s*(ifdef|ifndef|if|elif|else|endif)\b(.*)$")?.captures(line)?
                {
                    let op = found[1].to_string();
                    let expression = found[2].to_string();
                    let guard_name = if op == "ifndef" {
                        Some(expression.trim().to_string())
                    } else {
                        regex(r"^\s*!\s*defined\s*(?:\(\s*(\w+)\s*\)|(\w+))\s*$")?
                            .captures(&expression)?
                            .map(|found| {
                                found
                                    .get(1)
                                    .or_else(|| found.get(2))
                                    .unwrap()
                                    .as_str()
                                    .to_string()
                            })
                    };
                    let guard = if let Some(name) = guard_name
                        && regex(r"^\w+$")?.is_match(&name)?
                    {
                        if let Some(next) = lines[i + 1..]
                            .iter()
                            .find(|line| line.trim_start().starts_with('#'))
                        {
                            regex(&format!(
                                r"^\s*#\s*define\s+{}(?:\s*$|\()",
                                ::regex::escape(&name)
                            ))?
                            .is_match(next)?
                        } else {
                            false
                        }
                    } else {
                        false
                    };
                    events.push(Event::Branch {
                        line: i as u32 + 1,
                        op,
                        expression,
                        guard,
                    });
                } else if let Some(found) =
                    regex(r"^\s*#\s*(define|undef)\s+(\w+)(\(?)")?.captures(line)?
                {
                    let function = &found[3] == "(";
                    let name = found[2].to_string();
                    let mut text = line.clone();
                    let mut next = i;
                    while lines[next].trim_end().ends_with('\\') && next + 1 < lines.len() {
                        next += 1;
                        text.push(' ');
                        text.push_str(&lines[next]);
                    }
                    let wrapper = function
                        && regex(&format!(
                            r"(?:\b{}\s*\(|\(\s*{}\s*\)\s*\()",
                            ::regex::escape(&name),
                            ::regex::escape(&name)
                        ))?
                        .is_match(text.split_once('(').map_or("", |(_, body)| body))?;
                    events.push(Event::Define {
                        line: i as u32 + 1,
                        name,
                        defining: &found[1] == "define",
                        function,
                        value: line[found.get(0).unwrap().end()..].to_string(),
                        wrapper,
                    });
                } else if let Some(found) =
                    regex(r#"^\s*#\s*include\s*[<"]([^>"]+)[>"]"#)?.captures(line)?
                {
                    events.push(Event::Include {
                        line: i as u32 + 1,
                        spec: found[1].to_string(),
                    });
                } else if regex(r"^\s*#\s*pragma\s+once\b")?.is_match(line)? {
                    events.push(Event::Once { line: i as u32 + 1 });
                }
                count += 1;
                ensure!(
                    count <= 2_000_000,
                    "C/C++ directive source exceeds graph budget"
                );
            }
            files.insert(file.to_string(), events);
        }
        Ok(Self {
            events: files,
            roots: RefCell::new(HashMap::new()),
            resolve,
        })
    }
    pub fn visible(&self, at: &UnresolvedReference) -> Result<bool> {
        let file = at.file_path.as_deref().unwrap_or("");
        if !self.roots.borrow().contains_key(file) {
            let mut walk = Walk {
                macros: self,
                timeline: Timeline::default(),
                definitions: HashMap::new(),
                scanning: HashSet::new(),
                names: HashSet::new(),
                once: HashMap::new(),
                budget: 1_000_000,
            };
            walk.scan(file, Some(true), None, 0)?;
            if self.roots.borrow().len() >= 32 {
                self.roots.borrow_mut().clear();
            }
            self.roots
                .borrow_mut()
                .insert(file.to_string(), walk.timeline);
        }
        let roots = self.roots.borrow();
        let timeline = &roots[file];
        Ok(timeline.cutoff.is_none_or(|cutoff| at.line < cutoff)
            && timeline
                .events
                .get(&at.reference_name)
                .and_then(|events| events.iter().rev().find(|(line, _)| *line <= at.line))
                .is_some_and(|(_, defined)| *defined == Some(true)))
    }
}
struct Walk<'a, 'b> {
    macros: &'a Macros<'b>,
    timeline: Timeline,
    definitions: HashMap<String, Definition>,
    scanning: HashSet<String>,
    names: HashSet<String>,
    once: HashMap<String, Truth>,
    budget: usize,
}
impl Walk<'_, '_> {
    fn scan(
        &mut self,
        file: &str,
        inherited: Truth,
        include_line: Option<u32>,
        depth: usize,
    ) -> Result<()> {
        if inherited == Some(false)
            || self.once.get(file) == Some(&Some(true))
            || self.timeline.cutoff.is_some()
            || self.scanning.contains(file)
        {
            return Ok(());
        }
        if depth >= 128 {
            self.timeline.cutoff = Some(include_line.unwrap_or(1));
            return Ok(());
        }
        self.scanning.insert(file.to_string());
        let mut active = inherited;
        let mut frames = Vec::new();
        for event in self.macros.events.get(file).into_iter().flatten() {
            let line = include_line.unwrap_or(event.line());
            if self.budget == 0 {
                self.timeline.cutoff = Some(line);
                break;
            }
            self.budget -= 1;
            if let Event::Branch {
                op,
                expression,
                guard,
                ..
            } = event
            {
                if matches!(op.as_str(), "if" | "ifdef" | "ifndef") {
                    let known = self
                        .definitions
                        .get(expression.trim())
                        .and_then(|value| value.defined);
                    let mut selected = if op == "if" {
                        condition(expression, &self.definitions)?
                    } else if op == "ifndef" {
                        not(known)
                    } else {
                        known
                    };
                    if selected.is_none() && *guard {
                        selected = Some(true);
                    }
                    frames.push((active, selected));
                    active = and(active, selected);
                } else if op == "endif" {
                    active = frames.pop().map_or(inherited, |(parent, _)| parent);
                } else if let Some((parent, taken)) = frames.last_mut() {
                    let test = if op == "else" {
                        Some(true)
                    } else {
                        condition(expression, &self.definitions)?
                    };
                    active = and(*parent, and(not(*taken), test));
                    *taken = or(*taken, test);
                }
                continue;
            }
            if active == Some(false) {
                continue;
            }
            match event {
                Event::Once { .. } => {
                    self.once.insert(
                        file.to_string(),
                        or(self.once.get(file).copied().unwrap_or(Some(false)), active),
                    );
                }
                Event::Include { spec, .. } => {
                    if let Some(target) = (self.macros.resolve)(spec, file) {
                        self.scan(&target, active, Some(line), depth + 1)?;
                    }
                }
                Event::Define {
                    name,
                    defining,
                    function,
                    value,
                    wrapper,
                    ..
                } => {
                    let prior = self.definitions.get(name).copied().unwrap_or_default();
                    let mac = *defining && *function && !*wrapper;
                    let now = if active == Some(true) || prior.mac == Some(mac) {
                        Some(mac)
                    } else {
                        None
                    };
                    let next = Definition {
                        defined: if *defining {
                            or(prior.defined, active)
                        } else {
                            and(prior.defined, not(active))
                        },
                        value: if *defining && active == Some(true) {
                            condition(value, &self.definitions)?
                        } else {
                            None
                        },
                        mac: now,
                    };
                    self.definitions.insert(name.clone(), next);
                    if *function {
                        self.names.insert(name.clone());
                    }
                    if self.names.contains(name) {
                        self.timeline
                            .events
                            .entry(name.clone())
                            .or_default()
                            .push((line, now));
                    }
                }
                Event::Branch { .. } => {}
            }
        }
        self.scanning.remove(file);
        Ok(())
    }
}
