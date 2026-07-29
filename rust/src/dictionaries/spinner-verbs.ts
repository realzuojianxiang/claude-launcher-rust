// Claude Code 状态提示词（Spinner Verbs）词典数据。
// 由 docs/《Claude Code 状态词汇表.md》解析生成，共 187 条。
// 校验说明：zh 中文翻译、memory 记忆方法经逐条审定——
//   常见词依据标准英文词义（高置信）；生僻 / 奇趣词已核对权威词典
//   （Merriam-Webster / Cambridge / Wiktionary 等，2026-07-29）；
//   memory 为可靠词源或语义联想，非编造词根拆解。Clauding / Gitifying
//   为 Claude 专属新词（名词动词化）。Evaporating 不在官方二进制默认词表，属用户补充。

import type { Dictionary } from "./types";

export const spinnerVerbsDict: Dictionary = {
  id: "spinner-verbs",
  name: "Claude Code 状态提示词（Spinner Verbs）",
  description: "Claude Code 运行时随机轮换显示的动名词状态词，共 187 个。中英对照 + 记忆方法，主打背单词。",
  builtin: true,
  words:     [
      {
        "en": "Accomplishing",
        "zh": "完成中",
        "category": "行动",
        "memory": "accomplish 完成；词源拉丁 complēre「填满、完成」，-ing 表进行"
      },
      {
        "en": "Actioning",
        "zh": "执行中",
        "category": "行动",
        "memory": "action 行动 + -ing；把计划付诸行动"
      },
      {
        "en": "Actualizing",
        "zh": "实现中",
        "category": "行动",
        "memory": "actual（实际的）引申为「使之成为现实」"
      },
      {
        "en": "Architecting",
        "zh": "架构中",
        "category": "行动",
        "memory": "architect 建筑师 → 像搭建筑一样设计结构"
      },
      {
        "en": "Baking",
        "zh": "烘焙中",
        "category": "烹饪",
        "memory": "bake 烘烤（烹饪）"
      },
      {
        "en": "Beaming",
        "zh": "发光 / 微笑中",
        "category": "行动",
        "memory": "beam 光束 / 笑容；容光焕发"
      },
      {
        "en": "Beboppin'",
        "zh": "玩比波普爵士中",
        "category": "音乐",
        "memory": "bebop 一种轻快爵士乐风，+-in' 口语化"
      },
      {
        "en": "Befuddling",
        "zh": "把人搞糊涂中",
        "category": "奇趣",
        "memory": "befuddle 使困惑、弄晕"
      },
      {
        "en": "Billowing",
        "zh": "翻腾 / 鼓胀中",
        "category": "自然",
        "memory": "billow 巨浪翻涌、蓬起"
      },
      {
        "en": "Blanching",
        "zh": "焯水 / 泛白中",
        "category": "烹饪",
        "memory": "blanch 焯烫（烹饪）；也指（脸色）发白"
      },
      {
        "en": "Bloviating",
        "zh": "滔滔不绝吹嘘中",
        "category": "奇趣",
        "memory": "bloviate 美式，长篇大论说空话"
      },
      {
        "en": "Boogieing",
        "zh": "跳布吉舞中",
        "category": "音乐",
        "memory": "boogie 布吉舞 / 摇摆律动"
      },
      {
        "en": "Boondoggling",
        "zh": "磨洋工 / 瞎忙中",
        "category": "奇趣",
        "memory": "boondoggle 无意义又费时的琐事"
      },
      {
        "en": "Booping",
        "zh": "哔哔轻点中",
        "category": "奇趣",
        "memory": "boop 拟声：轻触或电子「哔」声"
      },
      {
        "en": "Bootstrapping",
        "zh": "自助启动中",
        "category": "行动",
        "memory": "bootstrap 提鞋带自立；计算机「自举」启动"
      },
      {
        "en": "Brewing",
        "zh": "酿造中",
        "category": "烹饪",
        "memory": "brew 酿（啤酒 / 茶）"
      },
      {
        "en": "Bunning",
        "zh": "揉小圆面包中",
        "category": "烹饪",
        "memory": "bun 小圆面包（烹饪）"
      },
      {
        "en": "Burrowing",
        "zh": "掘洞中",
        "category": "移动",
        "memory": "burrow 地洞；钻入"
      },
      {
        "en": "Calculating",
        "zh": "计算中",
        "category": "思考",
        "memory": "calculate 计算"
      },
      {
        "en": "Canoodling",
        "zh": "亲昵搂抱中",
        "category": "奇趣",
        "memory": "canoodle 亲热、搂抱"
      },
      {
        "en": "Caramelizing",
        "zh": "焦糖化中",
        "category": "烹饪",
        "memory": "caramel 焦糖（烹饪）"
      },
      {
        "en": "Cascading",
        "zh": "层叠倾泻中",
        "category": "自然",
        "memory": "cascade 瀑布 / 级联"
      },
      {
        "en": "Catapulting",
        "zh": "弹射中",
        "category": "移动",
        "memory": "catapult 弹弓、弹射器"
      },
      {
        "en": "Cerebrating",
        "zh": "动脑思考中",
        "category": "思考",
        "memory": "cerebrate 用大脑思考（cerebrum 大脑）"
      },
      {
        "en": "Channeling",
        "zh": "引导 / 疏通中",
        "category": "思考",
        "memory": "channel 渠道、引导（美式拼写）"
      },
      {
        "en": "Channelling",
        "zh": "引导 / 疏通中",
        "category": "思考",
        "memory": "channel 的英式拼写"
      },
      {
        "en": "Choreographing",
        "zh": "编排舞蹈中",
        "category": "行动",
        "memory": "choreography 编舞"
      },
      {
        "en": "Churning",
        "zh": "搅拌 / 翻腾中",
        "category": "行动",
        "memory": "churn 搅乳、翻腾"
      },
      {
        "en": "Clauding",
        "zh": "像 Claude 一样干活中",
        "category": "奇趣",
        "memory": "Claude 专属新词（名词动词化），指以 Claude 的方式处理事务"
      },
      {
        "en": "Coalescing",
        "zh": "汇聚 / 合并中",
        "category": "自然",
        "memory": "coalesce 合并、聚结（co- 一起 + alescere 长大）"
      },
      {
        "en": "Cogitating",
        "zh": "深思中",
        "category": "思考",
        "memory": "cogitate 思考、盘算"
      },
      {
        "en": "Combobulating",
        "zh": "整理得有条理中",
        "category": "行动",
        "memory": "combobulate 非标准词，与 discombobulate 相对，意为「使条理分明」"
      },
      {
        "en": "Composing",
        "zh": "创作 / 作曲中",
        "category": "行动",
        "memory": "compose 组成、创作"
      },
      {
        "en": "Computing",
        "zh": "计算中",
        "category": "思考",
        "memory": "compute 计算"
      },
      {
        "en": "Concocting",
        "zh": "调制 / 编造中",
        "category": "烹饪",
        "memory": "concoct 调配（药水）或编造（借口）"
      },
      {
        "en": "Considering",
        "zh": "考虑中",
        "category": "思考",
        "memory": "consider 考虑"
      },
      {
        "en": "Contemplating",
        "zh": "沉思中",
        "category": "思考",
        "memory": "contemplate 凝视、沉思"
      },
      {
        "en": "Cooking",
        "zh": "烹饪中",
        "category": "烹饪",
        "memory": "cook 烹饪"
      },
      {
        "en": "Crafting",
        "zh": "精心制作中",
        "category": "行动",
        "memory": "craft 手艺、精心制作"
      },
      {
        "en": "Creating",
        "zh": "创造中",
        "category": "行动",
        "memory": "create 创造"
      },
      {
        "en": "Crunching",
        "zh": "处理（数据）/ 咬碎中",
        "category": "思考",
        "memory": "crunch 嘎吱咬碎；crunch numbers 处理数据"
      },
      {
        "en": "Crystallizing",
        "zh": "结晶 / 明朗化中",
        "category": "自然",
        "memory": "crystallize 结晶；想法变清晰"
      },
      {
        "en": "Cultivating",
        "zh": "培养中",
        "category": "自然",
        "memory": "cultivate 耕作、培养"
      },
      {
        "en": "Deciphering",
        "zh": "破译中",
        "category": "思考",
        "memory": "decipher 解码、辨认"
      },
      {
        "en": "Deliberating",
        "zh": "审议中",
        "category": "思考",
        "memory": "deliberate 深思、审议"
      },
      {
        "en": "Determining",
        "zh": "确定中",
        "category": "思考",
        "memory": "determine 决定、确定"
      },
      {
        "en": "Dilly-dallying",
        "zh": "磨蹭 / 拖拖拉拉中",
        "category": "奇趣",
        "memory": "dilly-dally 浪费时间、犹豫不决"
      },
      {
        "en": "Discombobulating",
        "zh": "弄得晕头转向中",
        "category": "奇趣",
        "memory": "discombobulate 打乱、使混乱；本词表最长的奇趣词"
      },
      {
        "en": "Doing",
        "zh": "正在做中",
        "category": "行动",
        "memory": "do 做"
      },
      {
        "en": "Doodling",
        "zh": "涂鸦中",
        "category": "行动",
        "memory": "doodle 信手涂画"
      },
      {
        "en": "Drizzling",
        "zh": "毛毛雨淋 / 细流中",
        "category": "自然",
        "memory": "drizzle 毛毛雨、细流"
      },
      {
        "en": "Ebbing",
        "zh": "退潮 / 衰退中",
        "category": "自然",
        "memory": "ebb 退潮、消退"
      },
      {
        "en": "Effecting",
        "zh": "实现 / 促的中",
        "category": "行动",
        "memory": "effect 实现、促成"
      },
      {
        "en": "Elucidating",
        "zh": "阐明中",
        "category": "思考",
        "memory": "elucidate 解释清楚（e- 出 + lucid 清晰）"
      },
      {
        "en": "Embellishing",
        "zh": "装饰 / 润色中",
        "category": "行动",
        "memory": "embellish 美化、润色"
      },
      {
        "en": "Enchanting",
        "zh": "施魔法 / 迷人中",
        "category": "奇趣",
        "memory": "enchant 施咒、使陶醉"
      },
      {
        "en": "Envisioning",
        "zh": "设想中",
        "category": "思考",
        "memory": "envision 想象、展望"
      },
      {
        "en": "Evaporating",
        "zh": "蒸发中",
        "category": "自然",
        "memory": "evaporate = e-（出）+ vapor 蒸汽；注：此词不在官方二进制默认词表，为用户补充"
      },
      {
        "en": "Fermenting",
        "zh": "发酵中",
        "category": "烹饪",
        "memory": "ferment 发酵"
      },
      {
        "en": "Fiddle-faddling",
        "zh": "瞎摆弄 / 无聊捣鼓中",
        "category": "奇趣",
        "memory": "fiddle-faddle 无聊琐事、瞎折腾"
      },
      {
        "en": "Finagling",
        "zh": "耍手段搞定中",
        "category": "奇趣",
        "memory": "finagle 用计谋巧妙获得"
      },
      {
        "en": "Flambéing",
        "zh": "火焰炙烧中",
        "category": "烹饪",
        "memory": "flambé 法式火焰料理（烹饪）"
      },
      {
        "en": "Flibbertigibbeting",
        "zh": "疯疯癫癫碎碎念中",
        "category": "奇趣",
        "memory": "flibbertigibbet 傻乎乎爱闲扯的人",
        "source": "Merriam-Webster / Wiktionary：flibbertigibbet = a silly, frivolous person who chatters"
      },
      {
        "en": "Flowing",
        "zh": "流淌中",
        "category": "自然",
        "memory": "flow 流动"
      },
      {
        "en": "Flummoxing",
        "zh": "把人难住中",
        "category": "奇趣",
        "memory": "flummox 使困惑、难倒"
      },
      {
        "en": "Fluttering",
        "zh": "扑动 / 飘动中",
        "category": "自然",
        "memory": "flutter 扑翅、飘动"
      },
      {
        "en": "Forging",
        "zh": "锻造 / 伪造中",
        "category": "行动",
        "memory": "forge 锻造；引申伪造"
      },
      {
        "en": "Forming",
        "zh": "形成中",
        "category": "行动",
        "memory": "form 形成"
      },
      {
        "en": "Frolicking",
        "zh": "嬉戏中",
        "category": "奇趣",
        "memory": "frolic 嬉戏、玩闹"
      },
      {
        "en": "Frosting",
        "zh": "撒糖霜中",
        "category": "烹饪",
        "memory": "frost 糖霜（烹饪）"
      },
      {
        "en": "Gallivanting",
        "zh": "闲游 / 寻欢作中",
        "category": "移动",
        "memory": "gallivant 闲荡玩耍"
      },
      {
        "en": "Galloping",
        "zh": "飞奔中",
        "category": "移动",
        "memory": "gallop 马疾驰"
      },
      {
        "en": "Garnishing",
        "zh": "装饰配菜中",
        "category": "烹饪",
        "memory": "garnish 配菜点缀（烹饪）"
      },
      {
        "en": "Generating",
        "zh": "生成中",
        "category": "行动",
        "memory": "generate 产生、生成"
      },
      {
        "en": "Gesticulating",
        "zh": "比手画脚中",
        "category": "奇趣",
        "memory": "gesticulate 打手势表达"
      },
      {
        "en": "Germinating",
        "zh": "发芽中",
        "category": "自然",
        "memory": "germinate 发芽、萌芽"
      },
      {
        "en": "Gitifying",
        "zh": "用 git 管理 / git 化中",
        "category": "行动",
        "memory": "git（版本控制工具）动词化新词"
      },
      {
        "en": "Grooving",
        "zh": "跟着节奏摇摆中",
        "category": "音乐",
        "memory": "groove 凹槽节奏；跟着感觉走"
      },
      {
        "en": "Gusting",
        "zh": "阵风刮过中",
        "category": "自然",
        "memory": "gust 一阵风"
      },
      {
        "en": "Harmonizing",
        "zh": "调和 / 和声中",
        "category": "音乐",
        "memory": "harmonize 协调、和声"
      },
      {
        "en": "Hashing",
        "zh": "哈希 / 切碎中",
        "category": "行动",
        "memory": "hash 哈希；也指剁碎"
      },
      {
        "en": "Hatching",
        "zh": "孵化 / 策划中",
        "category": "自然",
        "memory": "hatch 孵化、策划"
      },
      {
        "en": "Herding",
        "zh": "驱赶 / 聚拢中",
        "category": "行动",
        "memory": "herd 驱赶畜群"
      },
      {
        "en": "Honking",
        "zh": "按喇叭 / 雁鸣中",
        "category": "奇趣",
        "memory": "honk 喇叭声、雁叫"
      },
      {
        "en": "Hullaballooing",
        "zh": "闹腾 / 大惊小怪中",
        "category": "奇趣",
        "memory": "hullabaloo 喧闹、大惊小怪"
      },
      {
        "en": "Hyperspacing",
        "zh": "超空间跃迁中",
        "category": "奇趣",
        "memory": "hyperspace 超空间（科幻概念）"
      },
      {
        "en": "Ideating",
        "zh": "构思中",
        "category": "思考",
        "memory": "ideate 形成想法"
      },
      {
        "en": "Imagining",
        "zh": "想象中",
        "category": "思考",
        "memory": "imagine 想象"
      },
      {
        "en": "Improvising",
        "zh": "即兴发挥中",
        "category": "音乐",
        "memory": "improvise 即兴创作"
      },
      {
        "en": "Incubating",
        "zh": "孵化 / 培育中",
        "category": "自然",
        "memory": "incubate 孵化、酝酿"
      },
      {
        "en": "Inferring",
        "zh": "推断中",
        "category": "思考",
        "memory": "infer 推断"
      },
      {
        "en": "Infusing",
        "zh": "注入 / 泡制中",
        "category": "烹饪",
        "memory": "infuse 注入（茶 / 精神）"
      },
      {
        "en": "Ionizing",
        "zh": "电离中",
        "category": "自然",
        "memory": "ionize 电离"
      },
      {
        "en": "Jitterbugging",
        "zh": "跳吉特巴舞中",
        "category": "音乐",
        "memory": "jitterbug 一种欢快双人舞"
      },
      {
        "en": "Julienning",
        "zh": "切丝中",
        "category": "烹饪",
        "memory": "julienne 切成细丝（烹饪）"
      },
      {
        "en": "Kneading",
        "zh": "揉（面）中",
        "category": "烹饪",
        "memory": "knead 揉面（烹饪）"
      },
      {
        "en": "Leavening",
        "zh": "发酵 / 使膨松中",
        "category": "烹饪",
        "memory": "leaven 酵母、使发酵（烹饪）"
      },
      {
        "en": "Levitating",
        "zh": "悬浮中",
        "category": "奇趣",
        "memory": "levitate 漂浮、悬浮"
      },
      {
        "en": "Lollygagging",
        "zh": "懒洋洋闲荡中",
        "category": "奇趣",
        "memory": "lollygag 美俚，闲荡磨蹭",
        "source": "Merriam-Webster：lollygag = to spend time aimlessly; to dawdle"
      },
      {
        "en": "Manifesting",
        "zh": "显化 / 实现中",
        "category": "行动",
        "memory": "manifest 显现、实现"
      },
      {
        "en": "Marinating",
        "zh": "腌制中",
        "category": "烹饪",
        "memory": "marinate 腌渍入味（烹饪）"
      },
      {
        "en": "Meandering",
        "zh": "蜿蜒漫步中",
        "category": "移动",
        "memory": "meander 蜿蜒、闲逛"
      },
      {
        "en": "Metamorphosing",
        "zh": "蜕变中",
        "category": "自然",
        "memory": "metamorphose 变形、蜕变"
      },
      {
        "en": "Misting",
        "zh": "喷雾中",
        "category": "自然",
        "memory": "mist 薄雾、喷雾"
      },
      {
        "en": "Moonwalking",
        "zh": "跳太空步中",
        "category": "移动",
        "memory": "moonwalk 月球漫步 / 迈克尔·杰克逊招牌滑步"
      },
      {
        "en": "Moseying",
        "zh": "溜达中",
        "category": "移动",
        "memory": "mosey 闲逛、溜达"
      },
      {
        "en": "Mulling",
        "zh": "斟酌 / 温酒中",
        "category": "思考",
        "memory": "mull 细想；也指温酒（mulled wine）"
      },
      {
        "en": "Mustering",
        "zh": "召集 / 鼓起中",
        "category": "行动",
        "memory": "muster 集结、鼓起（勇气）"
      },
      {
        "en": "Musing",
        "zh": "冥想中",
        "category": "思考",
        "memory": "muse 沉思、冥想"
      },
      {
        "en": "Nebulizing",
        "zh": "雾化中",
        "category": "自然",
        "memory": "nebulize 雾化（如雾化器）"
      },
      {
        "en": "Nesting",
        "zh": "筑巢 / 嵌套中",
        "category": "自然",
        "memory": "nest 巢；也指代码嵌套"
      },
      {
        "en": "Newspapering",
        "zh": "排报纸 / 编辑中",
        "category": "行动",
        "memory": "newspaper 报纸；做报刊编辑"
      },
      {
        "en": "Noodling",
        "zh": "即兴弹琴 / 徒手摸鲶鱼中",
        "category": "思考",
        "memory": "noodle 双义：(1)吉他即兴 (2)徒手抓鲶鱼",
        "source": "Merriam-Webster：noodle (v.) = to improvise on a musical instrument; also to catch catfish bare-handed"
      },
      {
        "en": "Nucleating",
        "zh": "成核 / 聚成核心中",
        "category": "自然",
        "memory": "nucleate 成核、形成核心"
      },
      {
        "en": "Orbiting",
        "zh": "绕轨运行中",
        "category": "自然",
        "memory": "orbit 轨道、绕行"
      },
      {
        "en": "Orchestrating",
        "zh": "统筹 / 编排中",
        "category": "行动",
        "memory": "orchestrate 管弦乐编排；引申统筹"
      },
      {
        "en": "Osmosing",
        "zh": "渗透 / 潜移中",
        "category": "自然",
        "memory": "osmose 渗透、潜移默化"
      },
      {
        "en": "Perambulating",
        "zh": "漫步中",
        "category": "移动",
        "memory": "perambulate 散步（常指推婴儿车）"
      },
      {
        "en": "Percolating",
        "zh": "滤煮 / 渗透中",
        "category": "自然",
        "memory": "percolate 渗透；也指滴滤咖啡"
      },
      {
        "en": "Perusing",
        "zh": "细读中",
        "category": "思考",
        "memory": "peruse 研读、审视"
      },
      {
        "en": "Philosophising",
        "zh": "哲思中",
        "category": "思考",
        "memory": "philosophize 的英式拼写"
      },
      {
        "en": "Photosynthesizing",
        "zh": "光合作用中",
        "category": "自然",
        "memory": "photosynthesis 光合作用"
      },
      {
        "en": "Pollinating",
        "zh": "传粉中",
        "category": "自然",
        "memory": "pollinate 授粉"
      },
      {
        "en": "Pondering",
        "zh": "思索中",
        "category": "思考",
        "memory": "ponder 沉思、掂量"
      },
      {
        "en": "Pontificating",
        "zh": "居高临下说教中",
        "category": "奇趣",
        "memory": "pontificate 武断地说教"
      },
      {
        "en": "Pouncing",
        "zh": "猛扑中",
        "category": "移动",
        "memory": "pounce 猛扑、突袭"
      },
      {
        "en": "Precipitating",
        "zh": "沉淀 / 促成中",
        "category": "自然",
        "memory": "precipitate 沉淀；也指引发"
      },
      {
        "en": "Prestidigitating",
        "zh": "变戏法中",
        "category": "奇趣",
        "memory": "prestidigitation 手技、戏法"
      },
      {
        "en": "Processing",
        "zh": "处理中",
        "category": "行动",
        "memory": "process 处理"
      },
      {
        "en": "Proofing",
        "zh": "醒发 / 校验中",
        "category": "行动",
        "memory": "proof 面包醒发；也指校对 / 验证"
      },
      {
        "en": "Propagating",
        "zh": "传播 / 繁殖中",
        "category": "自然",
        "memory": "propagate 繁殖、传播"
      },
      {
        "en": "Puttering",
        "zh": "慢悠悠摆弄中",
        "category": "奇趣",
        "memory": "putter 闲荡、慢吞吞做事"
      },
      {
        "en": "Puzzling",
        "zh": "解谜 / 困惑中",
        "category": "思考",
        "memory": "puzzle 拼图、使困惑"
      },
      {
        "en": "Quantumizing",
        "zh": "量子化中",
        "category": "奇趣",
        "memory": "quantum 量子 + -ize 使…化"
      },
      {
        "en": "Razzle-dazzling",
        "zh": "花里胡哨炫目中",
        "category": "奇趣",
        "memory": "razzle-dazzle 炫目的表演 / 招摇"
      },
      {
        "en": "Razzmatazzing",
        "zh": "热闹炫目中",
        "category": "奇趣",
        "memory": "razzmatazz 喧闹刺激的活动"
      },
      {
        "en": "Recombobulating",
        "zh": "理顺 / 恢复正常中",
        "category": "奇趣",
        "memory": "recombobulate 与 dis- 相对，意为「使恢复条理」"
      },
      {
        "en": "Reticulating",
        "zh": "联网 / 成网状中",
        "category": "行动",
        "memory": "reticulate 成网状、连接"
      },
      {
        "en": "Roosting",
        "zh": "栖息中",
        "category": "自然",
        "memory": "roost 鸟禽栖息"
      },
      {
        "en": "Ruminating",
        "zh": "反刍 / 沉思中",
        "category": "思考",
        "memory": "ruminate 牛反刍；引申反复思考"
      },
      {
        "en": "Sautéing",
        "zh": "嫩煎中",
        "category": "烹饪",
        "memory": "sauté 法式快速煎炒（烹饪）"
      },
      {
        "en": "Scampering",
        "zh": "蹦跳奔跑中",
        "category": "移动",
        "memory": "scamper 小动物轻快蹦跑"
      },
      {
        "en": "Schlepping",
        "zh": "吃力拖拽中",
        "category": "移动",
        "memory": "schlep 意第绪语，费力拖着重物走",
        "source": "Merriam-Webster：schlep (Yiddish shlepen) = to drag or haul with effort"
      },
      {
        "en": "Scurrying",
        "zh": "急匆匆跑中",
        "category": "移动",
        "memory": "scurry 疾走、匆忙"
      },
      {
        "en": "Seasoning",
        "zh": "调味中",
        "category": "烹饪",
        "memory": "season 调味（烹饪）"
      },
      {
        "en": "Shenaniganing",
        "zh": "捣蛋 / 恶作剧中",
        "category": "奇趣",
        "memory": "shenanigan 胡闹、恶作剧"
      },
      {
        "en": "Shimmying",
        "zh": "摇摆颤动中",
        "category": "音乐",
        "memory": "shimmy 颤动、摇摆"
      },
      {
        "en": "Simmering",
        "zh": "小火煨中",
        "category": "烹饪",
        "memory": "simmer 微沸慢炖（烹饪）"
      },
      {
        "en": "Skedaddling",
        "zh": "溜走中",
        "category": "移动",
        "memory": "skedaddle 溜走、迅速离开"
      },
      {
        "en": "Sketching",
        "zh": "速写中",
        "category": "行动",
        "memory": "sketch 素描、草绘"
      },
      {
        "en": "Slithering",
        "zh": "蜿蜒滑行中",
        "category": "移动",
        "memory": "slither 蛇般滑行"
      },
      {
        "en": "Smooshing",
        "zh": "压扁 / 挤拢中",
        "category": "行动",
        "memory": "smoosh 拟声：把东西压扁"
      },
      {
        "en": "Sock-hopping",
        "zh": "跳摇摆舞中",
        "category": "音乐",
        "memory": "sock hop 1950 年代穿袜跳舞的舞会"
      },
      {
        "en": "Spelunking",
        "zh": "探洞中",
        "category": "移动",
        "memory": "spelunk 洞穴探险"
      },
      {
        "en": "Spinning",
        "zh": "旋转中",
        "category": "行动",
        "memory": "spin 旋转"
      },
      {
        "en": "Sprouting",
        "zh": "发芽中",
        "category": "自然",
        "memory": "sprout 抽芽、冒出"
      },
      {
        "en": "Stewing",
        "zh": "炖 / 焖中",
        "category": "烹饪",
        "memory": "stew 炖煮（烹饪）"
      },
      {
        "en": "Sublimating",
        "zh": "升华中",
        "category": "自然",
        "memory": "sublimate 升华（固→气）"
      },
      {
        "en": "Swirling",
        "zh": "打旋中",
        "category": "自然",
        "memory": "swirl 漩涡、打转"
      },
      {
        "en": "Swooping",
        "zh": "俯冲中",
        "category": "移动",
        "memory": "swoop 猛地俯冲而下"
      },
      {
        "en": "Symbioting",
        "zh": "共生中",
        "category": "自然",
        "memory": "symbiosis 共生关系"
      },
      {
        "en": "Synthesizing",
        "zh": "综合 / 合成中",
        "category": "思考",
        "memory": "synthesize 合成、综合"
      },
      {
        "en": "Tempering",
        "zh": "回火 / 调和中",
        "category": "烹饪",
        "memory": "temper 金属回火；调和"
      },
      {
        "en": "Thinking",
        "zh": "思考中",
        "category": "思考",
        "memory": "think 思考"
      },
      {
        "en": "Thundering",
        "zh": "雷鸣中",
        "category": "自然",
        "memory": "thunder 雷"
      },
      {
        "en": "Tinkering",
        "zh": "鼓捣 / 修修补补中",
        "category": "行动",
        "memory": "tinker 小修小补、摆弄"
      },
      {
        "en": "Tomfoolering",
        "zh": "胡闹中",
        "category": "奇趣",
        "memory": "tomfoolery 傻事、胡闹"
      },
      {
        "en": "Topsy-turvying",
        "zh": "颠倒混乱中",
        "category": "奇趣",
        "memory": "topsy-turvy 颠倒、乱七八糟"
      },
      {
        "en": "Transfiguring",
        "zh": "变身 / 显圣中",
        "category": "行动",
        "memory": "transfigure 变形、使改观"
      },
      {
        "en": "Transmuting",
        "zh": "嬗变 / 转化中",
        "category": "行动",
        "memory": "transmute 转变、蜕变"
      },
      {
        "en": "Twisting",
        "zh": "扭转中",
        "category": "行动",
        "memory": "twist 扭、绕"
      },
      {
        "en": "Undulating",
        "zh": "起伏波动中",
        "category": "自然",
        "memory": "undulate 波浪般起伏"
      },
      {
        "en": "Unfurling",
        "zh": "展开中",
        "category": "行动",
        "memory": "unfurl 展开、舒卷"
      },
      {
        "en": "Unravelling",
        "zh": "解开 / 厘清中",
        "category": "思考",
        "memory": "unravel 解开（英式拼写）"
      },
      {
        "en": "Vibing",
        "zh": "感受氛围中",
        "category": "音乐",
        "memory": "vibe 氛围、共鸣"
      },
      {
        "en": "Waddling",
        "zh": "摇摇摆摆走中",
        "category": "移动",
        "memory": "waddle 鸭步蹒跚"
      },
      {
        "en": "Wandering",
        "zh": "漫游中",
        "category": "移动",
        "memory": "wander 漫游、闲逛"
      },
      {
        "en": "Warping",
        "zh": "弯曲 / 变形中",
        "category": "行动",
        "memory": "warp 扭曲、翘曲"
      },
      {
        "en": "Whatchamacalliting",
        "zh": "捣鼓那个叫不出名的东西中",
        "category": "奇趣",
        "memory": "whatchamacallit 那个「啥」（想不起名字的物件）",
        "source": "Merriam-Webster：whatchamacallit = something that one does not readily recall the name of"
      },
      {
        "en": "Whirlpooling",
        "zh": "打转 / 漩涡中",
        "category": "自然",
        "memory": "whirlpool 漩涡"
      },
      {
        "en": "Whirring",
        "zh": "嗡嗡作响中",
        "category": "行动",
        "memory": "whir 嗡鸣、呼啸"
      },
      {
        "en": "Whisking",
        "zh": "打发 / 拂过中",
        "category": "烹饪",
        "memory": "whisk 打蛋器；快速拂过（烹饪）"
      },
      {
        "en": "Wibbling",
        "zh": "摇摇晃晃 / 东拉西扯中",
        "category": "奇趣",
        "memory": "wibble 英式：wobble 摇晃；也指说废话",
        "source": "Cambridge / Wiktionary：wibble (British) = to shake or move from side to side; to talk nonsense"
      },
      {
        "en": "Working",
        "zh": "工作中",
        "category": "行动",
        "memory": "work 工作"
      },
      {
        "en": "Wrangling",
        "zh": "争执 / 驯马中",
        "category": "行动",
        "memory": "wrangle 争吵；也指驯马"
      },
      {
        "en": "Zesting",
        "zh": "刨柠檬皮 / 添风味中",
        "category": "烹饪",
        "memory": "zest 柠檬皮；引申热情（烹饪）"
      },
      {
        "en": "Zigzagging",
        "zh": "蜿蜒曲折中",
        "category": "移动",
        "memory": "zigzag Z 字形折线前进"
      }
    ]
  };
