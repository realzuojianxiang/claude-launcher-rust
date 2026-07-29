// 日志页：实时动态显示应用日志（含 NVIDIA 代理活动）。
// - 挂载时拉取内存环形缓冲的最近日志回填；
// - 监听后端 nvidia-log 事件，实时追加并自动滚到底；
// - 列出 logs/ 历史文件，点击可查看某个分卷的完整内容。
// 从 App.tsx 抽出，含局部 LogLevel/LOG_LEVEL_RANK/getLineLogLevel。自带 React + Tauri 导入。

import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

type LogLevel = "INFO" | "WARN" | "ERROR";

const LOG_LEVEL_RANK: Record<LogLevel, number> = {
  INFO: 1,
  WARN: 2,
  ERROR: 3,
};

const getLineLogLevel = (line: string): LogLevel | null => {
  const match = line.match(/\b(INFO|WARN|ERROR)\b/);
  return match ? (match[1] as LogLevel) : null;
};

export function LogPage() {
  const [lines, setLines] = useState<string[]>([]);
  const [files, setFiles] = useState<{ name: string; size: number }[]>([]);
  const [autoScroll, setAutoScroll] = useState(true);
  const [logLevel, setLogLevelState] = useState<LogLevel>("INFO");
  const [clearBusy, setClearBusy] = useState(false);
  const [histName, setHistName] = useState<string | null>(null);
  const [histContent, setHistContent] = useState<string>("");
  const [histBusy, setHistBusy] = useState(false);
  const scrollRef = useRef<HTMLDivElement>(null);

  // 自动滚到底
  useEffect(() => {
    if (autoScroll && scrollRef.current) {
      scrollRef.current.scrollTop = scrollRef.current.scrollHeight;
    }
  }, [lines, logLevel, autoScroll]);

  const refreshFiles = useCallback(async () => {
    try {
      const fs = await invoke<{ name: string; size: number }[]>("list_log_files");
      setFiles(fs);
    } catch {
      setFiles([]);
    }
  }, []);

  useEffect(() => {
    let unlisten: UnlistenFn | undefined;
    // 初始回填
    invoke<string[]>("get_logs")
      .then((ls) => setLines(ls))
      .catch(() => setLines([]));
    invoke<LogLevel>("get_log_level")
      .then((level) => setLogLevelState(level))
      .catch(() => setLogLevelState("INFO"));
    refreshFiles();
    // 实时订阅
    listen<string>("nvidia-log", (e) => {
      setLines((prev) => {
        const next = prev.length >= 2000 ? prev.slice(prev.length - 1999) : prev.slice();
        next.push(e.payload);
        return next;
      });
    })
      .then((u) => {
        unlisten = u;
      })
      .catch(() => {});
    return () => {
      if (unlisten) unlisten();
    };
  }, [refreshFiles]);

  const changeLogLevel = async (level: LogLevel) => {
    try {
      const applied = await invoke<LogLevel>("set_log_level", { level });
      setLogLevelState(applied);
    } catch (e) {
      console.error("设置日志等级失败", e);
    }
  };

  const clearLiveLogs = async () => {
    setClearBusy(true);
    try {
      await invoke("clear_logs");
      setLines([]);
    } catch (e) {
      console.error("清除实时日志失败", e);
    } finally {
      setClearBusy(false);
    }
  };

  const visibleLines = lines.filter((line) => {
    const level = getLineLogLevel(line);
    return level === null || LOG_LEVEL_RANK[level] >= LOG_LEVEL_RANK[logLevel];
  });

  const openHist = async (name: string) => {
    setHistBusy(true);
    setHistName(name);
    setHistContent("");
    try {
      const c = await invoke<string>("read_log_file", { name });
      setHistContent(c);
    } catch (e) {
      setHistContent(`❌ ${e}`);
    } finally {
      setHistBusy(false);
    }
  };

  const fmtSize = (n: number) => {
    if (n >= 1024 * 1024) return `${(n / 1024 / 1024).toFixed(2)} MB`;
    if (n >= 1024) return `${(n / 1024).toFixed(1)} KB`;
    return `${n} B`;
  };

  return (
    <div className="page">
      <h2 className="page-title">日志</h2>
      <p className="page-desc">
        实时显示应用与 NVIDIA 代理运行日志；日志同时持久化到{" "}
        <code>logs/</code> 目录（单文件超 1MB 自动分卷，文件名带日期时间）。
      </p>

      <div className="card">
        <div className="status-header">
          <span className="status-icon">📜</span>
          <span className="status-text">实时日志（{visibleLines.length}/{lines.length} 行）</span>
          <div className="logbar-controls">
            <label className="checkbox-label">
              <span>等级</span>
              <select
                className="select-input level-select"
                value={logLevel}
                onChange={(e) => changeLogLevel(e.target.value as LogLevel)}
              >
                <option value="INFO">INFO</option>
                <option value="WARN">WARN</option>
                <option value="ERROR">ERROR</option>
              </select>
            </label>
            <label className="checkbox-label">
              <input
                type="checkbox"
                checked={autoScroll}
                onChange={(e) => setAutoScroll(e.target.checked)}
              />
              <span>自动滚动</span>
            </label>
            <button
              className="btn btn-secondary"
              onClick={clearLiveLogs}
              disabled={clearBusy || lines.length === 0}
              aria-label="清除实时日志"
              title="只清除实时显示，不删除历史日志文件"
            >
              🧹 {clearBusy ? "清除中…" : "清除实时日志"}
            </button>
          </div>
        </div>
        <div
          ref={scrollRef}
          className="log-viewer"
          style={{
            height: 360,
            overflowY: "auto",
            background: "#1c1c1e",
            color: "#d4d4d4",
            fontFamily: "'SF Mono', 'SFMono-Regular', Consolas, monospace",
            fontSize: 12,
            padding: 12,
            borderRadius: 10,
            whiteSpace: "pre-wrap",
            wordBreak: "break-all",
          }}
        >
          {visibleLines.length === 0 ? (
            <span style={{ color: "#888" }}>（暂无日志，启动 NVIDIA 代理或操作后将出现）</span>
          ) : (
            visibleLines.map((l, i) => (
              <div key={i} className="log-line">
                {l}
              </div>
            ))
          )}
        </div>
      </div>

      <div className="card">
        <div className="status-header">
          <span className="status-icon">🗂️</span>
          <span className="status-text">历史日志文件（{files.length}）</span>
          <button
            className="btn btn-refresh"
            onClick={refreshFiles}
            style={{ marginLeft: "auto" }}
          >
            🔄 刷新
          </button>
        </div>
        {files.length === 0 ? (
          <p className="form-hint">（logs/ 目录暂无文件）</p>
        ) : (
          <ul className="recent-list">
            {files.map((f) => (
              <li
                key={f.name}
                className={`recent-item ${histName === f.name ? "active" : ""}`}
                onClick={() => openHist(f.name)}
                title={f.name}
              >
                <span className="recent-folder">📄</span>
                <span className="recent-path">{f.name}</span>
                <span style={{ marginLeft: "auto", color: "#888", fontSize: 12 }}>
                  {fmtSize(f.size)}
                </span>
              </li>
            ))}
          </ul>
        )}
      </div>

      {/* 历史日志文件内容弹层 */}
      {histName && (
        <div
          className="modal-mask"
          onClick={() => setHistName(null)}
          style={{
            position: "fixed",
            inset: 0,
            background: "rgba(0,0,0,0.3)",
            backdropFilter: "blur(8px)",
            WebkitBackdropFilter: "blur(8px)",
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
            zIndex: 50,
          }}
        >
          <div
            className="modal-box"
            onClick={(e) => e.stopPropagation()}
            style={{
              background: "#fff",
              borderRadius: 14,
              maxWidth: 820,
              width: "90%",
              maxHeight: "80%",
              display: "flex",
              flexDirection: "column",
              overflow: "hidden",
              boxShadow: "0 20px 60px -10px rgba(0,0,0,0.3)",
            }}
          >
            <div
              className="modal-head"
              style={{
                display: "flex",
                alignItems: "center",
                padding: "12px 16px",
                borderBottom: "0.5px solid rgba(0,0,0,0.08)",
              }}
            >
              <strong style={{ fontSize: 13 }}>{histName}</strong>
              <button
                className="btn"
                style={{ marginLeft: "auto", padding: "2px 10px", fontSize: 12 }}
                onClick={() => setHistName(null)}
              >
                关闭
              </button>
            </div>
            <div
              className="log-viewer"
              style={{
                flex: 1,
                overflowY: "auto",
                background: "#1c1c1e",
                color: "#d4d4d4",
                fontFamily: "'SF Mono', 'SFMono-Regular', Consolas, monospace",
                fontSize: 12,
                padding: 12,
                whiteSpace: "pre-wrap",
                wordBreak: "break-all",
              }}
            >
              {histBusy ? "读取中…" : histContent || "（空文件）"}
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
