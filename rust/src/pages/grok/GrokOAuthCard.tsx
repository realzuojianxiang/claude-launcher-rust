// Grok OAuth 授权卡：Device Code Flow 前端交互。
//
// 后端 grok_oauth_start 一次性发起 + 后台轮询 + emit 事件，前端只负责：
//   - 展示后端返回的 user_code / verification_uri（扫码或点击跳转）；
//   - 监听 grok-oauth-done / grok-oauth-error 事件，到点刷新授权状态；
//   - 已授权时显示账号 + 过期/可刷新情况 + 登出按钮。
//
// 不在前端做高频轮询——OAuth 进度由后端 task 推送，与 CLAUDE.md「无前端高频 fetch」一致。
import { useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { GrokOAuthState } from "../../types";

// 格式化距过期剩余时间（expires_at 是 epoch 秒）。已过期/0 返回空串。
function fmtRemaining(expiresAt: number): string {
  if (!expiresAt) return "";
  const now = Math.floor(Date.now() / 1000);
  const left = expiresAt - now;
  if (left <= 0) return "已过期";
  if (left < 60) return `剩余 ${left}s`;
  if (left < 3600) return `剩余 ${Math.floor(left / 60)}min`;
  return `剩余 ${Math.floor(left / 3600)}h${Math.floor((left % 3600) / 60)
    .toString()
    .padStart(2, "0")}min`;
}

export function GrokOAuthCard({
  state,
  setState,
  onMessage,
}: {
  state: GrokOAuthState;
  setState: (updater: (prev: GrokOAuthState) => GrokOAuthState) => void;
  onMessage: (message: string) => void;
}) {
  // 后端事件监听：授权完成 / 失败。后端轮询 task 到点会推这两个事件。
  useEffect(() => {
    let unDone: UnlistenFn | undefined;
    let unErr: UnlistenFn | undefined;
    listen<{ account: string; expires_at: number; expired?: boolean; refreshable?: boolean }>(
      "grok-oauth-done",
      (e) => {
        const { account, expires_at, expired, refreshable } = e.payload;
        setState((prev) => ({
          ...prev,
          authorized: true,
          account,
          expires_at,
          // 后端事件现已带 expired / refreshable（与 grok_oauth_status 同口径），
          // 直接采用回传值；缺字段时退回 prev，保证旧后端兼容。
          expired: expired ?? prev.expired,
          refreshable: refreshable ?? prev.refreshable,
          busy: false,
          userCode: null,
          verificationUri: null,
          verificationUriComplete: null,
          expires_in: null,
          error: null,
        }));
        onMessage(`✅ 已授权 Grok 账号 ${account}`);
      },
    ).then((u) => {
      unDone = u;
    });
    listen<{ message: string }>("grok-oauth-error", (e) => {
      setState((prev) => ({ ...prev, busy: false, error: e.payload.message }));
      onMessage(`❌ Grok 授权失败：${e.payload.message}`);
    }).then((u) => {
      unErr = u;
    });
    return () => {
      if (unDone) unDone();
      if (unErr) unErr();
    };
  }, [setState, onMessage]);

  const start = async () => {
    setState((prev) => ({ ...prev, busy: true, error: null }));
    try {
      // 后端短路：已授权会回 already_authorized:true + account/expires_at；
      // 否则回 user_code/verification_uri 等（device_code/token_endpoint 机密不回）。
      const r = await invoke<
        | {
            already_authorized: true;
            account: string;
            expires_at: number;
          }
        | {
            already_authorized: false;
            user_code: string;
            verification_uri: string;
            verification_uri_complete?: string | null;
            expires_in?: number | null;
            interval?: number | null;
          }
      >("grok_oauth_start");
      if ("already_authorized" in r && r.already_authorized) {
        setState((prev) => ({
          ...prev,
          authorized: true,
          account: r.account,
          expires_at: r.expires_at,
          busy: false,
        }));
        onMessage(`✅ 已授权 Grok 账号 ${r.account}`);
      } else {
        setState((prev) => ({
          ...prev,
          userCode: r.user_code,
          verificationUri: r.verification_uri,
          verificationUriComplete: r.verification_uri_complete ?? null,
          expires_in: r.expires_in ?? null,
          // busy 保持 true：后端在轮询，授权完成后由事件置 false。
        }));
      }
    } catch (e) {
      setState((prev) => ({ ...prev, busy: false, error: String(e) }));
      onMessage(`❌ ${e}`);
    }
  };

  const revoke = async () => {
    setState((prev) => ({ ...prev, busy: true, error: null }));
    try {
      const r = await invoke<string>("grok_oauth_revoke");
      setState((prev) => ({
        ...prev,
        authorized: false,
        account: "",
        expires_at: 0,
        expired: false,
        refreshable: false,
        busy: false,
        userCode: null,
        verificationUri: null,
        verificationUriComplete: null,
        error: null,
      }));
      onMessage(`✅ ${r}`);
    } catch (e) {
      setState((prev) => ({ ...prev, busy: false, error: String(e) }));
      onMessage(`❌ ${e}`);
    }
  };

  return (
    <div className="card">
      <div className="status-header">
        <span className="status-icon">🪪</span>
        <span className="status-text">Grok 账号授权（OAuth）</span>
      </div>
      <p className="form-hint">
        用 Grok Plus 同一个 x.ai 账号走 Device Code Flow 拿 token，调官方 CLI Chat-Proxy
        消费账号权益额度。token 经 DPAPI 加密存本机 <code>grok-oauth.json</code>，配置文件
        不含 token，<b>仅</b>在此页显示授权账号 email 作可见标识。
      </p>

      {state.authorized ? (
        <>
          <div className="form-group">
            <label>已授权账号</label>
            <code
              style={{
                background: "rgba(0,0,0,0.03)",
                padding: "7px 10px",
                borderRadius: 8,
                width: "100%",
                boxSizing: "border-box",
              }}
            >
              {state.account || "（未知账号）"}
            </code>
            {state.expires_at > 0 && (
              <small className="form-hint">
                凭证 {fmtRemaining(state.expires_at)}
                {state.expired ? "，需重新授权" : ""}
                {state.refreshable ? "；过期会自动用 refresh_token 续期" : "；无可续期凭证"}
              </small>
            )}
          </div>
          <div className="button-row">
            <button className="btn btn-secondary" onClick={revoke} disabled={state.busy}>
              {state.busy ? "登出中…" : "登出（清除本地凭证）"}
            </button>
          </div>
        </>
      ) : (
        <>
          {state.userCode ? (
            <div className="form-group">
              <label>授权指引</label>
              <ol
                style={{
                  paddingLeft: 18,
                  lineHeight: 1.7,
                  margin: "6px 0",
                }}
              >
                <li>
                  打开验证地址：
                  {state.verificationUriComplete ? (
                    <a
                      href={state.verificationUriComplete}
                      target="_blank"
                      rel="noreferrer"
                      style={{ marginLeft: 6 }}
                    >
                      {state.verificationUriComplete}
                    </a>
                  ) : (
                    <a
                      href={state.verificationUri ?? undefined}
                      target="_blank"
                      rel="noreferrer"
                      style={{ marginLeft: 6 }}
                    >
                      {state.verificationUri}
                    </a>
                  )}
                </li>
                <li>
                  输入授权码（大小写敏感）：
                  <code
                    style={{
                      background: "var(--primary)",
                      color: "#fff",
                      padding: "3px 10px",
                      borderRadius: 6,
                      marginLeft: 6,
                      letterSpacing: 2,
                      fontFamily: "'SF Mono', monospace",
                      fontSize: 14,
                    }}
                  >
                    {state.userCode}
                  </code>
                </li>
                <li>
                  用 Grok Plus 同一个 x.ai 账号登录并同意授权，本页会自动收到授权完成通知。
                </li>
              </ol>
              <small className="form-hint">
                授权码 {state.expires_in ? `约 ${state.expires_in}s 有效` : "有时效"}；
                后端正在轮询，完成后自动刷新本页。{state.verificationUriComplete &&
                  "「授权码直达链接」已预填授权码，点开直接登入即可。"}
              </small>
            </div>
          ) : (
            <div className="form-group">
              <small className="form-hint">尚未授权，点击下方按钮发起授权流程。</small>
            </div>
          )}
          <div className="button-row">
            <button className="btn btn-start" onClick={start} disabled={state.busy}>
              {state.busy ? "处理中…" : "授权 Grok 账号"}
            </button>
          </div>
        </>
      )}

      {state.error && (
        <div className="form-group">
          <code style={{ color: "#c0392b", whiteSpace: "pre-wrap" }}>
            ❌ {state.error}
          </code>
        </div>
      )}
    </div>
  );
}
