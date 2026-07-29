// 词典注册表：内置词典在此登记，页面会优先加载它们。
// 想挂载更多内置词典，只需在本文件 import 并加入 builtinDictionaries 即可；
// 运行时用户也可通过「导入字典」加载任意 JSON 词典（见 DictionaryPage）。

import { spinnerVerbsDict } from "./spinner-verbs";
import { ieltsDict } from "./ielts";
import type { Dictionary } from "./types";

export const builtinDictionaries: Dictionary[] = [spinnerVerbsDict, ieltsDict];

export type { Dictionary, DictWord } from "./types";
