// 单词本页面：加载提示词字典（及其他可挂载词典），主打背单词。
// 两种模式：
//   - 浏览：中英文对照 + 分类 + 记忆方法，支持搜索与分类筛选；
//   - 背诵：闪卡式自测，显示答案 / 认识 / 不认识，进度与「只看未掌握」「打乱」等。
// 可导入任意 JSON 词典（{ name?, description?, words:[{en,zh,category?,memory?}] } 或 words 数组），
// 导入后存 localStorage，重启不丢；内置词典不可删除。

import { useEffect, useMemo, useRef, useState } from "react";
import {
  BookOpen,
  Shuffle,
  Eye,
  Check,
  X,
  Search,
  Plus,
  Trash2,
  RotateCcw,
  Layers,
  GraduationCap,
} from "lucide-react";
import {
  builtinDictionaryMetas,
  type Dictionary,
  type DictWord,
} from "../dictionaries";

const LS_IMPORTED = "claude-launcher:dict:imported";
const knownKey = (id: string) => `claude-launcher:dict:known:${id}`;

function loadImported(): Dictionary[] {
  try {
    const raw = localStorage.getItem(LS_IMPORTED);
    if (!raw) return [];
    const arr = JSON.parse(raw);
    return Array.isArray(arr) ? (arr as Dictionary[]) : [];
  } catch {
    return [];
  }
}
function saveImported(list: Dictionary[]) {
  try {
    localStorage.setItem(LS_IMPORTED, JSON.stringify(list));
  } catch {
    /* ignore */
  }
}
function loadKnown(id: string): Set<string> {
  try {
    const raw = localStorage.getItem(knownKey(id));
    if (!raw) return new Set();
    const arr = JSON.parse(raw);
    return new Set(Array.isArray(arr) ? (arr as string[]) : []);
  } catch {
    return new Set();
  }
}
function saveKnown(id: string, set: Set<string>) {
  try {
    localStorage.setItem(knownKey(id), JSON.stringify([...set]));
  } catch {
    /* ignore */
  }
}

type Mode = "browse" | "study";

export function DictionaryPage() {
  const [imported, setImported] = useState<Dictionary[]>(() => loadImported());
  // 内置词典词条按需加载：进入页面 / 切换词典时才动态 import 对应数据 chunk。
  const [loadedBuiltins, setLoadedBuiltins] = useState<
    Record<string, Dictionary>
  >({});
  const [activeId, setActiveId] = useState<string>(
    () => builtinDictionaryMetas[0]?.id ?? ""
  );
  const [mode, setMode] = useState<Mode>("browse");

  // 词典选择 tab 数据：内置词典用元信息（无需词条），导入词典直接用本体
  const tabs = useMemo(
    () => [
      ...builtinDictionaryMetas.map((m) => ({
        id: m.id,
        name: m.name,
        description: m.description,
        builtin: true,
      })),
      ...imported.map((d) => ({
        id: d.id,
        name: d.name,
        description: d.description,
        builtin: false,
      })),
    ],
    [imported]
  );

  // 激活的内置词典未加载时触发动态加载（注册表内部有缓存，不会重复请求）
  useEffect(() => {
    const meta = builtinDictionaryMetas.find((m) => m.id === activeId);
    if (!meta || loadedBuiltins[meta.id]) return;
    let cancelled = false;
    meta
      .load()
      .then((dict) => {
        if (cancelled) return;
        setLoadedBuiltins((prev) =>
          prev[dict.id] ? prev : { ...prev, [dict.id]: dict }
        );
      })
      .catch((e) => console.error("加载词典失败", e));
    return () => {
      cancelled = true;
    };
  }, [activeId, loadedBuiltins]);

  // 浏览态
  const [query, setQuery] = useState("");
  const [cat, setCat] = useState<string>("all");

  // 背诵态
  const [shuffle, setShuffle] = useState(false);
  const [shuffleVersion, setShuffleVersion] = useState(0);
  const [onlyUnknown, setOnlyUnknown] = useState(false);
  const [revealed, setRevealed] = useState(false);
  const [idx, setIdx] = useState(0);
  const [known, setKnown] = useState<Set<string>>(() => loadKnown(activeId));

  const activeDict =
    loadedBuiltins[activeId] ?? imported.find((d) => d.id === activeId);
  // 内置词典选中但词条还没加载完：展示轻量加载占位（延迟淡入，避免闪烁）
  const dictLoading =
    !activeDict && builtinDictionaryMetas.some((m) => m.id === activeId);

  // 切换词典时重置背诵进度态并加载该词典的掌握集合
  useEffect(() => {
    setKnown(loadKnown(activeId));
    setIdx(0);
    setRevealed(false);
    setQuery("");
    setCat("all");
  }, [activeId]);

  const categories = useMemo(() => {
    const s = new Set<string>();
    activeDict?.words.forEach((w) => w.category && s.add(w.category));
    return [...s].sort();
  }, [activeDict]);

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    return (activeDict?.words ?? []).filter((w) => {
      if (cat !== "all" && w.category !== cat) return false;
      if (
        q &&
        !`${w.en} ${w.zh} ${w.category ?? ""}`.toLowerCase().includes(q)
      )
        return false;
      return true;
    });
  }, [activeDict, query, cat]);

  // 打乱结果只在切换词典/开关打乱时生成。掌握状态变化只负责过滤，
  // 不能让剩余卡片每答一张就重新洗牌。
  const orderedStudyWords = useMemo(() => {
    const list = (activeDict?.words ?? []).slice();
    if (shuffle) list.sort(() => Math.random() - 0.5);
    return list;
  }, [activeDict, shuffle, shuffleVersion]);

  const studyList = useMemo(
    () =>
      onlyUnknown
        ? orderedStudyWords.filter((w) => !known.has(w.en))
        : orderedStudyWords,
    [orderedStudyWords, known, onlyUnknown]
  );

  const total = activeDict?.words.length ?? 0;
  const knownCount = useMemo(
    () => (activeDict?.words ?? []).filter((w) => known.has(w.en)).length,
    [activeDict, known]
  );
  const current = studyList[Math.min(idx, Math.max(0, studyList.length - 1))];
  const done = studyList.length === 0 || idx >= studyList.length;

  const mark = (en: string, isKnown: boolean) => {
    setKnown((prev) => {
      const next = new Set(prev);
      if (isKnown) next.add(en);
      else next.delete(en);
      saveKnown(activeId, next);
      return next;
    });
  };
  const goNext = () => {
    setIdx((i) => i + 1);
    setRevealed(false);
  };
  const onKnow = () => {
    if (current) mark(current.en, true);
    // 「只看未掌握」会把当前卡从列表移除；保持索引不变即可自然落到下一张。
    // 若此时再 +1，会跳过紧随其后的词条。
    if (onlyUnknown) setRevealed(false);
    else goNext();
  };
  const onUnknown = () => {
    if (current) mark(current.en, false);
    goNext();
  };
  const reveal = () => setRevealed(true);

  // 键盘快捷键（仅背诵模式、且焦点不在输入框时）：空格/回车=显示答案或认识，→=下一个，1=认识，2=不认识
  const handlers = useRef({ reveal, onKnow, onUnknown, goNext });
  handlers.current = { reveal, onKnow, onUnknown, goNext };
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const t = e.target as HTMLElement | null;
      if (
        t &&
        (t.tagName === "INPUT" ||
          t.tagName === "TEXTAREA" ||
          t.tagName === "SELECT")
      )
        return;
      if (mode !== "study" || !current || done) return;
      if (e.code === "Space" || e.code === "Enter") {
        e.preventDefault();
        revealed ? handlers.current.onKnow() : handlers.current.reveal();
      } else if (e.code === "ArrowRight") {
        e.preventDefault();
        handlers.current.goNext();
      } else if (e.key === "1") {
        handlers.current.onKnow();
      } else if (e.key === "2") {
        handlers.current.onUnknown();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [mode, current, revealed, done]);

  // 导入 JSON 词典
  const fileRef = useRef<HTMLInputElement>(null);
  const onImportFile = async (e: React.ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0];
    e.target.value = ""; // 允许重复选同一文件
    if (!file) return;
    try {
      const parsed = JSON.parse(await file.text());
      const rawWords: unknown = Array.isArray(parsed) ? parsed : parsed?.words;
      if (!Array.isArray(rawWords)) throw new Error("找不到 words 数组");
      const norm: DictWord[] = (rawWords as Record<string, unknown>[])
        .filter((w) => w && w.en && w.zh)
        .map((w) => ({
          en: String(w.en).replace(/…/g, "").trim(),
          zh: String(w.zh).trim(),
          category: w.category ? String(w.category).trim() : undefined,
          memory: w.memory ? String(w.memory).trim() : undefined,
          source: w.source ? String(w.source).trim() : undefined,
        }));
      if (norm.length === 0)
        throw new Error("没有有效词条（每条需含 en 与 zh）");
      const baseName = file.name.replace(/\.json$/i, "");
      const name = Array.isArray(parsed)
        ? baseName
        : (parsed?.name as string) || baseName;
      const dict: Dictionary = {
        id: `import-${Date.now()}`,
        name,
        description: Array.isArray(parsed)
          ? undefined
          : (parsed?.description as string | undefined),
        words: norm,
      };
      const nextImported = [...imported, dict];
      setImported(nextImported);
      saveImported(nextImported);
      setActiveId(dict.id);
      setMode("browse");
    } catch (err) {
      alert(
        "导入失败：" + (err instanceof Error ? err.message : String(err)) +
          "\n\n格式示例：\n{\n  \"name\": \"我的词典\",\n  \"words\": [\n    { \"en\": \"Apple\", \"zh\": \"苹果\", \"category\": \"水果\", \"memory\": \"red fruit\" }\n  ]\n}"
      );
    }
  };
  const removeImport = (id: string) => {
    const nextImported = imported.filter((d) => d.id !== id);
    setImported(nextImported);
    saveImported(nextImported);
    if (activeId === id) setActiveId(builtinDictionaryMetas[0]?.id ?? "");
  };

  const progressPct = total > 0 ? Math.round((knownCount / total) * 100) : 0;

  return (
    <div className="page">
      <h2 className="page-title">单词本</h2>
      <p className="page-desc">
        已内置「Spinner Verbs」与「雅思核心词汇」两本词典，可切换背诵：英文 + 中文 + 记忆方法。
        也支持导入你自己的 JSON 词典，随时挂载、随时背。
      </p>

      {/* 词典选择器 + 导入 */}
      <div className="dict-tabs">
        {tabs.map((d) => (
          <div
            key={d.id}
            className={`dict-tab ${d.id === activeId ? "active" : ""}`}
            onClick={() => setActiveId(d.id)}
            title={d.description ?? d.name}
          >
            <BookOpen size={14} className="dict-tab-icon" />
            <span className="dict-tab-name">{d.name}</span>
            {d.builtin ? (
              <span className="dict-tab-badge">内置</span>
            ) : (
              <button
                className="dict-tab-remove"
                title="移除该导入词典"
                onClick={(e) => {
                  e.stopPropagation();
                  removeImport(d.id);
                }}
              >
                <Trash2 size={12} />
              </button>
            )}
          </div>
        ))}
        <button
          className="dict-tab dict-tab-import"
          onClick={() => fileRef.current?.click()}
          title="导入 JSON 词典"
        >
          <Plus size={14} />
          <span>导入词典</span>
        </button>
        <input
          ref={fileRef}
          type="file"
          accept=".json,application/json"
          style={{ display: "none" }}
          onChange={onImportFile}
        />
      </div>

      {/* 模式切换 */}
      <div className="dict-modebar">
        <div className="dict-mode-toggle">
          <button
            className={`dict-mode-btn ${mode === "browse" ? "active" : ""}`}
            onClick={() => setMode("browse")}
          >
            <Layers size={14} /> 浏览
          </button>
          <button
            className={`dict-mode-btn ${mode === "study" ? "active" : ""}`}
            onClick={() => {
              setMode("study");
              setIdx(0);
              setRevealed(false);
            }}
          >
            <GraduationCap size={14} /> 背诵
          </button>
        </div>
        <div className="dict-progress">
          <div className="dict-progress-bar">
            <div
              className="dict-progress-fill"
              style={{ width: `${progressPct}%` }}
            />
          </div>
          <span className="dict-progress-text">
            已掌握 {knownCount}/{total}（{progressPct}%）
          </span>
        </div>
      </div>

      {/* 词典数据按需加载中：轻量占位，延迟淡入避免快速加载时闪烁 */}
      {dictLoading && (
        <div className="card">
          <div className="lazy-loading" role="status">
            词典加载中…
          </div>
        </div>
      )}

      {/* ============ 浏览模式 ============ */}
      {mode === "browse" && !dictLoading && (
        <div className="card">
          <div className="dict-browse-controls">
            <div className="dict-search">
              <Search size={15} className="dict-search-icon" />
              <input
                className="dict-search-input"
                placeholder="搜索英文 / 中文 / 分类…"
                value={query}
                onChange={(e) => setQuery(e.target.value)}
              />
            </div>
            <select
              className="select-input"
              value={cat}
              onChange={(e) => setCat(e.target.value)}
            >
              <option value="all">全部分类（{total}）</option>
              {categories.map((c) => (
                <option key={c} value={c}>
                  {c}
                </option>
              ))}
            </select>
          </div>

          <div className="dict-list">
            {filtered.length === 0 ? (
              <p className="form-hint">没有匹配的词条。</p>
            ) : (
              filtered.map((w) => (
                <div key={w.en} className="dict-row">
                  <div className="dict-row-main">
                    <span className="dict-en">{w.en}</span>
                    <span className="dict-zh">{w.zh}</span>
                    {w.category && (
                      <span className="dict-cat">{w.category}</span>
                    )}
                    {known.has(w.en) && (
                      <span className="dict-known-dot" title="已掌握">
                        <Check size={11} />
                      </span>
                    )}
                  </div>
                  {w.memory && (
                    <div className="dict-memory">💡 {w.memory}</div>
                  )}
                  {w.source && (
                    <div className="dict-source" title="该词含义已逐条查权威词典核验">
                      🔗 出处：{w.source}
                    </div>
                  )}
                </div>
              ))
            )}
          </div>
        </div>
      )}

      {/* ============ 背诵模式 ============ */}
      {mode === "study" && !dictLoading && (
        <div className="card">
          <div className="dict-study-toolbar">
            <label className="checkbox-label">
              <input
                type="checkbox"
                checked={onlyUnknown}
                onChange={(e) => {
                  setOnlyUnknown(e.target.checked);
                  setIdx(0);
                  setRevealed(false);
                }}
              />
              <span>只看未掌握</span>
            </label>
            <label className="checkbox-label">
              <input
                type="checkbox"
                checked={shuffle}
                onChange={(e) => {
                  setShuffle(e.target.checked);
                  setShuffleVersion((version) => version + 1);
                  setIdx(0);
                  setRevealed(false);
                }}
              />
              <span>
                <Shuffle size={12} /> 打乱顺序
              </span>
            </label>
            <button
              className="btn btn-secondary dict-reset"
              onClick={() => {
                setKnown(new Set());
                saveKnown(activeId, new Set());
                setIdx(0);
                setRevealed(false);
              }}
              title="清空当前词典的掌握进度"
            >
              <RotateCcw size={13} /> 重置进度
            </button>
            <span className="dict-study-count">
              第 {Math.min(idx + 1, studyList.length)} / {studyList.length} 张
            </span>
          </div>

          {done ? (
            <div className="dict-done">
              <div className="dict-done-emoji">🎉</div>
              <p className="dict-done-title">这一组都过完啦！</p>
              <p className="muted">
                {onlyUnknown
                  ? "当前「只看未掌握」已无可复习词条，说明都掌握了。"
                  : "全部词条已浏览一遍。"}
              </p>
              <div className="dict-done-actions">
                <button
                  className="btn btn-primary"
                  onClick={() => {
                    setOnlyUnknown(false);
                    setIdx(0);
                    setRevealed(false);
                  }}
                >
                  重看全部
                </button>
                <button
                  className="btn btn-secondary"
                  onClick={() => {
                    setOnlyUnknown(true);
                    setIdx(0);
                    setRevealed(false);
                  }}
                >
                  只看未掌握
                </button>
              </div>
            </div>
          ) : (
            <div className="dict-flashcard">
              <div className="dict-flash-en">
                {current?.en}
                {current?.category && (
                  <span className="dict-cat dict-cat-center">
                    {current.category}
                  </span>
                )}
              </div>

              {revealed ? (
                <div className="dict-flash-answer">
                  <div className="dict-flash-zh">{current?.zh}</div>
                  {current?.memory && (
                    <div className="dict-memory dict-memory-center">
                      💡 {current.memory}
                    </div>
                  )}
                  {current?.source && (
                    <div className="dict-source dict-memory-center">
                      🔗 出处：{current.source}
                    </div>
                  )}
                </div>
              ) : (
                <div className="dict-flash-hidden">按空格 / 点击显示答案</div>
              )}

              <div className="dict-flash-actions">
                {!revealed ? (
                  <button className="btn btn-primary" onClick={reveal}>
                    <Eye size={15} /> 显示答案
                  </button>
                ) : (
                  <>
                    <button className="btn btn-unknown" onClick={onUnknown}>
                      <X size={15} /> 不认识
                    </button>
                    <button className="btn btn-known" onClick={onKnow}>
                      <Check size={15} /> 认识
                    </button>
                  </>
                )}
                <button className="btn btn-secondary" onClick={goNext}>
                  跳过 →
                </button>
              </div>
            </div>
          )}
        </div>
      )}

      <p className="form-hint dict-foot-hint">
        快捷键：背诵模式下 空格/回车 = 显示答案或「认识」，→ = 下一张，1 = 认识，2 =
        不认识。
      </p>
    </div>
  );
}
