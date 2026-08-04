// 模型名映射表编辑器：claude-* -> grok-*。
//
// 代理在转换请求时按入站 model 查此表改写为 grok slug（未命中回退 models[0] 或按
// claude-haiku/sonnet 前缀通配）。表本身随 GrokConfig.model_map 落盘 + 即时热更新
// 运行中代理（通过 set_grok_config，无需重启——map_model 每次请求现读现用 cfg）。
import { useState } from "react";
import { ConfirmButton } from "../../components/ConfirmButton";
import type { ModelMapEntry } from "../../types";

export function ModelMapEditor({
  modelMap,
  onChange,
}: {
  modelMap: ModelMapEntry[];
  onChange: (next: ModelMapEntry[]) => void;
}) {
  const [anthropic, setAnthropic] = useState("");
  const [grok, setGrok] = useState("");

  const update = (index: number, patch: Partial<ModelMapEntry>) => {
    onChange(modelMap.map((e, i) => (i === index ? { ...e, ...patch } : e)));
  };
  const remove = (index: number) => {
    onChange(modelMap.filter((_, i) => i !== index));
  };
  const add = () => {
    const a = anthropic.trim();
    const g = grok.trim();
    if (!a || !g) return;
    // 同一 Anthropic 模型重复映射：覆盖即可（取最新），不强制去重
    if (modelMap.some((e) => e.anthropic_model === a)) {
      onChange(
        modelMap.map((e) =>
          e.anthropic_model === a ? { ...e, grok_model: g } : e
        )
      );
    } else {
      onChange([...modelMap, { anthropic_model: a, grok_model: g }]);
    }
    setAnthropic("");
    setGrok("");
  };

  return (
    <div className="form-group">
      <label>
        模型名映射（Anthropic 侧 → Grok 上游 slug，未命中回退 models[0] / 通配前缀）
      </label>
      {modelMap.length === 0 ? (
        <p className="form-hint" style={{ margin: "6px 0" }}>
          暂无显式映射；入站 claude-* 会按通配规则处理（haiku 系→含 mini/max 的模型、
          其余→models[0]）。
        </p>
      ) : (
        <ul className="mp-list" style={{ marginBottom: 8 }}>
          {modelMap.map((entry, index) => (
            <li key={index} className="mp-item">
              <div className="mp-row">
                <input
                  type="text"
                  value={entry.anthropic_model}
                  placeholder="claude-sonnet-4"
                  onChange={(e) => update(index, { anthropic_model: e.target.value })}
                  style={{ flex: 1 }}
                />
                <span style={{ padding: "0 6px", color: "#888" }} aria-hidden="true">
                  →
                </span>
                <input
                  type="text"
                  value={entry.grok_model}
                  placeholder="grok-4.3"
                  onChange={(e) => update(index, { grok_model: e.target.value })}
                  style={{ flex: 1 }}
                />
                <ConfirmButton
                  className="mp-btn mp-btn-del"
                  title="删除该映射"
                  onConfirm={() => remove(index)}
                >
                  ✕
                </ConfirmButton>
              </div>
            </li>
          ))}
        </ul>
      )}
      <div className="mp-add" style={{ display: "flex", gap: 8, alignItems: "center" }}>
        <input
          type="text"
          value={anthropic}
          placeholder="Anthropic 模型，如 claude-sonnet-4"
          onChange={(e) => setAnthropic(e.target.value)}
          style={{ flex: 1 }}
          onKeyDown={(e) => {
            if (e.key === "Enter") {
              e.preventDefault();
              add();
            }
          }}
        />
        <span style={{ color: "#888" }} aria-hidden="true">
          →
        </span>
        <input
          type="text"
          value={grok}
          placeholder="Grok slug，如 grok-4.3"
          onChange={(e) => setGrok(e.target.value)}
          style={{ flex: 1 }}
          onKeyDown={(e) => {
            if (e.key === "Enter") {
              e.preventDefault();
              add();
            }
          }}
        />
        <button className="btn" onClick={add} disabled={!anthropic.trim() || !grok.trim()}>
          添加
        </button>
      </div>
      <small className="form-hint">
        后端的 <code>map_model</code> 优先按此表（大小写不敏感）匹配；未命中时通配：
        claude-haiku 系列回退含 <code>mini</code> 的 grok slug，其它回退 <code>models[0]</code>。
        保存配置后即时生效，无需重启代理。
      </small>
    </div>
  );
}
