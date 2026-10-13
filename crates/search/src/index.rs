//! 全文搜索索引(T-213):tantivy + NGram 分词(中文子串可搜)。
//!
//! 索引文档:工作项(title+description)与 Wiki 页(title+content);
//! 管线 = 启动全量重建(PG 只读投影)+ 领域事件增量 upsert。

use std::path::Path;
use std::sync::Mutex;

use codeplan_common::error::AppError;
use tantivy::collector::TopDocs;
use tantivy::query::QueryParser;
use tantivy::schema::{IndexRecordOption, Schema, TextFieldIndexing, TextOptions};
use tantivy::{Index, IndexReader, IndexWriter, ReloadPolicy, TantivyDocument, Term};

/// 文档类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DocKind {
    /// 工作项。
    WorkItem,
    /// Wiki 页。
    WikiPage,
    /// 仓库代码文件(T-306)。
    Code,
}

impl DocKind {
    /// 存储值。
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::WorkItem => "workitem",
            Self::WikiPage => "wiki",
            Self::Code => "code",
        }
    }

    /// 解析。
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "workitem" => Some(Self::WorkItem),
            "wiki" => Some(Self::WikiPage),
            "code" => Some(Self::Code),
            _ => None,
        }
    }
}

/// 待索引文档。
#[derive(Debug, Clone)]
pub struct SearchDoc {
    /// 文档 ID(`{kind}:{uuid}`)。
    pub id: String,
    /// 类型。
    pub kind: DocKind,
    /// 所属项目(过滤用,可空)。
    pub project_id: Option<uuid::Uuid>,
    /// 标题。
    pub title: String,
    /// 正文。
    pub body: String,
}

/// 搜索命中。
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct SearchHit {
    /// 文档 ID。
    pub id: String,
    /// 类型。
    pub kind: &'static str,
    /// 项目 ID。
    pub project_id: Option<String>,
    /// 标题。
    pub title: String,
    /// 正文片段(前 120 字符)。
    pub snippet: String,
    /// 相关度(0-1)。
    pub score: f64,
}

/// 搜索服务(tantivy 索引持有)。
pub struct SearchService {
    index: Index,
    reader: IndexReader,
    writer: std::sync::Mutex<IndexWriter>,
    fields: Fields,
    /// 写锁(索引重建与增量互斥)。
    lock: Mutex<()>,
}

#[derive(Clone, Copy)]
struct Fields {
    id: tantivy::schema::Field,
    kind: tantivy::schema::Field,
    project_id: tantivy::schema::Field,
    title: tantivy::schema::Field,
    body: tantivy::schema::Field,
}

/// 从文档取文本字段(0.26:Value trait 经 as_value 解 Leaf)。
fn text_of(doc: &TantivyDocument, f: tantivy::schema::Field) -> Option<String> {
    use tantivy::schema::Value as _;
    match doc.get_first(f)?.as_value() {
        tantivy::schema::document::ReferenceValue::Leaf(leaf) => leaf.as_str().map(str::to_owned),
        _ => None,
    }
}

impl SearchService {
    /// 建索引(目录持久化;目录为空则初始化 schema)。
    ///
    /// # Errors
    /// 索引目录创建/打开失败。
    pub fn open(dir: &Path) -> Result<Self, AppError> {
        let mut sb = Schema::builder();
        let ngram_options = TextOptions::default()
            .set_indexing_options(
                TextFieldIndexing::default()
                    .set_tokenizer("ngram")
                    .set_index_option(IndexRecordOption::WithFreqsAndPositions),
            )
            .set_stored();
        // raw 索引:delete_term / 精确匹配必需
        let raw_options = TextOptions::default().set_indexing_options(
            TextFieldIndexing::default()
                .set_tokenizer("raw")
                .set_index_option(IndexRecordOption::Basic),
        );
        let id = sb.add_text_field("id", raw_options.clone().set_stored());
        let kind = sb.add_text_field("kind", raw_options.clone().set_stored());
        let project_id = sb.add_text_field("project_id", raw_options.set_stored());
        let title = sb.add_text_field("title", ngram_options.clone());
        let body = sb.add_text_field("body", ngram_options);
        let schema = sb.build();

        std::fs::create_dir_all(dir)
            .map_err(|e| AppError::internal("SR").with_details(serde_json::json!(e.to_string())))?;
        let index = Index::open_or_create(MmapDirectoryHolder::open(dir), schema).map_err(|e| {
            AppError::internal("SR").with_details(serde_json::json!(format!("tantivy: {e}")))
        })?;
        index.tokenizers().register(
            "ngram",
            tantivy::tokenizer::NgramTokenizer::new(2, 4, false).expect("ngram 参数合法"),
        );

        let reader = index
            .reader_builder()
            .reload_policy(ReloadPolicy::OnCommitWithDelay)
            .try_into()
            .map_err(|e: tantivy::TantivyError| {
                AppError::internal("SR").with_details(serde_json::json!(e.to_string()))
            })?;
        let writer = index
            .writer(16 * 1024 * 1024)
            .map_err(|e| AppError::internal("SR").with_details(serde_json::json!(e.to_string())))?;
        Ok(Self {
            fields: Fields {
                id,
                kind,
                project_id,
                title,
                body,
            },
            index,
            reader,
            writer: std::sync::Mutex::new(writer),
            lock: Mutex::new(()),
        })
    }

    fn to_document(&self, doc: &SearchDoc) -> TantivyDocument {
        let f = self.fields;
        let mut d = TantivyDocument::default();
        d.add_text(f.id, &doc.id);
        d.add_text(f.kind, doc.kind.as_str());
        if let Some(p) = doc.project_id {
            d.add_text(f.project_id, p.to_string());
        }
        d.add_text(f.title, &doc.title);
        d.add_text(f.body, &doc.body);
        d
    }

    /// 增量 upsert(按 id 覆盖)。
    ///
    /// # Errors
    /// 写入失败。
    pub fn upsert(&self, docs: &[SearchDoc]) -> Result<(), AppError> {
        let _guard = self.lock.lock().expect("搜索写锁中毒");
        let mut writer = self.writer.lock().expect("tantivy 写锁中毒");
        for doc in docs {
            let id_field = self.fields.id;
            writer.delete_term(Term::from_field_text(id_field, &doc.id));
            writer.add_document(self.to_document(doc)).map_err(|e| {
                AppError::internal("SR").with_details(serde_json::json!(e.to_string()))
            })?;
        }
        writer
            .commit()
            .map_err(|e| AppError::internal("SR").with_details(serde_json::json!(e.to_string())))?;
        self.reader
            .reload()
            .map_err(|e| AppError::internal("SR").with_details(serde_json::json!(e.to_string())))?;
        Ok(())
    }

    /// 全量重建(先清空)。
    ///
    /// # Errors
    /// 写入失败。
    pub fn rebuild(&self, docs: &[SearchDoc]) -> Result<(), AppError> {
        let _guard = self.lock.lock().expect("搜索写锁中毒");
        let mut writer = self.writer.lock().expect("tantivy 写锁中毒");
        writer
            .delete_all_documents()
            .map_err(|e| AppError::internal("SR").with_details(serde_json::json!(e.to_string())))?;
        for doc in docs {
            writer.add_document(self.to_document(doc)).map_err(|e| {
                AppError::internal("SR").with_details(serde_json::json!(e.to_string()))
            })?;
        }
        writer
            .commit()
            .map_err(|e| AppError::internal("SR").with_details(serde_json::json!(e.to_string())))?;
        self.reader
            .reload()
            .map_err(|e| AppError::internal("SR").with_details(serde_json::json!(e.to_string())))?;
        Ok(())
    }

    /// 检索(kind 可空 = 全部;project_id 可空 = 全项目)。
    ///
    /// # Errors
    /// 查询解析/执行失败。
    pub fn search(
        &self,
        query: &str,
        kind: Option<DocKind>,
        limit: usize,
    ) -> Result<Vec<SearchHit>, AppError> {
        let query = query.trim();
        if query.is_empty() {
            return Err(AppError::invalid("SR", "查询不能为空"));
        }
        let f = self.fields;
        let parser = QueryParser::for_index(&self.index, vec![f.title, f.body]);
        let parsed = parser
            .parse_query(query)
            .map_err(|e| AppError::invalid("SR", format!("查询语法非法: {e}")))?;
        let searcher = self.reader.searcher();
        let hits = searcher
            .search(&parsed, &TopDocs::with_limit(limit).order_by_score())
            .map_err(|e| AppError::internal("SR").with_details(serde_json::json!(e.to_string())))?;
        let mut out = Vec::new();
        for (score, addr) in hits {
            let Ok(doc) = searcher.doc::<TantivyDocument>(addr) else {
                continue;
            };
            let Some(id) = text_of(&doc, f.id) else {
                continue;
            };
            let kind_text = text_of(&doc, f.kind).unwrap_or_default();
            if let Some(k) = kind
                && kind_text != k.as_str()
            {
                continue;
            }
            let title = text_of(&doc, f.title).unwrap_or_default();
            let body = text_of(&doc, f.body).unwrap_or_default();
            out.push(SearchHit {
                id: id.to_owned(),
                // 回读存储的 kind;未知值按 workitem 兜底(勿再写死二选一,曾把 code 误报为 workitem)
                kind: DocKind::parse(&kind_text)
                    .map(DocKind::as_str)
                    .unwrap_or("workitem"),
                project_id: text_of(&doc, f.project_id),
                title,
                snippet: body.chars().take(120).collect(),
                score: score as f64,
            });
        }
        Ok(out)
    }
}

/// mmap 目录持有(tantivy 0.26 需要显式目录来源)。
struct MmapDirectoryHolder;

impl MmapDirectoryHolder {
    fn open(dir: &Path) -> tantivy::directory::MmapDirectory {
        tantivy::directory::MmapDirectory::open(dir).expect("mmap 目录")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir() -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("cp-search-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn doc(id: &str, kind: DocKind, title: &str, body: &str) -> SearchDoc {
        SearchDoc {
            id: id.to_owned(),
            kind,
            project_id: Some(uuid::Uuid::new_v4()),
            title: title.to_owned(),
            body: body.to_owned(),
        }
    }

    #[test]
    fn upsert_and_search_chinese() {
        let svc = SearchService::open(&temp_dir()).unwrap();
        let docs = vec![
            doc(
                "workitem:1",
                DocKind::WorkItem,
                "登录崩溃",
                "iOS 客户端登录时崩溃,需修复",
            ),
            doc(
                "workitem:2",
                DocKind::WorkItem,
                "导出报表",
                "支持 Excel 与 PDF 导出",
            ),
            doc(
                "wiki:1",
                DocKind::WikiPage,
                "架构说明",
                "四层架构与依赖规则",
            ),
        ];
        svc.rebuild(&docs).unwrap();

        let hits = svc.search("崩溃", None, 10).unwrap();
        assert_eq!(hits.len(), 1, "中文 ngram 命中");
        assert_eq!(hits[0].id, "workitem:1");
        assert!(hits[0].score > 0.0);
        assert_eq!(hits[0].kind, "workitem");

        // 英文
        let hits = svc.search("导出", None, 10).unwrap();
        assert_eq!(hits[0].id, "workitem:2");

        // kind 过滤
        let hits = svc.search("架构", Some(DocKind::WikiPage), 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].id, "wiki:1");
    }

    #[test]
    fn upsert_replaces_old_content() {
        let svc = SearchService::open(&temp_dir()).unwrap();
        svc.rebuild(&[doc("workitem:1", DocKind::WorkItem, "旧标题", "旧内容")])
            .unwrap();
        svc.upsert(&[doc("workitem:1", DocKind::WorkItem, "新标题", "新内容")])
            .unwrap();
        assert_eq!(svc.search("旧标题", None, 10).unwrap().len(), 0);
        assert_eq!(svc.search("新标题", None, 10).unwrap().len(), 1);
    }

    #[test]
    fn empty_query_is_invalid() {
        let svc = SearchService::open(&temp_dir()).unwrap();
        svc.rebuild(&[doc("workitem:1", DocKind::WorkItem, "t", "b")])
            .unwrap();
        assert!(svc.search("", None, 10).is_err(), "空查询应被拒");
    }
}
