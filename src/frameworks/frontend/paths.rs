//! Route path conventions for Next, Expo Router, and SvelteKit.
use super::*;

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Kind {
    Page,
    Handler,
    Api,
}
pub(crate) struct NextRoute {
    pub kind: Kind,
    pub path: String,
    pub root: String,
}

pub(crate) fn next(path: &str) -> Result<Option<NextRoute>> {
    if regex(r"(?:^|/)(?:__tests__|__mocks__|node_modules)/")?.is_match(path)? {
        return Ok(None);
    }
    if let Some(app) = regex(r"^((?:[^/]+/)*?)(?:src/)?app/(.+)$")?.captures(path)? {
        let Some(route) = regex(r"^(.*?)(?:^|/)(page|route)\.(?:tsx|ts|jsx|js|mjs|cjs|mdx?)$")?
            .captures(&app[2])?
        else {
            return Ok(None);
        };
        let parts: Vec<_> = route[1].split('/').filter(|s| !s.is_empty()).collect();
        let intercept = regex(r"^\(\.{1,3}\)")?;
        for part in &parts {
            if part.starts_with('@') || intercept.is_match(part)? {
                return Ok(None);
            }
        }
        let mut kept = Vec::new();
        for part in parts {
            if !group(part) {
                kept.push(next_segment(part)?);
            }
        }
        return Ok(Some(NextRoute {
            kind: if &route[2] == "page" {
                Kind::Page
            } else {
                Kind::Handler
            },
            path: format!("/{}", kept.join("/")),
            root: app[1].to_owned(),
        }));
    }
    if let Some(pages) = regex(r"^((?:[^/]+/)*?)(?:src/)?pages/(.+)$")?.captures(path)? {
        let relative = &pages[2];
        let Some(extension) = regex(r"\.(?:tsx|ts|jsx|js|mjs|cjs|mdx?)$")?.find(relative)? else {
            return Ok(None);
        };
        let bare = &relative[..extension.start()];
        let mut parts: Vec<_> = bare.split('/').collect();
        if parts.last().is_some_and(|s| s.starts_with('_'))
            || regex(r"\.(?:test|spec|stories|config|d)$")?.is_match(bare)?
        {
            return Ok(None);
        }
        if parts.last() == Some(&"index") {
            parts.pop();
        }
        let kind = if parts.first() == Some(&"api") {
            Kind::Api
        } else {
            Kind::Page
        };
        let segments = parts
            .into_iter()
            .map(next_segment)
            .collect::<Result<Vec<_>>>()?;
        return Ok(Some(NextRoute {
            kind,
            path: format!("/{}", segments.join("/")),
            root: pages[1].to_owned(),
        }));
    }
    Ok(None)
}
fn next_segment(segment: &str) -> Result<String> {
    for (pattern, suffix) in [
        (r"^\[\[\.\.\.([^\]]+)\]\]$", "*"),
        (r"^\[\.\.\.([^\]]+)\]$", "*"),
        (r"^\[([^\]]+)\]$", ""),
    ] {
        if let Some(found) = regex(pattern)?.captures(segment)? {
            return Ok(format!(":{}{suffix}", &found[1]));
        }
    }
    Ok(segment.to_owned())
}
pub(crate) fn expo(path: &str) -> Result<Option<String>> {
    let Some(directory) = regex(r"(?:^|/)(?:src/)?app/")?.find(path)? else {
        return Ok(None);
    };
    let relative = &path[directory.end()..];
    let Some(extension) =
        regex(r"\.(?:(?:ios|android|native|web)\.)?(tsx|ts|jsx|js|mjs|cjs)$")?.find(relative)?
    else {
        return Ok(None);
    };
    let bare = &relative[..extension.start()];
    if bare.ends_with(".d") || regex(r"\.(?:test|spec|stories)$")?.is_match(bare)? {
        return Ok(None);
    }
    let parts: Vec<_> = bare.split('/').collect();
    if parts.contains(&"__tests__") || parts.contains(&"__mocks__") {
        return Ok(None);
    }
    let base = parts.last().copied().unwrap_or("");
    if base.ends_with("+api")
        || base.starts_with('_')
        || base.starts_with('+') && base != "+not-found"
    {
        return Ok(None);
    }
    let mut parts: Vec<_> = parts.into_iter().filter(|part| !group(part)).collect();
    if parts.last() == Some(&"index") {
        parts.pop();
    }
    Ok(Some(format!("/{}", parts.join("/"))))
}
pub(crate) fn expo_api(path: &str) -> Result<Option<String>> {
    let Some(directory) = regex(r"(?:^|/)(?:src/)?app/")?.find(path)? else {
        return Ok(None);
    };
    let relative = &path[directory.end()..];
    let Some(extension) = regex(r"\.(tsx|ts|jsx|js|mjs|cjs)$")?.find(relative)? else {
        return Ok(None);
    };
    let Some(bare) = relative[..extension.start()].strip_suffix("+api") else {
        return Ok(None);
    };
    let mut parts: Vec<_> = bare
        .split('/')
        .filter(|s| !s.is_empty() && !group(s))
        .collect();
    if parts.last() == Some(&"index") {
        parts.pop();
    }
    let rest = regex(r"^\[\.\.\.(.+)\]$")?;
    let parameter = regex(r"^\[(.+)\]$")?;
    let parts: Vec<_> = parts
        .iter()
        .map(|part| {
            parameter
                .replace(&rest.replace(part, ":$1*"), ":$1")
                .into_owned()
        })
        .collect();
    Ok(Some(format!("/{}", parts.join("/"))))
}
pub(crate) fn svelte(path: &str) -> Result<Option<String>> {
    if path.rsplit(['/', '\\']).next() != Some("+page.svelte") {
        return Ok(None);
    }
    let normalized = path.replace('\\', "/");
    let Some((_, relative)) = normalized.split_once("/routes/") else {
        return Ok(None);
    };
    let directory = relative
        .rsplit_once('/')
        .map_or("", |(directory, _)| directory);
    let path = format!(
        "/{}",
        directory
            .split('/')
            .filter(|s| !s.is_empty() && !group(s))
            .collect::<Vec<_>>()
            .join("/")
    );
    let rest = regex(r"\[\.\.\.([^\]=]+)(?:=[^\]]*)?\]")?
        .replace_all(&path, "*$1")
        .into_owned();
    let optional = regex(r"\[{2}([^\]=]+)(?:=[^\]]*)?\]{2}")?
        .replace_all(&rest, ":$1?")
        .into_owned();
    let parameter = regex(r"\[([^\]=]+)(?:=[^\]]*)?\]")?
        .replace_all(&optional, ":$1")
        .into_owned();
    Ok(Some(if parameter == "/" {
        parameter
    } else {
        parameter.strip_suffix('/').unwrap_or(&parameter).to_owned()
    }))
}
fn group(part: &str) -> bool {
    part.starts_with('(') && part.ends_with(')')
}
