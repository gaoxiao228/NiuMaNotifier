use chrono::{DateTime, Utc};

use crate::models::{
    ApprovalChannel, ApprovalDecisionKind, ApprovalProxyStatus, ApprovalRequest, ApprovalStatus,
    ToolKind,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ApprovalMutationResult {
    pub accepted: bool,
    pub request: ApprovalRequest,
}

pub fn upsert_request(requests: &mut Vec<ApprovalRequest>, request: ApprovalRequest) {
    if let Some(existing) = requests.iter_mut().find(|item| item.id == request.id) {
        if existing.status == ApprovalStatus::Pending {
            *existing = request;
        }
        return;
    }
    requests.push(request);
}

pub fn decide(
    requests: &mut [ApprovalRequest],
    request_id: &str,
    decision: ApprovalDecisionKind,
    decided_by: &str,
    decided_source: &str,
    reason: Option<String>,
    now: DateTime<Utc>,
) -> Result<ApprovalMutationResult, String> {
    let request = find_request_mut(requests, request_id)?;
    if !matches!(
        request.status,
        ApprovalStatus::Pending | ApprovalStatus::ReturnedToCodex | ApprovalStatus::ReturnedToTool
    ) {
        return Ok(ApprovalMutationResult {
            accepted: false,
            request: request.clone(),
        });
    }

    request.status = match decision {
        ApprovalDecisionKind::Allow => ApprovalStatus::Allowed,
        ApprovalDecisionKind::Deny => ApprovalStatus::Denied,
    };
    request.decided_by = Some(decided_by.to_string());
    request.decided_source = Some(decided_source.to_string());
    request.reason = reason;
    request.updated_at = now;

    Ok(ApprovalMutationResult {
        accepted: true,
        request: request.clone(),
    })
}

pub fn return_to_codex(
    requests: &mut [ApprovalRequest],
    request_id: &str,
    returned_by: &str,
    returned_source: &str,
    reason: &str,
    now: DateTime<Utc>,
) -> Result<ApprovalMutationResult, String> {
    let request = find_request_mut(requests, request_id)?;
    if request.status != ApprovalStatus::Pending {
        return Ok(ApprovalMutationResult {
            accepted: false,
            request: request.clone(),
        });
    }

    request.status = returned_status_for_tool(&request.tool);
    request.decided_by = Some(returned_by.to_string());
    request.decided_source = Some(returned_source.to_string());
    request.reason = Some(reason.to_string());
    request.updated_at = now;

    Ok(ApprovalMutationResult {
        accepted: true,
        request: request.clone(),
    })
}

pub fn resolve_in_tool(
    requests: &mut [ApprovalRequest],
    request_id: &str,
    resolved_by: &str,
    reason: Option<String>,
    now: DateTime<Utc>,
) -> Result<ApprovalMutationResult, String> {
    let request = find_request_mut(requests, request_id)?;
    if !matches!(
        request.status,
        ApprovalStatus::Pending | ApprovalStatus::ReturnedToCodex | ApprovalStatus::ReturnedToTool
    ) {
        return Ok(ApprovalMutationResult {
            accepted: false,
            request: request.clone(),
        });
    }

    request.status = ApprovalStatus::ResolvedInTool;
    request.decided_by = Some(resolved_by.to_string());
    request.decided_source = Some("tool_resolved".to_string());
    request.reason = reason;
    request.updated_at = now;

    Ok(ApprovalMutationResult {
        accepted: true,
        request: request.clone(),
    })
}

pub fn heartbeat_proxy(
    requests: &mut [ApprovalRequest],
    request_id: &str,
    now: DateTime<Utc>,
) -> Result<ApprovalMutationResult, String> {
    let request = find_request_mut(requests, request_id)?;
    if request.status != ApprovalStatus::Pending {
        return Ok(ApprovalMutationResult {
            accepted: false,
            request: request.clone(),
        });
    }

    // 心跳只证明 hook 代理仍存活，不改变授权决策本身。
    request.proxy_status = ApprovalProxyStatus::Active;
    request.last_heartbeat_at = Some(now);
    request.updated_at = now;

    Ok(ApprovalMutationResult {
        accepted: true,
        request: request.clone(),
    })
}

pub fn return_stale_proxies_to_codex(
    requests: &mut [ApprovalRequest],
    now: DateTime<Utc>,
    stale_after: chrono::Duration,
) -> Vec<ApprovalMutationResult> {
    requests
        .iter_mut()
        .filter_map(|request| {
            let last_seen = request.last_heartbeat_at?;
            if request.status != ApprovalStatus::Pending {
                return None;
            }
            if request.channel != ApprovalChannel::HookProxy {
                return None;
            }
            if request.proxy_status != ApprovalProxyStatus::Active {
                return None;
            }
            if now - last_seen <= stale_after {
                return None;
            }

            // hook 代理失联后，NiuMa UI 不能再承诺把决策回传给原工具。
            request.status = returned_status_for_tool(&request.tool);
            request.proxy_status = ApprovalProxyStatus::Lost;
            request.proxy_lost_at = Some(now);
            request.decided_by = Some("hook-helper".to_string());
            request.decided_source = Some("proxy_lost".to_string());
            request.reason = Some(format!(
                "hook 代理已失联，请回到 {} 中操作",
                approval_tool_display_name(&request.tool)
            ));
            request.updated_at = now;

            Some(ApprovalMutationResult {
                accepted: true,
                request: request.clone(),
            })
        })
        .collect()
}

fn returned_status_for_tool(tool: &ToolKind) -> ApprovalStatus {
    match tool {
        ToolKind::Codex => ApprovalStatus::ReturnedToCodex,
        ToolKind::ClaudeCode | ToolKind::Custom(_) => ApprovalStatus::ReturnedToTool,
    }
}

fn approval_tool_display_name(tool: &ToolKind) -> &str {
    match tool {
        ToolKind::Codex => "Codex",
        ToolKind::ClaudeCode => "Claude Code",
        ToolKind::Custom(value) => value.as_str(),
    }
}

fn find_request_mut<'a>(
    requests: &'a mut [ApprovalRequest],
    request_id: &str,
) -> Result<&'a mut ApprovalRequest, String> {
    requests
        .iter_mut()
        .find(|item| item.id == request_id)
        .ok_or_else(|| format!("授权请求不存在：{request_id}"))
}
