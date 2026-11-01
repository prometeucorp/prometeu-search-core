//! SQLite candidate ordering for context's search channels.
use super::{ContextHit, ContextQuery};
use anyhow::Result;
use rusqlite::{Connection, params_from_iter, types::Value};
use std::{collections::HashSet, path::Path};
pub(crate) fn search(
    db: &Connection,
    root: &Path,
    query: &ContextQuery,
) -> Result<Vec<ContextHit>> {
    match query {
        ContextQuery::Text {
            query,
            kinds,
            limit,
        } => {
            let hits = crate::search::ranked_kinds(db, root, query, *limit, kinds)?;
            let mut statement =
                db.prepare_cached("SELECT id FROM canonical_search WHERE symbol_id=?1")?;
            hits.into_iter()
                .take(*limit)
                .map(|hit| {
                    Ok(ContextHit {
                        id: statement.query_row([hit.id], |r| r.get(0))?,
                        score: hit.score,
                    })
                })
                .collect()
        }
        ContextQuery::ExactNames {
            names,
            kinds,
            limit,
        } => {
            if names.is_empty() {
                return Ok(vec![]);
            }
            let mut sql =
                "SELECT id,path FROM canonical_search WHERE lower(name)=lower(?)".to_string();
            if !kinds.is_empty() {
                sql.push_str(&format!(
                    " AND kind IN ({})",
                    vec!["?"; kinds.len()].join(",")
                ));
            }
            let mut distinct = HashSet::new();
            for name in names {
                let mut values = vec![Value::Text(name.clone())];
                values.extend(kinds.iter().cloned().map(Value::Text));
                let files = db
                    .prepare(
                        &(sql.replace("SELECT id,path", "SELECT DISTINCT path") + " LIMIT 100"),
                    )?
                    .query_map(params_from_iter(values), |r| r.get::<_, String>(0))?
                    .collect::<rusqlite::Result<HashSet<_>>>()?;
                if !files.is_empty() && files.len() < 10 {
                    distinct.extend(files);
                }
            }
            let per_name = 8.max(limit.div_ceil(names.len()));
            let mut result = Vec::new();
            let mut seen = HashSet::new();
            for name in names {
                let mut values = vec![Value::Text(name.clone())];
                values.extend(kinds.iter().cloned().map(Value::Text));
                values.push(Value::Integer((per_name * 3).max(50) as i64));
                let rows = db
                    .prepare(&(sql.clone() + " LIMIT ?"))?
                    .query_map(params_from_iter(values), |r| {
                        Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
                    })?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                let mut hits: Vec<_> = rows
                    .into_iter()
                    .filter(|(id, _)| !seen.contains(id))
                    .map(|(id, path)| ContextHit {
                        id,
                        score: if distinct.contains(&path) { 21.0 } else { 1.0 },
                    })
                    .collect();
                hits.sort_by(|a, b| b.score.total_cmp(&a.score));
                for hit in hits.into_iter().take(per_name) {
                    seen.insert(hit.id.clone());
                    result.push(hit);
                }
            }
            result.sort_by(|a, b| b.score.total_cmp(&a.score));
            result.truncate(*limit);
            Ok(result)
        }
        ContextQuery::Substring {
            text,
            kinds,
            exclude_prefix,
            limit,
        } => {
            let mut sql = "SELECT id FROM canonical_search WHERE name LIKE ?".to_string();
            let mut values = vec![Value::Text(format!("%{text}%"))];
            if *exclude_prefix {
                sql.push_str(" AND name NOT LIKE ?");
                values.push(Value::Text(format!("{text}%")));
            }
            if !kinds.is_empty() {
                sql.push_str(&format!(
                    " AND kind IN ({})",
                    vec!["?"; kinds.len()].join(",")
                ));
                values.extend(kinds.iter().cloned().map(Value::Text));
            }
            sql.push_str(" ORDER BY length(name) ASC LIMIT ?");
            values.push(Value::Integer(*limit as i64));
            Ok(db
                .prepare(&sql)?
                .query_map(params_from_iter(values), |r| {
                    Ok(ContextHit {
                        id: r.get(0)?,
                        score: 1.0,
                    })
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?)
        }
    }
}
