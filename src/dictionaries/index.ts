// 词典注册表：内置词典在此登记，页面会优先加载它们。
// 性能改造：词典数据（尤其雅思 ~540 KB）不再随首屏主包同步打入，
// 这里只登记「元信息 + 动态加载器」，词条数据在用户选中该词典时才
// 通过 import() 按需加载（Vite 自动拆成独立 chunk），并做模块级缓存。
// 想挂载更多内置词典，只需在 builtinDictionaryMetas 增加一条登记即可；
// 运行时用户也可通过「导入字典」加载任意 JSON 词典（见 DictionaryPage）。

import type { Dictionary } from "./types";

/** 内置词典元信息：足够渲染 tab / 简介，不含词条数据 */
export interface BuiltinDictMeta {
  id: string;
  name: string;
  description?: string;
  /** 动态加载完整词典（含词条）；返回 Promise，重复调用命中缓存 */
  load: () => Promise<Dictionary>;
}

// 加载结果缓存：同一词典只发起一次动态 import
const cache = new Map<string, Promise<Dictionary>>();

function cached(id: string, loader: () => Promise<Dictionary>): Promise<Dictionary> {
  let p = cache.get(id);
  if (!p) {
    p = loader().catch((err) => {
      // 加载失败（如极端情况下资源缺失）时清除缓存，允许下次重试
      cache.delete(id);
      throw err;
    });
    cache.set(id, p);
  }
  return p;
}

export const builtinDictionaryMetas: BuiltinDictMeta[] = [
  {
    id: "spinner-verbs",
    name: "Claude Code 状态提示词（Spinner Verbs）",
    description:
      "Claude Code 运行时随机轮换显示的动名词状态词，共 187 个。中英对照 + 记忆方法，主打背单词。",
    load: () =>
      cached("spinner-verbs", () =>
        import("./spinner-verbs").then((m) => m.spinnerVerbsDict)
      ),
  },
  {
    id: "ielts",
    name: "雅思词汇（有道词库）",
    description:
      "雅思常用词汇 3427 个（有道词库来源），含中英对照、词性与英文释义，主打背单词。",
    load: () => cached("ielts", () => import("./ielts").then((m) => m.ieltsDict)),
  },
];

export type { Dictionary, DictWord } from "./types";
