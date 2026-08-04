// 启动 Claude 页：选择工作目录 + 供应商 + 历史目录 + YOLO 开关 + 启动按钮。
// 从 App.tsx 抽出。props：config（全局配置快照）、onConfig（写回配置）。

import { useEffect, useRef, useState, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Folder, Rocket, Trash2, ChevronDown, ChevronUp } from "lucide-react";
import type { Config } from "../types";
import { buildProviderEnv, NVIDIA_PROVIDER, GROK_PROVIDER } from "../providerEnv";
import { Button } from "../components/ui/Button";
import {
  StatusBanner,
  type StatusMessage,
} from "../components/ui/StatusBanner";
import { ConfirmButton } from "../components/ConfirmButton";

// 最近历史目录的默认展示条数；超过则折叠展开
const RECENT_VISIBLE = 3;

export function LaunchPage({
  config,
  onConfig,
}: {
  config: Config | null;
  onConfig: (c: Config) => void;
}) {
  const [yolo, setYolo] = useState(false);
  // 默认选中内置的 NVIDIA 代理（本地 8082）：用户大多数时间用它，并手动在菜单启动 8082
  const [profile, setProfile] = useState(NVIDIA_PROVIDER);
  const [status, setStatus] = useState<StatusMessage | null>(null);
  const [launchBusy, setLaunchBusy] = useState(false);
  const [recent, setRecent] = useState<string[]>([]);
  const [recentFailed, setRecentFailed] = useState(false);
  const [expanded, setExpanded] = useState(false);

  // 单调自增的请求 id：旧响应落地时若已被更新的请求取代则忽略，避免竞态覆盖
  const recentReqId = useRef(0);

  // 加载历史目录（最近在前）。使用请求 id 守卫，过期的响应不会覆盖新列表。
  const loadRecent = useCallback(async () => {
    const reqId = ++recentReqId.current;
    try {
      const dirs = await invoke<string[]>("get_recent_dirs");
      if (reqId === recentReqId.current) {
        setRecent(dirs);
        setRecentFailed(false);
      }
    } catch {
      if (reqId === recentReqId.current) {
        setRecent([]);
        setRecentFailed(true);
      }
    }
  }, []);

  useEffect(() => {
    loadRecent();
  }, [loadRecent]);

  useEffect(() => {
    if (config) setYolo(config.yolo_mode);
  }, [config]);

  // 供应商选择：config 变化或当前选择失效时回退到第一套
  // （NVIDIA_PROVIDER / GROK_PROVIDER 是内置项、不在 profiles 里，需保留不回退）
  useEffect(() => {
    if (config) {
      const names = config.profiles.map((p) => p.name);
      if (
        profile !== NVIDIA_PROVIDER &&
        profile !== GROK_PROVIDER &&
        !names.includes(profile)
      ) {
        if (names.length > 0) setProfile(names[0]);
      }
    }
  }, [config, profile]);

  // 选择工作目录：弹目录对话框。后端已在本命令内持久化目录与历史，
  // 仅返回空串（取消哨兵）或拒绝（权限错误）需要特殊处理。
  const pickDir = async () => {
    setStatus(null);
    try {
      const dir = await invoke<string>("select_directory");
      if (dir === "") return; // 取消哨兵：静默 no-op
      setStatus({ kind: "success", title: `已选择工作目录：${dir}` });
      await loadRecent();
    } catch (e) {
      setStatus({
        kind: "error",
        title: "无法选择工作目录",
        detail: e instanceof Error ? e.message : String(e),
      });
    }
  };

  // 选用某历史目录：写回配置工作目录，同步后端状态，并记入历史（置顶）
  const useDir = async (dir: string) => {
    if (!config) return;
    try {
      await invoke<string>("set_work_dir", { dir });
      await invoke<string[]>("add_recent_dir", { dir });
      onConfig({ ...config, work_dir: dir });
      setStatus({ kind: "success", title: `已选用：${dir}` });
      await loadRecent();
    } catch (e) {
      setStatus({
        kind: "error",
        title: "无法选用该目录",
        detail: e instanceof Error ? e.message : String(e),
      });
    }
  };

  // 删除某历史目录：点击删除按钮时触发，独立按钮不会冒泡到选用
  const removeDir = async (dir: string) => {
    try {
      setRecent(await invoke<string[]>("remove_recent_dir", { dir }));
    } catch (e) {
      setStatus({
        kind: "error",
        title: "删除失败",
        detail: e instanceof Error ? e.message : String(e),
      });
    }
  };

  const launch = async () => {
    if (launchBusy) return;
    setLaunchBusy(true);
    setStatus(null);
    try {
      // 取注入 claude 的进程环境变量（不改动 settings.json）
      const env = buildProviderEnv(config, profile);
      const r = await invoke<string>("launch_claude", { yolo, env });
      setStatus({
        kind: "success",
        title: "Claude Code 已启动",
        detail: typeof r === "string" && r ? r : undefined,
      });
      await loadRecent();
    } catch (e) {
      setStatus({
        kind: "error",
        title: "无法启动 Claude Code",
        detail: e instanceof Error ? e.message : String(e),
      });
    } finally {
      setLaunchBusy(false);
    }
  };

  // 折叠展示：默认最近 3 个，展开后全部
  const visible = expanded ? recent : recent.slice(0, RECENT_VISIBLE);
  const hasMore = recent.length > RECENT_VISIBLE;

  return (
    <div className="page">
      <h2 className="page-title">启动 Claude Code</h2>
      <p className="page-desc">
        选择工作目录与供应商，启动 Claude Code（连接参数以环境变量注入，不改动 settings.json）。
      </p>
      <StatusBanner message={status} />

      <div className="card">
        <div className="form-group">
          <label htmlFor="launch-work-dir">工作目录</label>
          <div className="input-row">
            <input
              id="launch-work-dir"
              type="text"
              value={config?.work_dir || ""}
              placeholder="选择 Claude Code 工作目录"
              readOnly
            />
            <Button variant="secondary" onClick={pickDir}>
              选择工作目录
            </Button>
          </div>
        </div>

        <div className="form-group">
          <label htmlFor="launch-provider">供应商 / Provider</label>
          <div className="input-row">
            <select
              id="launch-provider"
              className="select-input"
              value={profile}
              onChange={(e) => setProfile(e.target.value)}
            >
              {(config?.profiles ?? []).map((p) => (
                <option key={p.name} value={p.name}>
                  {p.name}
                </option>
              ))}
              <option value={NVIDIA_PROVIDER}>{NVIDIA_PROVIDER}</option>
              <option value={GROK_PROVIDER}>{GROK_PROVIDER}</option>
            </select>
          </div>
          {profile === NVIDIA_PROVIDER ? (
            <small className="form-hint">
              将把 Claude Code 指向本地{" "}
              <code>http://127.0.0.1:{config?.nvidia?.port ?? 8082}</code>，使用配置里的
              NVIDIA 模型（{config?.nvidia?.models?.join("、") || "未配置"}）。
              请先在「NVIDIA 代理」页手动启动 8082 代理。
            </small>
          ) : profile === GROK_PROVIDER ? (
            <small className="form-hint">
              将把 Claude Code 指向本地{" "}
              <code>http://127.0.0.1:{config?.grok?.port ?? 8083}</code>，代理按模型映射表把
              claude-* 改写为 grok slug（默认上游{" "}
              {config?.grok?.auth_mode === "api-key"
                ? "api.x.ai（API Key 退路）"
                : "cli-chat-proxy.grok.com（OAuth）"}
              ）。请先在「Grok 代理」页授权并启动 8083 代理。
            </small>
          ) : (
            <small className="form-hint">
              不同 provider 的连接参数（ANTHROPIC_BASE_URL / KEY / AUTH_TOKEN / MODEL …）各自独立，
              启动时仅注入到本次进程，互不干扰、可并发多开。
            </small>
          )}
        </div>

        {recent.length > 0 && (
          <div className="form-group">
            <label>最近打开</label>
            <ul className="recent-list">
              {visible.map((d) => (
                <li
                  key={d}
                  className={`recent-item ${
                    config?.work_dir === d ? "active" : ""
                  }`}
                  title={d}
                >
                  <button
                    type="button"
                    className="recent-use"
                    onClick={() => useDir(d)}
                  >
                    <Folder aria-hidden="true" />
                    <span className="recent-path">{d}</span>
                  </button>
                  <ConfirmButton
                    className="recent-remove"
                    title="删除该历史目录"
                    ariaLabel="删除该历史目录"
                    onConfirm={() => removeDir(d)}
                  >
                    <Trash2 aria-hidden="true" />
                  </ConfirmButton>
                </li>
              ))}
            </ul>
            {hasMore && (
              <button
                type="button"
                className="btn-toggle-more"
                onClick={() => setExpanded((v) => !v)}
              >
                {expanded ? (
                  <>
                    <ChevronUp aria-hidden="true" /> 收起
                  </>
                ) : (
                  <>
                    <ChevronDown aria-hidden="true" /> 展开全部 ({recent.length})
                  </>
                )}
              </button>
            )}
          </div>
        )}

        {recentFailed && (
          <div className="form-group">
            <Button variant="ghost" onClick={loadRecent}>
              重新加载历史
            </Button>
          </div>
        )}

        <div className="form-group checkbox-group">
          <label className="checkbox-label">
            <input
              type="checkbox"
              checked={yolo}
              onChange={(e) => setYolo(e.target.checked)}
            />
            <span>
              <Rocket aria-hidden="true" /> YOLO 模式 (跳过所有权限确认)
            </span>
          </label>
        </div>

        <div className="button-row">
          <Button
            variant="primary"
            loading={launchBusy}
            loadingLabel="正在启动 Claude Code"
            onClick={launch}
          >
            启动 Claude Code
          </Button>
        </div>
        <small className="form-hint">
          启动时不改写全局 ~/.claude/settings.json：连接参数通过进程环境变量注入，
          每个启动独立、可同时开多个工作目录 / 不同 provider。
        </small>
      </div>
    </div>
  );
}
