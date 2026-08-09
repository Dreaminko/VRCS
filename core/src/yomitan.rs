//! Yomitan 词典包（zip + term_bank_*.json）解析。
//! 与 Python 版 `app/dictionary.py` 的校验规则和文本抽取逻辑保持一致。

use std::io::{Cursor, Read};

use regex::Regex;
use serde_json::Value;
use zip::ZipArchive;

use crate::error::{AppError, AppResult};

pub const MAX_ARCHIVE_BYTES: usize = 128 * 1024 * 1024;
const MAX_UNCOMPRESSED_BYTES: u64 = 512 * 1024 * 1024;
const MAX_JSON_FILE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_INDEX_BYTES: u64 = 1024 * 1024;
const MAX_FILES: usize = 4096;
const MAX_COMPRESSION_RATIO: u64 = 100;
const MAX_ENTRIES: u64 = 1_000_000;
const MAX_TOTAL_TEXT_CHARS: u64 = 64_000_000;
const MAX_TERM_CHARS: usize = 512;
const MAX_READING_CHARS: usize = 512;
const MAX_DEFINITION_CHARS: usize = 32_000;
const MAX_METADATA_CHARS: usize = 200;
const MAX_LANGUAGE_CHARS: usize = 32;

#[derive(Debug, Clone)]
pub struct DictionaryMetadata {
    pub title: String,
    pub revision: String,
    pub source_language: String,
    pub target_language: Option<String>,
}

#[derive(Debug, Clone)]
pub struct DictionaryRecord {
    pub term: String,
    pub reading: String,
    pub language: String,
    pub definition: String,
    pub score: f64,
}

pub struct YomitanImporter<'a> {
    archive: &'a [u8],
    prefix: String,
    term_files: Vec<(String, u64)>,
    pub metadata: DictionaryMetadata,
}

fn base_name(name: &str) -> &str {
    name.rsplit('/').next().unwrap_or(name)
}

impl<'a> YomitanImporter<'a> {
    pub fn new(archive: &'a [u8]) -> Result<Self, String> {
        if archive.is_empty() {
            return Err("Dictionary file is empty".into());
        }
        if archive.len() > MAX_ARCHIVE_BYTES {
            return Err("Dictionary archive exceeds the 128 MiB limit".into());
        }
        let mut importer = Self {
            archive,
            prefix: String::new(),
            term_files: Vec::new(),
            metadata: DictionaryMetadata {
                title: String::new(),
                revision: String::new(),
                source_language: String::new(),
                target_language: None,
            },
        };
        importer.inspect()?;
        Ok(importer)
    }

    fn zip_archive(bytes: &[u8]) -> Result<ZipArchive<Cursor<&[u8]>>, String> {
        ZipArchive::new(Cursor::new(bytes))
            .map_err(|_| "Dictionary file is not a valid ZIP archive".to_string())
    }

    /// 扫描压缩包：定位 index.json 与 term_bank_*.json，并解析元数据。
    fn inspect(&mut self) -> Result<(), String> {
        let mut zip = Self::zip_archive(self.archive)?;
        if zip.len() > MAX_FILES {
            return Err(format!(
                "Dictionary file count exceeds the limit of {MAX_FILES}"
            ));
        }
        let mut files: Vec<(String, u64)> = Vec::new();
        for i in 0..zip.len() {
            let entry = zip.by_index(i).map_err(|e| e.to_string())?;
            if entry.is_dir() {
                continue;
            }
            files.push((entry.name().to_string(), entry.size()));
        }
        let uncompressed = files.iter().try_fold(0u64, |total, (_, size)| {
            total
                .checked_add(*size)
                .ok_or("Dictionary extracted size overflow")
        })?;
        if uncompressed > MAX_UNCOMPRESSED_BYTES {
            return Err("Extracted dictionary exceeds the 512 MiB limit".into());
        }
        if uncompressed > (self.archive.len() as u64).saturating_mul(MAX_COMPRESSION_RATIO) {
            return Err("Dictionary compression ratio is suspicious; import was rejected".into());
        }

        let mut indexes: Vec<&str> = Vec::new();
        for (name, _) in &files {
            if base_name(name) == "index.json" {
                indexes.push(name.as_str());
            }
        }
        if indexes.is_empty() {
            return Err("Invalid Yomitan dictionary: index.json is missing".into());
        }
        // 取路径最浅的 index.json
        let index_name = indexes
            .into_iter()
            .min_by_key(|name| (name.matches('/').count(), name.len()))
            .unwrap()
            .to_string();
        let prefix = match index_name.rfind('/') {
            Some(pos) => index_name[..=pos].to_string(),
            None => String::new(),
        };
        let index_size = files
            .iter()
            .find(|(name, _)| name == &index_name)
            .map(|(_, size)| *size)
            .unwrap_or(0);
        if index_size > MAX_INDEX_BYTES {
            return Err("index.json exceeds the 1 MiB limit".into());
        }

        let bank_pattern = Regex::new(r"^term_bank_(\d+)\.json$").unwrap();
        let mut banks: Vec<(u64, String, u64)> = Vec::new();
        for (name, size) in &files {
            let Some(relative) = name.strip_prefix(&prefix) else {
                continue;
            };
            if let Some(caps) = bank_pattern.captures(relative) {
                if *size > MAX_JSON_FILE_BYTES {
                    return Err(format!("Term bank file is too large: {relative}"));
                }
                banks.push((caps[1].parse().unwrap_or(0), name.clone(), *size));
            }
        }
        if banks.is_empty() {
            return Err("Invalid Yomitan dictionary: term_bank_*.json is missing".into());
        }
        banks.sort_by_key(|(number, _, _)| *number);
        self.term_files = banks
            .into_iter()
            .map(|(_, name, size)| (name, size))
            .collect();
        self.prefix = prefix;

        let index = read_json(&mut zip, &index_name, MAX_INDEX_BYTES)?;
        self.metadata = parse_metadata(&index)?;
        Ok(())
    }

    /// 逐条处理词条，只在内存中保留当前 term bank。
    #[cfg(test)]
    pub fn for_each_entry(
        &self,
        process: impl FnMut(DictionaryRecord) -> AppResult<()>,
    ) -> AppResult<u64> {
        self.for_each_entry_with_progress(process, |_| {})
    }

    pub fn for_each_entry_with_progress(
        &self,
        mut process: impl FnMut(DictionaryRecord) -> AppResult<()>,
        mut report_progress: impl FnMut(f64),
    ) -> AppResult<u64> {
        let mut zip = Self::zip_archive(self.archive).map_err(AppError::validation)?;
        let mut count = 0u64;
        let mut total_text_chars = 0u64;
        let total_file_bytes = self
            .term_files
            .iter()
            .map(|(_, size)| *size)
            .sum::<u64>()
            .max(1);
        let mut completed_file_bytes = 0u64;
        report_progress(0.0);
        for (name, file_bytes) in &self.term_files {
            let bank =
                read_json(&mut zip, name, MAX_JSON_FILE_BYTES).map_err(AppError::validation)?;
            let Value::Array(items) = bank else {
                return Err(AppError::validation(format!(
                    "{name} must contain a term array"
                )));
            };
            for (index, raw) in items.iter().enumerate() {
                if index % 256 == 0 {
                    let bank_progress =
                        (*file_bytes as f64 * index as f64) / items.len().max(1) as f64;
                    report_progress(
                        (completed_file_bytes as f64 + bank_progress) / total_file_bytes as f64,
                    );
                }
                let Value::Array(fields) = raw else {
                    return Err(AppError::validation(format!(
                        "Term {} in {name} has an invalid format",
                        index + 1
                    )));
                };
                if fields.len() < 6 {
                    return Err(AppError::validation(format!(
                        "Term {} in {name} has an invalid format",
                        index + 1
                    )));
                }
                let term = json_text(&fields[0]);
                let reading = json_text(&fields[1]);
                let Value::Array(glossary) = &fields[5] else {
                    continue;
                };
                if term.is_empty() {
                    continue;
                }
                let term_chars = term.chars().count();
                let reading_chars = reading.chars().count();
                if term_chars > MAX_TERM_CHARS || reading_chars > MAX_READING_CHARS {
                    return Err(AppError::validation(format!(
                        "Term {} in {name} contains text that is too long",
                        index + 1
                    )));
                }
                let mut definition = glossary_text(glossary);
                if definition.is_empty() {
                    continue;
                }
                if count >= MAX_ENTRIES {
                    return Err(AppError::validation(format!(
                        "Dictionary entry count exceeds the limit of {MAX_ENTRIES}"
                    )));
                }
                let definition_chars = truncate_chars(&mut definition, MAX_DEFINITION_CHARS);
                let entry_chars =
                    term_chars as u64 + reading_chars as u64 + definition_chars as u64;
                total_text_chars = total_text_chars
                    .checked_add(entry_chars)
                    .ok_or_else(|| AppError::validation("Dictionary text size overflow"))?;
                if total_text_chars > MAX_TOTAL_TEXT_CHARS {
                    return Err(AppError::validation(
                        "Total dictionary text exceeds the limit",
                    ));
                }
                process(DictionaryRecord {
                    term,
                    reading,
                    language: self.metadata.source_language.clone(),
                    definition,
                    score: fields[4].as_f64().unwrap_or(0.0),
                })?;
                count += 1;
            }
            completed_file_bytes = completed_file_bytes.saturating_add(*file_bytes);
            report_progress(completed_file_bytes as f64 / total_file_bytes as f64);
        }
        report_progress(1.0);
        Ok(count)
    }
}

fn parse_metadata(index: &Value) -> Result<DictionaryMetadata, String> {
    let Value::Object(map) = index else {
        return Err("index.json content is invalid".into());
    };
    let title = map
        .get("title")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    let revision = map
        .get("revision")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    let format_version = map
        .get("format")
        .or_else(|| map.get("version"))
        .and_then(Value::as_i64)
        .ok_or_else(|| "Dictionary format version in index.json is invalid".to_string())?;
    if title.is_empty()
        || revision.is_empty()
        || title.chars().count() > MAX_METADATA_CHARS
        || revision.chars().count() > MAX_METADATA_CHARS
        || !(1..=3).contains(&format_version)
    {
        return Err("Only Yomitan dictionaries with a title, revision, and format version 1 to 3 are supported".into());
    }
    let source_language = map
        .get("sourceLanguage")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("ja")
        .to_string();
    let target_language = map
        .get("targetLanguage")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    if source_language.chars().count() > MAX_LANGUAGE_CHARS
        || target_language
            .as_deref()
            .is_some_and(|language| language.chars().count() > MAX_LANGUAGE_CHARS)
    {
        return Err("Dictionary language identifier is too long".into());
    }
    Ok(DictionaryMetadata {
        title,
        revision,
        source_language,
        target_language,
    })
}

fn read_json(
    zip: &mut ZipArchive<Cursor<&[u8]>>,
    name: &str,
    max_bytes: u64,
) -> Result<Value, String> {
    let entry = zip
        .by_name(name)
        .map_err(|_| format!("Failed to read {name}"))?;
    if entry.size() > max_bytes {
        return Err(format!("{name} exceeds the size limit"));
    }
    let mut bytes = Vec::new();
    entry
        .take(max_bytes + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| format!("Failed to read {name}"))?;
    if bytes.len() as u64 > max_bytes {
        return Err(format!("{name} exceeds the size limit"));
    }
    // 容忍 UTF-8 BOM（Python 侧用 utf-8-sig 解码）
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        bytes.drain(..3);
    }
    serde_json::from_slice(&bytes).map_err(|_| format!("Failed to read {name}"))
}

/// 词条字段的字符串化（Yomitan 词条首两项恒为字符串，容忍数字兜底）。
fn json_text(value: &Value) -> String {
    match value {
        Value::String(text) => text.trim().to_owned(),
        other => other.to_string().trim().to_owned(),
    }
}

fn glossary_text(glossary: &[Value]) -> String {
    let mut output = String::new();
    for value in glossary {
        let before_separator = output.len();
        if !output.is_empty() {
            output.push('\n');
        }
        let before_content = output.len();
        append_text_content(value, &mut output);
        if output.len() == before_content {
            output.truncate(before_separator);
        }
    }
    trim_in_place(&mut output);
    output
}

/// 从 Yomitan 结构化内容中抽取纯文本（图片使用替代文本，br 转换为换行）。
fn append_text_content(value: &Value, output: &mut String) {
    match value {
        Value::String(text) => output.push_str(text.trim()),
        Value::Array(items) => {
            for item in items {
                append_text_content(item, output);
            }
        }
        Value::Object(map) => {
            let entry_type = map.get("type").and_then(Value::as_str);
            let tag = map.get("tag").and_then(Value::as_str);
            if entry_type == Some("text") {
                output.push_str(map.get("text").and_then(Value::as_str).unwrap_or("").trim());
            } else if entry_type == Some("image") || tag == Some("img") {
                output.push_str(
                    map.get("alt")
                        .or_else(|| map.get("description"))
                        .or_else(|| map.get("title"))
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .trim(),
                );
            } else if tag == Some("br") {
                output.push('\n');
            } else if let Some(content) = map.get("content") {
                append_text_content(content, output);
            }
        }
        _ => {}
    }
}

fn trim_in_place(value: &mut String) {
    let start = value.len() - value.trim_start().len();
    let end = value.trim_end().len();
    if start >= end {
        value.clear();
        return;
    }
    value.truncate(end);
    value.drain(..start);
}

fn truncate_chars(value: &mut String, max_chars: usize) -> usize {
    let mut count = 0;
    let mut truncate_at = None;
    for (index, _) in value.char_indices() {
        if count == max_chars {
            truncate_at = Some(index);
            break;
        }
        count += 1;
    }
    if let Some(index) = truncate_at {
        value.truncate(index);
    }
    count
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// 构造一个最小合法的 Yomitan 词典包
    fn sample_archive() -> Vec<u8> {
        let buffer = Cursor::new(Vec::new());
        let mut writer = zip::ZipWriter::new(buffer);
        let options = zip::write::SimpleFileOptions::default();
        writer.start_file("index.json", options).unwrap();
        writer
            .write_all(r#"{"title":"TestDict","revision":"1","format":3,"sourceLanguage":"ja","targetLanguage":"zh"}"#.as_bytes())
            .unwrap();
        writer.start_file("term_bank_1.json", options).unwrap();
        writer
            .write_all(r#"[["食べる","たべる","","",5,[{"type":"text","text":"吃"},{"tag":"br"},"食用"]]]"#.as_bytes())
            .unwrap();
        writer.finish().unwrap().into_inner()
    }

    #[test]
    fn imports_minimal_dictionary() {
        let archive = sample_archive();
        let importer = YomitanImporter::new(&archive).unwrap();
        assert_eq!(importer.metadata.title, "TestDict");
        assert_eq!(importer.metadata.source_language, "ja");
        assert_eq!(importer.metadata.target_language.as_deref(), Some("zh"));
        let mut records = Vec::new();
        importer
            .for_each_entry(|record| {
                records.push(record);
                Ok(())
            })
            .unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].term, "食べる");
        assert_eq!(records[0].reading, "たべる");
        // 与 Python 版一致：text_content(br) 产生 "\n"，join 后再叠加换行，strip 只去首尾
        assert_eq!(records[0].definition, "吃\n\n\n食用");
        assert_eq!(records[0].score, 5.0);
    }

    #[test]
    fn rejects_non_zip() {
        assert!(YomitanImporter::new(b"not a zip").is_err());
    }

    #[test]
    fn rejects_missing_index() {
        let buffer = Cursor::new(Vec::new());
        let mut writer = zip::ZipWriter::new(buffer);
        writer
            .start_file("term_bank_1.json", zip::write::SimpleFileOptions::default())
            .unwrap();
        writer.write_all(b"[]").unwrap();
        let bytes = writer.finish().unwrap().into_inner();
        let err = YomitanImporter::new(&bytes).err().unwrap();
        assert!(err.contains("index.json"));
    }

    #[test]
    fn rejects_oversized_index() {
        let buffer = Cursor::new(Vec::new());
        let mut writer = zip::ZipWriter::new(buffer);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        writer.start_file("index.json", options).unwrap();
        let index = serde_json::json!({
            "title": "TestDict",
            "revision": "1",
            "format": 3,
            "padding": "x".repeat(MAX_INDEX_BYTES as usize),
        });
        writer.write_all(index.to_string().as_bytes()).unwrap();
        writer.start_file("term_bank_1.json", options).unwrap();
        writer.write_all(b"[]").unwrap();
        let bytes = writer.finish().unwrap().into_inner();

        assert!(YomitanImporter::new(&bytes)
            .err()
            .unwrap()
            .contains("index.json"));
    }

    #[test]
    fn rejects_oversized_term() {
        let buffer = Cursor::new(Vec::new());
        let mut writer = zip::ZipWriter::new(buffer);
        let options = zip::write::SimpleFileOptions::default();
        writer.start_file("index.json", options).unwrap();
        writer
            .write_all(br#"{"title":"TestDict","revision":"1","format":3}"#)
            .unwrap();
        writer.start_file("term_bank_1.json", options).unwrap();
        let bank = serde_json::json!([[
            "x".repeat(MAX_TERM_CHARS + 1),
            "",
            "",
            "",
            0,
            ["definition"]
        ]]);
        writer.write_all(bank.to_string().as_bytes()).unwrap();
        let bytes = writer.finish().unwrap().into_inner();
        let importer = YomitanImporter::new(&bytes).unwrap();

        assert!(importer
            .for_each_entry(|_| Ok(()))
            .unwrap_err()
            .to_string()
            .contains("text that is too long"));
    }
}
