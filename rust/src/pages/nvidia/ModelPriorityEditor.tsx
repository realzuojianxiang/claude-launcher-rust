import { useState } from "react";
import type { NvTestState } from "../../types";
import { ConfirmButton } from "../../components/ConfirmButton";

export function ModelPriorityEditor({
  models,
  running,
  chatTests,
  onMove,
  onMoveTop,
  onRemove,
  onAdd,
  onTest,
  addPlaceholder = "添加模型，如 nvidia/nemotron-3-ultra-550b-a55b",
}: {
  models: string[];
  running: boolean;
  chatTests: NvTestState["chatTests"];
  onMove: (index: number, direction: -1 | 1) => void;
  onMoveTop: (index: number) => void;
  onRemove: (index: number) => void;
  onAdd: (model: string) => boolean;
  onTest: (model: string) => void;
  addPlaceholder?: string;
}) {
  const [newModel, setNewModel] = useState("");
  const add = () => {
    const model = newModel.trim();
    if (model && onAdd(model)) setNewModel("");
  };

  return (
    <div className="form-group">
      <label>模型优先级（顺序即优先级，第 1 个为默认/最高，其余依次 Fallback）</label>
      <div className="model-prio">
        {models.length === 0 ? (
          <p className="form-hint" style={{ margin: "6px 0" }}>
            暂无模型，请在下方输入框添加。
          </p>
        ) : (
          <ul className="mp-list">
            {models.map((model, index) => {
              const test = chatTests[model] || { busy: false, result: null };
              return (
                <li
                  key={model}
                  className={index === 0 ? "mp-item mp-item-top" : "mp-item"}
                >
                  <div className="mp-row">
                    <span className="mp-rank">{index === 0 ? "★" : index + 1}</span>
                    <span className="mp-name" title={model}>
                      {model}
                    </span>
                    {index === 0 && <span className="mp-tag">最高优先级</span>}
                    <span className="mp-actions">
                      <button
                        className="mp-btn mp-btn-test"
                        title={
                          running
                            ? "向本机代理发送一条非流式测试消息（可多个模型同时测试）"
                            : "请先启动代理"
                        }
                        disabled={test.busy || !running}
                        onClick={() => onTest(model)}
                      >
                        {test.busy ? "⏳" : "▶ 测试"}
                      </button>
                      <button
                        className="mp-btn"
                        title="置顶"
                        disabled={index === 0}
                        onClick={() => onMoveTop(index)}
                      >
                        ⤒
                      </button>
                      <button
                        className="mp-btn"
                        title="上移"
                        disabled={index === 0}
                        onClick={() => onMove(index, -1)}
                      >
                        ↑
                      </button>
                      <button
                        className="mp-btn"
                        title="下移"
                        disabled={index === models.length - 1}
                        onClick={() => onMove(index, 1)}
                      >
                        ↓
                      </button>
                      <ConfirmButton
                        className="mp-btn mp-btn-del"
                        title="删除"
                        onConfirm={() => onRemove(index)}
                      >
                        ✕
                      </ConfirmButton>
                    </span>
                  </div>
                  {(test.busy || test.result) && (
                    <div className="mp-test-result">
                      {test.busy ? (
                        <code className="mp-testing">测试中…（可继续测试其他模型）</code>
                      ) : (
                        <code style={{ whiteSpace: "pre-wrap" }}>{test.result}</code>
                      )}
                    </div>
                  )}
                </li>
              );
            })}
          </ul>
        )}
        <div className="mp-add">
          <input
            type="text"
            value={newModel}
            placeholder={addPlaceholder}
            onChange={(event) => setNewModel(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Enter") {
                event.preventDefault();
                add();
              }
            }}
          />
          <button className="btn" onClick={add}>
            添加
          </button>
        </div>
        <small className="form-hint">
          ↑/↓/⤒ 调整顺序会 <b>即时生效</b>：自动保存并热更新运行中的代理，无需重启。 ▶
          测试 = 应用内直发一条非流式消息（走完整转换链），可<b>多个模型同时测试</b>、互不影响
          {running ? "" : "（需先启动代理）"}。
        </small>
      </div>
    </div>
  );
}
