//! Wiki 域用例:页面树组装、Markdown 渲染(comrak)、附件管理、权限校验。

use std::sync::Arc;

use codeplan_common::error::AppError;
use codeplan_kernel::authz::{Action, Authz};
use codeplan_kernel::identity::CurrentUser;
use codeplan_kernel::ids::ProjectId;
use serde_json::json;

use crate::DOMAIN_CODE;
use crate::domain::{Attachment, PageRepository, PageSummary, PageUpsert, WikiPage};
use crate::storage::FsAttachmentStore;

/// Wiki 域服务。
pub struct WikiService {
    pages: Arc<dyn PageRepository>,
    attachments: Arc<dyn crate::domain::AttachmentRepository>,
    store: Arc<FsAttachmentStore>,
    authz: Arc<dyn Authz>,
}

impl WikiService {
    /// 构造。
    #[must_use]
    pub fn new(
        pages: Arc<dyn PageRepository>,
        attachments: Arc<dyn crate::domain::AttachmentRepository>,
        store: Arc<FsAttachmentStore>,
        authz: Arc<dyn Authz>,
    ) -> Self {
        Self {
            pages,
            attachments,
            store,
            authz,
        }
    }

    /// 创建页面。
    ///
    /// # Errors
    /// slug 非法 `WIKI-400`;无写权限 `WIKI-403`;slug 冲突 `WIKI-409`。
    pub async fn create_page(
        &self,
        actor: &CurrentUser,
        project: &codeplan_kernel::ids::ProjectId,
        slug: &str,
        upsert: &PageUpsert,
    ) -> Result<WikiPage, AppError> {
        self.ensure(actor, Action::Write, *project).await?;
        if !crate::service::valid_slug(slug) {
            return Err(AppError::invalid(
                DOMAIN_CODE,
                "slug 须为小写字母/数字/连字符,1~128 位",
            ));
        }
        self.pages
            .create(
                *project,
                slug,
                &upsert.title,
                &upsert.content,
                upsert.parent_id,
                Some(actor.id),
            )
            .await
    }

    /// 更新页面。
    ///
    /// # Errors
    /// 不存在 `WIKI-404`;无写权限 `WIKI-403`。
    pub async fn update_page(
        &self,
        actor: &CurrentUser,
        page_id: crate::domain::PageId,
        upsert: &PageUpsert,
    ) -> Result<WikiPage, AppError> {
        let page = self
            .pages
            .find_by_id(page_id)
            .await?
            .ok_or_else(|| AppError::not_found(DOMAIN_CODE, "页面不存在"))?;
        self.ensure(actor, Action::Write, page.project_id).await?;
        self.pages
            .update(
                page_id,
                &upsert.title,
                &upsert.content,
                upsert.parent_id,
                Some(actor.id),
            )
            .await
    }

    /// 按 slug 取页面(含 Markdown 源与渲染 HTML)。
    ///
    /// # Errors
    /// 不存在 `WIKI-404`;无读权限 `WIKI-403`。
    pub async fn page_by_slug(
        &self,
        actor: &CurrentUser,
        project: &codeplan_kernel::ids::ProjectId,
        slug: &str,
    ) -> Result<(WikiPage, String), AppError> {
        let page = self
            .pages
            .find_by_slug(*project, slug)
            .await?
            .ok_or_else(|| AppError::not_found(DOMAIN_CODE, "页面不存在"))?;
        self.ensure(actor, Action::Read, page.project_id).await?;
        let html = render_markdown(&page.content);
        Ok((page, html))
    }

    /// 页面树(森林:parent 分组,树节点含附件数由前端按需拉取)。
    ///
    /// # Errors
    /// 无读权限 `WIKI-403`。
    pub async fn page_tree(
        &self,
        actor: &CurrentUser,
        project: &codeplan_kernel::ids::ProjectId,
    ) -> Result<Vec<serde_json::Value>, AppError> {
        self.ensure(actor, Action::Read, *project).await?;
        let pages: Vec<PageSummary> = self.pages.list_by_project(*project).await?;
        let node = |p: &PageSummary, children: Vec<serde_json::Value>| {
            json!({
                "id": p.id.as_uuid(),
                "slug": p.slug,
                "title": p.title,
                "parent_id": p.parent_id.map(|x| x.as_uuid()),
                "updated_at": codeplan_common::time::to_rfc3339(p.updated_at),
                "children": children,
            })
        };
        let by_parent = |parent: Option<crate::domain::PageId>| -> Vec<PageSummary> {
            let mut v: Vec<PageSummary> = pages
                .iter()
                .filter(|p| p.parent_id == parent)
                .cloned()
                .collect();
            v.sort_by(|a, b| a.title.cmp(&b.title));
            v
        };
        fn build(
            by_parent: &dyn Fn(Option<crate::domain::PageId>) -> Vec<PageSummary>,
            parent: Option<crate::domain::PageId>,
            node: &dyn Fn(&PageSummary, Vec<serde_json::Value>) -> serde_json::Value,
        ) -> Vec<serde_json::Value> {
            by_parent(parent)
                .iter()
                .map(|p| {
                    let children = build(by_parent, Some(p.id), node);
                    node(p, children)
                })
                .collect()
        }
        Ok(build(&by_parent, None, &node))
    }

    /// 删除页面(级联删附件元数据与文件)。
    ///
    /// # Errors
    /// 不存在 `WIKI-404`;无管理权限 `WIKI-403`。
    pub async fn delete_page(
        &self,
        actor: &CurrentUser,
        page_id: crate::domain::PageId,
    ) -> Result<(), AppError> {
        let page = self
            .pages
            .find_by_id(page_id)
            .await?
            .ok_or_else(|| AppError::not_found(DOMAIN_CODE, "页面不存在"))?;
        self.ensure(actor, Action::Write, page.project_id).await?;
        // 收集子树(带 visited 防环),先删附件(元数据+文件)再自底向上删页面
        let all = self.pages.list_by_project(page.project_id).await?;
        let mut stack = vec![page_id];
        let mut subtree = Vec::new();
        let mut visited = std::collections::HashSet::new();
        while let Some(cur) = stack.pop() {
            if !visited.insert(cur) {
                continue;
            }
            subtree.push(cur);
            for p in &all {
                if p.parent_id == Some(cur) {
                    stack.push(p.id);
                }
            }
        }
        for pid in &subtree {
            for att in self.attachments.list_by_page(*pid).await? {
                if let Some(key) = self.attachments.delete(att.id).await? {
                    self.store.delete(&key).await?;
                }
            }
        }
        for pid in subtree.iter().rev() {
            self.pages.delete(*pid).await?;
        }
        Ok(())
    }

    /// 上传附件(写权限)。
    ///
    /// # Errors
    /// 页面不存在 `WIKI-404`;无写权限 `WIKI-403`;空文件名/存盘失败。
    pub async fn save_attachment(
        &self,
        actor: &CurrentUser,
        page_id: crate::domain::PageId,
        filename: &str,
        content_type: Option<&str>,
        bytes: &[u8],
    ) -> Result<Attachment, AppError> {
        let page = self
            .pages
            .find_by_id(page_id)
            .await?
            .ok_or_else(|| AppError::not_found(DOMAIN_CODE, "页面不存在"))?;
        self.ensure(actor, Action::Write, page.project_id).await?;
        if filename.is_empty() {
            return Err(AppError::invalid(DOMAIN_CODE, "文件名不能为空"));
        }
        let key = format!("{}/{}", page.id.as_uuid(), uuid::Uuid::new_v4());
        self.store.save(&key, bytes).await?;
        self.attachments
            .create(
                page_id,
                filename,
                i64::try_from(bytes.len()).unwrap_or(0),
                content_type,
                &key,
                Some(actor.id),
            )
            .await
    }

    /// 读取附件字节(读权限)+ 元数据。
    ///
    /// # Errors
    /// 不存在 `WIKI-404`。
    pub async fn read_attachment(
        &self,
        actor: &CurrentUser,
        id: crate::domain::AttachmentId,
    ) -> Result<(Attachment, Vec<u8>), AppError> {
        let att = self
            .attachments
            .find_by_id(id)
            .await?
            .ok_or_else(|| AppError::not_found(DOMAIN_CODE, "附件不存在"))?;
        let page = self
            .pages
            .find_by_id(att.page_id)
            .await?
            .ok_or_else(|| AppError::not_found(DOMAIN_CODE, "页面不存在"))?;
        self.ensure(actor, Action::Read, page.project_id).await?;
        let bytes = self.store.read(&att.storage_key).await?;
        Ok((att, bytes))
    }

    /// 页面附件列表。
    ///
    /// # Errors
    /// 无读权限 `WIKI-403`。
    pub async fn list_attachments(
        &self,
        actor: &CurrentUser,
        page_id: crate::domain::PageId,
    ) -> Result<Vec<Attachment>, AppError> {
        let page = self
            .pages
            .find_by_id(page_id)
            .await?
            .ok_or_else(|| AppError::not_found(DOMAIN_CODE, "页面不存在"))?;
        self.ensure(actor, Action::Read, page.project_id).await?;
        self.attachments.list_by_page(page_id).await
    }

    async fn ensure(
        &self,
        actor: &CurrentUser,
        action: Action,
        project_id: ProjectId,
    ) -> Result<(), AppError> {
        let locator = codeplan_kernel::authz::ResourceLocator::Project(project_id.as_uuid());
        self.authz
            .check(actor.id, action, &locator.to_ref())
            .await
            .map_err(|e| AppError::internal(DOMAIN_CODE).with_details(serde_json::json!(e)))?
            .then_some(())
            .ok_or_else(|| AppError::forbidden(DOMAIN_CODE, "无权限执行该操作"))
    }
}

/// slug 规则:小写字母/数字/连字符,1~128 位。
#[must_use]
pub fn valid_slug(slug: &str) -> bool {
    let bytes = slug.as_bytes();
    (1..=128).contains(&bytes.len())
        && bytes
            .iter()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'-')
}

/// Markdown → HTML(GFM:表格/删除线/自动链接)。
#[must_use]
pub fn render_markdown(md: &str) -> String {
    let mut options = comrak::Options::default();
    options.extension.strikethrough = true;
    options.extension.table = true;
    options.extension.autolink = true;
    options.extension.tasklist = true;
    options.render.unsafe_ = true;
    comrak::markdown_to_html(md, &options)
}
