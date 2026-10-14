use anyhow::{Context, bail};
use chrono::{DateTime, Local, NaiveDate, NaiveDateTime};
use rusqlite::{OptionalExtension, params};
use std::collections::HashSet;
use std::fs;
use std::io::Write;
use std::path::{Component, Path, PathBuf};

use crate::{db::Db, hash, import::lock_library, layout};

const HEADER: &str =
    "library_path\toriginal_names\tsource_created\tsource_modified\tsuggested\tuse";

#[derive(Debug, Default)]
pub struct ApplyReport {
    pub moved: Vec<(String, String)>,
    pub kept_undated: usize,
}

struct AcceptedRow {
    line: usize,
    path: String,
    date: NaiveDateTime,
    basis: &'static str,
    hash: String,
    sidecars: Vec<(String, String)>,
}

pub fn apply(library: &Path, tsv: &Path) -> anyhow::Result<ApplyReport> {
    let text = fs::read_to_string(tsv)?;
    let mut lines = text.lines();
    if lines.next() != Some(HEADER) {
        bail!("line 1: invalid date preview header");
    }
    let mut rows = Vec::new();
    let mut report = ApplyReport::default();
    for (index, line) in lines.enumerate() {
        let line_number = index + 2;
        let fields: Vec<_> = line.split('\t').collect();
        if fields.len() != 6 {
            bail!("line {line_number}: expected 6 fields");
        }
        if fields[5].is_empty() {
            report.kept_undated += 1;
            continue;
        }
        let date = parse_date(fields[5])
            .with_context(|| format!("line {line_number}: invalid use date {:?}", fields[5]))?;
        rows.push(AcceptedRow {
            line: line_number,
            path: fields[0].into(),
            date,
            basis: if fields[5] == fields[4] {
                "estimate"
            } else {
                "manual"
            },
            hash: String::new(),
            sidecars: Vec::new(),
        });
    }

    let library = fs::canonicalize(library)?;
    let _lock = lock_library(&library)?;
    let db_path = library.join("library.db");
    if !db_path.is_file() {
        bail!("{} has no library.db; run import first", library.display());
    }
    let mut db = Db::open(&db_path)?;
    db.snapshot(&library.join(".catalog-backups"))?;
    let mut paths = HashSet::new();
    for row in &mut rows {
        let validated = (|| -> anyhow::Result<()> {
            validate_relative(&row.path)?;
            row.hash = db.conn.query_row(
                "SELECT hash FROM assets WHERE library_path=?1 AND taken_at IS NULL AND kind!='sidecar'",
                [&row.path], |r| r.get(0),
            ).optional()?.context("path is not a recorded undated asset")?;
            let paired: bool = db.conn.query_row(
                "SELECT EXISTS(SELECT 1 FROM live_pairs WHERE photo_hash=?1 OR mov_hash=?1)",
                [&row.hash],
                |r| r.get(0),
            )?;
            if paired {
                bail!("Live Photo pairs cannot be dated separately");
            }
            row.sidecars = db.conn.prepare(
                "SELECT sidecar_hash,library_path FROM sidecars WHERE asset_hash=?1 ORDER BY library_path",
            )?.query_map([&row.hash], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect::<rusqlite::Result<_>>()?;
            for (expected, relative) in std::iter::once((&row.hash, &row.path))
                .chain(row.sidecars.iter().map(|(hash, path)| (hash, path)))
            {
                let path = checked_file(&library, relative, expected)?;
                if !paths.insert(path) {
                    bail!("duplicate accepted file: {relative}");
                }
            }
            Ok(())
        })();
        validated.with_context(|| format!("line {}: {}", row.line, row.path))?;
    }

    for row in rows {
        let mut links = Vec::new();
        let result = (|| -> anyhow::Result<Vec<(String, String)>> {
            let old = library.join(&row.path);
            let ext = old
                .extension()
                .context("asset has no extension")?
                .to_string_lossy();
            let new = layout::unique_path(
                &library,
                &layout::dest_dir(Some(row.date)),
                &format!(
                    "{}_estimated_{}",
                    row.date.format("%Y-%m-%d"),
                    hash::short_hash(&row.hash)
                ),
                &ext,
            );
            let parent = new.parent().context("destination has no parent")?;
            let ancestor = parent
                .ancestors()
                .find(|p| p.exists())
                .context("destination has no existing ancestor")?;
            if !fs::canonicalize(ancestor)?.starts_with(&library) {
                bail!("destination leaves library");
            }
            fs::create_dir_all(parent)?;
            fs::hard_link(&old, &new)?;
            links.push(new.clone());
            let relative = new
                .strip_prefix(&library)?
                .to_string_lossy()
                .replace('\\', "/");
            let mut moved = vec![(row.path.clone(), relative.clone())];
            let tx = db.conn.transaction()?;
            tx.execute(
                "UPDATE assets SET taken_at=?1,date_basis=?2,library_path=?3 WHERE hash=?4",
                params![
                    row.date.format("%Y-%m-%dT%H:%M:%S").to_string(),
                    row.basis,
                    relative,
                    row.hash
                ],
            )?;
            for (sidecar_hash, old_relative) in &row.sidecars {
                let old = library.join(old_relative);
                let ext = old
                    .extension()
                    .context("sidecar has no extension")?
                    .to_string_lossy()
                    .to_ascii_lowercase();
                let target = new.with_extension(ext);
                fs::hard_link(&old, &target)?;
                links.push(target.clone());
                let relative = target
                    .strip_prefix(&library)?
                    .to_string_lossy()
                    .replace('\\', "/");
                tx.execute(
                    "UPDATE sidecars SET library_path=?1 WHERE sidecar_hash=?2 AND asset_hash=?3",
                    params![relative, sidecar_hash, row.hash],
                )?;
                moved.push((old_relative.clone(), relative));
            }
            tx.commit()?;
            Ok(moved)
        })();
        let moved = match result {
            Ok(moved) => moved,
            Err(error) => {
                let mut cleanup_errors = Vec::new();
                for path in links.iter().rev() {
                    if let Err(error) = fs::remove_file(path) {
                        cleanup_errors.push(format!("{}: {error}", path.display()));
                    }
                }
                bail!(
                    "line {}: {error:#}; new-link cleanup errors: {cleanup_errors:?}; already moved: {:?}",
                    row.line,
                    report.moved
                );
            }
        };
        report
            .moved
            .push((row.path.replace('\\', "/"), moved[0].1.clone()));
        for (index, (old, _)) in moved.into_iter().enumerate() {
            let expected = if index == 0 {
                &row.hash
            } else {
                &row.sidecars[index - 1].0
            };
            checked_file(&library, &old, expected).with_context(|| {
                format!(
                    "committed move, but old path {old} no longer matches; preserved it; already moved: {:?}",
                    report.moved
                )
            })?;
            fs::remove_file(library.join(&old)).with_context(|| {
                format!(
                    "committed move, but could not remove old path {old}; already moved: {:?}",
                    report.moved
                )
            })?;
        }
    }
    Ok(report)
}

fn parse_date(text: &str) -> anyhow::Result<NaiveDateTime> {
    let (date, format) = match text.len() {
        10 => (
            NaiveDate::parse_from_str(text, "%Y-%m-%d")?
                .and_hms_opt(0, 0, 0)
                .unwrap(),
            "%Y-%m-%d",
        ),
        19 => (
            NaiveDateTime::parse_from_str(text, "%Y-%m-%d %H:%M:%S")?,
            "%Y-%m-%d %H:%M:%S",
        ),
        _ => bail!("expected YYYY-MM-DD or YYYY-MM-DD HH:MM:SS"),
    };
    if date.format(format).to_string() != text {
        bail!("expected YYYY-MM-DD or YYYY-MM-DD HH:MM:SS");
    }
    Ok(date)
}

fn validate_relative(text: &str) -> anyhow::Result<()> {
    if text.contains(['\t', '\n', '\r', ':'])
        || text
            .split(['/', '\\'])
            .any(|part| part.is_empty() || part == "." || part == "..")
        || Path::new(text)
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
    {
        bail!("invalid relative library path: {text:?}");
    }
    Ok(())
}

fn checked_file(library: &Path, relative: &str, expected: &str) -> anyhow::Result<PathBuf> {
    validate_relative(relative)?;
    let path = library.join(relative);
    let canonical = fs::canonicalize(&path)?;
    if !canonical.starts_with(library) || !fs::symlink_metadata(&path)?.file_type().is_file() {
        bail!("library path is not a regular file inside the library: {relative}");
    }
    if hash::hash_file(&path)? != expected {
        bail!("library file changed since import: {relative}");
    }
    Ok(canonical)
}

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
    writeln!(file, "{HEADER}")?;
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
