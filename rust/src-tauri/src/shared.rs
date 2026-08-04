// 跨 provider 共享的基础设施：安全闸（SSRF / 对外暴露强鉴权）与 SSE 分块解析。
//
// 本模块为 `nvidia` 与 `grok` 两个并行 provider 抽取出的无状态工具函数，
// 二者上游协议不同、配置结构不同，但对本地代理的「上游 base_url 安全校验」
// 「非回环绑定强制高熵 auth_token」「请求体上限」「跨越 chunk 的严格 UTF-8 按行 SSE 切分」
// 需求是完全一致的，因此抽到这里共用。
//
// 设计取舍：这里的函数全是自由函数、入参为原始值（host / base_url / token），
// 不绑定具体 Provider 的配置结构，便于后续若把 nvidia 的 `impl NvidiaConfig` 版本
// 也迁移过来时无需改动签名。当前 nvidia 仍保留其 `impl` 方法（行为不变），
// grok 模块直接引用本模块的自由函数；两边对同一约束暂时各有一份实现，
// 后续可统一收敛，但不在本阶段做（避免大面积重构 nvidia 引入回归）。

use std::sync::atomic::{AtomicUsize, Ordering};

/// /v1/messages 入站请求体上限：32 MiB。
///
/// Anthropic 请求常含 base64 图片/文档块、长 system、大 tools 定义，合计超 axum 默认 2 MiB 很现实。
/// 各 provider 的 server.rs 会以此为 `DefaultBodyLimit::max` 兜底；handler 自行用该上限读取请求体，
/// 超限则重塑为 Anthropic 风格的 413 error。
///
/// Phase 2 起 grok server.rs 引用；nvidia 暂保留其自有常量（行为不变），后续一并收敛。
#[allow(dead_code)] // Phase 2 grok server.rs 接入
pub const MAX_REQUEST_BODY_BYTES: usize = 32 * 1024 * 1024;

// === 安全闸 ===

/// 判断监听 host 是否仅对回环接口开放。
///
/// 空 / `127.0.0.1` / `localhost` / `::1` 视为回环；其余（含 `0.0.0.0` / `::` / 局域网 IP）
/// 视为对外暴露，需要强鉴权保护。
pub fn is_loopback_host(host: &str) -> bool {
    let h = host.trim().to_ascii_lowercase();
    h.is_empty() || h == "127.0.0.1" || h == "localhost" || h == "::1"
}

/// 对外监听安全校验：host 非回环时必须配置至少 24 位的本地鉴权 token，
/// 否则拒绝启动——默认无鉴权地暴露到局域网约等于公开用户的上游凭证。
///
/// 返回 `Err` 时携带可直接在 UI 展示的中文错误。
pub fn require_auth_if_exposed(host: &str, auth_token: &str) -> Result<(), String> {
    if !is_loopback_host(host) {
        let token = auth_token.trim();
        if token.len() < 24 {
            let state = if token.is_empty() { "为空" } else { "过短" };
            return Err(format!(
                "❌ 绑定地址 {host} 面向外部网络，必须配置至少 24 位的本地鉴权 token（当前{state}），否则同网设备可盗用你的上游凭证。"
            ));
        }
    }
    Ok(())
}

/// 上游 base_url 安全校验：保存与转发前调用，杜绝 SSRF / bearer token 被 30x 引流到任意主机。
///
///   - 必须以 `https://` 或 `http://` 开头且非空；
///   - host 必须存在（拒绝 `http:///path` 这类能被 reqwest 当成 localhost 的畸形 URL）。
///
/// 这是「第二道闸」；「第一道闸」是 proxy 构造 reqwest Client 时的 `redirect(Policy::none())`。
pub fn validate_base_url(base_url: &str, provider_label: &str) -> Result<(), String> {
    let url = base_url.trim();
    if url.is_empty() {
        return Err(format!("❌ {provider_label} Base URL 不能为空"));
    }
    let lower = url.to_ascii_lowercase();
    if !lower.starts_with("https://") && !lower.starts_with("http://") {
        return Err(format!(
            "❌ {provider_label} Base URL 必须以 http:// 或 https:// 开头（当前: {url}），否则可能泄露你的上游凭证。"
        ));
    }
    let host_part = {
        let s = lower
            .strip_prefix("https://")
            .or_else(|| lower.strip_prefix("http://"))
            .unwrap_or(url);
        s.split(['/', ':']).next().unwrap_or("")
    };
    if host_part.is_empty() {
        return Err(format!(
            "❌ {provider_label} Base URL 缺少主机名（当前: {url}）"
        ));
    }
    Ok(())
}

// === SSE 分块解析 ===

/// 从字节缓冲里切分出所有「已完整到达的 SSE 行」并就地 drain 不完整尾部。
///
/// 只在 `\n` 边界切分（兼容 CRLF），不完整尾部字节保留在 buf 中等下一 chunk，
/// 因此跨 chunk 的多字节 UTF-8 字符绝不会被劈开。每行做严格 UTF-8 解码：
///   - `Ok(s)`：trim 后的整行；
///   - `Err(e)`：解码失败，调用方应判为流异常截断并发 error，绝不产乱码/坏 JSON。
///
/// 这是热路径与单元测试共用的纯函数。
///
/// Phase 2 起 grok proxy 引用；nvidia 暂保留其自有同名函数（行为不变），后续一并收敛。
#[allow(dead_code)] // Phase 2 grok proxy 接入
pub fn split_complete_sse_lines(buf: &mut Vec<u8>) -> Vec<Result<String, std::str::Utf8Error>> {
    let mut out = Vec::new();
    while let Some(pos) = buf.iter().position(|b| *b == b'\n') {
        let line_end = if pos > 0 && buf[pos - 1] == b'\r' {
            pos - 1
        } else {
            pos
        };
        let raw: Vec<u8> = buf[..line_end].to_vec();
        buf.drain(..=pos);
        if raw.is_empty() || raw.iter().all(|b| *b == b' ') {
            continue;
        }
        match std::str::from_utf8(&raw) {
            Ok(s) => out.push(Ok(s.trim().to_string())),
            Err(e) => out.push(Err(e)),
        }
    }
    out
}

// 用于 grok OAuth 等场景生成稳定会话 id 的自增计数器（不依赖随机源，避免 Date::now 限制）。
// 这里仅供 future session id 用，当前未启用。
static _SESSION_SEQ: AtomicUsize = AtomicUsize::new(0);
#[allow(dead_code)]
fn _next_session_seq() -> usize {
    _SESSION_SEQ.fetch_add(1, Ordering::Relaxed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loopback_detection_covers_common_hosts() {
        assert!(is_loopback_host("127.0.0.1"));
        assert!(is_loopback_host("localhost"));
        assert!(is_loopback_host("::1"));
        assert!(is_loopback_host(""));
        assert!(!is_loopback_host("0.0.0.0"));
        assert!(!is_loopback_host("192.168.1.10"));
    }

    #[test]
    fn external_bind_requires_strong_token() {
        assert!(require_auth_if_exposed("0.0.0.0", "").is_err());
        assert!(require_auth_if_exposed("0.0.0.0", "short").is_err());
        assert!(require_auth_if_exposed("0.0.0.0", "0123456789abcdef01234567").is_ok());
        assert!(require_auth_if_exposed("127.0.0.1", "").is_ok());
    }

    #[test]
    fn validate_base_url_rejects_invalid_schemes_and_hosts() {
        assert!(validate_base_url("https://cli-chat-proxy.grok.com/v1", "Grok").is_ok());
        assert!(validate_base_url("http://localhost:1234/v1", "Grok").is_ok());
        assert!(validate_base_url("", "Grok").is_err());
        assert!(validate_base_url("ftp://example.com", "Grok").is_err());
        assert!(validate_base_url("https:///path", "Grok").is_err());
    }

    #[test]
    fn sse_lines_split_uses_newline_boundary_keep_trailing() {
        let mut buf: Vec<u8> = b"event: a\ndata: {\"x\":1}\n".to_vec();
        let lines = split_complete_sse_lines(&mut buf);
        assert_eq!(lines.len(), 2);
        assert!(buf.is_empty());
        assert_eq!(lines[0].as_ref().unwrap(), "event: a");
        assert_eq!(lines[1].as_ref().unwrap(), "data: {\"x\":1}");
    }

    #[test]
    fn sse_lines_keep_incomplete_trailing_bytes() {
        let mut buf: Vec<u8> = b"event: a".to_vec();
        let lines = split_complete_sse_lines(&mut buf);
        assert!(lines.is_empty());
        assert_eq!(buf, b"event: a");
    }

    #[test]
    fn sse_lines_handle_crlf() {
        let mut buf: Vec<u8> = b"event: a\r\ndata: 1\r\n".to_vec();
        let lines = split_complete_sse_lines(&mut buf);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].as_ref().unwrap(), "event: a");
    }

    #[test]
    fn sse_lines_utf8_split_across_chunks_preserved() {
        // "你好" 的 UTF-8 编码在第二字节边界切开，分两块到达应能正确拼出。
        let full = "data: 你好\n";
        let bytes_full = full.as_bytes();
        // 模拟：第一块到 "你好" 第一个完整字节就停（确保不切坏字节，前缀都是 ASCII）
        let mut buf = bytes_full[..5].to_vec(); // "data:"
        let mut lines = split_complete_sse_lines(&mut buf);
        buf.extend_from_slice(&bytes_full[5..]);
        lines.extend(split_complete_sse_lines(&mut buf));
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].as_ref().unwrap(), "data: 你好");
        assert!(buf.is_empty());
    }
}
