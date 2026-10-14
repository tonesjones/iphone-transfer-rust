use anyhow::{Context, bail};
use chrono::{DateTime, Local, NaiveDateTime};
use std::io::Write;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UndatedRow {
    pub library_path: String,
    pub original_names: String,
    pub created: Option<NaiveDateTime>,
    pub modified: Option<NaiveDateTime>,
    pub suggested: Option<NaiveDateTime>,
}

pub fn undated(library: &Path) -> anyhow::Result<Vec<UndatedRow>> {
    let db = crate::db::Db::open_read_only(&library.join("library.db"))?;
    let mut stmt = db.conn.prepare(
        "SELECT a.library_path,
                (SELECT group_concat(original_name, ', ') FROM (SELECT DISTINCT original_name FROM source_files WHERE hash=a.hash ORDER BY original_name)),
                (SELECT group_concat(source_path, char(10)) FROM (SELECT source_path FROM source_files WHERE hash=a.hash ORDER BY source_path))
         FROM assets a
         WHERE a.taken_at IS NULL AND a.kind!='sidecar'
         ORDER BY a.library_path",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, Option<String>>(1)?.unwrap_or_default(),
            row.get::<_, Option<String>>(2)?,
        ))
    })?;
    let mut result = Vec::new();
    for row in rows {
        let (library_path, original_names, sources) = row?;
        let metadata = sources
            .as_deref()
            .unwrap_or_default()
            .lines()
            .map(std::path::PathBuf::from)
            .find_map(|path| std::fs::metadata(path).ok());
        let created = metadata.as_ref().and_then(|meta| {
            meta.created()
                .ok()
                .map(|time| DateTime::<Local>::from(time).naive_local())
        });
        let modified = metadata.as_ref().and_then(|meta| {
            meta.modified()
                .ok()
                .map(|time| DateTime::<Local>::from(time).naive_local())
        });
        let suggested = match (created, modified) {
            (Some(created), Some(modified)) => Some(created.min(modified)),
            (Some(date), None) | (None, Some(date)) => Some(date),
            (None, None) => None,
        };
        result.push(UndatedRow {
            library_path,
            original_names,
            created,
            modified,
            suggested,
        });
    }
    Ok(result)
}

pub fn write_preview(rows: &[UndatedRow], out: &Path) -> anyhow::Result<()> {
    for row in rows {
        if [row.library_path.as_str(), row.original_names.as_str()]
            .iter()
            .any(|field| field.contains(['\t', '\n', '\r']))
        {
            bail!("date preview fields cannot contain tabs or newlines");
        }
    }
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(out)
        .with_context(|| format!("could not create {}", out.display()))?;
    writeln!(
        file,
        "library_path\toriginal_names\tsource_created\tsource_modified\tsuggested\tuse"
    )?;
    for row in rows {
        let fields = [
            row.library_path.as_str(),
            row.original_names.as_str(),
            &format_date(row.created),
            &format_date(row.modified),
            &format_date(row.suggested),
            "",
        ];
        writeln!(file, "{}", fields.join("\t"))?;
    }
    Ok(())
}

fn format_date(date: Option<NaiveDateTime>) -> String {
    date.map(|date| date.format("%Y-%m-%d %H:%M:%S").to_string())
        .unwrap_or_default()
}
