//! 域名与网关路由生成(T-315 / PAAS-05)。
//!
//! 本模块只做**纯计算**:主机名校验、自动域名分配、Nginx 配置与 APISIX 路由渲染。
//! 副作用(写网关)留在用例层,便于单测与复用。

use serde::Serialize;

/// 默认应用域名后缀(可用 `CODEPLAN_APP_DOMAIN_SUFFIX` 覆盖)。
pub const DEFAULT_DOMAIN_SUFFIX: &str = "apps.internal";

/// 取应用域名后缀。
#[must_use]
pub fn domain_suffix() -> String {
    std::env::var("CODEPLAN_APP_DOMAIN_SUFFIX")
        .ok()
        .map(|s| s.trim().trim_start_matches('.').to_owned())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| DEFAULT_DOMAIN_SUFFIX.to_owned())
}

/// 主机名校验(RFC 1123 的宽松版):标签 1~63 位,字母/数字/连字符,不以连字符开头结尾,
/// 至少两段,总长 ≤ 253,允许 `*.` 泛域名前缀。
#[must_use]
pub fn valid_hostname(host: &str) -> bool {
    let host = host.trim();
    if host.is_empty() || host.len() > 253 {
        return false;
    }
    let body = host.strip_prefix("*.").unwrap_or(host);
    if body.is_empty() {
        return false;
    }
    let labels: Vec<&str> = body.split('.').collect();
    if labels.len() < 2 {
        return false;
    }
    labels.iter().all(|l| {
        !l.is_empty()
            && l.len() <= 63
            && !l.starts_with('-')
            && !l.ends_with('-')
            && l.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-')
    })
}

/// 自动分配域名:`{app_key}-{env_name}.{suffix}`(小写化;非字母数字压成连字符)。
#[must_use]
pub fn auto_host(app_key: &str, env_name: &str, suffix: &str) -> String {
    let slug = |s: &str| {
        s.to_ascii_lowercase()
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
            .collect::<String>()
            .trim_matches('-')
            .to_owned()
    };
    format!("{}-{}.{}", slug(app_key), slug(env_name), suffix)
}

/// 渲染所需的路由信息。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct GatewayRoute {
    /// 应用键。
    pub app_key: String,
    /// 环境名。
    pub env_name: String,
    /// 绑定的域名(server_name / hosts)。
    pub hosts: Vec<String>,
    /// 后端节点 `(host, port)`。
    pub upstreams: Vec<(String, u16)>,
    /// 容器端口(上游端口)。
    pub upstream_port: u16,
    /// 是否启用 TLS。
    pub tls_enabled: bool,
    /// 证书引用(启用 TLS 时用于定位证书文件)。
    pub cert_ref: String,
}

impl GatewayRoute {
    /// TLS 是否**真正生效**:声明启用 **且** 证书已就绪。
    ///
    /// nginx 的 `listen ... ssl` 必须配 `ssl_certificate`,否则 `nginx -t` 直接报错、
    /// reload 会让网关起不来 —— 证书未签发(T-316)前一律退回明文 80。
    #[must_use]
    pub fn tls_active(&self) -> bool {
        self.tls_enabled && !self.cert_ref.is_empty()
    }

    /// 上游组名(小写、连字符,合法标识符)。
    #[must_use]
    pub fn upstream_name(&self) -> String {
        let clean = |s: &str| {
            s.to_ascii_lowercase()
                .chars()
                .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
                .collect::<String>()
        };
        format!("codeplan_{}_{}", clean(&self.app_key), clean(&self.env_name))
    }
}

/// 生成 Nginx 站点配置(upstream + server)。
#[must_use]
pub fn render_nginx(route: &GatewayRoute) -> String {
    let mut out = String::new();
    out.push_str("# 由 CodePlan 自动生成,请勿手工修改(T-315)\n");
    out.push_str(&format!("upstream {} {{\n", route.upstream_name()));
    for (host, _) in &route.upstreams {
        out.push_str(&format!("    server {host}:{};\n", route.upstream_port));
    }
    out.push_str("}\n\n");

    let server_name = if route.hosts.is_empty() {
        "_".to_owned()
    } else {
        route.hosts.join(" ")
    };
    let proxy = |listen: &str| {
        format!(
            "    listen {listen};\n    server_name {server_name};\n\n    \
             location / {{\n        proxy_pass http://{};\n        \
             proxy_http_version 1.1;\n        proxy_set_header Host $host;\n        \
             proxy_set_header X-Real-IP $remote_addr;\n        \
             proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;\n        \
             proxy_set_header X-Forwarded-Proto $scheme;\n        \
             proxy_set_header Upgrade $http_upgrade;\n        \
             proxy_set_header Connection \"upgrade\";\n    }}\n",
            route.upstream_name()
        )
    };

    // 80 端口:证书就绪时做跳转,否则直接代理
    out.push_str("server {\n");
    if route.tls_active() {
        out.push_str(&format!(
            "    listen 80;\n    server_name {server_name};\n    return 301 https://$host$request_uri;\n"
        ));
    } else {
        out.push_str(&proxy("80"));
    }
    out.push_str("}\n");

    if route.tls_active() {
        out.push_str("\nserver {\n");
        out.push_str(&proxy("443 ssl"));
        out.push_str(&format!(
            "    ssl_certificate /etc/codeplan/certs/{}.crt;\n    \
             ssl_certificate_key /etc/codeplan/certs/{}.key;\n",
            route.cert_ref, route.cert_ref
        ));
        out.push_str("}\n");
    }
    out
}

/// 生成 APISIX 路由对象(Admin API `PUT /apisix/admin/routes/{id}` 的 body)。
#[must_use]
pub fn render_apisix(route: &GatewayRoute, route_id: &str) -> serde_json::Value {
    let nodes: serde_json::Map<String, serde_json::Value> = route
        .upstreams
        .iter()
        .map(|(h, _)| {
            (
                format!("{h}:{}", route.upstream_port),
                serde_json::json!(1),
            )
        })
        .collect();
    let mut plugins = serde_json::Map::new();
    if route.tls_active() {
        // 证书由 APISIX 侧按 cert_ref 关联;这里只声明重定向
        plugins.insert(
            "redirect".to_owned(),
            serde_json::json!({"http_to_https": true}),
        );
    }
    serde_json::json!({
        "id": route_id,
        "name": format!("{}-{}", route.app_key, route.env_name),
        "uri": "/*",
        "hosts": route.hosts,
        "plugins": plugins,
        "upstream": {
            "type": "roundrobin",
            "scheme": "http",
            "nodes": nodes,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hostname_validation() {
        assert!(valid_hostname("web-portal-dev.apps.internal"));
        assert!(valid_hostname("a.b.co"));
        assert!(valid_hostname("*.example.com"), "泛域名允许");
        assert!(!valid_hostname("localhost"), "单段不允许");
        assert!(!valid_hostname("-bad.example.com"), "连字符开头不允许");
        assert!(!valid_hostname("bad-.example.com"), "连字符结尾不允许");
        assert!(!valid_hostname("bad_host.example.com"), "下划线不允许");
        assert!(!valid_hostname(""));
        assert!(!valid_hostname(&format!("{}.com", "x".repeat(64))), "标签超长");
    }

    #[test]
    fn auto_host_slugifies() {
        assert_eq!(
            auto_host("web-portal", "dev", "apps.internal"),
            "web-portal-dev.apps.internal"
        );
        assert_eq!(
            auto_host("Web Portal", "STAGING", "apps.internal"),
            "web-portal-staging.apps.internal",
            "非字母数字压成连字符并小写"
        );
        assert!(valid_hostname(&auto_host("my_app", "prod", "apps.internal")));
    }

    fn route(tls: bool, hosts: &[&str]) -> GatewayRoute {
        GatewayRoute {
            app_key: "web-portal".to_owned(),
            env_name: "dev".to_owned(),
            hosts: hosts.iter().map(|s| (*s).to_owned()).collect(),
            upstreams: vec![("10.0.0.11".to_owned(), 22), ("10.0.0.12".to_owned(), 22)],
            upstream_port: 8080,
            tls_enabled: tls,
            cert_ref: if tls { "web-portal-dev".to_owned() } else { String::new() },
        }
    }

    #[test]
    fn nginx_contains_upstream_and_server_names() {
        let cfg = render_nginx(&route(true, &["web-portal-dev.apps.internal"]));
        assert!(cfg.contains("upstream codeplan_web_portal_dev {"));
        assert!(cfg.contains("server 10.0.0.11:8080;"), "上游端口取规格端口");
        assert!(cfg.contains("server 10.0.0.12:8080;"));
        assert!(cfg.contains("server_name web-portal-dev.apps.internal;"));
        assert!(cfg.contains("proxy_pass http://codeplan_web_portal_dev;"));
        assert!(cfg.contains("return 301 https://$host$request_uri;"), "80 跳转");
        assert!(cfg.contains("listen 443 ssl;"));
        assert!(cfg.contains("ssl_certificate /etc/codeplan/certs/web-portal-dev.crt;"));
        assert!(cfg.contains("Upgrade $http_upgrade"), "支持 WebSocket 升级");
    }

    #[test]
    fn nginx_without_tls_proxies_on_80() {
        let cfg = render_nginx(&route(false, &["a.example.com"]));
        assert!(cfg.contains("listen 80;"));
        assert!(!cfg.contains("443"), "未启用 TLS 不应出现 443");
        assert!(!cfg.contains("return 301"));
    }

    #[test]
    fn nginx_without_hosts_uses_catch_all() {
        let cfg = render_nginx(&route(false, &[]));
        assert!(cfg.contains("server_name _;"));
    }

    #[test]
    fn apisix_route_shape() {
        let r = route(true, &["a.example.com", "b.example.com"]);
        let v = render_apisix(&r, "codeplan-web-portal-dev");
        assert_eq!(v["id"], "codeplan-web-portal-dev");
        assert_eq!(v["uri"], "/*");
        assert_eq!(v["hosts"][0], "a.example.com");
        assert_eq!(v["hosts"][1], "b.example.com");
        assert_eq!(v["upstream"]["type"], "roundrobin");
        assert_eq!(v["upstream"]["nodes"]["10.0.0.11:8080"], 1, "节点带上游端口");
        assert_eq!(v["plugins"]["redirect"]["http_to_https"], true);
    }

    #[test]
    fn tls_block_requires_certificate() {
        // 声明启用 TLS 但证书未就绪:不得出现 `listen ... ssl`,否则 nginx -t 失败
        let mut r = route(true, &["a.example.com"]);
        r.cert_ref = String::new();
        let cfg = render_nginx(&r);
        assert!(!cfg.contains("ssl"), "无证书时不得出现 ssl 指令:\n{cfg}");
        assert!(cfg.contains("listen 80;"));
        assert!(cfg.contains("proxy_pass"), "应退化为明文代理而不是报错");
        assert!(!r.tls_active());
    }

    #[test]
    fn ssl_directive_always_paired_with_certificate() {
        // 不变量:出现 `listen 443 ssl` 必伴随 ssl_certificate
        for (tls, cert) in [(true, "c1"), (false, ""), (true, "")] {
            let mut r = route(tls, &["a.example.com"]);
            r.cert_ref = cert.to_owned();
            let cfg = render_nginx(&r);
            if cfg.contains("443 ssl") {
                assert!(cfg.contains("ssl_certificate /etc/codeplan/certs/"), "缺证书:{cfg}");
                assert!(cfg.contains("ssl_certificate_key"));
            }
        }
    }

    #[test]
    fn apisix_without_tls_has_no_redirect_plugin() {
        let v = render_apisix(&route(false, &["a.example.com"]), "rid");
        assert!(v["plugins"].as_object().expect("对象").is_empty());
    }
}
