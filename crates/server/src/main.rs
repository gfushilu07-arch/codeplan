//! codeplan 组合根:装配所有业务域、基础设施与插件(L4,唯一二进制)。
//!
//! 当前为阶段 0 骨架:仅健康检查与 OpenAPI 契约端点,随批次任务逐步装配各域。

mod agent_gateway;

use futures_util::StreamExt as _;
use std::sync::Arc;
use std::time::Duration;

use axum::{Json, Router, routing::get};
use russh::server::Server as _;
use serde::Serialize;
use utoipa::{OpenApi, ToSchema};

#[derive(Serialize, ToSchema)]
struct Health {
    status: String,
    version: String,
}

/// Liveness/Readiness 探针。
#[utoipa::path(get, path = "/api/health", responses((status = 200, body = Health)))]
async fn health() -> Json<Health> {
    Json(Health {
        status: "ok".to_owned(),
        version: env!("CARGO_PKG_VERSION").to_owned(),
    })
}

#[derive(OpenApi)]
#[openapi(paths(health), components(schemas(Health)))]
struct ApiDoc;

/// 聚合各域 OpenAPI 片段。
fn build_openapi() -> serde_json::Value {
    let mut doc = ApiDoc::openapi();
    doc.merge(codeplan_identity::api::openapi());
    serde_json::to_value(doc).expect("OpenAPI 文档序列化失败")
}

/// 环境变量缺省值。
fn env_or(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_owned())
}

async fn openapi_json() -> Json<serde_json::Value> {
    Json(build_openapi())
}

/// 平台摘要指标(T-123;Prometheus 抓取经外部 exporter 转换)。
async fn metrics_summary() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "status": "ok",
        "note": "平台计数见 /api/dashboard/counts;进程指标由 OTel 采集"
    }))
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 数据库(迁移 + 连接池)
    let db_url = env_or(
        "CODEPLAN_DATABASE_URL",
        "postgres://codeplan:codeplan-dev@localhost:5434/codeplan",
    );
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(8)
        .acquire_timeout(Duration::from_secs(5))
        .connect(&db_url)
        .await?;
    sqlx::migrate!("../../migrations").run(&pool).await?;

    // 身份域装配
    let users: Arc<dyn codeplan_identity::domain::UserRepository> = Arc::new(
        codeplan_identity::infra::PgUserRepository::new(pool.clone()),
    );
    let jwt_secret = env_or("CODEPLAN_JWT_SECRET", "dev-secret-do-not-use-in-prod");

    // 审计槽(PG;T-106)
    let audit_sink: Arc<dyn codeplan_kernel::audit::AuditSink> =
        Arc::new(codeplan_identity::infra::PgAuditSink::new(pool.clone()));

    // LDAP(可选):登录回退 + 定时同步(T-105)
    let directory: Option<Arc<dyn codeplan_identity::ldap::LdapDirectory>> =
        codeplan_identity::ldap::LdapConfig::from_env()
            .map(|cfg| Arc::new(codeplan_identity::ldap::LdapDirectoryImpl::new(cfg)) as Arc<_>);

    let mut auth = codeplan_identity::service::AuthService::new(Arc::clone(&users), &jwt_secret);
    if let Some(dir) = &directory {
        auth = auth.with_directory(Arc::clone(dir));
    }
    auth = auth.with_audit(Arc::clone(&audit_sink));
    // PAT 存储(T-210):启用个人访问令牌认证
    auth = auth.with_pat_store(Arc::new(codeplan_identity::pat_infra::PgPatStore::new(
        pool.clone(),
    )));
    let auth = Arc::new(auth);

    if let Some(dir) = &directory {
        let hours: u64 = env_or("CODEPLAN_LDAP_SYNC_HOURS", "6").parse().unwrap_or(6);
        if hours > 0 {
            let dir = Arc::clone(dir);
            tokio::spawn(async move {
                let mut ticker =
                    tokio::time::interval(std::time::Duration::from_secs(hours * 3600));
                ticker.tick().await; // 启动即同步一次
                loop {
                    match dir.list_users().await {
                        Ok(list) => println!("LDAP 目录用户 {} 个(同步周期 {hours}h)", list.len()),
                        Err(e) => eprintln!("LDAP 同步失败: {e}"),
                    }
                    ticker.tick().await;
                }
            });
        }
    }

    let rbac = Arc::new(
        codeplan_identity::rbac::RbacService::new(
            Arc::new(codeplan_identity::infra::PgRbacStore::new(pool.clone())),
            Arc::clone(&users),
        )
        .with_audit(Arc::clone(&audit_sink)),
    );
    auth.bootstrap_admin_if_empty(&env_or(
        "CODEPLAN_BOOTSTRAP_ADMIN_PASSWORD",
        "codeplan-admin",
    ))
    .await?;

    let oidc = codeplan_identity::oidc::OidcConfig::from_env()
        .map(|cfg| Arc::new(codeplan_identity::oidc::OidcCtx::new(cfg)));

    // 事件总线(T-110):publish → outbox,中继器转发 NATS
    let nats_url = env_or("CODEPLAN_NATS_URL", "nats://localhost:4223");

    // app.created → 创建者获 app admin(T-310)
    if let Err(e) =
        codeplan_identity::listen::spawn_app_created_grant(&nats_url, Arc::clone(&rbac)).await
    {
        eprintln!("app.created 授权监听启动失败: {e}");
    }
    let event_bus: Arc<dyn codeplan_kernel::events::EventBus> =
        Arc::new(codeplan_eventbus::nats::OutboxEventBus::new(
            codeplan_eventbus::outbox::PgOutboxStore::new(pool.clone()),
        ));
    {
        let pool = pool.clone();
        let nats_url = nats_url.clone();
        tokio::spawn(async move {
            if let Err(e) = codeplan_eventbus::dispatcher::run(pool, &nats_url).await {
                eprintln!("eventbus 中继器退出: {e}");
            }
        });
    }

    // Wiki 域(T-115)
    let data_dir = env_or("CODEPLAN_DATA_DIR", "./data");
    let wiki_store = Arc::new(codeplan_wiki::storage::FsAttachmentStore::new(format!(
        "{data_dir}/attachments"
    )));
    // Git 代码域(T-120)
    let hook_secret = env_or("CODEPLAN_HOOK_SECRET", "dev-hook-secret");
    let repo_root = format!("{data_dir}/repositories");
    // 对外基础地址(LFS href / 钩子回调)
    let external_url = env_or("CODEPLAN_PUBLIC_URL", "http://localhost:8080");
    let bare_store = Arc::new(
        codeplan_git_service::storage::FsBareRepoStore::new(repo_root.clone()).with_hook(
            codeplan_git_service::storage::HookConfig {
                url: external_url.clone(),
                secret: hook_secret.clone(),
            },
        ),
    );
    // LFS 对象存储(T-202,OpenDAL fs 后端)
    let lfs_store = Arc::new(
        codeplan_storage::ObjectStore::fs(format!("{data_dir}/lfs"))
            .expect("LFS 对象存储初始化失败"),
    );

    let wiki_state = codeplan_wiki::api::WikiState {
        svc: Arc::new(codeplan_wiki::service::WikiService::new(
            Arc::new(codeplan_wiki::infra::PgPageRepository::new(pool.clone())),
            Arc::new(codeplan_wiki::infra::PgAttachmentRepository::new(
                pool.clone(),
            )),
            Arc::clone(&wiki_store),
            Arc::clone(&rbac) as Arc<dyn codeplan_kernel::authz::Authz>,
        )),
        authn: Arc::clone(&auth) as Arc<dyn codeplan_kernel::authn::TokenAuthenticator>,
    };

    // 通知(T-116)+ inbox 官方插件(T-117):EXT-01 扩展点首个验证
    let notification_store: Arc<dyn codeplan_notify::domain::NotificationStore> = Arc::new(
        codeplan_notify::infra::PgNotificationStore::new(pool.clone()),
    );
    let plugin_registry = Arc::new(codeplan_ext::channel::PluginRegistry::new());
    plugin_registry.register_channel(Arc::new(codeplan_plugin_inbox::InboxChannel::new(
        Arc::clone(&notification_store),
    )));

    // Git SSH 服务(T-201):russh,公钥认证 + exec git(默认 2222)
    let ssh_port = env_or("CODEPLAN_SSH_PORT", "2322");
    {
        let ssh_deps = codeplan_git_service::ssh::SshDeps {
            directory: Arc::new(codeplan_identity::infra::PgSshKeyStore::new(pool.clone())),
            repos: Arc::new(codeplan_git_service::infra::PgRepositoryStore::new(
                pool.clone(),
            )),
            authz: Arc::clone(&rbac) as Arc<dyn codeplan_kernel::authz::Authz>,
            bare: Arc::clone(&bare_store) as Arc<dyn codeplan_git_service::domain::BareRepoStore>,
            lfs: Some(codeplan_git_service::lfs::EndpointConfig {
                external_url: external_url.clone(),
                token_secret: hook_secret.clone(),
            }),
        };
        let host_key_path = std::path::PathBuf::from(format!("{data_dir}/ssh_host_ed25519"));
        let repo_root_for_ssh = repo_root.clone();
        tokio::spawn(async move {
            let host_key =
                match codeplan_git_service::ssh::load_or_generate_host_key(&host_key_path) {
                    Ok(k) => k,
                    Err(e) => {
                        eprintln!("SSH 宿主密钥失败: {e}");
                        return;
                    }
                };
            let mut ssh_server = codeplan_git_service::ssh::SshServer {
                deps: ssh_deps,
                config: codeplan_git_service::ssh::SshConfig {
                    addr: format!("0.0.0.0:{ssh_port}"),
                    repo_root: repo_root_for_ssh,
                },
                host_keys: vec![host_key],
            };
            let config = Arc::new(russh::server::Config {
                inactivity_timeout: Some(std::time::Duration::from_secs(3600)),
                auth_rejection_time: std::time::Duration::from_secs(1),
                keys: ssh_server.host_keys.clone(),
                ..Default::default()
            });
            let listener = match tokio::net::TcpListener::bind(&ssh_server.config.addr).await {
                Ok(l) => l,
                Err(e) => {
                    eprintln!("SSH 绑定失败: {e}");
                    return;
                }
            };
            println!("git SSH 服务监听 {}", ssh_server.config.addr);
            if let Err(e) = ssh_server.run_on_socket(config, &listener).await {
                eprintln!("SSH 服务退出: {e}");
            }
        });
    }

    // project.created → identity 授予创建者 admin(T-111 闭环)
    if let Err(e) =
        codeplan_identity::listen::spawn_repo_created_grant(&nats_url, Arc::clone(&rbac)).await
    {
        eprintln!("repo.created 监听器启动失败: {e}");
    }
    if let Err(e) =
        codeplan_identity::listen::spawn_project_created_grant(&nats_url, Arc::clone(&rbac)).await
    {
        eprintln!("project.created 监听器启动失败: {e}");
    }

    // 通知分发:workitem.* 事件 → 渠道(经插件注册表,T-116/T-117)
    if let Err(e) = codeplan_notify::listen::spawn_workitem_notifications(
        &nats_url,
        Arc::clone(&plugin_registry),
        Arc::clone(&notification_store),
    )
    .await
    {
        eprintln!("通知分发监听器启动失败: {e}");
    }

    let notify_state = codeplan_notify::api::NotifyState {
        store: Arc::clone(&notification_store),
        authn: Arc::clone(&auth) as Arc<dyn codeplan_kernel::authn::TokenAuthenticator>,
    };

    // 项目管理域(T-111)
    let project_svc = Arc::new(codeplan_project::service::ProjectService::new(
        codeplan_project::service::ProjectStores {
            projects: Arc::new(codeplan_project::infra::PgProjectRepository::new(
                pool.clone(),
            )),
            items: Arc::new(codeplan_project::infra::PgWorkItemRepository::new(
                pool.clone(),
            )),
            sprints: Arc::new(codeplan_project::infra::PgSprintRepository::new(
                pool.clone(),
            )),
            milestones: Arc::new(codeplan_project::infra::PgMilestoneRepository::new(
                pool.clone(),
            )),
            comments: Arc::new(codeplan_project::infra::PgCommentRepository::new(
                pool.clone(),
            )),
            worklogs: Arc::new(codeplan_project::infra::PgWorklogRepository::new(
                pool.clone(),
            )),
        },
        Arc::clone(&rbac) as Arc<dyn codeplan_kernel::authz::Authz>,
        Arc::clone(&event_bus),
    ));
    // 提交关联监听(T-207):commit.pushed 建链 + pr.merged 自动流转
    let commit_link_store = Arc::new(codeplan_project::infra::PgCommitLinkStore::new(
        pool.clone(),
    ));
    {
        let linker_deps = Arc::new(codeplan_project::listen::RefLinkerDeps {
            svc: Arc::clone(&project_svc),
            projects: Arc::new(codeplan_project::infra::PgProjectRepository::new(
                pool.clone(),
            )),
            items: Arc::new(codeplan_project::infra::PgWorkItemRepository::new(
                pool.clone(),
            )),
            links: Arc::clone(&commit_link_store),
        });
        if let Err(e) = codeplan_project::listen::spawn_ref_linker(&nats_url, linker_deps).await {
            eprintln!("提交关联监听器启动失败: {e}");
        }
    }

    let project_state = codeplan_project::api::ProjectState {
        svc: project_svc,
        authn: Arc::clone(&auth) as Arc<dyn codeplan_kernel::authn::TokenAuthenticator>,
        links: Some(commit_link_store),
    };

    // Git 代码域状态(T-120/T-121)
    let git_state = codeplan_git_service::api::GitState {
        svc: Arc::new(codeplan_git_service::service::GitService::new(
            Arc::new(codeplan_git_service::infra::PgRepositoryStore::new(
                pool.clone(),
            )),
            Arc::clone(&bare_store) as Arc<dyn codeplan_git_service::domain::BareRepoStore>,
            Arc::clone(&rbac) as Arc<dyn codeplan_kernel::authz::Authz>,
            Arc::clone(&event_bus),
        )),
        authn: Arc::clone(&auth) as Arc<dyn codeplan_kernel::authn::TokenAuthenticator>,
        repo_root: repo_root.clone(),
        hook_secret: hook_secret.clone(),
    };

    // Tag/Release(T-208):发布 + 附件(对象存储)+ 自动 Notes
    let release_state = codeplan_git_service::release_api::ReleaseState {
        svc: Arc::new(codeplan_git_service::release_service::ReleaseService::new(
            Arc::new(codeplan_git_service::release_infra::PgReleaseStore::new(
                pool.clone(),
            )),
            Arc::new(codeplan_git_service::infra::PgRepositoryStore::new(
                pool.clone(),
            )),
            Arc::new(codeplan_git_service::adapter::CliGitAccess::new(
                repo_root.clone(),
                Arc::new(codeplan_git_service::infra::PgRepositoryStore::new(
                    pool.clone(),
                )) as Arc<dyn codeplan_git_service::domain::RepositoryRepository>,
            )),
            Arc::clone(&rbac) as Arc<dyn codeplan_kernel::authz::Authz>,
            Arc::new(
                codeplan_storage::ObjectStore::fs(format!("{data_dir}/release-assets"))
                    .expect("Release 附件存储初始化失败"),
            ),
            Arc::clone(&event_bus),
        )),
        authn: Arc::clone(&auth) as Arc<dyn codeplan_kernel::authn::TokenAuthenticator>,
    };

    // Webhook 出站(T-209)
    let webhook_store = Arc::new(codeplan_git_service::webhook_infra::PgWebhookStore::new(
        pool.clone(),
    )) as Arc<dyn codeplan_git_service::webhook::WebhookRepository>;
    let webhook_state = codeplan_git_service::webhook_api::WebhookState {
        svc: Arc::new(codeplan_git_service::webhook_service::WebhookService::new(
            Arc::clone(&webhook_store),
            Arc::new(codeplan_git_service::infra::PgRepositoryStore::new(
                pool.clone(),
            )),
            Arc::clone(&rbac) as Arc<dyn codeplan_kernel::authz::Authz>,
        )),
        authn: Arc::clone(&auth) as Arc<dyn codeplan_kernel::authn::TokenAuthenticator>,
    };
    if let Err(e) = codeplan_git_service::webhook_service::spawn_webhook_worker(
        &nats_url,
        Arc::clone(&webhook_state.svc),
        Arc::clone(&webhook_store),
    )
    .await
    {
        eprintln!("Webhook 投递 worker 启动失败: {e}");
    }

    // 提交签名验证(T-210)
    let signing_state = codeplan_git_service::signing_api::SigningState {
        svc: Arc::new(codeplan_git_service::signing::SigningService::new(
            Arc::new(codeplan_git_service::adapter::CliGitAccess::new(
                repo_root.clone(),
                Arc::new(codeplan_git_service::infra::PgRepositoryStore::new(
                    pool.clone(),
                )) as Arc<dyn codeplan_git_service::domain::RepositoryRepository>,
            )),
            Arc::new(codeplan_identity::pat_infra::PgGpgKeyStore::new(
                pool.clone(),
            )),
            Arc::new(codeplan_identity::infra::PgSshKeyStore::new(pool.clone())),
            Arc::new(codeplan_identity::infra::PgSshKeyStore::new(pool.clone())),
            Arc::new(codeplan_identity::lookup_impl::PgIdentityLookup::new(
                Arc::clone(&users),
            )),
        )),
        authn: Arc::clone(&auth) as Arc<dyn codeplan_kernel::authn::TokenAuthenticator>,
    };

    // PR 评审域(T-203/T-204):经 kernel git 契约访问 git,server 装配注入实现
    let pr_api_state = {
        let git_access = Arc::new(codeplan_git_service::adapter::CliGitAccess::new(
            repo_root.clone(),
            Arc::new(codeplan_git_service::infra::PgRepositoryStore::new(
                pool.clone(),
            )) as Arc<dyn codeplan_git_service::domain::RepositoryRepository>,
        ));
        let pr_store = Arc::new(codeplan_code_review::infra::PgPullRequestStore::new(
            pool.clone(),
        )) as Arc<dyn codeplan_code_review::domain::PullRequestRepository>;
        let protection_store = Arc::new(codeplan_code_review::review_infra::PgProtectionStore::new(
            pool.clone(),
        ))
            as Arc<dyn codeplan_code_review::protection::ProtectionRepository>;
        let review_store = Arc::new(codeplan_code_review::review_infra::PgReviewStore::new(
            pool.clone(),
        )) as Arc<dyn codeplan_code_review::review::ReviewRepository>;
        let status_store = Arc::new(codeplan_code_review::review_infra::PgStatusStore::new(
            pool.clone(),
        )) as Arc<dyn codeplan_code_review::review::StatusRepository>;
        let reviewer_store = Arc::new(codeplan_code_review::review_infra::PgReviewerStore::new(
            pool.clone(),
        ))
            as Arc<dyn codeplan_code_review::review::ReviewerRepository>;
        let identity_lookup = Arc::new(codeplan_identity::lookup_impl::PgIdentityLookup::new(
            Arc::clone(&users),
        ));
        let pr_service = Arc::new(codeplan_code_review::service::PullRequestService::new(
            Arc::clone(&pr_store),
            Arc::clone(&git_access) as Arc<dyn codeplan_kernel::git::GitRepoLookup>,
            Arc::clone(&git_access) as Arc<dyn codeplan_kernel::git::GitAccess>,
            Arc::clone(&rbac) as Arc<dyn codeplan_kernel::authz::Authz>,
            Arc::clone(&event_bus),
            Arc::clone(&protection_store),
            Arc::clone(&review_store),
            Arc::clone(&status_store),
            Arc::clone(&reviewer_store),
            Arc::clone(&identity_lookup) as Arc<dyn codeplan_kernel::lookup::IdentityLookup>,
        ));
        let review_service = Arc::new(codeplan_code_review::review_service::ReviewService::new(
            Arc::clone(&pr_store),
            Arc::new(codeplan_code_review::review_infra::PgCommentStore::new(
                pool.clone(),
            )),
            Arc::clone(&review_store),
            Arc::clone(&status_store),
            Arc::clone(&protection_store),
            Arc::clone(&reviewer_store),
            Arc::clone(&identity_lookup) as Arc<dyn codeplan_kernel::lookup::IdentityLookup>,
            Arc::clone(&git_access) as Arc<dyn codeplan_kernel::git::GitRepoLookup>,
            Arc::clone(&git_access) as Arc<dyn codeplan_kernel::git::GitAccess>,
            Arc::clone(&rbac) as Arc<dyn codeplan_kernel::authz::Authz>,
            Arc::clone(&event_bus),
        ));
        // 推送门禁(T-205):pre-receive 钩子回调经此校验
        let push_guard: Arc<dyn codeplan_kernel::git::PushGuard> =
            Arc::new(codeplan_code_review::guard::RulePushGuard::new(
                Arc::clone(&protection_store),
                Arc::clone(&git_access) as Arc<dyn codeplan_kernel::git::GitRepoLookup>,
            ));
        codeplan_code_review::api::PrApiState {
            svc: pr_service,
            review: review_service,
            authn: Arc::clone(&auth) as Arc<dyn codeplan_kernel::authn::TokenAuthenticator>,
            push_guard,
        }
    };

    // 访问凭证(T-210):PAT + GPG 公钥
    let pat_store = Arc::new(codeplan_identity::pat_infra::PgPatStore::new(pool.clone()))
        as Arc<dyn codeplan_identity::pat::PatStore>;
    let gpg_store = Arc::new(codeplan_identity::pat_infra::PgGpgKeyStore::new(
        pool.clone(),
    )) as Arc<dyn codeplan_identity::gpg::GpgKeyRepository>;

    let state = codeplan_identity::api::AuthState {
        auth: Arc::clone(&auth),
        oidc,
        rbac: Arc::clone(&rbac),
        ldap: directory,
        audit: Arc::new(codeplan_identity::infra::PgAuditSink::new(pool.clone())),
        event_bus: Arc::clone(&event_bus),
        users: Arc::clone(&users),
        ssh_keys: Arc::new(codeplan_identity::infra::PgSshKeyStore::new(pool.clone())),
        pats: Some(Arc::clone(&pat_store)),
        gpg_keys: Some(gpg_store),
        orgs: Arc::new(codeplan_identity::infra::PgOrgRepository::new(pool.clone())),
    };

    let mut app: Router = Router::new()
        .route("/api/health", get(health))
        .route("/api/metrics/summary", get(metrics_summary))
        .route("/api/docs/openapi.json", get(openapi_json))
        .merge(codeplan_identity::api::routes().with_state(state.clone()));
    if state.oidc.is_some() {
        app = app.merge(codeplan_identity::oidc::routes().with_state(state.clone()));
    }
    app = app.merge(
        codeplan_project::api::routes(project_state.clone()).with_state(project_state.clone()),
    );
    app = app.merge(codeplan_wiki::api::routes(wiki_state.clone()).with_state(wiki_state));

    // Git 仓库路由(T-120)+ smart HTTP 传输(T-121)
    app = app
        .merge(codeplan_git_service::api::routes(git_state.clone()).with_state(git_state.clone()));
    let transport_state = codeplan_git_service::transport::TransportState {
        svc: git_state.svc.clone(),
        authn: Arc::clone(&auth) as Arc<dyn codeplan_kernel::authn::TokenAuthenticator>,
        repo_root: repo_root.clone(),
        hook_secret: hook_secret.clone(),
        lfs: Some(codeplan_git_service::lfs::LfsState {
            store: Arc::clone(&lfs_store),
            external_url: external_url.clone(),
        }),
        push_guard: Some(pr_api_state.push_guard()),
    };
    app = app.merge(
        codeplan_git_service::transport::routes(transport_state.clone())
            .with_state(transport_state),
    );
    // PR 评审路由(T-203)
    app = app.merge(
        codeplan_code_review::api::routes(pr_api_state.clone()).with_state(pr_api_state.clone()),
    );
    app = app.merge(
        codeplan_git_service::release_api::routes(release_state.clone()).with_state(release_state),
    );
    app = app.merge(
        codeplan_git_service::webhook_api::routes(webhook_state.clone()).with_state(webhook_state),
    );
    app = app.merge(
        codeplan_git_service::signing_api::routes(signing_state.clone()).with_state(signing_state),
    );

    app = app.merge(codeplan_notify::api::routes(notify_state.clone()).with_state(notify_state));

    // 全文搜索(T-213):tantivy 索引 + 全量重建 + 事件增量
    {
        let search_dir = std::path::PathBuf::from(format!("{data_dir}/search-index"));
        let search_svc = Arc::new(codeplan_search::index::SearchService::open(&search_dir)?);
        let n_indexed = codeplan_search::pipeline::rebuild_all(&pool, &search_svc).await?;
        println!("search:全量重建完成,索引 {n_indexed} 篇文档");
        // 代码索引(T-306):仓库文本文件全量 + commit.pushed 增量
        match codeplan_search::pipeline::rebuild_code(&pool, &search_svc, &repo_root).await {
            Ok(n) => println!("search:代码索引完成,索引 {n} 个文件"),
            Err(e) => eprintln!("search:代码全量索引失败(跳过): {e}"),
        }
        if let Err(e) = codeplan_search::pipeline::spawn_code_index_worker(
            &nats_url,
            Arc::clone(&search_svc),
            repo_root.clone(),
        )
        .await
        {
            eprintln!("代码索引 worker 启动失败: {e}");
        }
        if let Err(e) = codeplan_search::pipeline::spawn_search_worker(
            &nats_url,
            pool.clone(),
            Arc::clone(&search_svc),
        )
        .await
        {
            eprintln!("搜索增量 worker 启动失败: {e}");
        }
        let search_state = codeplan_search::api::SearchState {
            svc: search_svc,
            authn: Arc::clone(&auth) as Arc<dyn codeplan_kernel::authn::TokenAuthenticator>,
        };
        app =
            app.merge(codeplan_search::api::routes(search_state.clone()).with_state(search_state));
    }

    // CI(T-301/T-302):workflow 校验 + runner 池
    let runner_pool = Arc::new(codeplan_ci::runner::RunnerPool::new());
    {
        // 内置本机 shell runner(标签可配;外部执行器经注册 API 接入)
        let labels: Vec<String> = std::env::var("CODEPLAN_RUNNER_LABELS")
            .unwrap_or_else(|_| "ubuntu-latest,shell".to_owned())
            .split(',')
            .map(str::trim)
            .map(str::to_owned)
            .collect();
        runner_pool
            .register(
                "local-shell".to_owned(),
                labels,
                codeplan_ci::runner::RunnerKind::Shell,
            )
            .await?;
        println!("ci:内置 shell runner 已注册(local-shell)");
    }
    let ci_store = Arc::new(codeplan_ci::store::CiStore::new(pool.clone()));
    // 制品库(T-304):OpenDAL artifacts/(engine 产物归档 + 四协议共用)
    let artifact_store = Arc::new(codeplan_ci::artifact_store::ArtifactStore::new(
        pool.clone(),
        Arc::new(
            codeplan_storage::ObjectStore::fs(format!("{data_dir}/artifacts"))
                .expect("制品对象存储初始化失败"),
        ),
    ));
    let ci_engine = codeplan_ci::engine::Engine::start(
        Arc::clone(&ci_store),
        Arc::clone(&runner_pool),
        repo_root.clone(),
        std::path::PathBuf::from(format!("{data_dir}/ci")),
        Arc::clone(&event_bus),
        Some(Arc::clone(&artifact_store)),
    );
    let registry_state = codeplan_ci::artifact_api::RegistryState {
        store: Arc::clone(&artifact_store),
        authn: Arc::clone(&auth) as Arc<dyn codeplan_kernel::authn::TokenAuthenticator>,
        repo_hint: repo_root.clone(),
    };

    let ci_state = codeplan_ci::api::CiState {
        authn: Arc::clone(&auth) as Arc<dyn codeplan_kernel::authn::TokenAuthenticator>,
        runners: runner_pool,
        engine: Some(Arc::clone(&ci_engine)),
        store: Arc::clone(&ci_store),
        pool: Some(pool.clone()),
    };
    app = app.merge(codeplan_ci::api::routes(ci_state.clone()).with_state(ci_state));
    app = app.merge(
        codeplan_ci::artifact_api::routes(registry_state.clone()).with_state(registry_state),
    );

    // 安全扫描(T-305):Semgrep/gitleaks/trivy 容器编排 + 发现落库
    let scan_state = codeplan_ci::scan_api::ScanState {
        authn: Arc::clone(&auth) as Arc<dyn codeplan_kernel::authn::TokenAuthenticator>,
        store: Arc::new(codeplan_ci::scan::PgScanStore::new(pool.clone())),
        pool: pool.clone(),
        repo_root: repo_root.clone(),
        events: Arc::clone(&event_bus),
    };
    app = app.merge(codeplan_ci::scan_api::routes(scan_state.clone()).with_state(scan_state));

    // CI push 触发(T-303):commit.pushed → 分支匹配 pipeline → enqueue;
    // ci.run.finished 桥接 → PR commit status(组合根直调 review 服务)
    {
        let nats_client = async_nats::connect(&nats_url).await?;
        let engine_for_push = Arc::clone(&ci_engine);
        let store_for_push = Arc::clone(&ci_store);
        tokio::spawn(async move {
            let mut sub = match nats_client.subscribe("codeplan.events.commit.pushed").await {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("ci push 触发订阅失败: {e}");
                    return;
                }
            };
            println!("ci:已订阅 commit.pushed(流水线触发)");
            while let Some(msg) = sub.next().await {
                let Ok(env) = serde_json::from_slice::<serde_json::Value>(&msg.payload) else {
                    continue;
                };
                let get = |k: &str| {
                    env.get("payload")
                        .and_then(|p| p.get(k))
                        .or_else(|| env.get(k))
                        .and_then(|v| v.as_str())
                        .map(str::to_owned)
                };
                let (Some(full_name), Some(repo_id)) = (get("full_name"), get("repo_id")) else {
                    continue;
                };
                let Ok(repo_uuid) = uuid::Uuid::parse_str(&repo_id) else {
                    continue;
                };
                let Ok(pipelines) = store_for_push
                    .enabled_pipelines(codeplan_kernel::ids::RepoId::from_uuid(repo_uuid))
                    .await
                else {
                    continue;
                };
                if pipelines.is_empty() {
                    continue;
                }
                let refs_line = env
                    .get("payload")
                    .and_then(|p| p.get("refs"))
                    .and_then(|r| r.as_array())
                    .and_then(|a| a.first())
                    .and_then(|r| r.as_str())
                    .unwrap_or_default()
                    .to_owned();
                let branch = refs_line
                    .split_whitespace()
                    .nth(2)
                    .and_then(|r| r.strip_prefix("refs/heads/"))
                    .unwrap_or("")
                    .to_owned();
                let sha = refs_line.split_whitespace().nth(1).unwrap_or("").to_owned();
                for p in pipelines {
                    let Ok(wf) = codeplan_ci::workflow::parse_workflow(&p.yaml) else {
                        continue;
                    };
                    let hit = wf.triggers.push_branches.is_empty()
                        || wf.triggers.push_branches.contains(&branch);
                    if !hit {
                        continue;
                    }
                    match store_for_push
                        .create_run(
                            p.id,
                            codeplan_kernel::ids::RepoId::from_uuid(repo_uuid),
                            &branch,
                            &sha,
                            "push",
                            None,
                        )
                        .await
                    {
                        Ok(run_id) => {
                            let _ = engine_for_push.enqueue(run_id).await;
                            println!(
                                "ci:push 触发 {full_name}:{branch} → run {run_id}(pipeline {})",
                                p.name
                            );
                        }
                        Err(e) => eprintln!("ci:create_run 失败: {e}"),
                    }
                }
            }
        });

        let review_for_status = Arc::clone(&pr_api_state.review);
        let nats_client2 = async_nats::connect(&nats_url).await?;
        tokio::spawn(async move {
            let mut sub = match nats_client2
                .subscribe("codeplan.events.ci.run.finished")
                .await
            {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("ci status 桥订阅失败: {e}");
                    return;
                }
            };
            println!("ci:已订阅 ci.run.finished(commit status 回写)");
            while let Some(msg) = sub.next().await {
                let Ok(env) = serde_json::from_slice::<serde_json::Value>(&msg.payload) else {
                    continue;
                };
                let get = |k: &str| {
                    env.get("payload")
                        .and_then(|p| p.get(k))
                        .or_else(|| env.get(k))
                };
                let (Some(full), Some(sha), Some(ctx), Some(state)) = (
                    get("full_name").and_then(|v| v.as_str()),
                    get("sha").and_then(|v| v.as_str()),
                    get("context").and_then(|v| v.as_str()),
                    get("state").and_then(|v| v.as_str()),
                ) else {
                    continue;
                };
                let check_state = match state {
                    "success" => codeplan_code_review::review::CheckState::Success,
                    _ => codeplan_code_review::review::CheckState::Failure,
                };
                if sha.is_empty() {
                    continue;
                }
                if let Err(e) = review_for_status
                    .set_status_by_system(
                        full,
                        sha,
                        ctx,
                        check_state,
                        &format!(
                            "流水线 {} · {state}",
                            get("pipeline").and_then(|v| v.as_str()).unwrap_or("")
                        ),
                        None,
                    )
                    .await
                {
                    eprintln!("ci status 回写失败: {e}");
                }
            }
        });

        // ci.scan.finished 桥接 → PR 门禁卡点(T-305):
        // critical/high 存在 → 该提交的扫描检查置 failure,阻断合并。
        let review_for_scan = Arc::clone(&pr_api_state.review);
        let nats_client3 = async_nats::connect(&nats_url).await?;
        tokio::spawn(async move {
            let mut sub = match nats_client3
                .subscribe("codeplan.events.ci.scan.finished")
                .await
            {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("scan 门禁桥订阅失败: {e}");
                    return;
                }
            };
            println!("ci:已订阅 ci.scan.finished(PR 安全门禁回写)");
            while let Some(msg) = sub.next().await {
                let Ok(env) = serde_json::from_slice::<serde_json::Value>(&msg.payload) else {
                    continue;
                };
                let get = |k: &str| {
                    env.get("payload")
                        .and_then(|p| p.get(k))
                        .or_else(|| env.get(k))
                };
                let (Some(full), Some(sha), Some(tool)) = (
                    get("full_name").and_then(|v| v.as_str()),
                    get("commit_sha").and_then(|v| v.as_str()),
                    get("tool").and_then(|v| v.as_str()),
                ) else {
                    continue;
                };
                if sha.is_empty() {
                    continue;
                }
                let gated = get("gated").and_then(serde_json::Value::as_bool).unwrap_or(false);
                let total = get("total").and_then(serde_json::Value::as_u64).unwrap_or(0);
                let state = if gated {
                    codeplan_code_review::review::CheckState::Failure
                } else {
                    codeplan_code_review::review::CheckState::Success
                };
                let desc = if gated {
                    format!("{tool}:{total} 项发现(含高危),已阻断合并")
                } else {
                    format!("{tool}:{total} 项发现,无高危")
                };
                if let Err(e) = review_for_scan
                    .set_status_by_system(full, sha, &format!("scan/{tool}"), state, &desc, None)
                    .await
                {
                    eprintln!("scan 门禁回写失败: {e}");
                }
            }
        });
    }

    // 报表域(T-212):proj_* 只读投影 + Excel/PDF 导出
    let insights_state = codeplan_insights::api::InsightsState {
        pool: pool.clone(),
        authn: Arc::clone(&auth) as Arc<dyn codeplan_kernel::authn::TokenAuthenticator>,
    };
    app = app
        .merge(codeplan_insights::api::routes(insights_state.clone()).with_state(insights_state));

    // 应用中心(T-310):应用/环境/部署目标/变量 + 聚合视图
    let appcenter_repo = Arc::new(codeplan_appcenter::infra::PgAppCenterRepository::new(
        pool.clone(),
    ));
    // agent 命令通道与观测汇聚先于 appcenter 服务创建(服务需要前者作为投递口)
    let agent_hub = Arc::new(agent_gateway::AgentHub::new());
    let deploy_events = Arc::new(agent_gateway::DeployEventHub::new());
    // 证书插件(T-316):未配置 CODEPLAN_ACME_DIRECTORY 时不启用。
    // 先装 rustls provider:依赖树同时含 aws-lc-rs 与 ring 时,不显式选定会在握手期 panic。
    codeplan_plugin_cert_acme::install_crypto_provider();
    let cert_issuer: Option<Arc<dyn codeplan_ext::CertIssuer>> =
        codeplan_plugin_cert_acme::AcmeCertIssuer::from_env(std::path::Path::new(&data_dir))
            .map(|i| Arc::new(i) as Arc<dyn codeplan_ext::CertIssuer>);
    match &cert_issuer {
        Some(i) => println!("plugin:cert-acme 已装配({})", i.name()),
        None => println!("plugin:cert-acme 未启用(未配置 CODEPLAN_ACME_DIRECTORY)"),
    }
    let config_key = codeplan_appcenter::crypto::ConfigKey::from_env();
    if config_key.is_dev_key {
        eprintln!("[appcenter] 未配置 CODEPLAN_CONFIG_KEY,敏感配置使用开发密钥(生产必须显式配置)");
    }
    let app_state = codeplan_appcenter::api::AppState {
        svc: Arc::new(codeplan_appcenter::service::AppCenterService::new(
            codeplan_appcenter::service::AppCenterStores {
                repo: Arc::clone(&appcenter_repo)
                    as Arc<dyn codeplan_appcenter::domain::AppCenterRepository>,
                specs: Arc::clone(&appcenter_repo)
                    as Arc<dyn codeplan_appcenter::domain::SpecRepository>,
                configs: Arc::clone(&appcenter_repo)
                    as Arc<dyn codeplan_appcenter::domain::ConfigRepository>,
                domains: Arc::clone(&appcenter_repo)
                    as Arc<dyn codeplan_appcenter::domain::DomainRepository>,
                certs: Arc::clone(&appcenter_repo)
                    as Arc<dyn codeplan_appcenter::domain::CertificateRepository>,
                issuer: cert_issuer,
                hosts: Arc::clone(&appcenter_repo)
                    as Arc<dyn codeplan_appcenter::domain::DeployHostRepository>,
                deployments: Arc::clone(&appcenter_repo)
                    as Arc<dyn codeplan_appcenter::domain::DeploymentRepository>,
                agent: Some(Arc::clone(&agent_hub)
                    as Arc<dyn codeplan_appcenter::domain::AgentCommandSink>),
                key: config_key,
            },
            Arc::clone(&rbac) as Arc<dyn codeplan_kernel::authz::Authz>,
            Arc::clone(&event_bus),
        )),
        authn: Arc::clone(&auth) as Arc<dyn codeplan_kernel::authn::TokenAuthenticator>,
    };
    // 续期任务先取到服务句柄,避免与路由装配的所有权冲突
    let appcenter_svc = Arc::clone(&app_state.svc);
    app = app.merge(codeplan_appcenter::api::routes(app_state.clone()).with_state(app_state));

    // 部署观测 WebSocket(T-314)
    let obs_state = agent_gateway::ObservationState {
        events: Arc::clone(&deploy_events),
        authn: Arc::clone(&auth) as Arc<dyn codeplan_kernel::authn::TokenAuthenticator>,
    };
    app = app.merge(agent_gateway::observation_routes(obs_state.clone()).with_state(obs_state));

    // agent gRPC 网关(T-311):agent 主动外连,主机侧零入站端口
    {
        let addr: std::net::SocketAddr = env_or("CODEPLAN_GRPC_ADDR", "0.0.0.0:50051")
            .parse()
            .map_err(|e| format!("CODEPLAN_GRPC_ADDR 非法: {e}"))?;
        let gateway = agent_gateway::AgentGatewayImpl::new(
            Arc::clone(&appcenter_svc),
            Arc::clone(&agent_hub),
            Arc::clone(&deploy_events),
        )
        .into_service();
        // 每小时打印在线 agent 数(运维可见性)
        let hub_for_log = Arc::clone(&agent_hub);
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(3600)).await;
                println!("[agent] 在线 agent:{}", hub_for_log.online_count().await);
            }
        });
        println!("agent gRPC 网关监听 {addr}");
        tokio::spawn(async move {
            if let Err(e) = tonic::transport::Server::builder()
                .add_service(gateway)
                .serve(addr)
                .await
            {
                eprintln!("agent gRPC 服务退出: {e}");
            }
        });
    }

    // 部署超时收敛(T-313):每分钟把超时未回执的部署判失败,避免悬空
    {
        let svc = Arc::clone(&appcenter_svc);
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(60)).await;
                match svc.expire_stale_deployments().await {
                    Ok(0) => {}
                    Ok(n) => println!("[deploy] 收敛超时部署 {n} 条"),
                    Err(e) => eprintln!("[deploy] 超时收敛失败: {e}"),
                }
            }
        });
    }

    // 证书续期(T-316):每小时扫描一次,对 14 天内到期的证书重新签发
    {
        let svc = appcenter_svc;
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(3600)).await;
                match svc.renew_due_certificates(14).await {
                    Ok(0) => {}
                    Ok(n) => println!("[cert] 本轮续期 {n} 张证书"),
                    Err(e) => eprintln!("[cert] 续期扫描失败: {e}"),
                }
            }
        });
    }

    // DORA 事实采集(T-307):ci.run.finished / pr.merged → ins_dora_fact
    {
        let dora_store = Arc::new(codeplan_insights::dora::PgDoraStore::new(pool.clone()));
        if let Err(e) =
            codeplan_insights::dora_listen::spawn_dora_worker(&nats_url, pool.clone(), dora_store)
                .await
        {
            eprintln!("DORA 采集 worker 启动失败: {e}");
        }
    }

    // IM 卡片回调(T-211):统一回调协议 → 动作执行
    {
        let review_svc_for_im = Arc::clone(&pr_api_state.review);
        let project_svc_for_im = Arc::clone(&project_state.svc);
        let users_for_im = Arc::clone(&users);
        let users_for_im_wi = Arc::clone(&users);
        let user_map: std::collections::HashMap<String, String> =
            std::env::var("CODEPLAN_IM_USER_MAP")
                .ok()
                .and_then(|raw| serde_json::from_str(&raw).ok())
                .unwrap_or_default();
        let im_deps = Arc::new(codeplan_notify::im_actions::ActionDeps {
            pr_action: Arc::new(
                move |username: String, repo: String, number: i64, approve: bool| {
                    let svc = Arc::clone(&review_svc_for_im);
                    let users = Arc::clone(&users_for_im);
                    Box::pin(async move {
                        let uid = users
                            .find_by_username(&username)
                            .await
                            .map_err(|e| e.to_string())?
                            .ok_or_else(|| format!("用户 {username} 不存在"))?;
                        let actor = codeplan_notify::im_actions::actor_for(&username, uid.id);
                        let verdict = if approve {
                            codeplan_code_review::review::Verdict::Approved
                        } else {
                            codeplan_code_review::review::Verdict::ChangesRequested
                        };
                        svc.post_review(&actor, &repo, number, verdict, "IM 审批")
                            .await
                            .map(|_| {
                                if approve {
                                    format!("PR #{number} 已通过")
                                } else {
                                    format!("PR #{number} 已驳回")
                                }
                            })
                            .map_err(|e| e.message().to_owned())
                    })
                },
            ),
            wi_action: Arc::new(move |username: String, item_id: String, to: String| {
                let svc = Arc::clone(&project_svc_for_im);
                let users = Arc::clone(&users_for_im_wi);
                Box::pin(async move {
                    let uid = users
                        .find_by_username(&username)
                        .await
                        .map_err(|e| e.to_string())?
                        .ok_or_else(|| format!("用户 {username} 不存在"))?;
                    let actor = codeplan_notify::im_actions::actor_for(&username, uid.id);
                    let id = codeplan_project::domain::WorkItemId::try_from(item_id.as_str())
                        .map_err(|_| "ID 非法".to_owned())?;
                    svc.transition(&actor, id, &to)
                        .await
                        .map(|w| format!("工作项已流转到 {}", w.status))
                        .map_err(|e| e.message().to_owned())
                })
            }),
            user_map,
        });
        app = app.route(
            "/api/im/{provider}/callback",
            axum::routing::post(
                move |axum::extract::Path(provider): axum::extract::Path<String>,
                      Json(body): Json<serde_json::Value>| {
                    let im_deps = Arc::clone(&im_deps);
                    async move {
                        let Some(p) = codeplan_notify::im::Provider::parse(&provider) else {
                            return (
                                axum::http::StatusCode::NOT_FOUND,
                                Json(serde_json::json!({"code": "NOT_FOUND"})),
                            );
                        };
                        match codeplan_notify::im::parse_callback(p, &body) {
                            Some(cb) => {
                                let result =
                                    codeplan_notify::im_actions::execute(&im_deps, &cb).await;
                                (
                                    axum::http::StatusCode::OK,
                                    Json(serde_json::json!({"code": "OK", "result": match result {
                                        codeplan_notify::im_actions::ActionResult::Done(m) =>
                                            serde_json::json!({"status": "done", "message": m}),
                                        codeplan_notify::im_actions::ActionResult::Denied(m) =>
                                            serde_json::json!({"status": "denied", "message": m}),
                                        codeplan_notify::im_actions::ActionResult::Unknown =>
                                            serde_json::json!({"status": "unknown"}),
                                    }})),
                                )
                            }
                            None => (
                                axum::http::StatusCode::BAD_REQUEST,
                                Json(serde_json::json!({"code": "BAD_REQUEST"})),
                            ),
                        }
                    }
                },
            ),
        );
    }

    let addr = env_or("CODEPLAN_ADDR", "0.0.0.0:8080");
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    println!("codeplan-server 监听 http://{addr}");
    axum::serve(listener, app).await?;
    Ok(())
}
