// 配置页：编辑全局参数（YOLO / auto-compact / 执行目录）+ 供应商配置集管理。
// 从 App.tsx 抽出。props：config、onConfig、profiles/setProfiles、globals/setGlobals
// （编辑态已提升到 App，跨菜单保活，避免切走再回来未保存编辑丢失）。

import { useEffect, useState, type Dispatch, type SetStateAction } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  type Config,
  type Profile,
  type EditProfile,
  type CfgGlobals,
  fromEdit,
} from "../types";
import { MessageBanner } from "../components/MessageBanner";
import { ConfirmButton } from "../components/ConfirmButton";
import { EnvValueInput, isSecretKey } from "../components/EnvValueInput";

export function ConfigPage({
  config,
  onConfig,
  profiles,
  setProfiles,
  globals,
  setGlobals,
}: {
  config: Config | null;
  onConfig: (c: Config) => void;
  profiles: EditProfile[];
  setProfiles: Dispatch<SetStateAction<EditProfile[]>>;
  globals: CfgGlobals;
  setGlobals: Dispatch<SetStateAction<CfgGlobals>>;
}) {
  // 全局参数编辑态已提升到 App（globals/setGlobals 由 props 注入），跨菜单保活
  const { yolo, compactPct, compactWindow } = globals;
  const setYolo = (yolo: boolean) => setGlobals((g) => ({ ...g, yolo }));
  const setCompactPct = (compactPct: number) =>
    setGlobals((g) => ({ ...g, compactPct }));
  const setCompactWindow = (compactWindow: number) =>
    setGlobals((g) => ({ ...g, compactWindow }));
  // 供应商配置集（编辑态）已提升到 App（profiles/setProfiles 由 props 注入），跨菜单保活
  const [msg, setMsg] = useState<string | null>(null);
  // 配置文件实际保存路径：展示给用户，避免误以为「没保存」
  const [cfgPath, setCfgPath] = useState("");

  useEffect(() => {
    // 加载配置文件路径（与 load 无关，单独取一次即可）
    invoke<string>("config_path")
      .then(setCfgPath)
      .catch(() => setCfgPath(""));
  }, []);

  // 保存全局参数（YOLO / auto-compact 阈值）
  const saveGlobals = async () => {
    setMsg(null);
    try {
      const pct = Math.max(0, Math.min(100, Number(compactPct) || 0));
      const win = Math.max(0, Math.round(Number(compactWindow) || 0));
      await invoke<string>("set_config", {
        yoloMode: yolo,
        compactWindow: win,
        compactPct: pct,
      });
      if (config) {
        onConfig({
          ...config,
          yolo_mode: yolo,
          compact_window: win,
          compact_pct: pct,
        });
      }
      setMsg(
        `✅ 全局配置已保存${pct === 0 ? "（已关闭 auto-compact 注入）" : `（阈值 ${pct}% @ ${win} token）`}`
      );
    } catch (e) {
      setMsg(`❌ ${e}`);
    }
  };

  // 保存供应商配置集
  const saveProfiles = async () => {
    setMsg(null);
    try {
      const cleaned = fromEdit(profiles);
      await invoke<string>("set_profiles", { profiles: cleaned });
      if (config) onConfig({ ...config, profiles: cleaned });
      setMsg("✅ 供应商配置已保存");
    } catch (e) {
      setMsg(`❌ ${e}`);
    }
  };

  if (!config) return null;

  // —— 未保存修改检测：编辑态与已保存的 config 比对，有差异时保存按钮变警告色 ——
  // env 以排序后的键值对生成签名，避免对象键顺序差异造成误报
  const profileSig = (ps: Profile[]) =>
    JSON.stringify(
      ps.map((p) => ({
        name: p.name,
        env: Object.entries(p.env).sort((a, b) => a[0].localeCompare(b[0])),
      }))
    );
  const profilesDirty =
    profileSig(fromEdit(profiles)) !== profileSig(config.profiles);
  // 未保存检测：对「编辑中可能暂时为空/NaN」的数值字段做归一化，
  // 避免用户清空输入框重输时 Number("")===0 与 config 默认值(70/1e6) 不等而误亮「未保存」。
  const numOr = (v: unknown, fallback: number) => {
    const n = typeof v === "number" ? v : Number(v);
    return Number.isNaN(n) ? fallback : n;
  };
  const globalsDirty =
    yolo !== config.yolo_mode ||
    numOr(compactPct, config.compact_pct ?? 70) !== (config.compact_pct ?? 70) ||
    numOr(compactWindow, config.compact_window ?? 1_000_000) !==
      (config.compact_window ?? 1_000_000);

  // 供应商配置集的增删改处理函数
  const setName = (i: number, name: string) =>
    setProfiles((ps) => ps.map((p, idx) => (idx === i ? { ...p, name } : p)));
  const setKey = (i: number, j: number, k: string) =>
    setProfiles((ps) =>
      ps.map((p, idx) =>
        idx === i ? { ...p, env: p.env.map((r, jj) => (jj === j ? { ...r, k } : r)) } : p
      )
    );
  const setVal = (i: number, j: number, v: string) =>
    setProfiles((ps) =>
      ps.map((p, idx) =>
        idx === i ? { ...p, env: p.env.map((r, jj) => (jj === j ? { ...r, v } : r)) } : p
      )
    );
  const addRow = (i: number) =>
    setProfiles((ps) =>
      ps.map((p, idx) => (idx === i ? { ...p, env: [...p.env, { k: "", v: "" }] } : p))
    );
  const delRow = (i: number, j: number) =>
    setProfiles((ps) =>
      ps.map((p, idx) => (idx === i ? { ...p, env: p.env.filter((_, jj) => jj !== j) } : p))
    );
  const addProfile = () =>
    setProfiles((ps) => [
      ...ps,
      { name: `供应商${ps.length + 1}`, env: [{ k: "ANTHROPIC_BASE_URL", v: "" }] },
    ]);
  const delProfile = (i: number) =>
    setProfiles((ps) => ps.filter((_, idx) => idx !== i));

  return (
    <div className="page">
      <h2 className="page-title">配置</h2>
      <p className="page-desc">
        全局参数与供应商配置集。供应商配置集在启动页下拉选择，连接参数注入到 Claude 进程。
      </p>
      <MessageBanner msg={msg} />

      {/* 供应商配置集管理 */}
      <div className="card">
        <div className="form-group">
          <label>供应商配置集 (Profiles)</label>
          <small className="form-hint">
            每套含名称与一组环境变量。启动页选择后，这些变量会作为进程环境变量注入 claude。
          </small>
          {profiles.map((p, i) => (
            <div className="profile-block" key={i}>
              <div className="input-row">
                <input
                  type="text"
                  className="profile-name"
                  value={p.name}
                  placeholder="供应商名称"
                  onChange={(e) => setName(i, e.target.value)}
                />
                <ConfirmButton
                  className="btn btn-danger-sm"
                  title="删除该供应商"
                  confirmLabel="确认删除?"
                  onConfirm={() => delProfile(i)}
                >
                  删除
                </ConfirmButton>
              </div>
              {p.env.map((row, j) => (
                <div className="env-row" key={j}>
                  <input
                    type="text"
                    className="env-key"
                    value={row.k}
                    placeholder="变量名，如 ANTHROPIC_BASE_URL"
                    onChange={(e) => setKey(i, j, e.target.value)}
                  />
                  <EnvValueInput
                    value={row.v}
                    secret={isSecretKey(row.k)}
                    placeholder={
                      isSecretKey(row.k) ? "敏感值（默认脱敏显示）" : "变量值"
                    }
                    onChange={(v) => setVal(i, j, v)}
                  />
                  <ConfirmButton
                    className="recent-remove"
                    title="删除该行"
                    onConfirm={() => delRow(i, j)}
                  >
                    ✕
                  </ConfirmButton>
                </div>
              ))}
              <button className="btn-toggle-more" onClick={() => addRow(i)}>
                + 添加变量
              </button>
            </div>
          ))}
          <button className="btn btn-secondary" onClick={addProfile}>
            + 新增供应商
          </button>
        </div>
        <div className="button-row">
          <button
            className={`btn btn-secondary ${profilesDirty ? "btn-unsaved" : ""}`}
            onClick={saveProfiles}
          >
            保存供应商配置
            {profilesDirty && <span className="unsaved-badge">未保存</span>}
          </button>
          {profilesDirty && (
            <span className="unsaved-hint">⚠ 有未保存的修改，关闭应用将丢失</span>
          )}
        </div>
      </div>

      {/* 全局参数 */}
      <div className="card">
        <div className="form-group checkbox-group">
          <label className="checkbox-label">
            <input
              type="checkbox"
              checked={yolo}
              onChange={(e) => setYolo(e.target.checked)}
            />
            <span>YOLO 模式（跳过权限确认，作为启动页未指定时的默认）</span>
          </label>
        </div>

        <div className="form-group">
          <label>Auto-compact 触发阈值 (%)</label>
          <input
            type="number"
            min={0}
            max={100}
            value={compactPct}
            placeholder="70"
            onChange={(e) => setCompactPct(Number(e.target.value))}
          />
          <small className="form-hint">
            上下文用到该比例时自动压缩；70 = 70%。填 0 则不注入该配置。
          </small>
        </div>
        <div className="form-group">
          <label>Auto-compact 窗口 (token)</label>
          <input
            type="number"
            min={0}
            step={100000}
            value={compactWindow}
            placeholder="1000000"
            onChange={(e) => setCompactWindow(Number(e.target.value))}
          />
          <small className="form-hint">
            纳入压缩计算的上下文容量，1M 窗口填 1000000。
          </small>
        </div>
        <div className="button-row">
          <button
            className={`btn btn-secondary ${globalsDirty ? "btn-unsaved" : ""}`}
            onClick={saveGlobals}
          >
            保存全局配置
            {globalsDirty && <span className="unsaved-badge">未保存</span>}
          </button>
          {globalsDirty && (
            <span className="unsaved-hint">⚠ 有未保存的修改，关闭应用将丢失</span>
          )}
        </div>
      </div>

      {/* 配置保存位置：明确告知用户文件落在 exe 同级的 claude-launcher/config.json，
          避免误以为「没保存」（旧路径 ~/.claude-launcher 已不再使用） */}
      <div className="card cfg-path-card">
        <div className="form-group">
          <label>配置保存位置</label>
          <code className="cfg-path">{cfgPath || "（加载中…）"}</code>
          <small className="form-hint">
            所有配置（含供应商 env、历史目录）均保存于此文件；旧路径
            <code>~/.claude-launcher</code> 已废弃，请直接查看上面的地址。
          </small>
        </div>
      </div>
    </div>
  );
}
