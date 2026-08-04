import { useState } from "react";

// Grok 本地测试面板：复刻 NvidiaTestPanel 结构，标题用 port 动态化（不写死 8082），
// curl/env 示例与 NVIDIA 版同形（Anthropic /v1/messages 端点，provider 无关）。
export function GrokTestPanel({
  port,
  testModel,
  onMessage,
}: {
  port: number;
  testModel: string;
  onMessage: (message: string) => void;
}) {
  const [shellKind, setShellKind] = useState<"bash" | "powershell" | "cmd">("bash");
  const testUrl = `http://127.0.0.1:${port}/v1/messages`;
  const bodyJson = JSON.stringify({
    model: testModel,
    max_tokens: 100,
    messages: [{ role: "user", content: "Introduce yourself in one sentence." }],
  });
  const curlCmd =
    shellKind === "bash"
      ? `curl ${testUrl} -H 'content-type: application/json' -d '${bodyJson}'`
      : shellKind === "powershell"
        ? `curl.exe ${testUrl} -H "content-type: application/json" -d '${bodyJson}'`
        : `curl ${testUrl} -H "content-type: application/json" -d "${bodyJson.replace(/"/g, '\\"')}"`;

  const copyText = (text: string) => {
    navigator.clipboard?.writeText(text).then(
      () => onMessage("✅ 已复制到剪贴板"),
      () => onMessage("❌ 复制失败，请手动选择文本")
    );
  };

  return (
    <div className="card">
      <div className="status-header">
        <span className="status-icon">🧪</span>
        <span className="status-text">本地测试 {port}</span>
      </div>
      <p className="form-hint">
        代理启动后，用下面的地址/命令即可直接测 {port} 端口（连接地址用 127.0.0.1，而非绑定的
        0.0.0.0）。请求里的 model 用 claude-* 名即可，代理会按映射表改写为 grok slug。
      </p>
      <div className="form-group">
        <label>测试地址（连得上的连接地址）</label>
        <div style={{ display: "flex", gap: 8, alignItems: "center" }}>
          <code
            style={{
              flex: 1,
              background: "rgba(0,0,0,0.03)",
              padding: "7px 10px",
              borderRadius: 8,
              fontFamily: "'SF Mono', monospace",
              fontSize: 12,
              overflowX: "auto",
            }}
          >
            {testUrl}
          </code>
          <button
            className="btn"
            style={{ padding: "4px 10px", fontSize: 12 }}
            onClick={() => copyText(testUrl)}
          >
            复制
          </button>
        </div>
      </div>
      <div className="form-group">
        <label>curl 非流式测试（复制到对应终端运行）</label>
        <div style={{ display: "flex", gap: 6, marginBottom: 6 }}>
          {(
            [
              ["bash", "Git Bash / WSL"],
              ["powershell", "PowerShell"],
              ["cmd", "CMD"],
            ] as const
          ).map(([kind, label]) => (
            <button
              key={kind}
              className="btn"
              style={{
                padding: "3px 10px",
                fontSize: 12,
                background: shellKind === kind ? "var(--primary)" : undefined,
                color: shellKind === kind ? "#fff" : undefined,
              }}
              onClick={() => setShellKind(kind)}
            >
              {label}
            </button>
          ))}
        </div>
        <textarea
          className="env-val"
          readOnly
          style={{ minHeight: 66, fontFamily: "'SF Mono', monospace", width: "100%" }}
          value={curlCmd}
        />
        <button
          className="btn"
          style={{ padding: "4px 10px", fontSize: 12 }}
          onClick={() => copyText(curlCmd)}
        >
          复制 curl
        </button>
        <small className="form-hint">
          CMD 不支持单引号；PowerShell 中 curl 是 Invoke-WebRequest 别名，需用
          curl.exe——已按所选终端生成正确写法。中文内容在 Windows 终端可能按 GBK
          发送导致编码错误，示例改用英文提示词。
        </small>
      </div>
      <div className="form-group">
        <label>Claude Code 客户端要设的环境变量</label>
        <div style={{ display: "flex", gap: 8, alignItems: "center" }}>
          <code
            style={{
              flex: 1,
              background: "rgba(0,0,0,0.03)",
              padding: "7px 10px",
              borderRadius: 8,
              fontFamily: "'SF Mono', monospace",
              fontSize: 12,
              overflowX: "auto",
            }}
          >
            $env:ANTHROPIC_BASE_URL="http://127.0.0.1:{port}"
          </code>
          <button
            className="btn"
            style={{ padding: "4px 10px", fontSize: 12 }}
            onClick={() =>
              copyText(`$env:ANTHROPIC_BASE_URL="http://127.0.0.1:${port}"`)
            }
          >
            复制
          </button>
        </div>
        <small className="form-hint">
          启动页选「Grok 代理 (本地 8083)」profile 即自动注入此环境变量，无需手设；
          此处仅作手动测试参考。cmd 用{" "}
          <code>set ANTHROPIC_BASE_URL=http://127.0.0.1:{port}</code>。
        </small>
      </div>
    </div>
  );
}
