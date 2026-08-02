// 词典数据结构：单词本页面加载的最小数据契约。
// 一份 Dictionary 就是一本可挂载的词典；DictWord 是单条词条。

export interface DictWord {
  /** 英文单词 / 短语（去掉了 Claude Code 显示用的省略号 "…"） */
  en: string;
  /** 中文释义 */
  zh: string;
  /** 分类标签（如「行动」「烹饪」「自然」），用于筛选与记忆分组 */
  category?: string;
  /** 记忆方法 / 助记提示 */
  memory?: string;
  /** 权威核验来源（仅对逐条查过权威词典的词填写，如 Merriam-Webster / Cambridge / Wiktionary） */
  source?: string;
}

export interface Dictionary {
  /** 唯一 id；内置词典固定，导入词典用 import-<timestamp> */
  id: string;
  /** 展示名称 */
  name: string;
  /** 简介 */
  description?: string;
  /** 是否为内置词典（内置词典不可删除） */
  builtin?: boolean;
  /** 词条列表 */
  words: DictWord[];
}
