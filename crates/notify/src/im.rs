//! 统一 IM 卡片协议(T-211):平台无关卡片模型 + 企微/钉钉/飞书渲染
//! 与回调解析(纯函数,L2 无 IO;回调端点在 server 组合根)。

/// 卡片动作(回调时原样带回)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CardAction {
    /// 动作 ID(如 `pr:approve:{repo}:{number}`)。
    pub action_id: String,
    /// 展示文本。
    pub label: String,
}

/// 统一卡片(通知渠道渲染入参)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Card {
    /// 标题。
    pub title: String,
    /// 正文(纯文本,`\\n` 换行)。
    pub body: String,
    /// 动作按钮(可空)。
    pub actions: Vec<CardAction>,
}

/// IM 平台。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Provider {
    /// 企业微信。
    WeCom,
    /// 钉钉。
    DingTalk,
    /// 飞书。
    Feishu,
}

impl Provider {
    /// 路径段名。
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::WeCom => "wecom",
            Self::DingTalk => "dingtalk",
            Self::Feishu => "feishu",
        }
    }

    /// 从路径段解析。
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "wecom" => Some(Self::WeCom),
            "dingtalk" => Some(Self::DingTalk),
            "feishu" => Some(Self::Feishu),
            _ => None,
        }
    }
}

/// 统一回调(适配器解析产物)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImCallback {
    /// 平台用户标识(企微 userid / 钉钉 staffId / 飞书 open_id)。
    pub platform_user: String,
    /// 动作 ID。
    pub action_id: String,
}

/// 渲染为平台消息体(发信用)。
#[must_use]
pub fn render(provider: Provider, card: &Card) -> serde_json::Value {
    match provider {
        // 企微机器人 webhook:markdown 消息;动作以链接形式(回调经应用侧)
        Provider::WeCom => serde_json::json!({
            "msgtype": "markdown",
            "markdown": {"content": markdown_text(card)},
        }),
        // 钉钉机器人:actionCard(按钮回调带 action_id)
        Provider::DingTalk => serde_json::json!({
            "msgtype": "actionCard",
            "actionCard": {
                "title": card.title,
                "text": markdown_text(card),
                "btns": card.actions.iter().map(|a| {
                    serde_json::json!({"title": a.label, "actionURL": format!("codeplan://action/{action}", action = a.action_id)})
                }).collect::<Vec<_>>(),
            },
        }),
        // 飞书:interactive 卡片
        Provider::Feishu => serde_json::json!({
            "msg_type": "interactive",
            "card": {
                "header": {"title": {"tag": "plain_text", "content": card.title}},
                "elements": [
                    {"tag": "div", "text": {"tag": "lark_md", "content": card.body.replace('\n', "<br>")}},
                    {"tag": "action", "actions": card.actions.iter().map(|a| {
                        serde_json::json!({
                            "tag": "button",
                            "text": {"tag": "plain_text", "content": a.label},
                            "type": "primary",
                            "value": {"action_id": a.action_id},
                        })
                    }).collect::<Vec<_>>()},
                ],
            },
        }),
    }
}

fn markdown_text(card: &Card) -> String {
    let mut text = format!("**{}**\\n{}", card.title, card.body);
    if !card.actions.is_empty() {
        text.push_str("\\n\\n");
        text.push_str(
            &card
                .actions
                .iter()
                .map(|a| format!("[{}]", a.label))
                .collect::<Vec<_>>()
                .join(" "),
        );
    }
    text
}

/// 解析回调体(平台差异在此收敛);无法识别返回 None。
#[must_use]
pub fn parse_callback(provider: Provider, body: &serde_json::Value) -> Option<ImCallback> {
    match provider {
        // 钉钉机器人按钮回调:{ "senderStaffId": "...", "content": { "actionURL": "codeplan://action/..." } }
        Provider::DingTalk => {
            let user = body.get("senderStaffId")?.as_str()?.to_owned();
            let url = body
                .pointer("/content/actionURL")
                .or_else(|| body.get("actionURL"))?
                .as_str()?;
            let action_id = url.strip_prefix("codeplan://action/")?.to_owned();
            Some(ImCallback {
                platform_user: user,
                action_id,
            })
        }
        // 飞书卡片按钮回调:{ "open_id": "...", "action": { "value": { "action_id": "..." } } }
        Provider::Feishu => {
            let user = body
                .get("open_id")
                .or_else(|| body.get("user_id"))?
                .as_str()?
                .to_owned();
            let action_id = body
                .pointer("/action/value/action_id")
                .or_else(|| body.pointer("/value/action_id"))?
                .as_str()?
                .to_owned();
            Some(ImCallback {
                platform_user: user,
                action_id,
            })
        }
        // 企微应用回调(平台转 JSON 后):{ "FromUserName": "...", "action_id": "..." }
        Provider::WeCom => {
            let user = body
                .get("FromUserName")
                .or_else(|| body.get("userid"))?
                .as_str()?
                .to_owned();
            let action_id = body.get("action_id")?.as_str()?.to_owned();
            Some(ImCallback {
                platform_user: user,
                action_id,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn card() -> Card {
        Card {
            title: "PR #1 待评审".into(),
            body: "admin 请求评审 feature → main".into(),
            actions: vec![
                CardAction {
                    action_id: "pr:approve:admin/demo:1".into(),
                    label: "通过".into(),
                },
                CardAction {
                    action_id: "pr:reject:admin/demo:1".into(),
                    label: "驳回".into(),
                },
            ],
        }
    }

    #[test]
    fn render_dingtalk_action_card() {
        let out = render(Provider::DingTalk, &card());
        assert_eq!(out["msgtype"], "actionCard");
        assert_eq!(out["actionCard"]["title"], "PR #1 待评审");
        assert_eq!(out["actionCard"]["btns"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn render_feishu_interactive() {
        let out = render(Provider::Feishu, &card());
        assert_eq!(out["msg_type"], "interactive");
        assert_eq!(out["card"]["header"]["title"]["content"], "PR #1 待评审");
        let btn = &out["card"]["elements"][1]["actions"][0];
        assert_eq!(btn["value"]["action_id"], "pr:approve:admin/demo:1");
    }

    #[test]
    fn render_wecom_markdown() {
        let out = render(Provider::WeCom, &card());
        assert_eq!(out["msgtype"], "markdown");
        let content = out["markdown"]["content"].as_str().unwrap();
        assert!(content.contains("[通过]"));
    }

    #[test]
    fn parse_dingtalk_callback() {
        let body = serde_json::json!({
            "senderStaffId": "staff-1",
            "content": {"actionURL": "codeplan://action/pr:approve:admin/demo:1"},
        });
        let cb = parse_callback(Provider::DingTalk, &body).unwrap();
        assert_eq!(
            cb,
            ImCallback {
                platform_user: "staff-1".into(),
                action_id: "pr:approve:admin/demo:1".into(),
            }
        );
    }

    #[test]
    fn parse_feishu_and_wecom() {
        let fs = serde_json::json!({
            "open_id": "ou_1",
            "action": {"value": {"action_id": "wi:transition:xxx:done"}},
        });
        assert_eq!(
            parse_callback(Provider::Feishu, &fs).unwrap().action_id,
            "wi:transition:xxx:done"
        );
        let wx =
            serde_json::json!({"FromUserName": "wx_1", "action_id": "pr:approve:admin/demo:1"});
        assert_eq!(
            parse_callback(Provider::WeCom, &wx).unwrap().platform_user,
            "wx_1"
        );
    }

    #[test]
    fn provider_paths() {
        assert_eq!(Provider::parse("dingtalk"), Some(Provider::DingTalk));
        assert_eq!(Provider::parse("nope"), None);
        assert_eq!(Provider::Feishu.as_str(), "feishu");
    }
}
