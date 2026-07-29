//! Yomitan 词典包（zip + term_bank_*.json）解析。
//! 与 Python 版 `app/dictionary.py` 的校验规则和文本抽取逻辑保持一致。

use std::io::{Cursor, Read};

use regex::Regex;
use serde_json::Value;
use zip::ZipArchive;

pub const MAX_ARCHIVE_BYTES: usize = 128 * 1024 * 1024;
const MAX_UNCOMPRESSED_BYTES: u64 = 1024 * 1024 * 1024;
const MAX_JSON_FILE_BYTES: u64 = 256 * 1024 * 1024;

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
    term_files: Vec<String>,
    pub metadata: DictionaryMetadata,
}

fn base_name(name: &str) -> &str {
    name.rsplit('/').next().unwrap_or(name)
}

impl<'a> YomitanImporter<'a> {
    pub fn new(archive: &'a [u8]) -> Result<Self, String> {
        if archive.is_empty() {
            return Err("词典文件为空".into());
        }
        if archive.len() > MAX_ARCHIVE_BYTES {
            return Err("词典压缩包超过 128 MiB 限制".into());
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
        ZipArchive::new(Cursor::new(bytes)).map_err(|_| "词典文件不是有效的 ZIP 压缩包".to_string())
    }

    /// 扫描压缩包：定位 index.json 与 term_bank_*.json，并解析元数据。
    fn inspect(&mut self) -> Result<(), String> {
        let mut zip = Self::zip_archive(self.archive)?;
        let mut files: Vec<(String, u64)> = Vec::new();
        for i in 0..zip.len() {
            let entry = zip.by_index(i).map_err(|e| e.to_string())?;
            if entry.is_dir() {
                continue;
            }
            files.push((entry.name().to_string(), entry.size()));
        }
        if files.iter().map(|(_, size)| size).sum::<u64>() > MAX_UNCOMPRESSED_BYTES {
            return Err("词典解压后超过 1 GB 限制".into());
        }

        let mut indexes: Vec<&str> = Vec::new();
        for (name, _) in &files {
            if base_name(name) == "index.json" {
                indexes.push(name.as_str());
            }
        }
        if indexes.is_empty() {
            return Err("不是有效的 Yomitan 词典：缺少 index.json".into());
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

        let bank_pattern = Regex::new(r"^term_bank_(\d+)\.json$").unwrap();
        let mut banks: Vec<(u64, String)> = Vec::new();
        for (name, size) in &files {
            let Some(relative) = name.strip_prefix(&prefix) else {
                continue;
            };
            if let Some(caps) = bank_pattern.captures(relative) {
                if *size > MAX_JSON_FILE_BYTES {
                    return Err(format!("词条文件过大：{relative}"));
                }
                banks.push((caps[1].parse().unwrap_or(0), name.clone()));
            }
        }
        if banks.is_empty() {
            return Err("不是有效的 Yomitan 词典：缺少 term_bank_*.json".into());
        }
        banks.sort_by_key(|(number, _)| *number);
        self.term_files = banks.into_iter().map(|(_, name)| name).collect();
        self.prefix = prefix;

        let index = read_json(&mut zip, &index_name)?;
        self.metadata = parse_metadata(&index)?;
        Ok(())
    }

    /// 逐条处理词条，只在内存中保留当前 term bank。
    pub fn for_each_entry(
        &self,
        mut process: impl FnMut(DictionaryRecord) -> Result<(), String>,
    ) -> Result<u64, String> {
        let mut zip = Self::zip_archive(self.archive)?;
        let mut count = 0;
        for name in &self.term_files {
            let bank = read_json(&mut zip, name)?;
            let Value::Array(items) = bank else {
                return Err(format!("{name} 必须包含词条数组"));
            };
            for (index, raw) in items.iter().enumerate() {
                let Value::Array(fields) = raw else {
                    return Err(format!("{name} 的第 {} 条词条格式无效", index + 1));
                };
                if fields.len() < 6 {
                    return Err(format!("{name} 的第 {} 条词条格式无效", index + 1));
                }
                let term = json_text(&fields[0]).trim().to_string();
                let reading = json_text(&fields[1]).trim().to_string();
                let Value::Array(glossary) = &fields[5] else {
                    continue;
                };
                if term.is_empty() {
                    continue;
                }
                let definition = glossary
                    .iter()
                    .map(text_content)
                    .filter(|part| !part.is_empty())
                    .collect::<Vec<_>>()
                    .join("\n")
                    .trim()
                    .to_string();
                if definition.is_empty() {
                    continue;
                }
                process(DictionaryRecord {
                    term,
                    reading,
                    language: self.metadata.source_language.clone(),
                    definition: definition.chars().take(32_000).collect(),
                    score: fields[4].as_f64().unwrap_or(0.0),
                })?;
                count += 1;
            }
        }
        Ok(count)
    }
}

fn parse_metadata(index: &Value) -> Result<DictionaryMetadata, String> {
    let Value::Object(map) = index else {
        return Err("index.json 内容无效".into());
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
        .ok_or_else(|| "index.json 中的词典格式版本无效".to_string())?;
    if title.is_empty() || revision.is_empty() || !(1..=3).contains(&format_version) {
        return Err("仅支持包含标题、修订号且格式为 1 到 3 的 Yomitan 词典".into());
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
    Ok(DictionaryMetadata {
        title,
        revision,
        source_language,
        target_language,
    })
}

fn read_json(zip: &mut ZipArchive<Cursor<&[u8]>>, name: &str) -> Result<Value, String> {
    let mut entry = zip.by_name(name).map_err(|_| format!("无法读取 {name}"))?;
    let mut bytes = Vec::new();
    entry
        .read_to_end(&mut bytes)
        .map_err(|_| format!("无法读取 {name}"))?;
    // 容忍 UTF-8 BOM（Python 侧用 utf-8-sig 解码）
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        bytes.drain(..3);
    }
    serde_json::from_slice(&bytes).map_err(|_| format!("无法读取 {name}"))
}

/// 词条字段的字符串化（Yomitan 词条首两项恒为字符串，容忍数字兜底）。
fn json_text(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}

/// 从 Yomitan 结构化内容中抽取纯文本（忽略图片，br 转换行）。
fn text_content(value: &Value) -> String {
    match value {
        Value::String(text) => text.trim().to_string(),
        Value::Array(items) => items
            .iter()
            .map(text_content)
            .filter(|p| !p.is_empty())
            .collect(),
        Value::Object(map) => {
            let entry_type = map.get("type").and_then(Value::as_str);
            let tag = map.get("tag").and_then(Value::as_str);
            if entry_type == Some("text") {
                map.get("text")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .trim()
                    .to_string()
            } else if entry_type == Some("image") || tag == Some("img") {
                map.get("alt")
                    .or_else(|| map.get("description"))
                    .or_else(|| map.get("title"))
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .trim()
                    .to_string()
            } else if tag == Some("br") {
                "\n".to_string()
            } else if let Some(content) = map.get("content") {
                text_content(content)
            } else {
                String::new()
            }
        }
        _ => String::new(),
    }
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
}
