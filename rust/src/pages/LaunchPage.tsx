// 启动 Claude 页：选择工作目录 + 供应商 + 历史目录 + YOLO 开关 + 启动按钮。
// 从 App.tsx 抽出。props：config（全局配置快照）、onConfig（写回配置）。

import { useEffect, useState, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { Config } from "../types";
import { buildProviderEnv, NVIDIA_PROVIDER } from "../providerEnv";
import { MessageBanner } from "../components/MessageBanner";
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
  const [msg, setMsg] = useState<string | null>(null);
  const [recent, setRecent] = useState<string[]>([]);
  const [expanded, setExpanded] = useState(false);

  // 加载历史目录（最近在前）
  const loadRecent = useCallback(async () => {
    try {
      setRecent(await invoke<string[]>("get_recent_dirs"));
    } catch {
      setRecent([]);
    }
  }, []);

  useEffect(() => {
    loadRecent();
  }, [loadRecent]);

  useEffect(() => {
    if (config) setYolo(config.yolo_mode);
  }, [config]);

  // 供应商选择：config 变化或当前选择失效时回退到第一套
  // （NVIDIA_PROVIDER 是内置项、不在 profiles 里，需保留不回退）
  useEffect(() => {
    if (config) {
      const names = config.profiles.map((p) => p.name);
      if (profile !== NVIDIA_PROVIDER && !names.includes(profile)) {
        if (names.length > 0) setProfile(names[0]);
      }
    }
  }, [config, profile]);

  const pickDir = async () => {
    try {
      const dir = await invoke<string>("select_directory");
      if (dir && config) {
        onConfig({ ...config, work_dir: dir });
        await loadRecent();
      }
    } catch (e) {
      setMsg(`❌ 选择目录失败: ${e}`);
    }
  };

  // 选用某历史目录：写回配置工作目录，同步后端状态，并记入历史（置顶）
  const useDir = async (dir: string) => {
    if (!config) return;
    onConfig({ ...config, work_dir: dir });
    setMsg(`已选用: ${dir}`);
    try {
      // 关键：同步更新后端 Config 中的 work_dir，确保启动时使用正确的目录
      await invoke<string>("set_work_dir", { dir });
      await invoke<string[]>("add_recent_dir", { dir });
      await loadRecent();
    } catch {
      /* 记录历史失败不影响选用 */
    }
  };

  // 删除某历史目录：点击删除按钮时触发，阻止冒泡以免触发 useDir
  const removeDir = async (dir: string) => {
    try {
      // 后端返回删除后的全量列表，直接用于刷新，省一次查询
      setRecent(await invoke<string[]>("remove_recent_dir", { dir }));
    } catch (e) {
      setMsg(`❌ 删除失败: ${e}`);
    }
  };

  const launch = async () => {
    setMsg(null);
    try {
      // 取注入 claude 的进程环境变量（不改动 settings.json）
      const env = buildProviderEnv(config, profile);
      const r = await invoke<string>("launch_claude", { yolo, env });
      setMsg(r);
      // 启动成功后刷新历史（已写入新的置顶目录）
      await loadRecent();
    } catch (e) {
      setMsg(`❌ ${e}`);
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
      <MessageBanner msg={msg} />
      <div className="card">
        <div className="form-group">
          <label>工作目录</label>
          <div className="input-row">
            <input
              type="text"
              value={config?.work_dir || ""}
              placeholder="选择 Claude Code 工作目录"
              readOnly
            />
            <button className="btn btn-secondary" onClick={pickDir}>
              选择目录
            </button>
          </div>
        </div>

        <div className="form-group">
          <label>供应商 / Provider</label>
          <div className="input-row">
            <select
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
            </select>
          </div>
          {profile === NVIDIA_PROVIDER ? (
            <small className="form-hint">
              将把 Claude Code 指向本地{" "}
              <code>http://127.0.0.1:{config?.nvidia?.port ?? 8082}</code>，使用配置里的
              NVIDIA 模型（{config?.nvidia?.models?.join("、") || "未配置"}）。
              请先在「🟩 NVIDIA 代理」页手动启动 8082 代理。
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
                  onClick={() => useDir(d)}
                >
                  <span className="recent-folder">📁</span>
                  <span className="recent-path">{d}</span>
                  <ConfirmButton
                    className="recent-remove"
                    title="删除该历史目录"
                    onConfirm={() => removeDir(d)}
                  >
                    ✕
                  </ConfirmButton>
                </li>
              ))}
            </ul>
            {hasMore && (
              <button
                className="btn-toggle-more"
                onClick={() => setExpanded((v) => !v)}
              >
                {expanded ? `⌃ 收起` : `⌄ 展开全部 (${recent.length})`}
              </button>
            )}
          </div>
        )}

        <div className="form-group checkbox-group">
          <label className="checkbox-label">
            <input
              type="checkbox"
              checked={yolo}
              onChange={(e) => setYolo(e.target.checked)}
            />
            <span>🚀 YOLO 模式 (跳过所有权限确认)</span>
          </label>
        </div>
        <div className="button-row">
          <button className="btn btn-primary" onClick={launch}>
            🚀 启动 Claude Code
          </button>
        </div>
        <small className="form-hint">
          启动时不改写全局 ~/.claude/settings.json：连接参数通过进程环境变量注入，
          每个启动独立、可同时开多个工作目录 / 不同 provider。
        </small>
      </div>
    </div>
  );
}
